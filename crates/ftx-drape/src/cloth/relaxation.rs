//! Accepted per-element Maxwell history, shared by free draping and prescribed-body continuation.
//! Trial Newton/contact iterations read history; only a converged, audited physical step advances it.

use ftx_core::{Refusal, RefusalCode, Vec3};
use ftx_pattern::material_relaxation::{CompiledRelaxation, Memory};
use ftx_pattern::material_response::{CompiledMembrane, StrainDomain};
use serde_json::{Value, json};

use super::{ClothModel, SolveReport, Solver};
use crate::elements::{Local, Membrane, MembraneElement, soften};

#[cfg(test)]
mod tests;

#[derive(Clone, Debug)]
struct Bound {
    element: MembraneElement,
    material: Membrane,
    equilibrium: Option<CompiledMembrane>,
    domain: Option<StrainDomain>,
    law: CompiledRelaxation,
    memory: Memory,
    // Cumulative accepted physical dashpot work; independent of elastic/numerical energy loss.
    dissipated_j: f64,
}

#[derive(Clone, Debug)]
pub(super) struct Field {
    entries: Vec<Option<Bound>>,
    time_seconds: f64,
    dissipated_j: f64,
    steps: usize,
}

fn bad(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::OutOfRange, message)
}

impl Bound {
    fn effective_strain(&self, x: &[Vec3]) -> Result<[f64; 3], Refusal> {
        if self.element.v.iter().any(|i| *i >= x.len()) {
            return Err(bad("Maxwell triangle indexes a missing position"));
        }
        let strain = self.element.strain(x);
        if !self.law.contains(strain) {
            return Err(bad(
                "triangle strain outside Maxwell characterization domain",
            ));
        }
        Ok(soften(strain, &self.material).0)
    }
}

impl Field {
    pub(super) fn has(&self, triangle: usize) -> bool {
        self.entries.get(triangle).is_some_and(Option::is_some)
    }

    // Public model fields may have been edited by a low-level caller. A different rest frame,
    // compression map or equilibrium law must not silently reuse an old material's history.
    pub(super) fn check(&self, model: &ClothModel, x: &[Vec3]) -> Result<(), Refusal> {
        if self.entries.len() != model.membranes.len()
            || model.compression.len() != model.membranes.len()
            || x.len() != model.rest.len()
        {
            return Err(bad("Maxwell history no longer matches the cloth topology"));
        }
        for (i, entry) in self.entries.iter().enumerate() {
            if let Some(b) = entry {
                if model.membranes[i] != b.element
                    || model.material(i) != b.material
                    || model.membrane_response(i) != b.equilibrium.as_ref()
                    || model.membrane_domain(i) != b.domain
                {
                    return Err(bad(
                        "changed material/rest triangle cannot reuse Maxwell history; build a new solver",
                    ));
                }
                b.effective_strain(x)?;
            }
        }
        Ok(())
    }

    pub(super) fn energy(&self, x: &[Vec3], h: Option<f64>) -> Result<f64, Refusal> {
        let mut energy = 0.0;
        for b in self.entries.iter().flatten() {
            energy += b.element.area * b.memory.response(b.effective_strain(x)?, h)?.energy;
        }
        if !energy.is_finite() {
            return Err(bad("Maxwell field energy overflow"));
        }
        Ok(energy)
    }

    pub(super) fn local(&self, i: usize, x: &[Vec3], h: Option<f64>) -> Option<Local<9, 81>> {
        self.entries[i].as_ref().map(|b| {
            b.element
                .local_with_response(x, &b.material, |strain| b.memory.response(strain, h).ok())
        })
    }

    fn advance(&mut self, x: &[Vec3], h: f64) -> Result<(), Refusal> {
        let mut updates = Vec::new();
        let mut dissipated = self.dissipated_j;
        for (i, b) in self.entries.iter().enumerate() {
            if let Some(b) = b {
                let u = b.memory.advance(b.effective_strain(x)?, h)?;
                let work = b.element.area * u.dissipated_j_m2;
                let local_total = b.dissipated_j + work;
                if !local_total.is_finite() || work < 0. {
                    return Err(bad("Maxwell triangle dissipation overflow"));
                }
                dissipated += work;
                updates.push((i, u.memory, local_total));
            }
        }
        let time = self.time_seconds + h;
        let steps = self
            .steps
            .checked_add(1)
            .ok_or_else(|| bad("Maxwell step count overflow"))?;
        if !time.is_finite() || time <= self.time_seconds || !dissipated.is_finite() {
            return Err(bad("Maxwell elapsed time or dissipation overflow"));
        }
        for (i, memory, dissipated_j) in updates {
            let bound = self.entries[i]
                .as_mut()
                .expect("an update has a bound element");
            bound.memory = memory;
            bound.dissipated_j = dissipated_j;
        }
        self.time_seconds = time;
        self.steps = steps;
        self.dissipated_j = dissipated;
        Ok(())
    }
}

impl Solver {
    /// Bind optional Maxwell laws to retained material triangles AFTER welding and grain/gather
    /// mapping, initializing memory once. None entries retain their existing equilibrium behavior.
    /// Cloning or motion-continuing this solver preserves the accepted state instead of reinitializing.
    ///
    /// # Errors
    /// Wrong field size, repeated binding, stale/invalid tangent or initial strain outside the domain.
    pub fn with_relaxation(mut self, laws: &[Option<CompiledRelaxation>]) -> Result<Self, Refusal> {
        if self.relaxation.is_some() || laws.len() != self.model.membranes.len() {
            return Err(bad(
                "one Maxwell selector per triangle is required; history cannot be rebound",
            ));
        }
        if !laws.iter().any(Option::is_some) {
            return Ok(self);
        }
        if self.model.compression.len() != laws.len() {
            return Err(bad(
                "one compression mapping per material triangle is required",
            ));
        }
        self.model.check_membrane_domains(&self.x)?;
        let entries = laws
            .iter()
            .enumerate()
            .map(|(i, law)| {
                let Some(law) = law else {
                    return Ok(None);
                };
                if self.membrane_plasticity.as_ref().is_some_and(|field| field.has(i)) {
                    return Err(bad("Maxwell memory and plastic membrane need a jointly characterized law on the same triangle"));
                }
                let material = self.model.material(i);
                material.validate()?;
                if material.q != law.origin_tangent() {
                    return Err(bad(
                        "Maxwell branch tangent differs from the bound equilibrium material",
                    ));
                }
                let element = self.model.membranes[i];
                if element.v.iter().any(|j| *j >= self.x.len()) {
                    return Err(bad("Maxwell triangle indexes a missing position"));
                }
                let strain = element.strain(&self.x);
                if !law.contains(strain) {
                    return Err(bad("initial triangle strain outside Maxwell domain"));
                }
                let memory = law.initialize(soften(strain, &material).0)?;
                Ok(Some(Bound {
                    element,
                    material,
                    memory,
                    dissipated_j: 0.,
                    law: law.clone(),
                    equilibrium: self.model.membrane_response(i).cloned(),
                    domain: self.model.membrane_domain(i),
                }))
            })
            .collect::<Result<Vec<_>, Refusal>>()?;
        self.relaxation = Some(Field {
            entries,
            time_seconds: 0.0,
            dissipated_j: 0.0,
            steps: 0,
        });
        Ok(self)
    }

    // The opt-in history path is transactional, including contact lag and linear-solver caches.
    // Memoryless steps use the same accepted-state boundary in step_at_body.
    pub(super) fn relaxing_step(
        &mut self,
        h: f64,
        body_start: Option<&[Vec3]>,
    ) -> Result<SolveReport, Refusal> {
        if !h.is_finite()
            || h <= 0.0
            || self.v.len() != self.x.len()
            || self.v.iter().any(|v| !v.is_finite())
        {
            return Err(bad(
                "material-history step needs a positive finite duration and finite velocities",
            ));
        }
        if let Some(field) = &self.relaxation {
            field.check(&self.model, &self.x)?;
        }
        if let Some(field) = &self.plasticity {
            field.check(&self.model, &self.x)?;
        }
        if let Some(field) = &self.membrane_plasticity {
            field.check(&self.model, &self.x)?;
        }
        let mut trial = self.clone();
        let report = trial.step_inner(h, body_start)?;
        if !report.converged {
            return Err(Refusal::new(
                RefusalCode::NotConverged,
                format!(
                    "material-history step did not converge after {} iterations (residual {} N, last step {} m); positions, velocities and material history are unchanged",
                    report.iterations, report.residual, report.last_step
                ),
            ));
        }
        if trial.v.iter().any(|v| !v.is_finite()) {
            return Err(bad("material-history step produced non-finite velocity"));
        }
        if let Some(field) = &mut trial.relaxation {
            field.advance(&trial.x, h)?;
        }
        if let Some(field) = &mut trial.plasticity {
            field.advance(&trial.x)?;
        }
        if let Some(field) = &mut trial.membrane_plasticity {
            field.advance(&trial.x)?;
        }
        *self = trial;
        Ok(report)
    }

    /// Cumulative accepted Maxwell viscous work in joules, in retained triangle order.
    /// Unselected triangles contribute zero. None means no Maxwell field is bound. This read does
    /// not evolve memory. Values include prior drape work: heat consumers must subtract their own
    /// accepted baseline and qualify conversion separately. Spring storage, backward-Euler
    /// numerical dissipation and contact/friction work are not included.
    ///
    /// # Errors
    /// Changed material/topology or invalid current strain.
    pub fn membrane_relaxation_dissipation_j(&self) -> Result<Option<Vec<f64>>, Refusal> {
        let Some(field) = &self.relaxation else {
            return Ok(None);
        };
        field.check(&self.model, &self.x)?;
        Ok(Some(
            field
                .entries
                .iter()
                .map(|entry| entry.as_ref().map_or(0., |bound| bound.dissipated_j))
                .collect(),
        ))
    }

    /// Source-law identities and accepted internal strains, storage and viscous step-work in SI units.
    /// This is an output report, not a restart-file import. Geometric settledness is not stress
    /// equilibrium, and reported dissipation is not coupled automatically into the thermal solver.
    ///
    /// # Errors
    /// Changed material/topology, invalid current strain or non-finite physical branch storage.
    pub fn relaxation_report(&self) -> Result<Option<Value>, Refusal> {
        let Some(field) = &self.relaxation else {
            return Ok(None);
        };
        field.check(&self.model, &self.x)?;
        let mut elements = Vec::new();
        for (i, b) in field.entries.iter().enumerate() {
            if let Some(b) = b {
                let response = b.memory.response(b.effective_strain(&self.x)?, None)?;
                elements.push(json!({"triangle":i,"law_sha256":b.law.fingerprint(),
                    "viscous_green_strain":b.memory.viscous_strains(),
                    "effective_transient_stress_n_m":response.stress}));
            }
        }
        let scope = if field
            .entries
            .iter()
            .flatten()
            .any(|b| b.law.has_orthotropic_branches())
        {
            "Constant source-qualified proportional and orthotropic branch tensors in effective Green strain; excludes numerical dissipation, plastic damage and thermal feedback"
        } else {
            "Constant proportional branch tensors in effective Green strain; excludes numerical dissipation, plastic damage and thermal feedback"
        };
        Ok(Some(
            json!({"schema":"ftx-membrane-history/1", "model":"green_maxwell/1",
            "time_seconds":field.time_seconds,"accepted_steps":field.steps,
            "stored_branch_energy_j":field.energy(&self.x,None)?,
            "viscous_step_work_j":field.dissipated_j,"elements":elements,
            "scope":scope,
            "stress_equilibrium_claim":false,"empirical_validation_claim":false}),
        ))
    }
}

//! Accepted permanent-curvature history in source-panel material coordinates.
//! The field replaces selected elastic patch energies and derivatives; seams remain separate.
use ftx_core::{Refusal, RefusalCode, Vec3};
use ftx_pattern::material_plasticity::{MODEL, Memory, PlasticBending};
use serde_json::{Value, json};

use super::{ClothModel, Solver};
use crate::bending::BendingPatch;
use crate::elements::{Local, MembraneElement};

fn bad(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}
fn invalid_local() -> Local<18, 324> {
    Local {
        energy: f64::INFINITY,
        grad: [f64::NAN; 18],
        hess: [f64::NAN; 324],
        psd: false,
    }
}

#[derive(Clone, Debug)]
struct Bound {
    patch: BendingPatch,
    triangle: usize,
    element: MembraneElement,
    law_sha256: String,
    memory: Memory,
}

#[derive(Clone, Debug)]
pub(super) struct Field {
    entries: Vec<Option<Bound>>,
    triangles: Vec<usize>,
    triangle_count: usize,
    dissipated_j: f64,
    steps: usize,
}

impl Field {
    pub(super) fn check(&self, model: &ClothModel, x: &[Vec3]) -> Result<(), Refusal> {
        if self.entries.len() != model.panel_bending_patches().len()
            || self.triangles != model.panel_bending_triangles()
            || self.triangle_count != model.membranes.len()
            || x.len() != model.rest.len()
        {
            return Err(bad(
                "plastic bending cannot reuse history after topology or ownership changes",
            ));
        }
        for (i, b) in self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, b)| b.as_ref().map(|b| (i, b)))
        {
            if b.patch != model.panel_bending_patches()[i]
                || b.element != model.membranes[b.triangle]
            {
                return Err(bad(
                    "plastic bending cannot reuse history after material, grain, metric or stencil changes",
                ));
            }
            b.memory.response(b.patch.bending_strain(x)?, false)?;
        }
        Ok(())
    }

    pub(super) fn energy(
        &self,
        model: &ClothModel,
        x: &[Vec3],
        incremental: bool,
    ) -> Result<f64, Refusal> {
        // The binding check is at step/report entry, not repeated for every line-search candidate.
        let mut energy = 0.;
        for (i, p) in model.panel_bending_patches().iter().enumerate() {
            energy += match &self.entries[i] {
                Some(b) => {
                    p.reference_area()
                        * b.memory.response(p.bending_strain(x)?, incremental)?.energy
                }
                None => p.energy(x),
            };
        }
        if !energy.is_finite() {
            return Err(bad("plastic panel energy overflows"));
        }
        Ok(energy)
    }

    pub(super) fn local(&self, i: usize, x: &[Vec3], incremental: bool) -> Option<Local<18, 324>> {
        self.entries[i].as_ref().map(|b| {
            b.patch
                .normal_local_with_response(x, |k| b.memory.response(k, incremental))
                .unwrap_or_else(|_| invalid_local())
        })
    }

    pub(super) fn advance(&mut self, x: &[Vec3]) -> Result<(), Refusal> {
        let mut proposed = Vec::new();
        let mut dissipated = self.dissipated_j;
        for (i, b) in self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, b)| b.as_ref().map(|b| (i, b)))
        {
            let update = b.memory.advance(b.patch.bending_strain(x)?)?;
            dissipated += b.patch.reference_area() * update.dissipated_j_m2;
            proposed.push((i, update.memory));
        }
        let steps = self
            .steps
            .checked_add(1)
            .ok_or_else(|| bad("plastic step count overflows"))?;
        if !dissipated.is_finite() || dissipated < self.dissipated_j {
            return Err(bad("plastic dissipation overflows"));
        }
        for (i, memory) in proposed {
            self.entries[i].as_mut().expect("bound update").memory = memory;
        }
        self.dissipated_j = dissipated;
        self.steps = steps;
        Ok(())
    }
}

impl Solver {
    /// Bind per-retained-triangle plastic laws to already built normal-curvature patches.
    /// Every Some must own a patch with the EXACT declared tensor/target. None preserves elastic
    /// behavior. Initialization is pristine relative to that target; clone/motion retains history.
    ///
    /// # Errors
    /// Rebinding, field mismatch, unsupported/unpatched triangles, stale tensor or invalid start.
    pub fn with_bending_plasticity(
        mut self,
        laws: &[Option<PlasticBending>],
    ) -> Result<Self, Refusal> {
        if self.plasticity.is_some() || laws.len() != self.model.membranes.len() {
            return Err(bad(
                "one plasticity selector per retained triangle is required; history cannot be rebound",
            ));
        }
        if !laws.iter().any(Option::is_some) {
            return Ok(self);
        }
        let triangles = self.model.panel_bending_triangles();
        let mut patched = vec![false; laws.len()];
        for &triangle in triangles {
            patched[triangle] = true;
        }
        for (i, law) in laws.iter().enumerate() {
            if law.is_some() && !patched[i] {
                return Err(bad(
                    "plastic curvature cannot be attached to an unpatched triangle or scalar seam hinge",
                ));
            }
        }
        let entries = triangles
            .iter()
            .zip(self.model.panel_bending_patches())
            .map(|(&triangle, patch)| {
                let Some(law) = &laws[triangle] else {
                    return Ok(None);
                };
                if law.tensor() != patch.tensor() {
                    return Err(bad(
                        "plastic bending tensor/target differs from the source-panel law",
                    ));
                }
                Ok(Some(Bound {
                    patch: patch.clone(),
                    triangle,
                    element: self.model.membranes[triangle],
                    law_sha256: law.fingerprint(),
                    memory: law.pristine(),
                }))
            })
            .collect::<Result<Vec<_>, Refusal>>()?;
        let field = Field {
            entries,
            triangles: triangles.to_vec(),
            triangle_count: laws.len(),
            dissipated_j: 0.,
            steps: 0,
        };
        field.check(&self.model, &self.x)?;
        self.plasticity = Some(field);
        Ok(self)
    }

    /// Actual stored panel energy, including accepted plastic curvature and hardening. This is NOT
    /// the incremental return-map potential and excludes scalar seam, membrane and contact energy.
    ///
    /// # Errors
    /// Stale history, invalid reconstructed curvature or non-finite energy.
    pub fn panel_bending_energy(&self) -> Result<f64, Refusal> {
        self.model.check_panel_bending_domains(&self.x)?;
        if let Some(field) = &self.plasticity {
            field.check(&self.model, &self.x)?;
            field.energy(&self.model, &self.x, false)
        } else {
            let energy = self
                .model
                .panel_bending_patches()
                .iter()
                .map(|p| p.energy(&self.x))
                .sum::<f64>();
            if !energy.is_finite() {
                return Err(bad("panel energy overflows"));
            }
            Ok(energy)
        }
    }

    /// Output accepted source-panel state and rate-independent dissipation. This report is NOT a
    /// restart import, physical wrinkle validation, stress-free garment or thermal source term.
    ///
    /// # Errors
    /// Changed material/topology, invalid current frame or non-finite stored response.
    pub fn bending_plasticity_report(&self) -> Result<Option<Value>, Refusal> {
        let Some(field) = &self.plasticity else {
            return Ok(None);
        };
        field.check(&self.model, &self.x)?;
        let mut patches = Vec::new();
        let mut stored = 0.;
        for b in field.entries.iter().flatten() {
            let k = b.patch.bending_strain(&self.x)?;
            let r = b.memory.response(k, false)?;
            let energy = b.patch.reference_area() * r.energy;
            stored += energy;
            patches.push(json!({"triangle":b.triangle,"vertices":b.patch.v,
                "law_sha256":b.law_sha256,"reference_area_m2":b.patch.reference_area(),
                "plastic_curvature_m_inv":b.memory.plastic_curvature(),
                "accumulated_curvature_m_inv":b.memory.accumulated_curvature(),
                "authored_preferred_curvature_m_inv":b.patch.preferred_curvature(),
                "current_curvature_departure_m_inv":k,"elastic_moment_n":r.moment_n,
                "stored_elastic_and_hardening_energy_j":energy}));
        }
        if !stored.is_finite() {
            return Err(bad("plastic history storage overflows"));
        }
        Ok(Some(json!({"schema":"ftx-bending-history/1","model":MODEL,
            "initialization":"pristine_relative_to_authored_preferred_curvature",
            "accepted_steps":field.steps,"stored_elastic_and_hardening_energy_j":stored,
            "plastic_dissipation_j":field.dissipated_j,"patches":patches,
            "scope":"Rate-independent orthotropic yield with isotropic directional hardening in covariant material curvature; excludes seam plasticity, damage, rate/temperature effects, remeshing and thermal feedback",
            "empirical_validation_claim":false,"restart_import":false})))
    }
}

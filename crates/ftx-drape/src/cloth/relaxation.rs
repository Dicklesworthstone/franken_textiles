//! Accepted per-element Maxwell history, shared by free draping and prescribed-body continuation.
//! Trial Newton/contact iterations read history; only a converged, audited physical step advances it.

use ftx_core::{Refusal, RefusalCode, Vec3};
use ftx_pattern::material_relaxation::{CompiledRelaxation, IntegrationScheme, Memory};
use ftx_pattern::material_response::{CompiledMembrane, StrainDomain};
use serde_json::{Value, json};

use super::{ClothModel, SolveReport, Solver};
use crate::elements::{CompressionModel, Local, Membrane, MembraneElement, effective_strain};

#[cfg(test)]
mod tests;

#[derive(Clone, Debug)]
struct Bound {
    element: MembraneElement,
    material: Membrane,
    compression_model: CompressionModel,
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
        Ok(effective_strain(
            strain,
            &self.material,
            self.compression_model,
        ))
    }
}

impl Field {
    pub(super) fn has(&self, triangle: usize) -> bool {
        self.entries.get(triangle).is_some_and(Option::is_some)
    }

    pub(super) fn admits_plastic(
        &self,
        triangle: usize,
        law: &ftx_pattern::material_membrane_plasticity::PlasticMembrane,
    ) -> bool {
        self.entries
            .get(triangle)
            .and_then(Option::as_ref)
            .is_some_and(|bound| law.accepts_parallel(&bound.law))
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
                    || model.compression_model != b.compression_model
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
                .local_with_response_model(x, &b.material, b.compression_model, |strain| {
                    b.memory.response(strain, h).ok()
                })
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
    pub fn with_relaxation(self, laws: &[Option<CompiledRelaxation>]) -> Result<Self, Refusal> {
        if laws
            .iter()
            .flatten()
            .any(|law| law.parallel_material_sha256().is_some())
        {
            return Err(bad(
                "a parallel Maxwell half-law requires atomic with_membrane_histories binding",
            ));
        }
        self.bind_relaxation(laws)
    }

    pub(super) fn bind_relaxation(
        mut self,
        laws: &[Option<CompiledRelaxation>],
    ) -> Result<Self, Refusal> {
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
                if self.membrane_plasticity.as_ref().is_some_and(|field|
                    field.has(i) && !field.admits_relaxation(i, law)) {
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
                let compression_model = self.model.compression_model;
                let memory = law.initialize(effective_strain(strain, &material, compression_model))?;
                Ok(Some(Bound {
                    element,
                    material,
                    compression_model,
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
        guess: Option<&[Vec3]>,
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
        // Check and freeze accepted material history before seeding Newton. The inner step keeps
        // inertia and friction anchored at x_n; only its converged endpoint advances the memories.
        let report = trial.step_inner_with_guess(h, body_start, guess)?;
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
        let mut has_exact_exp = false;
        let mut all_exact_exp = true;
        for (i, b) in field.entries.iter().enumerate() {
            if let Some(b) = b {
                let response = b.memory.response(b.effective_strain(&self.x)?, None)?;
                let mut element = json!({"triangle":i,"law_sha256":b.law.fingerprint(),
                    "viscous_green_strain":b.memory.viscous_strains(),
                    "effective_transient_stress_n_m":response.stress});
                if b.law.integration() == IntegrationScheme::ExactExponential {
                    has_exact_exp = true;
                    element["integration"] = json!("exact_exponential");
                } else {
                    all_exact_exp = false;
                }
                if let Some(identity) = b.law.parallel_material_sha256() {
                    element["parallel_network"] = json!({"model":ftx_pattern::material_relaxation::parallel::MODEL,
                        "material_sha256":identity,"strain_basis":"total_physical_green_strain"});
                }
                elements.push(element);
            }
        }
        let has_parallel = field
            .entries
            .iter()
            .flatten()
            .any(|b| b.law.parallel_material_sha256().is_some());
        let has_orthotropic = field
            .entries
            .iter()
            .flatten()
            .any(|b| b.law.has_orthotropic_branches());
        let scope = match (has_exact_exp, has_parallel, has_orthotropic) {
            (false, true, _) => {
                "Source-qualified Maxwell branch tensors; marked parallel networks use TOTAL physical Green strain and add transient stress independently of equilibrium-branch plastic flow. Excludes same-branch viscoplasticity, numerical dissipation, damage and thermal feedback"
            }
            (false, false, true) => {
                "Constant source-qualified proportional and orthotropic branch tensors in effective Green strain; excludes numerical dissipation, plastic damage and thermal feedback"
            }
            (false, false, false) => {
                "Constant proportional branch tensors in effective Green strain; excludes numerical dissipation, plastic damage and thermal feedback"
            }
            (true, true, _) => {
                "Source-qualified Maxwell branch tensors with opt-in exact piecewise-linear exponential integration where marked; marked parallel networks use TOTAL physical Green strain and add transient stress independently of equilibrium-branch plastic flow. Excludes same-branch viscoplasticity, backward-Euler numerical dissipation where applicable, damage and thermal feedback"
            }
            (true, false, true) => {
                "Constant source-qualified proportional and orthotropic branch tensors in effective Green strain with opt-in exact piecewise-linear exponential integration where marked; excludes backward-Euler numerical dissipation where applicable, plastic damage and thermal feedback"
            }
            (true, false, false) => {
                "Constant proportional branch tensors in effective Green strain with opt-in exact piecewise-linear exponential integration where marked; excludes backward-Euler numerical dissipation where applicable, plastic damage and thermal feedback"
            }
        };
        let mut report = json!({"schema":"ftx-membrane-history/1", "model":"green_maxwell/1",
            "time_seconds":field.time_seconds,"accepted_steps":field.steps,
            "stored_branch_energy_j":field.energy(&self.x,None)?,
            "viscous_step_work_j":field.dissipated_j,"elements":elements,
            "scope":scope,
            "stress_equilibrium_claim":false,"empirical_validation_claim":false});
        if has_exact_exp {
            report["integration"] = json!(if all_exact_exp {
                "exact_exponential"
            } else {
                "per_element"
            });
        }
        Ok(Some(report))
    }
}

#[cfg(test)]
mod carried_start_tests {
    use super::*;
    use crate::{Body, ContactParams};
    use ftx_core::Vec2;
    use ftx_pattern::{material_catalog::Selection, materials::Catalog};

    fn sheet(parallel_plasticity: bool) -> Solver {
        let selection: Selection = serde_json::from_str(if parallel_plasticity {
            include_str!("../../../../tests/fixtures/materials/synthetic-parallel-membrane.json")
        } else {
            include_str!("../../../../tests/fixtures/materials/synthetic-relaxation.json")
        })
        .unwrap();
        let material = Catalog::builtin().unwrap().resolve(&selection).unwrap();
        let m = material.mechanics().unwrap();
        let model = ClothModel::new(
            vec![Vec2::ZERO, Vec2::new(0.1, 0.), Vec2::new(0., 0.1)],
            &[[0, 1, 2]],
            Membrane::orthotropic(m.warp_n_m, m.weft_n_m, m.poisson, m.shear_n_m, 1.),
            0.,
            m.areal_kg_m2,
        )
        .unwrap();
        let body = Body::new(
            vec![
                Vec3::new(-1., -1., 0.),
                Vec3::new(1., -1., 0.),
                Vec3::new(0., 1., 0.),
            ],
            vec![[0, 1, 2]],
            0.1,
        )
        .unwrap();
        let mut solver = Solver::new(
            model,
            vec![
                Vec3::new(0., 0., 0.0007),
                Vec3::new(0.11, 0., 0.0007),
                Vec3::new(0., 0.1, 0.0007),
            ],
            &[[true; 3], [false, true, true], [true; 3]],
            Vec3::ZERO,
        )
        .with_membrane_histories(
            &[material.membrane_relaxation().unwrap()],
            &[material.membrane_plasticity().unwrap()],
        )
        .unwrap()
        .with_contact(
            body,
            ContactParams {
                dhat: 0.001,
                thickness: 0.0005,
                kappa: 100.,
                mu: 0.4,
                eps_v: 0.001,
            },
        )
        .unwrap();
        solver.v[1].x = 0.02;
        solver.tol = 1e-10;
        solver.max_newton = 100;
        solver.friction_rounds = 1;
        solver
    }

    #[test]
    fn carried_history_guess_keeps_physical_inertia_friction_and_single_memory_commit() {
        let h = 0.0005;
        let translation = Vec3::new(0.0003, 0., 0.);
        for plastic in [false, true] {
            let mut original = sheet(plastic);
            let x0 = original.x.clone();
            let body_start = original.contact.as_ref().unwrap().body.vertices.clone();
            let body_end: Vec<_> = body_start.iter().map(|p| *p + translation).collect();
            let raw_guess: Vec<_> = x0.iter().map(|p| *p + translation).collect();
            let guess = original.keep_fixed(&raw_guess);
            assert_eq!(
                original
                    .contact
                    .as_ref()
                    .unwrap()
                    .motion_bound(&x0, &guess, &body_end, 1000)
                    .unwrap()
                    .fraction,
                1.0
            );
            original
                .contact
                .as_mut()
                .unwrap()
                .body
                .refit_vertices(body_end)
                .unwrap();

            // Independently evaluate the initial incremental potential using accepted x_n and v_n.
            // Changing the friction origin to the guess must give a different answer.
            let xt: Vec<_> = x0
                .iter()
                .zip(&original.v)
                .map(|(x, v)| *x + *v * h)
                .collect();
            let mut expected = original.clone();
            expected
                .contact
                .as_mut()
                .unwrap()
                .update_lag_motion(&x0, &x0, h, Some(&body_start));
            let expected_energy = expected.potential(&guess, Some((&xt, h))).unwrap();
            expected.contact.as_mut().unwrap().update_lag_motion(
                &guess,
                &guess,
                h,
                Some(&body_start),
            );
            let wrong_origin_energy = expected.potential(&guess, Some((&xt, h))).unwrap();
            assert!((expected_energy - wrong_origin_energy).abs() > 1e-10);

            let mut plain = original.clone();
            let plain_report = plain.step_at_body(h, Some(&body_start)).unwrap();
            let mut carried = original.clone();
            let report = carried
                .step_at_body_with_guess(h, Some(&body_start), &raw_guess)
                .unwrap();
            assert!(report.converged);
            assert!((report.energy_start - expected_energy).abs() < 1e-12);
            assert!(
                (report.energy_start - plain_report.energy_start).abs() > 1e-10,
                "the free-DOF guess must actually seed Newton"
            );
            assert_ne!(carried.x[1], x0[1]);
            for (i, &start) in x0.iter().enumerate() {
                assert!((carried.x[i] - plain.x[i]).norm() < 1e-8);
                assert!((carried.v[i] - (carried.x[i] - start) / h).norm() < 1e-10);
            }
            assert_eq!(carried.x[0], x0[0]);
            assert_eq!(carried.x[2], x0[2]);

            let mut memory = original.relaxation.clone().unwrap();
            memory.advance(&carried.x, h).unwrap();
            assert_eq!(
                format!("{:?}", carried.relaxation.as_ref().unwrap()),
                format!("{memory:?}")
            );
            assert_eq!(memory.steps, 1);
            assert_eq!(memory.time_seconds, h);
            assert!(memory.dissipated_j > 0.);
            if let Some(mut memory) = original.membrane_plasticity.clone() {
                memory.advance(&carried.x).unwrap();
                assert_eq!(
                    format!("{:?}", carried.membrane_plasticity.as_ref().unwrap()),
                    format!("{memory:?}")
                );
                assert!(carried.membrane_plastic_dissipation_j().unwrap().unwrap()[0] > 0.);
            }
        }
    }

    #[test]
    fn carried_history_failure_restores_the_entire_solver_and_replays() {
        let original = sheet(true);
        let valid_guess: Vec<_> = original
            .x
            .iter()
            .map(|x| *x + Vec3::new(0.0003, 0., 0.))
            .collect();
        let h = 0.0005;
        let mut expected = original.clone();
        expected
            .step_at_body_with_guess(h, None, &valid_guess)
            .unwrap();
        for fault in [
            "length",
            "domain",
            "non_finite",
            "newton",
            "history_commit",
            "integrator",
        ] {
            let mut solver = original.clone();
            let mut guess = valid_guess.clone();
            match fault {
                "length" => {
                    guess.pop();
                }
                "domain" => guess[1].x = 1.,
                "non_finite" => guess[1].x = f64::NAN,
                "newton" => solver.max_newton = 0,
                "history_commit" => solver.relaxation.as_mut().unwrap().steps = usize::MAX,
                "integrator" => {
                    solver.integrator = crate::TimeIntegrator::GeneralizedAlpha { rho_inf: 0.5 }
                }
                _ => unreachable!(),
            }
            let before = format!("{solver:?}");
            let error = solver.step_at_body_with_guess(h, None, &guess).unwrap_err();
            if fault == "history_commit" {
                assert!(
                    error.message.contains("Maxwell step count overflow"),
                    "{error:?}"
                );
            }
            assert_eq!(format!("{solver:?}"), before, "{fault}");
            solver.max_newton = original.max_newton;
            solver.integrator = original.integrator;
            solver.relaxation.as_mut().unwrap().steps = 0;
            solver
                .step_at_body_with_guess(h, None, &valid_guess)
                .unwrap();
            assert_eq!(format!("{solver:?}"), format!("{expected:?}"), "{fault}");
        }
    }
}

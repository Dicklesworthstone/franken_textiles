//! Accepted permanent in-plane strain in each retained triangle's material frame.
//! Plastic response replaces the selected elastic membrane; trials never advance memory.

use ftx_core::{Refusal, RefusalCode, Vec3};
use ftx_pattern::material_membrane_plasticity::{MODEL, Memory, PlasticMembrane};
use ftx_pattern::material_relaxation::CompiledRelaxation;
use ftx_pattern::material_response::StrainDomain;
use serde_json::{Value, json};

use super::{ClothModel, Solver};
use crate::elements::{Local, Membrane, MembraneElement};

fn bad(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::OutOfRange, message)
}

#[derive(Clone, Debug)]
struct Bound {
    element: MembraneElement,
    material: Membrane,
    domain: Option<StrainDomain>,
    law: PlasticMembrane,
    memory: Memory,
    dissipated_j: f64,
}

impl Bound {
    fn strain(&self, x: &[Vec3]) -> Result<[f64; 3], Refusal> {
        if self.element.v.iter().any(|i| *i >= x.len()) {
            return Err(bad("plastic membrane triangle indexes a missing position"));
        }
        let strain = self.element.strain(x);
        if !self.law.contains(strain) {
            return Err(bad(
                "physical Green strain is outside the plastic membrane domain",
            ));
        }
        Ok(strain)
    }
}

#[derive(Clone, Debug)]
pub(super) struct Field {
    entries: Vec<Option<Bound>>,
    dissipated_j: f64,
    steps: usize,
}

impl Field {
    pub(super) fn has(&self, triangle: usize) -> bool {
        self.entries.get(triangle).is_some_and(Option::is_some)
    }

    pub(super) fn admits_relaxation(&self, triangle: usize, law: &CompiledRelaxation) -> bool {
        self.entries
            .get(triangle)
            .and_then(Option::as_ref)
            .is_some_and(|bound| bound.law.accepts_parallel(law))
    }

    pub(super) fn check(&self, model: &ClothModel, x: &[Vec3]) -> Result<(), Refusal> {
        if self.entries.len() != model.membranes.len()
            || model.compression.len() != model.membranes.len()
            || x.len() != model.rest.len()
            || !self.dissipated_j.is_finite()
            || self.dissipated_j < 0.
        {
            return Err(bad(
                "plastic membrane history no longer matches the cloth topology",
            ));
        }
        for (i, bound) in self.entries.iter().enumerate() {
            if let Some(b) = bound {
                if model.membranes[i] != b.element
                    || model.material(i) != b.material
                    || model.membrane_response(i).is_some()
                    || model.membrane_domain(i) != b.domain
                    || !b.dissipated_j.is_finite()
                    || b.dissipated_j < 0.
                {
                    return Err(bad(
                        "changed material, grain or rest triangle cannot reuse plastic membrane history",
                    ));
                }
                b.memory.response(b.strain(x)?, false)?;
            }
        }
        Ok(())
    }

    // Supply the whole membrane contribution in its original order. Subtracting an elastic
    // baseline would both lose accuracy near cancellation and risk counting a yielded sheet twice.
    pub(super) fn energy(
        &self,
        model: &ClothModel,
        x: &[Vec3],
        incremental: bool,
    ) -> Result<f64, Refusal> {
        let mut energy = 0.;
        for (i, element) in model.membranes.iter().enumerate() {
            energy += match &self.entries[i] {
                Some(b) => element.area * b.memory.response(b.strain(x)?, incremental)?.energy,
                None => model.membrane_response(i).map_or_else(
                    || element.energy(x, &model.material(i)),
                    |law| element.response_energy(x, law, &model.material(i)),
                ),
            };
        }
        if !energy.is_finite() {
            return Err(bad("plastic membrane field energy overflows"));
        }
        Ok(energy)
    }

    pub(super) fn local(&self, i: usize, x: &[Vec3], incremental: bool) -> Option<Local<9, 81>> {
        self.entries[i].as_ref().map(|b| {
            // Binding requires unit compression factors: the shared geometric derivative then
            // receives physical Green strain, never the artificial gather-softened coordinate.
            b.element.local_with_response(x, &b.material, |strain| {
                b.memory.response(strain, incremental).ok()
            })
        })
    }

    pub(super) fn advance(&mut self, x: &[Vec3]) -> Result<(), Refusal> {
        let mut updates = Vec::new();
        let mut dissipated = self.dissipated_j;
        for (i, b) in self.entries.iter().enumerate() {
            if let Some(b) = b {
                let update = b.memory.advance(b.strain(x)?)?;
                let increment = b.element.area * update.dissipated_j_m2;
                let local = b.dissipated_j + increment;
                if !increment.is_finite()
                    || increment < 0.
                    || !local.is_finite()
                    || local < b.dissipated_j
                {
                    return Err(bad("plastic membrane triangle dissipation overflows"));
                }
                dissipated += increment;
                updates.push((i, update.memory, local));
            }
        }
        let steps = self
            .steps
            .checked_add(1)
            .ok_or_else(|| bad("plastic membrane step count overflows"))?;
        if !dissipated.is_finite() || dissipated < self.dissipated_j {
            return Err(bad("plastic membrane dissipation overflows"));
        }
        for (i, memory, dissipated_j) in updates {
            let bound = self.entries[i].as_mut().expect("bound material update");
            bound.memory = memory;
            bound.dissipated_j = dissipated_j;
        }
        self.steps = steps;
        self.dissipated_j = dissipated;
        Ok(())
    }
}

impl Solver {
    /// Atomically install complete membrane histories on the retained triangle ownership map.
    /// Same-triangle Maxwell and permanent-strain branches require a common source-qualified
    /// parallel-network lock. An incomplete joint network cannot escape as an executable solver.
    /// Independent laws on different triangles retain their original initialization and arithmetic.
    ///
    /// # Errors
    /// Incomplete/mismatched joint laws, unqualified same-triangle combinations, rebinding, or any
    /// original branch's topology, tangent, strain-domain or compression-mapping refusal.
    pub fn with_membrane_histories(
        self,
        relaxation: &[Option<CompiledRelaxation>],
        plasticity: &[Option<PlasticMembrane>],
    ) -> Result<Self, Refusal> {
        if relaxation.len() != self.model.membranes.len() || plasticity.len() != relaxation.len() {
            return Err(bad(
                "one complete membrane-history selection per retained triangle is required",
            ));
        }
        for (relaxation, plasticity) in relaxation.iter().zip(plasticity) {
            let admitted = match (relaxation, plasticity) {
                (Some(relaxation), Some(plasticity)) => plasticity.accepts_parallel(relaxation),
                (Some(relaxation), None) => relaxation.parallel_material_sha256().is_none(),
                (None, Some(plasticity)) => plasticity.parallel_material_sha256().is_none(),
                (None, None) => true,
            };
            if !admitted {
                return Err(bad(
                    "each parallel membrane network requires both branches from the same complete material and common domain",
                ));
            }
        }
        self.bind_relaxation(relaxation)?
            .bind_membrane_plasticity(plasticity)
    }

    /// Bind one optional permanent-set law per retained material triangle after grain mapping
    /// and welding. Selected triangles require the exact linear tensor, unit compression factors,
    /// and no Maxwell/nonlinear membrane response. Other triangles retain their own laws.
    /// Initialization is pristine; cloning and motion continuation preserve accepted history.
    ///
    /// # Errors
    /// Rebinding, topology mismatch, unsupported coupled laws, softened compression/gathers,
    /// a different elastic tensor or an initial physical strain outside the declared domain.
    pub fn with_membrane_plasticity(
        self,
        laws: &[Option<PlasticMembrane>],
    ) -> Result<Self, Refusal> {
        if laws
            .iter()
            .flatten()
            .any(|law| law.parallel_material_sha256().is_some())
        {
            return Err(bad(
                "a parallel plastic half-law requires atomic with_membrane_histories binding",
            ));
        }
        self.bind_membrane_plasticity(laws)
    }

    pub(super) fn bind_membrane_plasticity(
        mut self,
        laws: &[Option<PlasticMembrane>],
    ) -> Result<Self, Refusal> {
        if self.membrane_plasticity.is_some() || laws.len() != self.model.membranes.len() {
            return Err(bad(
                "one permanent-set selector per triangle is required; history cannot be rebound",
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
        let entries = laws.iter().enumerate().map(|(i, law)| {
            let Some(law) = law else { return Ok(None); };
            let material = self.model.material(i);
            material.validate()?;
            if material.compression != [1., 1.] {
                return Err(bad("plastic membrane uses physical Green strain; selected triangles require compression=1 and no gather-softening zone"));
            }
            if material.q != law.tensor() || self.model.membrane_response(i).is_some() {
                return Err(bad("plastic membrane requires its exact linear elastic tensor without a nonlinear equilibrium law"));
            }
            if self.relaxation.as_ref().is_some_and(|field|
                field.has(i) && !field.admits_plastic(i, law)) {
                return Err(bad("plastic membrane and Maxwell memory need a jointly characterized law on the same triangle"));
            }
            let element = self.model.membranes[i];
            if !element.area.is_finite()
                || element.area <= 0.0
                || element.g.iter().flatten().any(|v| !v.is_finite())
            {
                return Err(bad("invalid permanent-stretch reference triangle"));
            }
            Ok(Some(Bound { element, material, domain: self.model.membrane_domain(i), law: law.clone(), memory: law.pristine(), dissipated_j: 0. }))
        }).collect::<Result<Vec<_>, Refusal>>()?;
        let field = Field {
            entries,
            dissipated_j: 0.,
            steps: 0,
        };
        field.check(&self.model, &self.x)?;
        self.membrane_plasticity = Some(field);
        Ok(self)
    }

    /// Actual stored membrane energy, including accepted permanent-set hardening and Maxwell
    /// branch storage, including qualified parallel networks. Excludes bending, contact, inertia
    /// and dissipated work.
    /// This is not the incremental potential minimized during a trial step.
    ///
    /// # Errors
    /// Changed material/topology, invalid current strain or non-finite stored energy.
    pub fn membrane_energy(&self) -> Result<f64, Refusal> {
        self.model.check_membrane_domains(&self.x)?;
        let mut energy = if let Some(field) = &self.membrane_plasticity {
            field.check(&self.model, &self.x)?;
            field.energy(&self.model, &self.x, false)?
        } else {
            self.model
                .membranes
                .iter()
                .enumerate()
                .map(|(i, e)| {
                    self.model.membrane_response(i).map_or_else(
                        || e.energy(&self.x, &self.model.material(i)),
                        |law| e.response_energy(&self.x, law, &self.model.material(i)),
                    )
                })
                .sum()
        };
        if let Some(field) = &self.relaxation {
            field.check(&self.model, &self.x)?;
            energy += field.energy(&self.x, None)?;
        }
        if !energy.is_finite() {
            return Err(bad("stored membrane energy overflows"));
        }
        Ok(energy)
    }

    /// Cumulative accepted irreversible membrane work in joules, in retained triangle order.
    /// Unselected triangles contribute zero; absence means no permanent membrane law is bound.
    /// Values include initial forming/drape history. A later heat consumer must retain its own
    /// initial baseline and consume only new accepted work, with explicit conversion qualification.
    /// This read never advances memory or changes the caller's state.
    ///
    /// # Errors
    /// Stale material/topology, invalid current strain, or non-finite accepted dissipation.
    pub fn membrane_plastic_dissipation_j(&self) -> Result<Option<Vec<f64>>, Refusal> {
        let Some(field) = &self.membrane_plasticity else {
            return Ok(None);
        };
        field.check(&self.model, &self.x)?;
        Ok(Some(
            field
                .entries
                .iter()
                .map(|bound| bound.as_ref().map_or(0., |b| b.dissipated_j))
                .collect(),
        ))
    }

    /// Report accepted permanent Green strain, directional accumulation and plastic dissipation
    /// for selected triangles. This does not import a restart state or supply a thermal heat source.
    ///
    /// # Errors
    /// Stale material/topology, invalid strain or non-finite material storage.
    pub fn membrane_plasticity_report(&self) -> Result<Option<Value>, Refusal> {
        let Some(field) = &self.membrane_plasticity else {
            return Ok(None);
        };
        field.check(&self.model, &self.x)?;
        let mut elements = Vec::new();
        let mut stored = 0.;
        for (i, b) in field.entries.iter().enumerate() {
            if let Some(b) = b {
                let strain = b.strain(&self.x)?;
                let response = b.memory.response(strain, false)?;
                let energy = b.element.area * response.energy;
                stored += energy;
                let mut element = json!({"triangle":i,"vertices":b.element.v,
                    "reference_area_m2":b.element.area,"law_sha256":b.law.fingerprint(),
                    "plastic_green_strain":b.memory.plastic_strain(),
                    "accumulated_plastic_green_strain":b.memory.accumulated_strain(),
                    "current_green_strain":strain,"elastic_stress_n_m":response.stress,
                    "plastic_dissipation_j":b.dissipated_j,
                    "stored_elastic_and_hardening_energy_j":energy});
                if let Some(identity) = b.law.parallel_material_sha256() {
                    element["parallel_network"] = json!({"model":ftx_pattern::material_relaxation::parallel::MODEL,
                        "material_sha256":identity,"stress_basis":"elastoplastic_equilibrium_branch_only"});
                }
                elements.push(element);
            }
        }
        if !stored.is_finite() {
            return Err(bad("plastic membrane storage overflows"));
        }
        let scope = if field
            .entries
            .iter()
            .flatten()
            .any(|b| b.law.parallel_material_sha256().is_some())
        {
            "Rate-independent orthotropic equilibrium-branch yield and hardening in physical Green strain, with source-qualified Maxwell branches in parallel where marked. Yield excludes Maxwell stress. Excludes same-branch viscoplasticity, damage, rupture, nonlinear equilibrium curves, gather softening, remeshing and thermal feedback"
        } else {
            "Rate-independent orthotropic yield and directional isotropic hardening in physical Green strain; excludes damage, rupture, simultaneous Maxwell/nonlinear membrane response, gather softening, remeshing and thermal feedback"
        };
        Ok(Some(
            json!({"schema":"ftx-membrane-plasticity-history/1","model":MODEL,
            "initialization":"pristine_relative_to_authored_reference_metric",
            "accepted_steps":field.steps,"stored_elastic_and_hardening_energy_j":stored,
            "plastic_dissipation_j":field.dissipated_j,"elements":elements,
            "scope":scope,
            "empirical_validation_claim":false,"restart_import":false}),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ftx_core::Vec2;

    #[test]
    fn parallel_network_geometric_derivatives_match_the_actual_incremental_potential() {
        use ftx_pattern::{
            material_catalog::Selection,
            materials::{CATALOG_SCHEMA, Catalog},
        };
        let selected: Selection = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/materials/synthetic-parallel-membrane.json"
        ))
        .unwrap();
        let material = Catalog {
            schema: CATALOG_SCHEMA.into(),
            materials: Vec::new(),
        }
        .resolve(&selected)
        .unwrap();
        let model = ClothModel::new(
            vec![Vec2::ZERO, Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0)],
            &[[0, 1, 2]],
            Membrane::orthotropic(1000.0, 600.0, 0.0, 300.0, 1.0),
            0.0,
            0.2,
        )
        .unwrap();
        let x = vec![
            Vec3::ZERO,
            Vec3::new(1.15, 0.05, 0.08),
            Vec3::new(0.035, 1.0, 0.0),
        ];
        let solver = Solver::new(model, x.clone(), &[[false; 3]; 3], Vec3::ZERO)
            .with_membrane_histories(
                &[material.membrane_relaxation().unwrap()],
                &[material.membrane_plasticity().unwrap()],
            )
            .unwrap();
        let plastic = solver.membrane_plasticity_report().unwrap();
        let relaxation = solver.relaxation_report().unwrap();
        let (gradient, hessian) = solver.assemble(&x, Some((&x, 0.2)), false);
        let eps = 2e-6;
        for i in 0..9 {
            let mut plus = x.clone();
            let mut minus = x.clone();
            let offset = match i % 3 {
                0 => Vec3::new(eps, 0.0, 0.0),
                1 => Vec3::new(0.0, eps, 0.0),
                _ => Vec3::new(0.0, 0.0, eps),
            };
            plus[i / 3] += offset;
            minus[i / 3] -= offset;
            let numeric = (solver.potential(&plus, Some((&x, 0.2))).unwrap()
                - solver.potential(&minus, Some((&x, 0.2))).unwrap())
                / (2.0 * eps);
            assert!(
                (numeric - gradient[i]).abs() < 2e-6 * gradient[i].abs().max(1.0),
                "gradient {i}: {numeric} vs {}",
                gradient[i]
            );
            let gp = solver.assemble(&plus, Some((&x, 0.2)), false).0;
            let gm = solver.assemble(&minus, Some((&x, 0.2)), false).0;
            for j in 0..9 {
                let numeric = (gp[j] - gm[j]) / (2.0 * eps);
                let exact = hessian.get(j, i);
                assert!(
                    (numeric - exact).abs() < 2e-6 * exact.abs().max(1.0),
                    "Hessian {j},{i}: {numeric} vs {exact}"
                );
            }
        }
        assert_eq!(plastic, solver.membrane_plasticity_report().unwrap());
        assert_eq!(relaxation, solver.relaxation_report().unwrap());
    }

    #[test]
    fn condensed_geometric_derivatives_match_the_actual_incremental_potential() {
        // Independent mixed active case: warp and shear yield; the coupled weft stays elastic.
        let q = [[1100., 140., 0.], [140., 650., 0.], [0., 0., 380.]];
        let material = Membrane {
            q,
            compression: [1., 1.],
            transition: 1e-3,
        };
        let model = ClothModel::new(
            vec![Vec2::new(0., 0.), Vec2::new(1., 0.), Vec2::new(0., 1.)],
            &[[0, 1, 2]],
            material,
            0.,
            0.2,
        )
        .unwrap();
        let x = vec![
            Vec3::ZERO,
            Vec3::new(1.15, 0.05, 0.08),
            Vec3::new(0.035, 1., 0.),
        ];
        let law = PlasticMembrane::new(
            q,
            [25., 30., 8.],
            [70., 40., 30.],
            [0.35, 0.35, 0.4],
            [0.25, 0.25, 0.3],
        )
        .unwrap();
        let solver = Solver::new(model, x.clone(), &[[false; 3]; 3], Vec3::ZERO)
            .with_membrane_plasticity(&[Some(law)])
            .unwrap();
        let before = solver.membrane_plasticity_report().unwrap();
        let (gradient, hessian) = solver.assemble(&x, Some((&x, 0.2)), false);
        let eps = 2e-6;
        for i in 0..9 {
            let mut plus = x.clone();
            let mut minus = x.clone();
            let offset = match i % 3 {
                0 => Vec3::new(eps, 0., 0.),
                1 => Vec3::new(0., eps, 0.),
                _ => Vec3::new(0., 0., eps),
            };
            plus[i / 3] += offset;
            minus[i / 3] -= offset;
            let numeric = (solver.potential(&plus, Some((&x, 0.2))).unwrap()
                - solver.potential(&minus, Some((&x, 0.2))).unwrap())
                / (2. * eps);
            assert!(
                (numeric - gradient[i]).abs() < 2e-6 * gradient[i].abs().max(1.),
                "gradient {i}"
            );
            let gp = solver.assemble(&plus, Some((&x, 0.2)), false).0;
            let gm = solver.assemble(&minus, Some((&x, 0.2)), false).0;
            for j in 0..9 {
                let numeric = (gp[j] - gm[j]) / (2. * eps);
                let exact = hessian.get(j, i);
                assert!(
                    (numeric - exact).abs() < 2e-6 * exact.abs().max(1.),
                    "Hessian {j},{i}: {numeric} vs {exact}"
                );
            }
        }
        assert_eq!(before, solver.membrane_plasticity_report().unwrap());
    }
}

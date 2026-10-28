//! Evaporation-only reference-area liquid inertia with explicitly qualified frozen mechanics.
//! Mass leaves at the accepted endpoint velocity: no rocket impulse, incoming liquid, drainage,
//! wet-law extrapolation or inferred conversion of kinetic energy into heat is introduced.

use std::collections::BTreeMap;

use ftx_core::{Refusal, RefusalCode, Vec3, receipt::sha256_hex};
use ftx_pattern::material_catalog::{ResolvedMaterial, Selection};
use ftx_pattern::materials::{Evidence, Source};
use serde::{Deserialize, Serialize};

use crate::{
    ClothModel, Membrane, Solver,
    cloth::SheetMaterial,
    contact::{FrozenPolicy, SurfaceMaterial},
    run::DrapeRun,
};

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

/// Independently qualified invariant mechanical domain for one exact physical instance.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialDomain {
    /// Canonical normalized material hash, not a display name or a related dry specimen.
    pub material_sha256: String,
    /// Allowed co-moving free liquid is [0, maximum], kg per dry textile reference m².
    pub maximum_liquid_loading_kg_m2: f64,
    /// Inclusive temperatures over which the existing mechanics remain qualified, degrees Celsius.
    pub temperature_range_c: [f64; 2],
}

/// Source permission to retain the exact material and interface laws throughout evaporation.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Qualification {
    /// Must be `ftx-evaporating-liquid-mechanics/1`.
    pub schema: String,
    /// Must be `co_moving_evaporation_frozen_mechanics/1`.
    pub model: String,
    /// SHA-256 of the complete ordinary material report, including unlike-sheet interfaces.
    pub material_report_sha256: String,
    /// Every retained instance exactly once, including those starting dry.
    pub bindings: BTreeMap<String, MaterialDomain>,
    /// Independent evidence for retention and invariance of the exact mechanical/interface laws.
    pub source: Source,
    /// This closure's evidence class, separate from the underlying material selection policy.
    pub evidence: Evidence,
    /// Explicit consent when this independent qualification is an estimate.
    #[serde(default)]
    pub allow_estimates: bool,
    /// Measurement or modeling method for this closure.
    pub method: String,
    /// Basis for co-moving mass loss and invariant membrane, bending, contact and history laws.
    pub qualification: String,
}

impl Qualification {
    /// Validate structure before an expensive drape; binding later verifies actual source identities.
    ///
    /// # Errors
    /// Unsupported closure, incomplete source/domain, nonphysical bounds or missing estimate consent.
    pub fn validate(&self) -> Result<(), Refusal> {
        let text = |s: &str| !s.trim().is_empty() && s.len() <= 4096;
        let hash = |s: &str| {
            s.len() == 64
                && s.bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        };
        if self.schema != "ftx-evaporating-liquid-mechanics/1"
            || self.model != "co_moving_evaporation_frozen_mechanics/1"
            || !hash(&self.material_report_sha256)
            || self.bindings.is_empty()
            || self.bindings.len() > 65_536
            || !text(&self.source.reference)
            || !text(&self.source.note)
            || self.source.accessed_on.len() != 10
            || !text(&self.method)
            || !text(&self.qualification)
            || (self.evidence == Evidence::Estimate && !self.allow_estimates)
            || self.bindings.iter().any(|(id, d)| {
                let [lo, hi] = d.temperature_range_c;
                !text(id)
                    || !hash(&d.material_sha256)
                    || !d.maximum_liquid_loading_kg_m2.is_finite()
                    || d.maximum_liquid_loading_kg_m2 <= 0.
                    || !lo.is_finite()
                    || !hi.is_finite()
                    || lo < -273.15
                    || lo > hi
            })
        {
            return Err(invalid(
                "evaporating mechanics needs exact material/interface identities, independent source qualification, physical loading/temperature domains and estimate consent",
            ));
        }
        Ok(())
    }
}

/// Accepted mechanical liquid ledger; thermal sensible and latent energies remain separate.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// `ftx-evaporating-liquid-mechanics-result/1`.
    pub schema: &'static str,
    /// Exact independent qualification, retained for replay.
    pub qualification: Qualification,
    /// Original dry sheet plus independently attached accessories, kg.
    pub substrate_and_attached_mass_kg: f64,
    /// Liquid initialized co-moving at the declared post-drape state, kg.
    pub initial_liquid_mass_kg: f64,
    /// Current liquid in retained material triangles, kg.
    pub remaining_liquid_mass_kg: f64,
    /// Sum of initial minus current liquid on the same triangles, kg.
    pub evaporated_liquid_mass_kg: f64,
    /// Sum of the actual current solver mass vector, kg.
    pub total_inertial_mass_kg: f64,
    /// Actual solver mass minus original mass and current liquid, kg.
    pub mass_balance_residual_kg: f64,
    /// Momentum carried away at each accepted endpoint, kg·m/s.
    pub exported_momentum_kg_m_s: Vec3,
    /// Kinetic energy carried away at each accepted endpoint, J; not heat.
    pub exported_kinetic_energy_j: f64,
    /// Signed potential energy carried away, using -m g·x in the unchanged world frame, J.
    pub exported_gravitational_energy_j: f64,
    /// Accumulated numerical residual of remaining-plus-exported transfer momentum, kg·m/s.
    pub transfer_momentum_residual_kg_m_s: Vec3,
    /// Accumulated numerical residual of kinetic energy during mass transfer only, J.
    pub transfer_kinetic_residual_j: f64,
    /// Accumulated numerical residual of potential energy during mass transfer only, J.
    pub transfer_gravitational_residual_j: f64,
    /// Physical and numerical boundaries of this ledger.
    pub scope: &'static str,
}

impl Report {
    /// Compact accepted-step ledger without repeating the frozen source qualification.
    #[must_use]
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({
            "initial_liquid_kg": self.initial_liquid_mass_kg,
            "remaining_liquid_kg": self.remaining_liquid_mass_kg,
            "evaporated_liquid_kg": self.evaporated_liquid_mass_kg,
            "substrate_and_attached_mass_kg": self.substrate_and_attached_mass_kg,
            "total_inertial_mass_kg": self.total_inertial_mass_kg,
            "mass_balance_residual_kg": self.mass_balance_residual_kg,
            "exported_momentum_kg_m_s": self.exported_momentum_kg_m_s,
            "exported_kinetic_energy_j": self.exported_kinetic_energy_j,
            "exported_gravitational_energy_j": self.exported_gravitational_energy_j,
            "transfer_momentum_residual_kg_m_s": self.transfer_momentum_residual_kg_m_s,
            "transfer_kinetic_residual_j": self.transfer_kinetic_residual_j,
            "transfer_gravitational_residual_j": self.transfer_gravitational_residual_j,
        })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Binding {
    original: ClothModel,
    contact: Option<FrozenPolicy>,
    gravity: Vec3,
    history_kinds: [bool; 3],
    expected_mass: Vec<f64>,
    domains: Vec<MaterialDomain>,
    initial: Vec<f64>,
    remaining: Vec<f64>,
    report: Report,
}

impl Binding {
    fn check(&self, solver: &Solver) -> Result<(), Refusal> {
        if solver.model.mass != self.expected_mass
            || !solver.model.same_liquid_mechanics(&self.original)
            || solver.gravity != self.gravity
            || solver.liquid_history_kinds() != self.history_kinds
            || solver.x.len() != self.expected_mass.len()
            || solver.v.len() != self.expected_mass.len()
            || solver.free_dofs() != self.expected_mass.len() * 3
            || solver.x.iter().chain(&solver.v).any(|v| !v.is_finite())
            || match (&self.contact, &solver.contact) {
                (None, None) => false,
                (Some(a), Some(b)) => !a.matches(b),
                _ => true,
            }
        {
            return Err(invalid(
                "evaporating liquid cannot reuse changed inertia, material/rest geometry, contact policy, constraints or gravity",
            ));
        }
        solver.model.check_attached_mass()
    }

    fn admit(&self, liquid: &[f64], temperatures: &[Vec<f64>]) -> Result<Vec<f64>, Refusal> {
        if liquid.len() != self.remaining.len() || temperatures.len() != liquid.len() {
            return Err(invalid(
                "one liquid mass and temperature profile per retained triangle is required",
            ));
        }
        let mut mass = self.original.mass.clone();
        for (i, ((&water, cells), domain)) in liquid
            .iter()
            .zip(temperatures)
            .zip(&self.domains)
            .enumerate()
        {
            let e = &self.original.membranes[i];
            let maximum = domain.maximum_liquid_loading_kg_m2 * e.area;
            let [lo, hi] = domain.temperature_range_c;
            if !water.is_finite()
                || water < 0.
                || water > self.remaining[i]
                || !maximum.is_finite()
                || maximum <= 0.
                || water > maximum
                || cells.is_empty()
                || cells.len() > 64
                || cells.iter().any(|t| !t.is_finite() || *t < lo || *t > hi)
            {
                return Err(invalid(
                    "liquid may only evaporate within its source-qualified loading and temperature domain",
                ));
            }
            let share = water / 3.;
            for &v in &e.v {
                let next = mass[v] + share;
                if !next.is_finite() || (water > 0. && (share <= 0. || next <= mass[v])) {
                    return Err(invalid(
                        "liquid inertia is not representable on the retained welded vertices",
                    ));
                }
                mass[v] = next;
            }
        }
        Ok(mass)
    }

    fn update_report(&mut self) -> Result<(), Refusal> {
        let r = &mut self.report;
        r.remaining_liquid_mass_kg = self.remaining.iter().sum();
        r.evaporated_liquid_mass_kg = self
            .initial
            .iter()
            .zip(&self.remaining)
            .map(|(a, b)| a - b)
            .sum();
        r.total_inertial_mass_kg = self.expected_mass.iter().sum();
        r.mass_balance_residual_kg = r.total_inertial_mass_kg
            - r.substrate_and_attached_mass_kg
            - r.remaining_liquid_mass_kg;
        if [
            r.remaining_liquid_mass_kg,
            r.evaporated_liquid_mass_kg,
            r.total_inertial_mass_kg,
            r.mass_balance_residual_kg,
            r.exported_kinetic_energy_j,
            r.exported_gravitational_energy_j,
            r.transfer_kinetic_residual_j,
            r.transfer_gravitational_residual_j,
        ]
        .iter()
        .any(|v| !v.is_finite())
            || !r.exported_momentum_kg_m_s.is_finite()
            || !r.transfer_momentum_residual_kg_m_s.is_finite()
        {
            return Err(invalid(
                "evaporating mechanical mass, momentum or energy ledger overflows",
            ));
        }
        Ok(())
    }
}

/// Initialize co-moving liquid at the declared post-drape state, without claiming wet equilibrium.
/// Inputs follow the existing retained membrane/thermal-patch order and dry reference area.
///
/// # Errors
/// Missing/stale sources, unqualified wet domains, pre-existing wet mass, fixed DOFs, stale
/// attachments or unrepresentable mass. Failure leaves the complete solver unchanged.
pub fn bind(
    run: &mut DrapeRun,
    qualification: &Qualification,
    initial_liquid_kg: &[f64],
    temperatures_c: &[Vec<f64>],
) -> Result<Report, Refusal> {
    qualification.validate()?;
    let material = &run.report["material"];
    if sha256_hex(&serde_json::to_vec(material).map_err(|e| invalid(e.to_string()))?)
        != qualification.material_report_sha256
        || run.scene.mesh.instances.len() != qualification.bindings.len()
        || run
            .scene
            .mesh
            .instances
            .iter()
            .any(|id| !qualification.bindings.contains_key(id))
    {
        return Err(invalid(
            "evaporating mechanics qualification differs from the complete material/interface/instance authority",
        ));
    }
    let (records, assignments) = crate::thermal::surface::io::retained_materials(
        material,
        &run.scene.mesh.instances,
        &run.scene.triangle_owner,
    )?;
    let solver = &mut run.scene.solver;
    if assignments.len() != solver.model.membranes.len()
        || solver.model.compression.len() != assignments.len()
        || solver.model.mass.len() != solver.model.rest.len()
        || solver.model.membranes.iter().any(|e| {
            !e.area.is_finite() || e.area <= 0. || e.v.iter().any(|v| *v >= solver.model.mass.len())
        })
    {
        return Err(invalid(
            "liquid source ownership differs from retained mechanics",
        ));
    }
    let mut domains = Vec::with_capacity(assignments.len());
    let mut sheets = Vec::with_capacity(assignments.len());
    let mut bending = Vec::with_capacity(assignments.len());
    let mut dry_mass = vec![0.; solver.model.mass.len()];
    for (i, &column) in assignments.iter().enumerate() {
        let record = &records[column];
        record.material().require_dry_mass_transport()?;
        let id = &run.scene.mesh.instances[run.scene.triangle_owner[i]];
        let domain = &qualification.bindings[id];
        let mechanics = record.mechanics()?;
        let reference = Membrane::orthotropic(
            mechanics.warp_n_m,
            mechanics.weft_n_m,
            mechanics.poisson,
            mechanics.shear_n_m,
            1.,
        );
        let e = &solver.model.membranes[i];
        if domain.material_sha256 != record.sha256()
            || solver.model.material(i).q != reference.q
            || solver.model.material(i).transition != reference.transition
            || solver.model.membrane_response(i) != record.membrane_response()?.as_ref()
        {
            return Err(invalid(
                "liquid mechanics differs from the qualified material law",
            ));
        }
        sheets.push(SheetMaterial {
            membrane: reference,
            bending_nm: mechanics.bending_nm,
            areal_kg_m2: mechanics.areal_kg_m2,
        });
        bending.push(record.bending_response()?);
        for &v in &e.v {
            dry_mass[v] += mechanics.areal_kg_m2 * e.area / 3.;
        }
        domains.push(domain.clone());
    }
    solver.model.check_source_bending(&sheets, &bending)?;
    check_source_contact(solver, material, &records, &assignments)?;
    if let Some(attached) = solver.model.attached_mass_report()? {
        for group in &attached.groups {
            for target in &group.targets {
                dry_mass[target.solver_vertex] += target.mass_kg;
            }
        }
    }
    if dry_mass != solver.model.mass {
        return Err(invalid(
            "original mechanical mass differs from source dry sheets and retained accessories",
        ));
    }
    start(
        solver,
        qualification.clone(),
        domains,
        initial_liquid_kg,
        temperatures_c,
    )
}

fn check_source_contact(
    solver: &Solver,
    material: &serde_json::Value,
    records: &[ResolvedMaterial],
    assignments: &[usize],
) -> Result<(), Refusal> {
    let contact = solver
        .contact
        .as_ref()
        .ok_or_else(|| invalid("liquid mechanics requires its source-bound drape contact"))?;
    if contact.cloth.triangles.len() != solver.model.membranes.len()
        || contact
            .cloth
            .triangles
            .iter()
            .zip(&solver.model.membranes)
            .any(|(triangle, e)| *triangle != e.v)
    {
        return Err(invalid(
            "liquid contact topology differs from the retained sheet",
        ));
    }
    let enabled = material["self_contact_enabled"]
        .as_bool()
        .ok_or_else(|| invalid("missing qualified self-contact policy"))?;
    let per_piece = material["basis"] == "per_piece_characterization";
    let hashes: Vec<String> = if per_piece {
        serde_json::from_value(material["contact_material_order"].clone())
            .map_err(|e| invalid(format!("qualified contact identities: {e}")))?
    } else {
        vec![records[0].sha256().into()]
    };
    if hashes.is_empty() || hashes.len() > 64 || hashes.windows(2).any(|h| h[0] >= h[1]) {
        return Err(invalid("invalid qualified contact material order"));
    }
    let mut mechanics = Vec::with_capacity(hashes.len());
    for hash in &hashes {
        if let Some(record) = records.iter().find(|r| r.sha256() == hash) {
            mechanics.push(record.mechanics()?);
            continue;
        }
        // Unplaced pieces still contribute to the canonical contact envelope and interface table.
        // Resolve their frozen snapshots as well; a live catalog or displayed coefficient is not
        // an authority for the source law that this qualification permits us to retain.
        let receipt = &material["records"][hash]["record"];
        let authored: ftx_pattern::materials::Material =
            serde_json::from_value(receipt["authored"].clone())
                .map_err(|e| invalid(format!("qualified contact material: {e}")))?;
        let record = ftx_pattern::materials::Catalog {
            schema: ftx_pattern::materials::CATALOG_SCHEMA.into(),
            materials: vec![],
        }
        .resolve(&Selection {
            id: authored.id.clone(),
            record: Some(authored),
            allow_estimates: receipt["allow_estimates"]
                .as_bool()
                .ok_or_else(|| invalid("missing contact material estimate policy"))?,
            expected_sha256: Some(hash.clone()),
            ..Selection::default()
        })?;
        if serde_json::to_value(&record).map_err(|e| invalid(e.to_string()))? != *receipt {
            return Err(invalid(
                "qualified contact material is not its canonical source",
            ));
        }
        mechanics.push(record.mechanics()?);
    }
    let triangle_ids = assignments
        .iter()
        .map(|&i| {
            hashes
                .binary_search_by(|hash| hash.as_str().cmp(records[i].sha256()))
                .map_err(|_| invalid("retained sheet is missing from qualified contact identities"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let surfaces: Vec<_> = mechanics
        .iter()
        .map(|m| SurfaceMaterial {
            half_thickness_m: 0.5 * m.thickness_m,
            friction_body: m.friction_skin,
        })
        .collect();
    let friction: Option<Vec<Vec<f64>>> = if per_piece {
        serde_json::from_value(material["self_friction_matrix"].clone())
            .map_err(|e| invalid(format!("qualified contact interfaces: {e}")))?
    } else {
        enabled.then(|| vec![vec![mechanics[0].friction_self]])
    };
    if enabled != friction.is_some()
        || friction.as_ref().is_some_and(|matrix| {
            mechanics
                .iter()
                .enumerate()
                .any(|(i, m)| matrix.get(i).and_then(|row| row.get(i)) != Some(&m.friction_self))
        })
    {
        return Err(invalid(
            "qualified contact interfaces differ from their source sheets",
        ));
    }
    contact.check_source_materials(
        solver.model.mass.len(),
        &triangle_ids,
        &surfaces,
        friction.as_deref(),
    )
}

fn start(
    solver: &mut Solver,
    qualification: Qualification,
    domains: Vec<MaterialDomain>,
    initial: &[f64],
    temperatures: &[Vec<f64>],
) -> Result<Report, Refusal> {
    solver.model.check_attached_mass()?;
    if solver.liquid_mass.is_some()
        || solver.model.membranes.is_empty()
        || initial.len() != solver.model.membranes.len()
        || domains.len() != initial.len()
        || solver.free_dofs() != solver.model.mass.len() * 3
        || solver.model.mass.iter().any(|m| !m.is_finite() || *m <= 0.)
        || !solver.gravity.is_finite()
    {
        return Err(invalid(
            "liquid inertia requires pristine, finite, unconstrained sheet dynamics and complete triangle ownership",
        ));
    }
    let initial_total: f64 = initial.iter().sum();
    let substrate: f64 = solver.model.mass.iter().sum();
    if !initial_total.is_finite() || !substrate.is_finite() {
        return Err(invalid("initial liquid or substrate mass overflows"));
    }
    let mut binding = Binding {
        original: solver.model.clone(),
        contact: solver.contact.as_ref().map(|c| c.frozen_policy()),
        gravity: solver.gravity,
        history_kinds: solver.liquid_history_kinds(),
        expected_mass: solver.model.mass.clone(),
        domains,
        initial: initial.to_vec(),
        remaining: initial.to_vec(),
        report: Report {
            schema: "ftx-evaporating-liquid-mechanics-result/1",
            qualification,
            substrate_and_attached_mass_kg: substrate,
            initial_liquid_mass_kg: initial_total,
            remaining_liquid_mass_kg: initial_total,
            evaporated_liquid_mass_kg: 0.,
            total_inertial_mass_kg: substrate,
            mass_balance_residual_kg: 0.,
            exported_momentum_kg_m_s: Vec3::ZERO,
            exported_kinetic_energy_j: 0.,
            exported_gravitational_energy_j: 0.,
            transfer_momentum_residual_kg_m_s: Vec3::ZERO,
            transfer_kinetic_residual_j: 0.,
            transfer_gravitational_residual_j: 0.,
            scope: "Source-qualified frozen mechanics with co-moving evaporation only. Initial liquid is an authored post-drape state, not wet equilibrium. Endpoint mass removal preserves velocity and exports its momentum, kinetic and signed gravitational energy. No incoming flow, drainage, recoil, damage, mixing heat, constrained-DOF impulse, full time-integration energy conservation or empirical wet-garment claim.",
        },
    };
    binding.check(solver)?;
    binding.expected_mass = binding.admit(initial, temperatures)?;
    binding.update_report()?;
    solver.accept_liquid_binding(binding);
    solver
        .liquid_mass_report()?
        .ok_or_else(|| invalid("missing initialized liquid ledger"))
}

impl Solver {
    fn accept_liquid_binding(&mut self, binding: Binding) {
        if self.model.mass != binding.expected_mass {
            self.invalidate_mass_preconditioner();
        }
        self.model.accept_liquid_mass(
            binding.expected_mass.clone(),
            binding.report.remaining_liquid_mass_kg,
        );
        self.liquid_mass = Some(binding);
    }

    pub(crate) fn check_liquid_mass(&self) -> Result<(), Refusal> {
        self.liquid_mass.as_ref().map_or(Ok(()), |b| b.check(self))
    }

    /// Read the ledger only when mass, mechanics, attachments and contact still match the binding.
    ///
    /// # Errors
    /// The public low-level solver/model fields were changed incompatibly after admission.
    pub fn liquid_mass_report(&self) -> Result<Option<Report>, Refusal> {
        self.check_liquid_mass()?;
        Ok(self.liquid_mass.as_ref().map(|b| b.report.clone()))
    }

    /// Remove the exact thermal kernel's evaporated liquid after an accepted mechanical substep.
    /// Its one-third triangle shares leave at each welded vertex's CURRENT velocity, so no recoil
    /// is introduced. Original dry and accessory mass are reconstructed rather than subtracted.
    ///
    /// # Errors
    /// Any mass increase, source-domain escape, stale binding or arithmetic loss. Failure preserves
    /// mass, velocity, material/contact history, numerical caches and every ledger field.
    pub fn evaporate_liquid(
        &mut self,
        remaining_liquid_kg: &[f64],
        temperatures_c: &[Vec<f64>],
    ) -> Result<Report, Refusal> {
        self.check_liquid_mass()?;
        let mut binding = self
            .liquid_mass
            .clone()
            .ok_or_else(|| invalid("liquid mechanics is not initialized"))?;
        let next = binding.admit(remaining_liquid_kg, temperatures_c)?;
        let mut exported = vec![0.; next.len()];
        for (i, (&old, &new)) in binding
            .remaining
            .iter()
            .zip(remaining_liquid_kg)
            .enumerate()
        {
            let share = (old - new) / 3.;
            if old > new && share <= 0. {
                return Err(invalid("evaporated triangle mass is not representable"));
            }
            for &v in &binding.original.membranes[i].v {
                exported[v] += share;
            }
        }
        for (i, (&new, &lost)) in next.iter().zip(&exported).enumerate() {
            let old = self.model.mass[i];
            if !lost.is_finite() || (lost > 0. && new >= old) || new > old {
                return Err(invalid(
                    "evaporated mass is lost by welded inertia arithmetic",
                ));
            }
            let v = self.v[i];
            let half_speed2 = 0.5 * v.norm2();
            let potential = -self.gravity.dot(self.x[i]);
            let residual = new - old + lost;
            let r = &mut binding.report;
            r.exported_momentum_kg_m_s += v * lost;
            r.exported_kinetic_energy_j += lost * half_speed2;
            r.exported_gravitational_energy_j += lost * potential;
            r.transfer_momentum_residual_kg_m_s += v * residual;
            r.transfer_kinetic_residual_j += residual * half_speed2;
            r.transfer_gravitational_residual_j += residual * potential;
        }
        binding.expected_mass = next;
        binding.remaining = remaining_liquid_kg.to_vec();
        binding.update_report()?;
        let report = binding.report.clone();
        self.accept_liquid_binding(binding);
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::{ELASTIC_ONSET, ElasticSegment};
    use ftx_core::Vec2;
    use serde_json::json;

    fn qualification() -> Qualification {
        serde_json::from_value(json!({
            "schema":"ftx-evaporating-liquid-mechanics/1",
            "model":"co_moving_evaporation_frozen_mechanics/1",
            "material_report_sha256":"a".repeat(64),
            "bindings":{"sheet":{"material_sha256":"b".repeat(64),
                "maximum_liquid_loading_kg_m2":10.,"temperature_range_c":[0.,50.]}},
            "source":{"reference":"test:evaporating-mass","accessed_on":"2026-10-28",
                "note":"Independent synthetic numerical controls, no physical textile"},
            "evidence":"estimate","allow_estimates":true,"method":"Analytic variable-mass mechanics",
            "qualification":"Artificial invariant constitutive, contact and co-moving outflow laws"
        })).unwrap()
    }

    fn solver() -> Solver {
        let rest = [
            Vec2::ZERO,
            Vec2::new(1., 0.),
            Vec2::new(0., 1.),
            Vec2::new(1., -2.),
        ];
        let tris = [[0, 1, 2], [1, 0, 3]];
        let model = ClothModel::from_triangles(
            4,
            &tris,
            &tris.map(|t| t.map(|v| rest[v])),
            Membrane::orthotropic(8., 2., 0.2, 1., 1.),
            0.,
            0.2,
        )
        .unwrap();
        Solver::new(
            model,
            rest.iter().map(|p| Vec3::new(p.x, p.y, 2.)).collect(),
            &[[false; 3]; 4],
            Vec3::new(0., 0., -9.81),
        )
    }

    fn initialize(solver: &mut Solver, masses: &[f64]) -> Report {
        let q = qualification();
        let domains = vec![q.bindings["sheet"].clone(); masses.len()];
        start(solver, q, domains, masses, &vec![vec![20.]; masses.len()]).unwrap()
    }

    fn source_run() -> (DrapeRun, Qualification) {
        let raw = serde_json::from_str(include_str!(
            "../../../tests/fixtures/materials/synthetic-scenario-pattern.json"
        ))
        .unwrap();
        let material = crate::material::resolve(
            &ftx_pattern::Pattern::new(raw),
            &crate::run::DrapeOptions::default(),
        )
        .unwrap();
        let mut s = solver();
        s.model
            .set_materials(
                &[SheetMaterial {
                    membrane: material.fabric.membrane(1.),
                    bending_nm: material.fabric.bending_nm,
                    areal_kg_m2: material.fabric.areal_kg_m2,
                }; 2],
            )
            .unwrap();
        let s = s
            .with_contact(crate::contact::Body::empty(), material.options.contact)
            .unwrap();
        let mut q = qualification();
        q.material_report_sha256 = sha256_hex(&serde_json::to_vec(&material.report).unwrap());
        q.bindings.get_mut("sheet").unwrap().material_sha256 =
            material.report["record_sha256"].as_str().unwrap().into();
        let mesh = crate::garment::GarmentMesh {
            instances: vec!["sheet".into()],
            verts2d: s.model.rest.iter().map(|v| *v * 1000.).collect(),
            faces: s.model.membranes.iter().map(|e| e.v).collect(),
            owner: vec![0; 4],
            inst_range: vec![(0, 4)],
            pairs: vec![],
            seams: vec![],
            corner: BTreeMap::new(),
            index: BTreeMap::new(),
        };
        let run = DrapeRun {
            scene: crate::scene::Scene {
                mesh,
                placements: vec![],
                start: crate::start::StartState {
                    wrapped: s.x.clone(),
                    presewn: s.x.clone(),
                    groups: vec![],
                    relaxed: s.x.clone(),
                    relax_iterations: 0,
                    block_p95: vec![],
                    gather_weights: vec![1.; 4],
                },
                weld: (0..4).collect(),
                collapsed: 0,
                triangle_owner: vec![0; 2],
                untangled: vec![],
                target_thickness: material.options.contact.thickness,
                solver: s,
                elastic_rings: vec![],
                elastic_groups: vec![],
            },
            fold: crate::animation::FoldUp::default(),
            report: json!({"material":material.report}),
            sew: crate::scene::PhaseReport::default(),
            drape: crate::scene::PhaseReport::default(),
            fabric: material.fabric,
        };
        (run, q)
    }

    #[test]
    fn first_binding_refuses_preexisting_bending_and_contact_source_substitution() {
        let (mut run, q) = source_run();
        let original = run.scene.solver.clone();
        for kind in 0..6 {
            run.scene.solver = original.clone();
            let s = &mut run.scene.solver;
            match kind {
                0 => s.model.bending *= 2.,
                1 => s.model.membrane.transition *= 2.,
                2 => s.contact.as_mut().unwrap().params.mu *= 2.,
                3 => s.contact.as_mut().unwrap().params.thickness *= 2.,
                4 => s.contact = None,
                _ => {
                    let contact = s.contact.take().unwrap();
                    let params = contact.params;
                    s.contact = Some(contact.with_self_contact(&s.x, params).unwrap());
                }
            }
            let before = format!("{:?}", run.scene.solver);
            assert!(bind(&mut run, &q, &[0.12, 0.3], &[vec![20.], vec![20.]]).is_err());
            assert_eq!(format!("{:?}", run.scene.solver), before);
        }
        run.scene.solver = original;
        bind(&mut run, &q, &[0.12, 0.3], &[vec![20.], vec![20.]]).unwrap();
        assert_eq!(
            run.scene
                .solver
                .liquid_mass_report()
                .unwrap()
                .unwrap()
                .initial_liquid_mass_kg,
            0.42
        );
    }

    #[test]
    fn nonuniform_triangle_loss_conserves_welded_mass_momentum_and_boundary_energy() {
        let mut s = solver();
        s.v = vec![
            Vec3::new(1., 2., -3.),
            Vec3::new(-2., 1., 0.),
            Vec3::new(0., 0., 4.),
            Vec3::new(2., -3., 1.),
        ];
        let dry = s.model.mass.clone();
        initialize(&mut s, &[0.12, 0.3]);
        let initial_mass = s.model.mass.clone();
        for (i, extra) in [0.14, 0.14, 0.04, 0.1].into_iter().enumerate() {
            assert!((initial_mass[i] - dry[i] - extra).abs() < 1e-15);
        }
        let velocity = s.v.clone();
        let before_p = initial_mass
            .iter()
            .zip(&velocity)
            .fold(Vec3::ZERO, |p, (m, v)| p + *v * *m);
        let before_k: f64 = initial_mass
            .iter()
            .zip(&velocity)
            .map(|(m, v)| 0.5 * m * v.norm2())
            .sum();
        let report = s
            .evaporate_liquid(&[0.06, 0.1], &[vec![25.], vec![30., 35.]])
            .unwrap();
        assert_eq!(s.v, velocity);
        let lost = [0.08 + 2. / 300., 0.08 + 2. / 300., 0.02, 2. / 30.];
        let mut expected_p = Vec3::ZERO;
        let mut expected_k = 0.;
        for i in 0..4 {
            assert!((initial_mass[i] - s.model.mass[i] - lost[i]).abs() < 1e-15);
            expected_p += velocity[i] * lost[i];
            expected_k += 0.5 * lost[i] * velocity[i].norm2();
        }
        assert!((report.exported_momentum_kg_m_s - expected_p).norm() < 1e-15);
        assert!((report.exported_kinetic_energy_j - expected_k).abs() < 1e-15);
        assert!((report.exported_gravitational_energy_j - 0.26 * 9.81 * 2.).abs() < 1e-14);
        let after_p = s
            .model
            .mass
            .iter()
            .zip(&velocity)
            .fold(Vec3::ZERO, |p, (m, v)| p + *v * *m);
        let after_k: f64 = s
            .model
            .mass
            .iter()
            .zip(&velocity)
            .map(|(m, v)| 0.5 * m * v.norm2())
            .sum();
        assert!((after_p + report.exported_momentum_kg_m_s - before_p).norm() < 1e-15);
        assert!((after_k + report.exported_kinetic_energy_j - before_k).abs() < 1e-15);
        assert!(report.mass_balance_residual_kg.abs() < 1e-15);
        assert!(report.transfer_momentum_residual_kg_m_s.norm() < 1e-15);
        s.evaporate_liquid(&[0., 0.], &[vec![20.], vec![20.]])
            .unwrap();
        assert_eq!(s.model.mass, dry);
    }

    #[test]
    fn refused_mass_source_domains_and_modified_mechanics_preserve_complete_state() {
        let mut s = solver();
        initialize(&mut s, &[0.12, 0.3]);
        for (masses, temperatures) in [
            (vec![0.13, 0.3], vec![vec![20.], vec![20.]]),
            (vec![-0.1, 0.3], vec![vec![20.], vec![20.]]),
            (vec![0.1, 0.3], vec![vec![51.], vec![20.]]),
            (vec![0.1, 0.3], vec![vec![20.], vec![]]),
            (vec![0.1], vec![vec![20.]]),
        ] {
            let before = format!("{s:?}");
            assert!(s.evaporate_liquid(&masses, &temperatures).is_err());
            assert_eq!(format!("{s:?}"), before);
        }
        for kind in 0..5 {
            let mut changed = s.clone();
            match kind {
                0 => changed.model.mass[0] *= 2.,
                1 => changed.model.rest[0].x += 0.01,
                2 => changed.model.membrane.q[0][0] *= 2.,
                3 => changed.gravity.z *= 2.,
                _ => changed.model.membranes[0].area *= 2.,
            }
            let before = format!("{changed:?}");
            assert!(changed.step(0.01).is_err());
            assert!(changed.liquid_mass_report().is_err());
            assert_eq!(format!("{changed:?}"), before);
        }
        let base = solver();
        let mut pinned = Solver::new(base.model, base.x, &[[true; 3]; 4], Vec3::ZERO);
        let q = qualification();
        let before = format!("{pinned:?}");
        assert!(
            start(
                &mut pinned,
                q.clone(),
                vec![q.bindings["sheet"].clone(); 2],
                &[0.1, 0.2],
                &[vec![20.], vec![20.]]
            )
            .is_err()
        );
        assert_eq!(format!("{pinned:?}"), before);
        let mut q = qualification();
        q.allow_estimates = false;
        assert!(q.validate().is_err());
        q.allow_estimates = true;
        q.material_report_sha256 = "changed".into();
        assert!(q.validate().is_err());
    }

    #[test]
    fn co_moving_evaporation_does_not_invent_a_rocket_term_in_free_fall() {
        let mut s = solver();
        let x0 = s.x.clone();
        s.tol = 1e-10;
        let v0 = Vec3::new(0.1, -0.2, 0.3);
        s.v.fill(v0);
        initialize(&mut s, &[0.12, 0.3]);
        let h = 0.01;
        for i in 1..=10 {
            s.step(h).unwrap();
            let f = 1. - i as f64 / 10.;
            s.evaporate_liquid(&[0.12 * f, 0.3 * f], &[vec![20.], vec![20.]])
                .unwrap();
            let time = i as f64 * h;
            for (p, origin) in s.x.iter().zip(&x0) {
                let expected =
                    *origin + v0 * time + s.gravity * (h * h * (i * (i + 1)) as f64 / 2.);
                assert!((*p - expected).norm() < 1e-10);
            }
            for v in &s.v {
                assert!((*v - v0 - s.gravity * time).norm() < 1e-9);
            }
        }
    }

    #[test]
    fn evaporation_driven_inertia_converges_to_independent_variable_mass_oscillator() {
        // Isolate the production axial elastic and inertial integrator on a free triangle.
        // Zero membrane coefficients are a reduced numerical system, not a source material.
        // Two equal endpoint masses m(t)=m0*(1-alpha*t)^2 obey q''+2k/m(t)*q=0.
        let q0 = 0.2;
        let alpha = 0.5;
        let m0 = 0.5;
        let k = 0.1;
        let beta = f64::sqrt(2. * k / (m0 * alpha * alpha) - 0.25);
        let u: f64 = 1. - alpha;
        let phase = beta * u.ln();
        let exact = q0 * u.sqrt() * (phase.cos() - 0.5 / beta * phase.sin());
        let mut errors = Vec::new();
        for steps in [40, 80, 160] {
            let rest = [Vec2::ZERO, Vec2::new(1., 0.), Vec2::new(0., 1.)];
            let mut model = ClothModel::from_triangles(
                3,
                &[[0, 1, 2]],
                &[rest],
                Membrane {
                    q: [[0.; 3]; 3],
                    compression: [1.; 2],
                    transition: 0.001,
                },
                0.,
                0.6,
            )
            .unwrap();
            model.elastics.push(ElasticSegment {
                v: [0, 1],
                rest: 1.,
                ea: k,
            });
            let length = 1. - 0.5 * ELASTIC_ONSET + q0;
            let mut s = Solver::new(
                model,
                vec![
                    Vec3::new(-length / 2., 0., 0.),
                    Vec3::new(length / 2., 0., 0.),
                    Vec3::new(0., 1., 0.),
                ],
                &[[false; 3]; 3],
                Vec3::ZERO,
            );
            s.tol = 1e-11;
            initialize(&mut s, &[3. * (m0 - 0.1)]);
            for i in 1..=steps {
                s.step(1. / steps as f64).unwrap();
                let factor = 1. - alpha * i as f64 / steps as f64;
                s.evaporate_liquid(&[3. * (m0 * factor * factor - 0.1)], &[vec![20.]])
                    .unwrap();
            }
            let q = (s.x[1] - s.x[0]).norm() - 1. + 0.5 * ELASTIC_ONSET;
            assert!(q > 0.1);
            errors.push((q - exact).abs());
            assert!((s.x[0].x + s.x[1].x).abs() < 1e-12);
        }
        assert!(errors[2] < 0.0005, "{errors:?}");
        assert!(
            errors[0] / errors[1] > 1.8 && errors[1] / errors[2] > 1.8,
            "{errors:?}"
        );
    }
}

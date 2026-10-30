//! Reference-area liquid inertia with explicitly qualified frozen mechanics.
//! Public evaporation-only binding remains strict. Accepted-step thermal histories may separately
//! qualify incoming liquid and condensed vapor, either co-moving or at prescribed world velocities.
//! Inelastic mixing work stays separate from thermal heat; drainage and wet laws are not inferred.

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
    /// World-frame velocity of all incoming liquid/vapor for this instance, m/s; required only
    /// by the independently qualified prescribed-velocity exchange model.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "incoming_velocity"
    )]
    pub incoming_velocity_world_m_s: Option<Vec3>,
    /// Separately qualified supply/condensation velocities for coupled mechanics version 3.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "incoming_stream_velocities"
    )]
    pub incoming_stream_velocities_world_m_s: Option<IncomingVelocities>,
}

/// Constant world-frame velocities of the two independently supplied incoming phases, m/s.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IncomingVelocities {
    /// Velocity of supplied liquid before inelastic capture.
    pub supply: Vec3,
    /// Velocity of condensed vapor before inelastic capture.
    pub condensation: Vec3,
}

fn incoming_stream_velocities<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<IncomingVelocities>, D::Error> {
    IncomingVelocities::deserialize(deserializer).map(Some)
}

fn incoming_velocity<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Vec3>, D::Error> {
    Vec3::deserialize(deserializer).map(Some)
}

impl MaterialDomain {
    fn incoming_streams(
        &self,
        total: f64,
        components: Option<[f64; 2]>,
    ) -> Result<[(f64, Vec3); 2], Refusal> {
        if let Some(velocities) = self.incoming_stream_velocities_world_m_s {
            let [supply, condensation] = components.ok_or_else(|| {
                invalid("phase-specific mechanics requires separate thermal flows")
            })?;
            Ok([
                (supply, velocities.supply),
                (condensation, velocities.condensation),
            ])
        } else {
            let velocity = self
                .incoming_velocity_world_m_s
                .ok_or_else(|| invalid("missing qualified incoming world velocity"))?;
            Ok([(total, velocity), (0., Vec3::ZERO)])
        }
    }
}

/// Source permission to retain the exact material and interface laws throughout liquid exchange.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Qualification {
    /// `ftx-evaporating-liquid-mechanics/1`, or coupled mechanics `/1`, `/2` or `/3`.
    pub schema: String,
    /// Evaporation, co-moving exchange, or prescribed incoming velocity with frozen mechanics.
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
    /// Basis for co-moving mass transfer and invariant membrane, bending, contact and history laws.
    pub qualification: String,
}

impl Qualification {
    /// Validate structure before an expensive drape; binding later verifies actual source identities.
    ///
    /// # Errors
    /// Unsupported closure, incomplete source/domain, nonphysical bounds or missing estimate consent.
    pub fn validate(&self) -> Result<(), Refusal> {
        if self.schema != "ftx-evaporating-liquid-mechanics/1"
            || self.model != "co_moving_evaporation_frozen_mechanics/1"
            || self.bindings.values().any(|domain| {
                domain.incoming_velocity_world_m_s.is_some()
                    || domain.incoming_stream_velocities_world_m_s.is_some()
            })
        {
            return Err(invalid(
                "unsupported evaporation-only mechanical qualification",
            ));
        }
        self.validate_fields()
    }

    pub(crate) fn validate_exchange(&self) -> Result<(), Refusal> {
        let valid = match (self.schema.as_str(), self.model.as_str()) {
            ("ftx-coupled-liquid-mechanics/1", "co_moving_mass_exchange_frozen_mechanics/1") => {
                self.bindings.values().all(|domain| {
                    domain.incoming_velocity_world_m_s.is_none()
                        && domain.incoming_stream_velocities_world_m_s.is_none()
                })
            }
            (
                "ftx-coupled-liquid-mechanics/2",
                "prescribed_incoming_velocity_mass_exchange_frozen_mechanics/1",
            ) => self.bindings.values().all(|domain| {
                domain
                    .incoming_velocity_world_m_s
                    .is_some_and(|velocity| velocity.is_finite())
                    && domain.incoming_stream_velocities_world_m_s.is_none()
            }),
            (
                "ftx-coupled-liquid-mechanics/3",
                "phase_specific_incoming_velocity_mass_exchange_frozen_mechanics/1",
            ) => self.bindings.values().all(|domain| {
                domain.incoming_velocity_world_m_s.is_none()
                    && domain
                        .incoming_stream_velocities_world_m_s
                        .is_some_and(|v| v.supply.is_finite() && v.condensation.is_finite())
            }),
            _ => false,
        };
        if !valid {
            return Err(invalid(
                "liquid exchange needs a supported source qualification and exactly the incoming velocity fields required by its model",
            ));
        }
        self.validate_fields()
    }

    pub(crate) fn is_exchange(&self) -> bool {
        matches!(
            self.schema.as_str(),
            "ftx-coupled-liquid-mechanics/1"
                | "ftx-coupled-liquid-mechanics/2"
                | "ftx-coupled-liquid-mechanics/3"
        )
    }

    fn validate_fields(&self) -> Result<(), Refusal> {
        let text = |s: &str| !s.trim().is_empty() && s.len() <= 4096;
        let hash = |s: &str| {
            s.len() == 64
                && s.bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        };
        if !hash(&self.material_report_sha256)
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
    /// `ftx-evaporating-liquid-mechanics-result/1` or coupled result `/1`, `/2` or `/3`.
    pub schema: &'static str,
    /// Exact independent qualification, retained for replay.
    pub qualification: Qualification,
    /// Original dry sheet plus independently attached accessories, kg.
    pub substrate_and_attached_mass_kg: f64,
    /// Liquid initialized co-moving at the declared post-drape state, kg.
    pub initial_liquid_mass_kg: f64,
    /// Current liquid in retained material triangles, kg.
    pub remaining_liquid_mass_kg: f64,
    /// Cumulative outgoing evaporated mass on the same triangles, kg.
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
    /// Accumulated numerical residual of new minus old minus incoming plus outgoing momentum.
    pub transfer_momentum_residual_kg_m_s: Vec3,
    /// Accumulated kinetic transfer residual including separately retained mixing dissipation, J.
    pub transfer_kinetic_residual_j: f64,
    /// Accumulated numerical residual of potential energy during mass transfer only, J.
    pub transfer_gravitational_residual_j: f64,
    /// Additional incoming-flow ledger only for independently qualified bidirectional exchange.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exchange: Option<ExchangeReport>,
    /// Physical and numerical boundaries of this ledger.
    pub scope: &'static str,
}

/// Mechanical energy and momentum of co-moving admission and subsequent incoming liquid/vapor.
/// The enclosing report retains outgoing transfer and the same reference-owned mass authority.
#[derive(Clone, Debug, Default, Serialize)]
pub struct ExchangeReport {
    /// Cumulative incoming liquid plus condensed vapor mass after initialization, kg.
    pub incoming_mass_kg: f64,
    /// Cumulative evaporated mass after initialization, kg.
    pub outgoing_mass_kg: f64,
    /// Initial liquid momentum at post-drape admission, kg·m/s.
    pub initial_momentum_kg_m_s: Vec3,
    /// Initial admitted liquid kinetic energy, J.
    pub initial_kinetic_energy_j: f64,
    /// Initial admitted signed gravitational energy, J.
    pub initial_gravitational_energy_j: f64,
    /// Subsequent incoming momentum at accepted endpoints, kg·m/s.
    pub incoming_momentum_kg_m_s: Vec3,
    /// Subsequent incoming bulk kinetic energy, J, not heat.
    pub incoming_kinetic_energy_j: f64,
    /// Subsequent incoming signed gravitational energy, J.
    pub incoming_gravitational_energy_j: f64,
    /// Cumulative inelastic incoming mixing work, J, present only for prescribed incoming velocity.
    /// This positive mechanical dissipation is not deposited as thermal sensible or latent heat.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mixing_dissipation_j: Option<f64>,
}

impl Report {
    /// Compact accepted-step ledger without repeating the frozen source qualification.
    #[must_use]
    pub fn summary(&self) -> serde_json::Value {
        let mut value = serde_json::json!({
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
        });
        if let Some(flow) = &self.exchange {
            value["exchange"] = serde_json::json!(flow);
        }
        value
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
    exchange: bool,
}

struct Mixing {
    velocity: Vec<Vec3>,
    incoming_momentum: Vec<Vec3>,
    incoming_kinetic: Vec<f64>,
    dissipation: Vec<f64>,
}

impl Binding {
    fn mix_incoming(
        &self,
        solver: &Solver,
        incoming: &[f64],
        entering: &[f64],
        components: Option<[&[f64]; 2]>,
    ) -> Result<Mixing, Refusal> {
        let mut result = Mixing {
            velocity: solver.v.clone(),
            incoming_momentum: vec![Vec3::ZERO; solver.v.len()],
            incoming_kinetic: vec![0.; solver.v.len()],
            dissipation: vec![0.; solver.v.len()],
        };
        let mut relative_momentum = vec![Vec3::ZERO; solver.v.len()];
        for (i, ((element, domain), &amount)) in self
            .original
            .membranes
            .iter()
            .zip(&self.domains)
            .zip(incoming)
            .enumerate()
        {
            if amount == 0. {
                continue;
            }
            for (mass, velocity) in
                domain.incoming_streams(amount, components.map(|c| [c[0][i], c[1][i]]))?
            {
                if mass == 0. {
                    continue;
                }
                let share = mass / 3.;
                if share <= 0. {
                    return Err(invalid("incoming phase mass underflows at welded vertices"));
                }
                for &v in &element.v {
                    result.incoming_momentum[v] += velocity * share;
                    relative_momentum[v] += (velocity - solver.v[v]) * share;
                    result.incoming_kinetic[v] += 0.5 * share * velocity.norm2();
                }
            }
        }
        for i in 0..solver.v.len() {
            if entering[i] == 0. {
                continue;
            }
            let old = solver.model.mass[i];
            let combined = old + entering[i];
            if !combined.is_finite() || combined <= old {
                return Err(invalid("incoming mixing mass is not representable"));
            }
            // Relative momentum preserves an exactly co-moving stream without a spurious impulse.
            result.velocity[i] += relative_momentum[i] / combined;
            result.dissipation[i] = 0.5 * old * (solver.v[i] - result.velocity[i]).norm2();
        }
        for (i, ((element, domain), &amount)) in self
            .original
            .membranes
            .iter()
            .zip(&self.domains)
            .zip(incoming)
            .enumerate()
        {
            if amount == 0. {
                continue;
            }
            for (mass, velocity) in
                domain.incoming_streams(amount, components.map(|c| [c[0][i], c[1][i]]))?
            {
                if mass == 0. {
                    continue;
                }
                for &v in &element.v {
                    // Sum squared relative speeds, not a cancellation-prone energy difference.
                    result.dissipation[v] +=
                        0.5 * (mass / 3.) * (velocity - result.velocity[v]).norm2();
                }
            }
        }
        if result
            .velocity
            .iter()
            .chain(&result.incoming_momentum)
            .chain(&relative_momentum)
            .any(|v| !v.is_finite())
            || result
                .incoming_kinetic
                .iter()
                .chain(&result.dissipation)
                .any(|value| !value.is_finite() || *value < 0.)
        {
            return Err(invalid(
                "prescribed incoming momentum, velocity or mixing work overflows",
            ));
        }
        Ok(result)
    }

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
                || (!self.exchange && water > self.remaining[i])
                || !maximum.is_finite()
                || maximum <= 0.
                || water > maximum
                || cells.is_empty()
                || cells.len() > 64
                || cells.iter().any(|t| !t.is_finite() || *t < lo || *t > hi)
            {
                return Err(invalid(
                    "liquid transfer must remain within its source-qualified loading and temperature domain",
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
        r.evaporated_liquid_mass_kg = r.exchange.as_ref().map_or_else(
            || {
                self.initial
                    .iter()
                    .zip(&self.remaining)
                    .map(|(a, b)| a - b)
                    .sum()
            },
            |flow| flow.outgoing_mass_kg,
        );
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
        if let Some(flow) = &r.exchange
            && (!flow.initial_momentum_kg_m_s.is_finite()
                || !flow.incoming_momentum_kg_m_s.is_finite()
                || [
                    flow.incoming_mass_kg,
                    flow.outgoing_mass_kg,
                    flow.initial_kinetic_energy_j,
                    flow.initial_gravitational_energy_j,
                    flow.incoming_kinetic_energy_j,
                    flow.incoming_gravitational_energy_j,
                ]
                .iter()
                .any(|v| !v.is_finite())
                || flow
                    .mixing_dissipation_j
                    .is_some_and(|work| !work.is_finite() || work < 0.))
        {
            return Err(invalid(
                "co-moving incoming mass, momentum or energy overflows",
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
    bind_checked(run, qualification, initial_liquid_kg, temperatures_c, false)
}

/// Same complete source admission as evaporation, with separately authorized incoming flow.
pub(crate) fn bind_exchange(
    run: &mut DrapeRun,
    qualification: &Qualification,
    initial_liquid_kg: &[f64],
    temperatures_c: &[Vec<f64>],
) -> Result<Report, Refusal> {
    qualification.validate_exchange()?;
    bind_checked(run, qualification, initial_liquid_kg, temperatures_c, true)
}

fn bind_checked(
    run: &mut DrapeRun,
    qualification: &Qualification,
    initial_liquid_kg: &[f64],
    temperatures_c: &[Vec<f64>],
    exchange: bool,
) -> Result<Report, Refusal> {
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
    start_with_exchange(
        solver,
        qualification.clone(),
        domains,
        initial_liquid_kg,
        temperatures_c,
        exchange,
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

#[cfg(test)]
fn start(
    solver: &mut Solver,
    qualification: Qualification,
    domains: Vec<MaterialDomain>,
    initial: &[f64],
    temperatures: &[Vec<f64>],
) -> Result<Report, Refusal> {
    start_with_exchange(solver, qualification, domains, initial, temperatures, false)
}

fn start_with_exchange(
    solver: &mut Solver,
    qualification: Qualification,
    domains: Vec<MaterialDomain>,
    initial: &[f64],
    temperatures: &[Vec<f64>],
    exchange: bool,
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
    let phases = qualification.schema == "ftx-coupled-liquid-mechanics/3";
    let prescribed = phases || qualification.schema == "ftx-coupled-liquid-mechanics/2";
    if !initial_total.is_finite() || !substrate.is_finite() {
        return Err(invalid("initial liquid or substrate mass overflows"));
    }
    let mut binding = Binding {
        exchange,
        original: solver.model.clone(),
        contact: solver.contact.as_ref().map(|c| c.frozen_policy()),
        gravity: solver.gravity,
        history_kinds: solver.liquid_history_kinds(),
        expected_mass: solver.model.mass.clone(),
        domains,
        initial: initial.to_vec(),
        remaining: initial.to_vec(),
        report: Report {
            schema: if phases {
                "ftx-coupled-liquid-mechanics-result/3"
            } else if prescribed {
                "ftx-coupled-liquid-mechanics-result/2"
            } else if exchange {
                "ftx-coupled-liquid-mechanics-result/1"
            } else {
                "ftx-evaporating-liquid-mechanics-result/1"
            },
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
            exchange: exchange.then(|| ExchangeReport {
                mixing_dissipation_j: prescribed.then_some(0.),
                ..ExchangeReport::default()
            }),
            scope: "Source-qualified frozen mechanics with co-moving evaporation only. Initial liquid is an authored post-drape state, not wet equilibrium. Endpoint mass removal preserves velocity and exports its momentum, kinetic and signed gravitational energy. No incoming flow, drainage, recoil, damage, mixing heat, constrained-DOF impulse, full time-integration energy conservation or empirical wet-garment claim.",
        },
    };
    binding.check(solver)?;
    binding.expected_mass = binding.admit(initial, temperatures)?;
    if let Some(flow) = &mut binding.report.exchange {
        for (element, &water) in binding.original.membranes.iter().zip(initial) {
            for &v in &element.v {
                let share = water / 3.;
                flow.initial_momentum_kg_m_s += solver.v[v] * share;
                flow.initial_kinetic_energy_j += share * 0.5 * solver.v[v].norm2();
                flow.initial_gravitational_energy_j -= share * solver.gravity.dot(solver.x[v]);
            }
        }
        binding.report.scope = "Source-qualified frozen mechanical/interface laws with co-moving endpoint mass exchange. Initial liquid is admitted after a dry drape. Incoming and outgoing mass preserve velocity, carry their own momentum, kinetic and signed gravitational energy, and update reference-owned inertia. Thermal sensible/latent energies remain separate. No relative recoil, mixing heat, constrained-DOF impulse, inferred wet law or empirical garment claim.";
        if prescribed {
            binding.report.scope = "Source-qualified frozen mechanical/interface laws with prescribed incoming world velocities. At each accepted endpoint, incoming streams mix inelastically with the previous total welded mass; outgoing evaporation then leaves at that common velocity. Initial liquid remains co-moving after a dry drape. Reference-owned mass, momentum, bulk kinetic/gravitational energy and nonnegative mixing dissipation are retained separately. Mixing work is not deposited into the thermal sensible/latent ledger. Mechanical time-step refinement remains required. No drainage, constrained-DOF impulse, inferred wet law, heat conversion or empirical garment claim.";
        }
        if phases {
            binding.report.scope = "Source-qualified frozen mechanical/interface laws with separately prescribed supply and condensation world velocities. At each accepted endpoint both streams mix once with the previous welded mass before evaporation leaves at their common mixed velocity. Thermal inventory owns final inertia; incoming momentum, bulk kinetic/gravitational energy and nonnegative mixing dissipation remain separate from sensible/latent heat. Initial liquid is co-moving after a dry drape. No inferred velocity, drainage, pinned-coordinate impulse, wet law, mixing-heat conversion or empirical garment claim; mechanical step refinement remains necessary.";
        }
    }
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
            binding.exchange,
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

    /// Accept the same thermal kernel's incoming/outgoing triangle masses. This private broader
    /// path requires its own complete source qualification; the public evaporation API stays strict.
    #[cfg(test)]
    pub(crate) fn exchange_liquid(
        &mut self,
        remaining: &[f64],
        incoming: &[f64],
        outgoing: &[f64],
        temperatures: &[Vec<f64>],
    ) -> Result<Report, Refusal> {
        self.exchange_liquid_inner(remaining, incoming, outgoing, temperatures, None)
    }

    /// Retain exact supplied and condensed increments until the source-qualified mixing step.
    pub(crate) fn exchange_liquid_streams(
        &mut self,
        remaining: &[f64],
        incoming: &[f64],
        supplied: &[f64],
        condensed: &[f64],
        outgoing: &[f64],
        temperatures: &[Vec<f64>],
    ) -> Result<Report, Refusal> {
        self.exchange_liquid_inner(
            remaining,
            incoming,
            outgoing,
            temperatures,
            Some([supplied, condensed]),
        )
    }

    fn exchange_liquid_inner(
        &mut self,
        remaining: &[f64],
        incoming: &[f64],
        outgoing: &[f64],
        temperatures: &[Vec<f64>],
        components: Option<[&[f64]; 2]>,
    ) -> Result<Report, Refusal> {
        self.check_liquid_mass()?;
        let mut binding = self
            .liquid_mass
            .clone()
            .ok_or_else(|| invalid("liquid mechanics is not initialized"))?;
        if !binding.exchange
            || incoming.len() != binding.remaining.len()
            || outgoing.len() != incoming.len()
            || remaining.len() != incoming.len()
            || components.is_some_and(|c| c.iter().any(|v| v.len() != incoming.len()))
            || (binding.report.qualification.schema == "ftx-coupled-liquid-mechanics/3"
                && components.is_none())
        {
            return Err(invalid(
                "incoming liquid needs its independent complete exchange qualification and triangle ledger",
            ));
        }
        let next = binding.admit(remaining, temperatures)?;
        let mut entering = vec![0.; next.len()];
        let mut leaving = vec![0.; next.len()];
        let mut inventory_change = vec![0.; next.len()];
        let mut incoming_total = 0.;
        let mut outgoing_total = 0.;
        for i in 0..incoming.len() {
            let (a, b, old, new) = (incoming[i], outgoing[i], binding.remaining[i], remaining[i]);
            if let Some([supplied, condensed]) = components {
                let (s, c) = (supplied[i], condensed[i]);
                let scale = a + s + c;
                if !s.is_finite()
                    || !c.is_finite()
                    || s < 0.
                    || c < 0.
                    || !scale.is_finite()
                    || (a - s - c).abs() > 1024. * f64::EPSILON * scale
                {
                    return Err(invalid(
                        "thermal incoming mass differs from its supply/condensation components",
                    ));
                }
            }
            let scale = old + a + b;
            let tolerance = 1024. * f64::EPSILON * scale;
            if !a.is_finite()
                || !b.is_finite()
                || a < 0.
                || b < 0.
                || !scale.is_finite()
                || (b > old + a && b - (old + a) > tolerance)
                || (new - old - a + b).abs() > tolerance
            {
                return Err(invalid(
                    "incoming, outgoing and remaining liquid fail per-triangle mass conservation",
                ));
            }
            let change = (new - old) / 3.;
            if old != new && change == 0. {
                return Err(invalid(
                    "liquid inventory change underflows at welded vertices",
                ));
            }
            for &vertex in &binding.original.membranes[i].v {
                inventory_change[vertex] += change;
                if !inventory_change[vertex].is_finite() {
                    return Err(invalid("welded liquid inventory change overflows"));
                }
            }
            for (amount, total, nodal) in [
                (a, &mut incoming_total, &mut entering),
                (b, &mut outgoing_total, &mut leaving),
            ] {
                let sum = *total + amount;
                if !sum.is_finite() || (amount > 0. && sum <= *total) {
                    return Err(invalid("positive liquid exchange is lost in accumulation"));
                }
                *total = sum;
                let share = amount / 3.;
                for &vertex in &binding.original.membranes[i].v {
                    let next = nodal[vertex] + share;
                    if !next.is_finite() || (amount > 0. && (share <= 0. || next <= nodal[vertex]))
                    {
                        return Err(invalid(
                            "liquid exchange is unrepresentable at welded vertices",
                        ));
                    }
                    nodal[vertex] = next;
                }
            }
        }
        let mixing = binding
            .report
            .exchange
            .as_ref()
            .is_some_and(|flow| flow.mixing_dissipation_j.is_some())
            .then(|| binding.mix_incoming(self, incoming, &entering, components))
            .transpose()?;
        let r = &mut binding.report;
        let flow = r.exchange.as_mut().expect("qualified exchange ledger");
        for i in 0..next.len() {
            // Inventory owns inertia. Equal throughflow can have a tiny counter/assembly residual;
            // it must not invent a required change of dry mass. Residuals remain in the ledger.
            let net = inventory_change[i];
            let old = self.model.mass[i];
            if (net > 0. && next[i] <= old) || (net < 0. && next[i] >= old) {
                return Err(invalid(
                    "net liquid exchange is lost by welded inertia arithmetic",
                ));
            }
            let residual = next[i] - old - entering[i] + leaving[i];
            let velocity = self.v[i];
            let kinetic = 0.5 * velocity.norm2();
            let potential = -self.gravity.dot(self.x[i]);
            if let Some(mixing) = &mixing {
                let mixed = mixing.velocity[i];
                let mixed_kinetic = 0.5 * mixed.norm2();
                let dissipation = mixing.dissipation[i];
                flow.incoming_momentum_kg_m_s += mixing.incoming_momentum[i];
                flow.incoming_kinetic_energy_j += mixing.incoming_kinetic[i];
                *flow
                    .mixing_dissipation_j
                    .as_mut()
                    .expect("qualified mixing ledger") += dissipation;
                r.exported_momentum_kg_m_s += mixed * leaving[i];
                r.exported_kinetic_energy_j += mixed_kinetic * leaving[i];
                r.transfer_momentum_residual_kg_m_s +=
                    mixed * next[i] - velocity * old - mixing.incoming_momentum[i]
                        + mixed * leaving[i];
                r.transfer_kinetic_residual_j +=
                    mixed_kinetic * next[i] - kinetic * old - mixing.incoming_kinetic[i]
                        + mixed_kinetic * leaving[i]
                        + dissipation;
            } else {
                flow.incoming_momentum_kg_m_s += velocity * entering[i];
                flow.incoming_kinetic_energy_j += kinetic * entering[i];
                r.exported_momentum_kg_m_s += velocity * leaving[i];
                r.exported_kinetic_energy_j += kinetic * leaving[i];
                r.transfer_momentum_residual_kg_m_s += velocity * residual;
                r.transfer_kinetic_residual_j += kinetic * residual;
            }
            flow.incoming_gravitational_energy_j += potential * entering[i];
            r.exported_gravitational_energy_j += potential * leaving[i];
            r.transfer_gravitational_residual_j += potential * residual;
        }
        let incoming_sum = flow.incoming_mass_kg + incoming_total;
        let outgoing_sum = flow.outgoing_mass_kg + outgoing_total;
        if (incoming_total > 0. && incoming_sum <= flow.incoming_mass_kg)
            || (outgoing_total > 0. && outgoing_sum <= flow.outgoing_mass_kg)
        {
            return Err(invalid("positive cumulative exchange mass is lost"));
        }
        flow.incoming_mass_kg = incoming_sum;
        flow.outgoing_mass_kg = outgoing_sum;
        binding.expected_mass = next;
        binding.remaining = remaining.to_vec();
        binding.update_report()?;
        let report = binding.report.clone();
        self.accept_liquid_binding(binding);
        if let Some(mixing) = mixing {
            self.v = mixing.velocity;
        }
        Ok(report)
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
        if binding.exchange {
            return Err(invalid(
                "bidirectional liquid binding requires its complete exchange ledger",
            ));
        }
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

    fn exchange_qualification() -> Qualification {
        let mut q = qualification();
        q.schema = "ftx-coupled-liquid-mechanics/1".into();
        q.model = "co_moving_mass_exchange_frozen_mechanics/1".into();
        q.qualification =
            "Artificial invariant mechanics and co-moving incoming/outgoing mass".into();
        q
    }

    fn initialize_exchange(solver: &mut Solver, masses: &[f64]) -> Report {
        let q = exchange_qualification();
        q.validate_exchange().unwrap();
        let domains = vec![q.bindings["sheet"].clone(); masses.len()];
        start_with_exchange(
            solver,
            q,
            domains,
            masses,
            &vec![vec![20.]; masses.len()],
            true,
        )
        .unwrap()
    }

    fn initialize_prescribed(solver: &mut Solver, masses: &[f64], velocities: &[Vec3]) -> Report {
        assert_eq!(masses.len(), velocities.len());
        let mut q = exchange_qualification();
        q.schema = "ftx-coupled-liquid-mechanics/2".into();
        q.model = "prescribed_incoming_velocity_mass_exchange_frozen_mechanics/1".into();
        let original = q.bindings["sheet"].clone();
        q.bindings = velocities
            .iter()
            .enumerate()
            .map(|(i, velocity)| {
                let mut domain = original.clone();
                domain.incoming_velocity_world_m_s = Some(*velocity);
                (format!("sheet-{i}"), domain)
            })
            .collect();
        q.validate_exchange().unwrap();
        let domains = q.bindings.values().cloned().collect();
        start_with_exchange(
            solver,
            q,
            domains,
            masses,
            &vec![vec![20.]; masses.len()],
            true,
        )
        .unwrap()
    }

    fn initialize_phases(
        solver: &mut Solver,
        masses: &[f64],
        velocities: &[IncomingVelocities],
    ) -> Report {
        let mut q = exchange_qualification();
        q.schema = "ftx-coupled-liquid-mechanics/3".into();
        q.model = "phase_specific_incoming_velocity_mass_exchange_frozen_mechanics/1".into();
        let original = q.bindings["sheet"].clone();
        q.bindings = velocities
            .iter()
            .enumerate()
            .map(|(i, velocity)| {
                let mut domain = original.clone();
                domain.incoming_stream_velocities_world_m_s = Some(*velocity);
                (format!("sheet-{i}"), domain)
            })
            .collect();
        q.validate_exchange().unwrap();
        let domains = q.bindings.values().cloned().collect();
        start_with_exchange(
            solver,
            q,
            domains,
            masses,
            &vec![vec![20.]; masses.len()],
            true,
        )
        .unwrap()
    }

    #[test]
    fn separate_supply_and_condensation_mix_once_at_shared_vertices() {
        let mut s = solver();
        let v = Vec3::new(1., -2., 0.5);
        s.v.fill(v);
        let u = [
            IncomingVelocities {
                supply: Vec3::new(-3., 1., 2.),
                condensation: Vec3::new(1., -2., 0.5),
            },
            IncomingVelocities {
                supply: Vec3::new(2., 4., -1.),
                condensation: Vec3::new(-1., 0., 3.),
            },
        ];
        let admitted = initialize_phases(&mut s, &[0., 0.], &u);
        assert_eq!(admitted.schema, "ftx-coupled-liquid-mechanics-result/3");
        let old_mass = s.model.mass.clone();
        let report = s
            .exchange_liquid_streams(
                &[0., 0.],
                &[0.12, 0.3],
                &[0.06, 0.12],
                &[0.06, 0.18],
                &[0.12, 0.3],
                &[vec![20.], vec![20.]],
            )
            .unwrap();
        assert_eq!(s.model.mass, old_mass);
        let streams = [
            (0.06, u[0].supply),
            (0.06, u[0].condensation),
            (0.12, u[1].supply),
            (0.18, u[1].condensation),
        ];
        let expected_p = streams.iter().fold(Vec3::ZERO, |sum, (a, u)| sum + *u * *a);
        let expected_k: f64 = streams.iter().map(|(a, u)| 0.5 * a * u.norm2()).sum();
        let flow = report.exchange.as_ref().unwrap();
        assert!((flow.incoming_momentum_kg_m_s - expected_p).norm() < 2e-15);
        assert!((flow.incoming_kinetic_energy_j - expected_k).abs() < 2e-14);
        let mut after_p = Vec3::ZERO;
        let mut after_k = 0.;
        for (i, shares) in [
            [0.02, 0.02, 0.04, 0.06],
            [0.02, 0.02, 0.04, 0.06],
            [0.02, 0.02, 0., 0.],
            [0., 0., 0.04, 0.06],
        ]
        .into_iter()
        .enumerate()
        {
            let momentum = shares
                .iter()
                .zip(streams)
                .fold(v * old_mass[i], |sum, (a, (_, u))| sum + u * *a);
            let expected = momentum / (old_mass[i] + shares.iter().sum::<f64>());
            assert!((s.v[i] - expected).norm() < 2e-15);
            after_p += s.v[i] * old_mass[i];
            after_k += 0.5 * old_mass[i] * s.v[i].norm2();
        }
        let before_mass: f64 = old_mass.iter().sum();
        assert!(
            (after_p + report.exported_momentum_kg_m_s - v * before_mass - expected_p).norm()
                < 3e-15
        );
        assert!(
            (after_k + report.exported_kinetic_energy_j + flow.mixing_dissipation_j.unwrap()
                - 0.5 * before_mass * v.norm2()
                - expected_k)
                .abs()
                < 2e-14
        );
        assert!(report.transfer_momentum_residual_kg_m_s.norm() < 3e-15);
        assert!(report.transfer_kinetic_residual_j.abs() < 2e-14);
    }

    #[test]
    fn opposite_incoming_phases_keep_kinetic_energy_when_their_momentum_cancels() {
        let mut s = solver();
        let u = Vec3::new(3., -2., 1.);
        initialize_phases(
            &mut s,
            &[0., 0.],
            &[IncomingVelocities {
                supply: u,
                condensation: -u,
            }; 2],
        );
        let report = s
            .exchange_liquid_streams(
                &[0., 0.],
                &[0.12, 0.24],
                &[0.06, 0.12],
                &[0.06, 0.12],
                &[0.12, 0.24],
                &[vec![20.], vec![20.]],
            )
            .unwrap();
        assert_eq!(s.v, vec![Vec3::ZERO; 4]);
        let flow = report.exchange.unwrap();
        assert_eq!(flow.incoming_momentum_kg_m_s, Vec3::ZERO);
        let energy = 0.5 * 0.36 * u.norm2();
        assert!((flow.incoming_kinetic_energy_j - energy).abs() < 1e-14);
        assert!((flow.mixing_dissipation_j.unwrap() - energy).abs() < 1e-14);
        assert_eq!(report.exported_kinetic_energy_j, 0.);
    }

    #[test]
    fn separate_phase_mixing_is_galilean_and_exactly_comoving_when_qualified() {
        let v = Vec3::new(1., -2., 0.5);
        let boost = Vec3::new(4., 2., -3.);
        let run = |boost: Vec3, comoving: bool| {
            let mut s = solver();
            s.v.fill(v + boost);
            let u = if comoving {
                IncomingVelocities {
                    supply: v + boost,
                    condensation: v + boost,
                }
            } else {
                IncomingVelocities {
                    supply: Vec3::new(-1., 3., 2.) + boost,
                    condensation: Vec3::new(2., -1., -2.) + boost,
                }
            };
            initialize_phases(&mut s, &[0.06, 0.12], &[u; 2]);
            let report = s
                .exchange_liquid_streams(
                    &[0.12, 0.24],
                    &[0.12, 0.24],
                    &[0.06, 0.12],
                    &[0.06, 0.12],
                    &[0.06, 0.12],
                    &[vec![20.], vec![20.]],
                )
                .unwrap();
            (s.v, report)
        };
        let (base, a) = run(Vec3::ZERO, false);
        let (shifted, b) = run(boost, false);
        for (v, shifted) in base.iter().zip(&shifted) {
            assert!((*shifted - *v - boost).norm() < 2e-15);
        }
        let a = a.exchange.unwrap();
        let b = b.exchange.unwrap();
        assert!((a.mixing_dissipation_j.unwrap() - b.mixing_dissipation_j.unwrap()).abs() < 1e-14);
        let (velocities, report) = run(boost, true);
        assert_eq!(velocities, vec![v + boost; 4]);
        assert_eq!(report.exchange.unwrap().mixing_dissipation_j, Some(0.));
    }

    #[test]
    fn phase_velocity_admission_and_failed_transfer_preserve_the_solver() {
        let mut s = solver();
        let report = initialize_phases(
            &mut s,
            &[0., 0.],
            &[IncomingVelocities {
                supply: Vec3::ZERO,
                condensation: Vec3::new(1e200, 0., 0.),
            }; 2],
        );
        let q = serde_json::to_value(&report.qualification).unwrap();
        for invalid_streams in [
            serde_json::Value::Null,
            json!({"supply":[0.,0.,0.]}),
            json!({"supply":[0.,0.,0.],"condensation":[0.,null,0.]}),
            json!({"supply":[0.,0.],"condensation":[0.,0.,0.]}),
            json!({"supply":[0.,0.,0.],"condensation":[0.,0.,0.],"extra":[0.,0.,0.]}),
        ] {
            let mut bad = q.clone();
            bad["bindings"]["sheet-0"]["incoming_stream_velocities_world_m_s"] = invalid_streams;
            assert!(serde_json::from_value::<Qualification>(bad).is_err());
        }
        for (schema, model) in [
            (
                "ftx-evaporating-liquid-mechanics/1",
                "co_moving_evaporation_frozen_mechanics/1",
            ),
            (
                "ftx-coupled-liquid-mechanics/1",
                "co_moving_mass_exchange_frozen_mechanics/1",
            ),
            (
                "ftx-coupled-liquid-mechanics/2",
                "prescribed_incoming_velocity_mass_exchange_frozen_mechanics/1",
            ),
        ] {
            let mut bad = report.qualification.clone();
            bad.schema = schema.into();
            bad.model = model.into();
            if schema.ends_with("/2") {
                for domain in bad.bindings.values_mut() {
                    domain.incoming_velocity_world_m_s = Some(Vec3::ZERO);
                }
            }
            assert!(bad.validate_exchange().is_err());
            assert!(bad.validate().is_err());
        }
        let mut bad = report.qualification.clone();
        bad.bindings
            .get_mut("sheet-0")
            .unwrap()
            .incoming_velocity_world_m_s = Some(Vec3::ZERO);
        assert!(bad.validate_exchange().is_err());
        bad.bindings
            .get_mut("sheet-0")
            .unwrap()
            .incoming_velocity_world_m_s = None;
        bad.bindings
            .get_mut("sheet-0")
            .unwrap()
            .incoming_stream_velocities_world_m_s
            .as_mut()
            .unwrap()
            .supply
            .x = f64::NAN;
        assert!(bad.validate_exchange().is_err());
        // The inactive huge condensation velocity must not contribute zero times infinity.
        s.exchange_liquid_streams(
            &[0.06, 0.12],
            &[0.06, 0.12],
            &[0.06, 0.12],
            &[0., 0.],
            &[0., 0.],
            &[vec![20.], vec![20.]],
        )
        .unwrap();
        let before = format!("{s:?}");
        for (incoming, supplied, condensed) in [
            ([0.06, 0.12], [0., 0.], [0.06, 0.12]),
            ([0.06, 0.12], [0.03, 0.12], [0., 0.]),
            ([0.06, 0.12], [-0.01, 0.12], [0.07, 0.]),
        ] {
            assert!(
                s.exchange_liquid_streams(
                    &[0.12, 0.24],
                    &incoming,
                    &supplied,
                    &condensed,
                    &[0., 0.],
                    &[vec![20.], vec![20.]],
                )
                .is_err()
            );
            assert_eq!(format!("{s:?}"), before);
        }
        assert!(
            s.exchange_liquid(&[0.06, 0.12], &[0., 0.], &[0., 0.], &[vec![20.], vec![20.]],)
                .is_err()
        );
        assert_eq!(format!("{s:?}"), before);
    }

    #[test]
    fn prescribed_streams_mix_on_welded_vertices_before_equal_net_outflow() {
        let mut s = solver();
        let initial_velocity = Vec3::new(1., -2., 0.5);
        s.v.fill(initial_velocity);
        let incoming_velocity = [Vec3::new(-3., 1., 2.), Vec3::new(2., 4., -1.)];
        let admitted = initialize_prescribed(&mut s, &[0., 0.], &incoming_velocity);
        assert_eq!(admitted.schema, "ftx-coupled-liquid-mechanics-result/2");
        assert_eq!(admitted.exchange.unwrap().mixing_dissipation_j, Some(0.));
        let original_mass = s.model.mass.clone();
        let original_position = s.x.clone();
        let incoming = [0.12, 0.3];
        // All arriving liquid leaves within the endpoint. Inventory is unchanged, but the
        // control volume must still receive its momentum and dissipate relative kinetic energy.
        let report = s
            .exchange_liquid(&[0., 0.], &incoming, &incoming, &[vec![20.], vec![20.]])
            .unwrap();
        assert_eq!(s.model.mass, original_mass);
        assert_eq!(s.x, original_position);
        let flow = report.exchange.as_ref().unwrap();
        let expected_in = incoming_velocity[0] * 0.12 + incoming_velocity[1] * 0.3;
        assert!((flow.incoming_momentum_kg_m_s - expected_in).norm() < 1e-15);
        let expected_energy =
            0.5 * 0.12 * incoming_velocity[0].norm2() + 0.5 * 0.3 * incoming_velocity[1].norm2();
        assert!((flow.incoming_kinetic_energy_j - expected_energy).abs() < 1e-14);
        let mut before_p = Vec3::ZERO;
        let mut after_p = Vec3::ZERO;
        let mut before_k = 0.;
        let mut after_k = 0.;
        for (i, (first, second)) in [(0.04, 0.1), (0.04, 0.1), (0.04, 0.), (0., 0.1)]
            .into_iter()
            .enumerate()
        {
            let m = original_mass[i];
            let expected = (initial_velocity * m
                + incoming_velocity[0] * first
                + incoming_velocity[1] * second)
                / (m + first + second);
            assert!((s.v[i] - expected).norm() < 2e-15);
            before_p += initial_velocity * m;
            after_p += s.v[i] * m;
            before_k += 0.5 * m * initial_velocity.norm2();
            after_k += 0.5 * m * s.v[i].norm2();
        }
        assert!(
            (after_p + report.exported_momentum_kg_m_s - before_p - expected_in).norm() < 2e-15
        );
        let dissipated = flow.mixing_dissipation_j.unwrap();
        assert!(dissipated > 0.);
        assert!(
            (after_k + report.exported_kinetic_energy_j + dissipated - before_k - expected_energy)
                .abs()
                < 1e-14
        );
        assert!(report.transfer_momentum_residual_kg_m_s.norm() < 2e-15);
        assert!(report.transfer_kinetic_residual_j.abs() < 1e-14);
        assert!(report.transfer_gravitational_residual_j.abs() < 1e-14);
        let velocity = s.v.clone();
        let unchanged = s
            .exchange_liquid(&[0., 0.], &[0., 0.], &[0., 0.], &[vec![20.], vec![20.]])
            .unwrap();
        assert_eq!(s.v, velocity);
        assert_eq!(
            serde_json::to_value(unchanged.exchange).unwrap(),
            serde_json::to_value(report.exchange).unwrap()
        );
    }

    #[test]
    fn prescribed_incoming_momentum_is_galilean_invariant_and_comoving_is_exact() {
        let u = [Vec3::new(-1., 2., 0.5), Vec3::new(3., 0.5, -2.)];
        let v = Vec3::new(0.25, -0.5, 1.);
        let shift = Vec3::new(4., -3., 2.);
        let simulate = |boost: Vec3| {
            let mut s = solver();
            s.v.fill(v + boost);
            initialize_prescribed(&mut s, &[0.12, 0.3], &u.map(|x| x + boost));
            let report = s
                .exchange_liquid(
                    &[0.15, 0.24],
                    &[0.06, 0.12],
                    &[0.03, 0.18],
                    &[vec![20.], vec![20.]],
                )
                .unwrap();
            (s, report)
        };
        let (base, a) = simulate(Vec3::ZERO);
        let (boosted, b) = simulate(shift);
        for (v, shifted) in base.v.iter().zip(&boosted.v) {
            assert!((*shifted - *v - shift).norm() < 1e-15);
        }
        assert!(
            (a.exchange.unwrap().mixing_dissipation_j.unwrap()
                - b.exchange.unwrap().mixing_dissipation_j.unwrap())
            .abs()
                < 1e-14
        );
        let mut s = solver();
        s.v.fill(v);
        initialize_prescribed(&mut s, &[0.12, 0.3], &[v; 2]);
        let report = s
            .exchange_liquid(
                &[0.18, 0.42],
                &[0.06, 0.12],
                &[0., 0.],
                &[vec![20.], vec![20.]],
            )
            .unwrap();
        assert_eq!(s.v, vec![v; 4]);
        assert_eq!(report.exchange.unwrap().mixing_dissipation_j, Some(0.));
    }

    #[test]
    fn incoming_velocity_requires_exact_protocol_and_arithmetic_failure_rolls_back() {
        let mut q = exchange_qualification();
        let mut null_velocity = serde_json::to_value(&q).unwrap();
        null_velocity["bindings"]["sheet"]["incoming_velocity_world_m_s"] = serde_json::Value::Null;
        assert!(serde_json::from_value::<Qualification>(null_velocity).is_err());
        q.bindings
            .get_mut("sheet")
            .unwrap()
            .incoming_velocity_world_m_s = Some(Vec3::ZERO);
        assert!(q.validate_exchange().is_err());
        q.schema = "ftx-coupled-liquid-mechanics/2".into();
        assert!(q.validate_exchange().is_err());
        q.model = "prescribed_incoming_velocity_mass_exchange_frozen_mechanics/1".into();
        q.validate_exchange().unwrap();
        for velocity in [
            None,
            Some(Vec3::new(f64::NAN, 0., 0.)),
            Some(Vec3::new(0., f64::INFINITY, 0.)),
        ] {
            q.bindings
                .get_mut("sheet")
                .unwrap()
                .incoming_velocity_world_m_s = velocity;
            assert!(q.validate_exchange().is_err());
        }
        let mut s = solver();
        initialize_prescribed(&mut s, &[0., 0.], &[Vec3::new(1e200, 0., 0.); 2]);
        // An unused source velocity contributes neither work nor NaN from zero times overflow.
        s.exchange_liquid(&[0., 0.], &[0., 0.], &[0., 0.], &[vec![20.], vec![20.]])
            .unwrap();
        let before = format!("{s:?}");
        assert!(
            s.exchange_liquid(&[0.1, 0.2], &[0.1, 0.2], &[0., 0.], &[vec![20.], vec![20.]])
                .is_err()
        );
        assert_eq!(format!("{s:?}"), before);
    }

    #[test]
    fn simultaneous_nonuniform_exchange_closes_welded_momentum_and_bulk_energy() {
        let mut s = solver();
        s.v = vec![
            Vec3::new(1., 2., -3.),
            Vec3::new(-2., 1., 0.),
            Vec3::new(0., 0., 4.),
            Vec3::new(2., -3., 1.),
        ];
        let dry = s.model.mass.clone();
        let initial = initialize_exchange(&mut s, &[0.12, 0.3]);
        let initial_flow = initial.exchange.as_ref().unwrap();
        assert!((initial_flow.initial_gravitational_energy_j - 0.42 * 9.81 * 2.).abs() < 1e-14);
        let before = s.model.mass.clone();
        let velocity = s.v.clone();
        let report = s
            .exchange_liquid(
                &[0.15, 0.24],
                &[0.06, 0.12],
                &[0.03, 0.18],
                &[vec![25.], vec![30., 35.]],
            )
            .unwrap();
        assert_eq!(s.v, velocity);
        let flow = report.exchange.as_ref().unwrap();
        let mut expected_in = Vec3::ZERO;
        let mut expected_out = Vec3::ZERO;
        let mut initial_p = Vec3::ZERO;
        let mut final_p = Vec3::ZERO;
        let mut initial_k = 0.;
        let mut final_k = 0.;
        for (i, (incoming, outgoing)) in [(0.06, 0.07), (0.06, 0.07), (0.02, 0.01), (0.04, 0.06)]
            .into_iter()
            .enumerate()
        {
            assert!((s.model.mass[i] - before[i] - incoming + outgoing).abs() < 1e-15);
            expected_in += velocity[i] * incoming;
            expected_out += velocity[i] * outgoing;
            initial_p += velocity[i] * before[i];
            final_p += velocity[i] * s.model.mass[i];
            initial_k += 0.5 * before[i] * velocity[i].norm2();
            final_k += 0.5 * s.model.mass[i] * velocity[i].norm2();
        }
        assert!((flow.incoming_mass_kg - 0.18).abs() < 1e-15);
        assert!((flow.outgoing_mass_kg - 0.21).abs() < 1e-15);
        assert!((flow.incoming_momentum_kg_m_s - expected_in).norm() < 1e-15);
        assert!((report.exported_momentum_kg_m_s - expected_out).norm() < 1e-15);
        assert!(
            (final_p - initial_p - flow.incoming_momentum_kg_m_s + report.exported_momentum_kg_m_s)
                .norm()
                < 1e-15
        );
        assert!(
            (final_k - initial_k - flow.incoming_kinetic_energy_j
                + report.exported_kinetic_energy_j)
                .abs()
                < 1e-15
        );
        assert!((flow.incoming_gravitational_energy_j - 0.18 * 9.81 * 2.).abs() < 1e-14);
        assert!((report.exported_gravitational_energy_j - 0.21 * 9.81 * 2.).abs() < 1e-14);
        assert!(report.transfer_momentum_residual_kg_m_s.norm() < 1e-15);
        assert!(report.mass_balance_residual_kg.abs() < 1e-15);
        let before = format!("{s:?}");
        assert!(
            s.evaporate_liquid(&[0., 0.], &[vec![20.], vec![20.]])
                .is_err()
        );
        assert_eq!(format!("{s:?}"), before);
        s.exchange_liquid(&[0., 0.], &[0., 0.], &[0.15, 0.24], &[vec![20.], vec![20.]])
            .unwrap();
        assert_eq!(s.model.mass, dry);
    }

    #[test]
    fn exchange_refuses_unqualified_sources_or_unbalanced_transfer_atomically() {
        let (mut run, mut q) = source_run();
        let before = format!("{:?}", run.scene.solver);
        assert!(bind_exchange(&mut run, &q, &[0., 0.], &[vec![20.], vec![20.]]).is_err());
        assert_eq!(format!("{:?}", run.scene.solver), before);
        q.schema = "ftx-coupled-liquid-mechanics/1".into();
        q.model = "co_moving_mass_exchange_frozen_mechanics/1".into();
        assert!(bind(&mut run, &q, &[0., 0.], &[vec![20.], vec![20.]]).is_err());
        let mut changed = q.clone();
        changed.material_report_sha256 = "0".repeat(64);
        assert!(bind_exchange(&mut run, &changed, &[0., 0.], &[vec![20.], vec![20.]]).is_err());
        bind_exchange(&mut run, &q, &[0.12, 0.3], &[vec![20.], vec![20.]]).unwrap();
        let s = &mut run.scene.solver;
        for (remaining, incoming, outgoing, temperature) in [
            ([0.2, 0.3], [0., 0.], [0., 0.], 20.),
            ([0.1, 0.3], [-0.02, 0.], [0., 0.], 20.),
            ([0., 0.3], [0., 0.], [0.13, 0.], 20.),
            ([0.1, 0.3], [0., 0.], [0.02, 0.], 51.),
        ] {
            let before = format!("{s:?}");
            assert!(
                s.exchange_liquid(
                    &remaining,
                    &incoming,
                    &outgoing,
                    &[vec![temperature], vec![20.]]
                )
                .is_err()
            );
            assert_eq!(format!("{s:?}"), before);
        }
        s.model.mass[0] *= 2.;
        let before = format!("{s:?}");
        assert!(
            s.exchange_liquid(&[0.12, 0.3], &[0., 0.], &[0., 0.], &[vec![20.], vec![20.]])
                .is_err()
        );
        assert_eq!(format!("{s:?}"), before);
    }

    #[test]
    fn rounded_flow_ledgers_do_not_invent_inertia_at_dryout_or_dry_throughflow() {
        let mut s = solver();
        let dry = s.model.mass.clone();
        initialize_exchange(&mut s, &[0.12, 0.]);
        s.exchange_liquid(&[0.02, 0.], &[0., 0.], &[0.1, 0.], &[vec![20.], vec![20.]])
            .unwrap();
        let rounded = (0.1_f64 + 0.02) - 0.1;
        assert!(rounded > 0.02);
        s.exchange_liquid(
            &[0., 0.],
            &[0., 0.],
            &[rounded, 0.],
            &[vec![20.], vec![20.]],
        )
        .unwrap();
        assert_eq!(s.model.mass, dry);
        s.exchange_liquid(
            &[0., 0.],
            &[0.02, 0.],
            &[rounded, 0.],
            &[vec![20.], vec![20.]],
        )
        .unwrap();
        assert_eq!(s.model.mass, dry);
        let before = format!("{s:?}");
        assert!(
            s.exchange_liquid(
                &[0., 0.],
                &[0.02, 0.],
                &[0.021, 0.],
                &[vec![20.], vec![20.]]
            )
            .is_err()
        );
        assert_eq!(format!("{s:?}"), before);
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
        variable_mass_oscillator(0.5, false);
    }

    #[test]
    fn incoming_mass_inertia_converges_to_independent_variable_mass_oscillator() {
        variable_mass_oscillator(-0.5, true);
    }

    #[test]
    fn prescribed_velocity_accretion_matches_control_volume_momentum_and_refines_position() {
        // A freely translating mass m(t)=m0+r*t collecting a constant-velocity stream obeys
        // v(t)=u+m0*(v0-u)/m(t). Its integrated position contains ln(1+r*t/m0).
        let m0 = 0.3;
        let rate = 0.15;
        let v0 = Vec3::new(0.8, -0.2, 0.3);
        let incoming_velocity = Vec3::new(-0.4, 0.6, 0.);
        let exact_v = (v0 * m0 + incoming_velocity * rate) / (m0 + rate);
        let exact_dx =
            incoming_velocity + (v0 - incoming_velocity) * ((m0 / rate) * f64::ln(1. + rate / m0));
        let mut errors = Vec::new();
        for steps in [20, 40, 80] {
            let rest = [Vec2::ZERO, Vec2::new(1., 0.), Vec2::new(0., 1.)];
            let model = ClothModel::from_triangles(
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
            let positions: Vec<_> = rest.iter().map(|p| Vec3::new(p.x, p.y, 0.)).collect();
            let mut s = Solver::new(model, positions.clone(), &[[false; 3]; 3], Vec3::ZERO);
            s.v.fill(v0);
            s.tol = 1e-11;
            initialize_prescribed(&mut s, &[0.], &[incoming_velocity]);
            for i in 1..=steps {
                s.step(1. / steps as f64).unwrap();
                s.exchange_liquid(
                    &[rate * i as f64 / steps as f64],
                    &[rate / steps as f64],
                    &[0.],
                    &[vec![20.]],
                )
                .unwrap();
            }
            for velocity in &s.v {
                assert!((*velocity - exact_v).norm() < 1e-10);
            }
            let error = (s.x[0] - positions[0] - exact_dx).norm();
            errors.push(error);
            // Endpoint mixing leaves position as the left sum of the exact velocity history.
            // Its leading error is h*(v(0)-v(1))/2. For g(t)=1/(1+alpha*t), the
            // positive trapezoid remainder is bounded by h²*max(g'')/12=alpha²*h²/6.
            let h = 1. / steps as f64;
            let alpha = rate / m0;
            let leading = 0.5 * h * (v0 - exact_v).norm();
            let remainder = (v0 - incoming_velocity).norm() * alpha * alpha * h * h / 6.;
            assert!(error >= leading - 1e-10 && error <= leading + remainder + 1e-10);
            let report = s.liquid_mass_report().unwrap().unwrap();
            let flow = report.exchange.unwrap();
            let expected_loss = 0.5 * m0 * v0.norm2() + 0.5 * rate * incoming_velocity.norm2()
                - 0.5 * (m0 + rate) * exact_v.norm2();
            assert!((flow.mixing_dissipation_j.unwrap() - expected_loss).abs() < 1e-10);
            assert!(report.transfer_momentum_residual_kg_m_s.norm() < 1e-13);
        }
        assert!(
            errors[0] / errors[1] > 1.95 && errors[1] / errors[2] > 1.95,
            "{errors:?}"
        );
    }

    fn variable_mass_oscillator(alpha: f64, exchange: bool) {
        // Isolate the production axial elastic and inertial integrator on a free triangle.
        // Zero membrane coefficients are a reduced numerical system, not a source material.
        // Two equal endpoint masses m(t)=m0*(1-alpha*t)^2 obey q''+2k/m(t)*q=0.
        let q0 = 0.2;
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
            let mut previous = 3. * (m0 - 0.1);
            if exchange {
                initialize_exchange(&mut s, &[previous]);
            } else {
                initialize(&mut s, &[previous]);
            }
            for i in 1..=steps {
                s.step(1. / steps as f64).unwrap();
                let factor = 1. - alpha * i as f64 / steps as f64;
                let next = 3. * (m0 * factor * factor - 0.1);
                if exchange {
                    s.exchange_liquid(&[next], &[next - previous], &[0.], &[vec![20.]])
                        .unwrap();
                } else {
                    s.evaporate_liquid(&[next], &[vec![20.]]).unwrap();
                }
                previous = next;
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

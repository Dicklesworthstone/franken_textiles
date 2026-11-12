//! Bounded, deterministic inverse identification for 3D cylindrical-holder experiments.
//!
//! Identifies caller-declared subsets (`1..=5`) of canonical scalar orthotropic membrane and
//! hinge-bending properties (`warp_stiffness`, `weft_stiffness`, `poisson_warp_weft`,
//! `shear_stiffness`, `bending_rigidity`) by solving the production 3D cylindrical-holder
//! shell model (`super::Fixture`, isometric chord radius `R_mesh`, explicit `L_0 + Δz_pre + Δz - u_slip`,
//! explicit force/torque tare offsets, and grain-axis rotation) across `FIT` experiments and
//! verifying fixed-parameter predictions on independent held-out `CHECK` experiments and
//! doubled `[2 * n_theta, 2 * n_z]` meshes.
//! Independently qualified nonlinear membrane and directional panel-bending laws remain active
//! when that response is not targeted; their original source observations are retained unchanged.
//!
//! Published coefficients remain `Evidence::Estimate` with explicit `fitted_green_strain_range`
//! support. Unloaded contact `thickness` is never fabricated from presser-foot `reported_thickness`,
//! missing `friction_skin` is never invented, and `simulation_ready` stays `false` whenever any
//! garment mechanics prerequisite or estimate admission is absent.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode, Vec3, linalg::lstsq, receipt::sha256_hex};
use ftx_pattern::{
    material_bending::BendingTensor,
    material_calibration::{Partition, fitted_domain},
    material_catalog::Selection,
    material_response::{CompiledMembrane, StrainDomain},
    materials::{Evidence, Form, Material, Quantity, Source},
    textile_import,
};
use serde::{Deserialize, Serialize};

use super::{
    Controls, Fixture, NominalProtocol, ProtocolAudit, State, Waypoint, assemble_solver,
    audit_protocol, budget, build_cylinder_mesh, holder_separation_m, invalid, out_of_range,
    record_state, set_upper_ring,
};
use crate::Membrane;

/// Schema identifier of [`Input`].
pub const INPUT_SCHEMA: &str = "ftx-cylinder-calibration/1";
/// Schema identifier of [`Report`].
pub const REPORT_SCHEMA: &str = "ftx-cylinder-calibration-report/1";

const FIT_SCOPE: &str = "Bounded 3D cylindrical-holder inverse identification on an isometric chord-radius welded shell: rigid coaxial lower/upper holder rings, explicit pretension offset (never delta/nominal_height), explicit force/torque tare subtraction, grain-rotated scalar or independently qualified nonlinear Green-strain membrane, scalar hinge and independently qualified directional panel bending, three-start projected Levenberg-Marquardt search, local sensitivity Gram-pivot check, coarse-to-doubled-mesh stability check, held-out CHECK verification and fitted Green-strain domain replay. Zero gravity. Identified coefficients are model estimates; unloaded contact thickness, skin friction, inside-clamp slip physics, moisture/thermal coupling and physical garment fit are never inferred.";

/// Canonical scalar material property identifiable from cylindrical-holder experiments.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    /// Warp/wale tensile stiffness `E_1` (`N/m`).
    WarpStiffness,
    /// Weft/course tensile stiffness `E_2` (`N/m`).
    WeftStiffness,
    /// Major orthotropic Poisson ratio `nu_12` (`1`).
    PoissonWarpWeft,
    /// In-plane membrane shear stiffness `G_12` (`N/m`).
    ShearStiffness,
    /// Scalar dihedral hinge bending rigidity `B` (`N*m`).
    BendingRigidity,
}

impl TargetKind {
    /// Canonical `Material::properties` key.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::WarpStiffness => "warp_stiffness",
            Self::WeftStiffness => "weft_stiffness",
            Self::PoissonWarpWeft => "poisson_warp_weft",
            Self::ShearStiffness => "shear_stiffness",
            Self::BendingRigidity => "bending_rigidity",
        }
    }

    /// Canonical SI unit string stored on `Quantity`.
    #[must_use]
    pub const fn unit(self) -> &'static str {
        match self {
            Self::WarpStiffness | Self::WeftStiffness | Self::ShearStiffness => "N/m",
            Self::PoissonWarpWeft => "1",
            Self::BendingRigidity => "N*m",
        }
    }

    const fn requires_positive_lower_bound(self) -> bool {
        !matches!(self, Self::PoissonWarpWeft)
    }
}

/// One caller-selected parameter and its explicit admissible search interval `[lo, hi]`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetParameter {
    /// Canonical property to identify.
    pub parameter: TargetKind,
    /// Explicit finite search bounds `[lo, hi]` with `hi > lo` (strictly positive for stiffnesses/bending).
    pub bounds: [f64; 2],
}

/// Descriptor of one identified parameter in [`Report`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParameterDescriptor {
    /// Canonical property key.
    pub key: String,
    /// Canonical SI unit.
    pub unit: String,
    /// Declared search bounds `[lo, hi]`.
    pub bounds: [f64; 2],
}

/// Measured scalar channel reading and its explicit residual weighting scale.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Channel {
    /// Observed reading in the channel's SI unit (`N`, `N*m`, or `m`).
    pub value: f64,
    /// Positive residual weighting scale in the same SI unit (`> 0`), not an inferred confidence interval.
    pub scale: f64,
}

/// Observed holder reactions and/or mid-height radius at one loading waypoint.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    /// Globally unique observation row identifier across all experiments in [`Input`].
    pub id: String,
    /// Index into `experiment.loading` (`0..experiment.loading.len()`).
    pub waypoint_index: usize,
    /// Optional tare-adjusted upper axial force reading (`tare_adjusted_force_n`, `N`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub force_n: Option<Channel>,
    /// Optional tare-adjusted upper axial torque reading (`tare_adjusted_torque_nm`, `N*m`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub torque_nm: Option<Channel>,
    /// Optional mid-height mean cylinder radius reading (`mid_radius_range_m[2]`, `m`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mean_mid_radius_m: Option<Channel>,
}

impl Observation {
    fn channel_count(&self) -> usize {
        usize::from(self.force_n.is_some())
            + usize::from(self.torque_nm.is_some())
            + usize::from(self.mean_mid_radius_m.is_some())
    }
}

/// One cylindrical-holder specimen/orientation/protocol acquisition assigned to `FIT` or `CHECK`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Experiment {
    /// Unique experiment identifier.
    pub id: String,
    /// Cylindrical holder dimensions, pretension, tare, grain axis and coarse mesh cells.
    pub fixture: Fixture,
    /// Prescribed upper-holder kinematic waypoints (`1..=129`).
    pub loading: Vec<Waypoint>,
    /// Optional declared nominal rates/travel for protocol-discrepancy auditing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nominal_protocol: Option<NominalProtocol>,
    /// Source key in `input.material.sources` backing these observations.
    pub source: String,
    /// Evidence class of the observations.
    pub evidence: Evidence,
    /// `FIT` (participates in optimization) or held-out `CHECK` (evaluated only after parameters are fixed).
    pub partition: Partition,
    /// Nonempty list of waypoint observations.
    pub observations: Vec<Observation>,
}

/// Numerical residual, mesh-stability, identifiability and bounded-work gates.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    /// Maximum scaled RMS residual, enforced independently on `FIT` and `CHECK`.
    pub max_rms_scaled: f64,
    /// Maximum absolute scaled residual, enforced independently on `FIT` and `CHECK`.
    pub max_peak_scaled: f64,
    /// Maximum coarse-to-doubled-mesh change divided by each channel's scale.
    pub max_mesh_scaled: f64,
    /// Minimum column-normalized sensitivity Gram Cholesky pivot (`0 < pivot < 1`).
    pub min_excitation_pivot: f64,
    /// Per-start Levenberg-Marquardt iteration cap (`1..=80`).
    pub max_iterations: usize,
    /// Aggregate static + dynamic Newton equilibrium solve cap (`1..=32768`).
    pub max_forward_solves: usize,
}

/// Reproducible cylindrical-holder constitutive identification request.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    /// Must equal [`INPUT_SCHEMA`] (`"ftx-cylinder-calibration/1"`).
    pub schema: String,
    /// Base sheet record carrying `areal_density`, non-targeted baseline moduli and observation sources.
    pub material: Material,
    /// Explicit apparatus, clamping, pretension, tare and conditioning protocol description.
    pub method: String,
    /// Explicit test conditions recorded on published quantities.
    pub conditions: BTreeMap<String, String>,
    /// Caller-supplied date (`YYYY-MM-DD`) for deterministic source receipts.
    pub fitted_on: String,
    /// `1..=5` unique target parameters and their search bounds.
    pub targets: Vec<TargetParameter>,
    /// `1..=16` cylindrical-holder acquisitions (`<= 64` total observations), including held-out `CHECK` for publication.
    pub experiments: Vec<Experiment>,
    /// Explicit acknowledgment of the 3D cylindrical-shell forward model assumptions.
    pub admit_cylindrical_shell_model: bool,
    /// Explicit consent to fit against `Evidence::Estimate` observations or baseline properties.
    #[serde(default)]
    pub allow_estimated_observations: bool,
    /// Explicit consent to replace targeted properties already present on `material`.
    #[serde(default)]
    pub replace_existing: bool,
    /// Explicit consent to admit the resulting `Evidence::Estimate` properties on the returned `Selection`.
    #[serde(default)]
    pub allow_estimates: bool,
    /// Shared static/dynamic Newton and optional self-contact solver controls.
    #[serde(default)]
    pub controls: Controls,
    /// Acceptance and bounded-work policy.
    pub policy: Policy,
}

/// Scaled residual summary across active scalar observation channels.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Quality {
    /// Number of scalar observation channels summarized.
    pub components: usize,
    /// Root-mean-square scaled residual.
    pub rms_scaled: f64,
    /// Peak absolute scaled residual.
    pub peak_scaled: f64,
}

/// Diagnostic trace of one deterministic bounded local search start.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Start {
    /// Uniform normalized initial coordinate in `[0, 1]^N`.
    pub initial_fraction: f64,
    /// Final physical parameter values in `input.targets` order.
    pub parameters: Vec<f64>,
    /// Sum of squared scaled `FIT` residuals at this start's final point.
    pub fit_sse: f64,
    /// Whether first-order stationarity (`gradient < 1e-6`) was reached before `max_iterations`.
    pub converged: bool,
    /// Levenberg-Marquardt iterations executed.
    pub iterations: usize,
}

/// Bounded numerical recovery counters during exploratory `FIT` probes.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SearchRecovery {
    /// Exploratory forward solves refused by domain or Newton convergence gates.
    pub refused_forward_solves: usize,
    /// Finite-difference step halvings after a refused probe.
    pub sensitivity_halvings: usize,
    /// Sensitivity columns computed one-sided from the admitted center and one admissible neighbor.
    pub one_sided_sensitivity_columns: usize,
}

/// Predicted doubled-mesh state and signed channel residuals for one [`Observation`].
#[derive(Clone, Debug, Serialize)]
pub struct Prediction {
    /// Parent experiment ID.
    pub experiment: String,
    /// Observation ID.
    pub observation: String,
    /// `FIT` or held-out `CHECK` partition.
    pub partition: Partition,
    /// Waypoint index in `experiment.loading`.
    pub waypoint_index: usize,
    /// Protocol rate and pretension audit for the parent experiment.
    pub protocol_audit: ProtocolAudit,
    /// Production 3D cylinder state on the doubled `[2 * n_theta, 2 * n_z]` mesh.
    pub state: State,
    /// Predicted minus observed tare-adjusted upper axial force (`N`), when observed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub force_residual_n: Option<f64>,
    /// Predicted minus observed tare-adjusted upper axial torque (`N*m`), when observed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub torque_residual_nm: Option<f64>,
    /// Predicted minus observed mid-height mean cylinder radius (`m`), when observed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mean_mid_radius_residual_m: Option<f64>,
}

/// Complete cylindrical-holder calibration diagnostics and conditional frozen material publication.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// Equals [`REPORT_SCHEMA`] (`"ftx-cylinder-calibration-report/1"`).
    pub schema: String,
    /// Complete source input retained for exact audit and replay.
    pub input: Input,
    /// SHA-256 of canonical serialized `input`.
    pub input_sha256: String,
    /// Best `FIT` candidate values in `input.targets` order, whether or not accepted.
    pub parameters: Vec<f64>,
    /// Target keys, units and bounds in `parameters` order.
    pub parameter_descriptors: Vec<ParameterDescriptor>,
    /// Whether every convergence, excitation, mesh, `FIT`, `CHECK` and domain-replay gate passed.
    pub accepted: bool,
    /// Explicit reasons blocking publication when `accepted == false` (empty when `accepted == true`).
    pub reasons: Vec<String>,
    /// Published `Material` with `Evidence::Estimate` target properties only when `accepted == true`.
    pub material: Option<Material>,
    /// Frozen `Selection` carrying `input.allow_estimates` only when `accepted == true`.
    pub selection: Option<Selection>,
    /// Exact Rust JSON serialization of `selection` for lossless browser/CLI handoff.
    pub selection_json: Option<String>,
    /// All three deterministic starts (`0.25`, `0.5`, `0.75`).
    pub starts: Vec<Start>,
    /// Column-normalized `FIT` sensitivity Gram Cholesky pivot.
    pub excitation_pivot: f64,
    /// Total static and dynamic Newton equilibrium solves attempted across the entire run.
    pub forward_solves: usize,
    /// Present only when at least one exploratory `FIT` solve was refused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_recovery: Option<SearchRecovery>,
    /// `FIT` residual summary on the doubled mesh.
    pub fit: Quality,
    /// Held-out `CHECK` residual summary on the doubled mesh, when present.
    pub check: Option<Quality>,
    /// Largest coarse-to-doubled-mesh channel change in measurement-scale units.
    pub mesh_change_peak_scaled: f64,
    /// Whether every coarse and doubled `FIT` and `CHECK` experiment replayed cleanly inside `fitted_green_strain_range`.
    pub domain_replay_verified: bool,
    /// Pathwise `FIT` Green-strain envelope `[E_11, E_22, 2E_12]` including `[0, 0]` reference.
    pub fitted_green_strain_range: [[f64; 2]; 3],
    /// Predictions for all input observations in input order.
    pub predictions: Vec<Prediction>,
    /// Explicit physical and no-claim scope of this identification.
    pub model_scope: String,
}

#[derive(Clone)]
struct Baseline {
    mass_kg_m2: f64,
    warp_n_m: f64,
    weft_n_m: f64,
    poisson: f64,
    shear_n_m: f64,
    bending_nm: f64,
    membrane_response: Option<CompiledMembrane>,
    panel_bending: Option<BendingTensor>,
    self_contact: Option<(f64, f64)>,
}

impl Baseline {
    fn apply(&self, targets: &[TargetParameter], values: &[f64]) -> Self {
        let mut result = self.clone();
        for (target, &val) in targets.iter().zip(values) {
            match target.parameter {
                TargetKind::WarpStiffness => result.warp_n_m = val,
                TargetKind::WeftStiffness => result.weft_n_m = val,
                TargetKind::PoissonWarpWeft => result.poisson = val,
                TargetKind::ShearStiffness => result.shear_n_m = val,
                TargetKind::BendingRigidity => result.bending_nm = val,
            }
        }
        result
    }
}

enum ForwardFailure {
    Fatal(Refusal),
    Trial(Refusal),
}

impl ForwardFailure {
    fn refusal(self) -> Refusal {
        match self {
            Self::Fatal(r) | Self::Trial(r) => r,
        }
    }
}

impl From<Refusal> for ForwardFailure {
    fn from(r: Refusal) -> Self {
        Self::Fatal(r)
    }
}

struct ExperimentRun {
    waypoint_states: Vec<State>,
    path_strain_range: [[f64; 2]; 3],
    protocol_audit: ProtocolAudit,
}

struct Budget<'a> {
    used: usize,
    limit: usize,
    inherited_domain: Option<StrainDomain>,
    baseline: Baseline,
    input: &'a Input,
    recovery: SearchRecovery,
}

fn intersect(a: StrainDomain, b: StrainDomain) -> Result<StrainDomain, Refusal> {
    let a = a.ranges();
    let b = b.ranges();
    StrainDomain::new(std::array::from_fn(|i| {
        [a[i][0].max(b[i][0]), a[i][1].min(b[i][1])]
    }))
}

fn merge_strain_range(acc: &mut [[f64; 2]; 3], next: [[f64; 2]; 3]) {
    for (dst, src) in acc.iter_mut().zip(next) {
        dst[0] = dst[0].min(src[0]);
        dst[1] = dst[1].max(src[1]);
    }
}

fn validate_loading_counts(
    fixture: &Fixture,
    loading: &[Waypoint],
    controls: &Controls,
) -> Result<Vec<usize>, Refusal> {
    fixture.validate()?;
    controls.validate()?;
    if loading.is_empty() || loading.len() > 129 {
        return Err(invalid(
            "cylinder calibration experiment loading requires 1..=129 waypoints",
        ));
    }
    if loading[0].time_s != 0.0 {
        return Err(invalid(
            "cylinder calibration experiment loading must start at time_s = 0",
        ));
    }
    holder_separation_m(fixture, &loading[0])?;
    let mut total_steps = 0usize;
    let mut counts = Vec::with_capacity(loading.len().saturating_sub(1));
    for pair in loading.windows(2) {
        holder_separation_m(fixture, &pair[1])?;
        let dt = pair[1].time_s - pair[0].time_s;
        if !dt.is_finite() || dt <= 0.0 {
            return Err(invalid("cylinder loading times must increase strictly"));
        }
        if pair[1].clamp_slip_m + 1e-15 < pair[0].clamp_slip_m {
            return Err(invalid(
                "clamp_slip_m is cumulative irreversible pull-out and must not decrease",
            ));
        }
        let raw_count = (dt / controls.max_step_s).ceil().max(1.0);
        if !raw_count.is_finite()
            || raw_count > controls.max_steps.saturating_sub(total_steps) as f64
        {
            return Err(budget(
                "cylinder calibration experiment exceeds controls.max_steps",
            ));
        }
        let count = raw_count as usize;
        total_steps += count;
        counts.push(count);
    }
    let [n_theta, n_z] = fixture.cells;
    if n_theta * (n_z + 1) * (total_steps + 1) > 524_288 {
        return Err(budget(
            "cylinder calibration experiment exceeds 524288 vertex states",
        ));
    }
    Ok(counts)
}

impl Budget<'_> {
    fn admit(&mut self) -> Result<(), Refusal> {
        if self.used >= self.limit {
            return Err(budget(
                "cylinder calibration exhausted policy.max_forward_solves; no material is published",
            ));
        }
        self.used += 1;
        Ok(())
    }

    fn within_inherited_domain(&self, domain: StrainDomain) -> Result<StrainDomain, Refusal> {
        let Some(inherited) = self.inherited_domain else {
            return Ok(domain);
        };
        intersect(inherited, domain)
    }

    fn execute(
        &mut self,
        experiment: &Experiment,
        fixture: &Fixture,
        parameters: &[f64],
        domain: Option<StrainDomain>,
    ) -> Result<ExperimentRun, ForwardFailure> {
        let coeffs = self.baseline.apply(&self.input.targets, parameters);
        let membrane = Membrane::orthotropic(
            coeffs.warp_n_m,
            coeffs.weft_n_m,
            coeffs.poisson,
            coeffs.shear_n_m,
            1.0,
        );
        membrane.validate()?;
        if !coeffs.bending_nm.is_finite() || coeffs.bending_nm < 0.0 {
            return Err(out_of_range("bending_rigidity must be finite and nonnegative").into());
        }
        let any_net_compression = experiment.loading.iter().any(|w| {
            fixture.pretension_displacement_m + w.axial_displacement_m - w.clamp_slip_m < -1e-12
        });
        if any_net_compression && coeffs.bending_nm == 0.0 && coeffs.panel_bending.is_none() {
            return Err(invalid(
                "cylinder axial compression below nominal_free_height_m requires positive bending_rigidity or a BendingTensor",
            )
            .into());
        }
        let counts = validate_loading_counts(fixture, &experiment.loading, &self.input.controls)?;
        let protocol_audit = audit_protocol(
            fixture,
            &experiment.loading,
            experiment.nominal_protocol.as_ref(),
            self.input
                .material
                .structure
                .get("source_discrepancy")
                .cloned(),
        )?;
        let mesh = build_cylinder_mesh(fixture);
        let self_contact = if fixture.self_contact {
            coeffs.self_contact
        } else {
            None
        };

        self.admit()?;
        let map_trial = |refusal: Refusal| match refusal.code {
            RefusalCode::OutOfRange
            | RefusalCode::NotConverged
            | RefusalCode::DegenerateGeometry => ForwardFailure::Trial(refusal),
            _ => ForwardFailure::Fatal(refusal),
        };

        let (mut solver, initial_solve) = assemble_solver(
            fixture,
            &self.input.controls,
            &mesh,
            membrane,
            coeffs.bending_nm,
            coeffs.mass_kg_m2,
            coeffs.membrane_response,
            domain,
            coeffs.panel_bending,
            None,
            None,
            None,
            self_contact,
            &experiment.loading[0],
        )
        .map_err(map_trial)?;

        let zero_vel = vec![Vec3::ZERO; mesh.n_verts];
        let first_state = record_state(
            fixture,
            &self.input.controls,
            &solver,
            &experiment.loading[0],
            0.0,
            &zero_vel,
            initial_solve,
        )
        .map_err(map_trial)?;

        let mut path_strain_range = first_state.green_strain_range;
        let mut waypoint_states = Vec::with_capacity(experiment.loading.len());
        waypoint_states.push(first_state);

        let [n_theta, n_z] = fixture.cells;
        let mut current_time = 0.0_f64;
        let mut prev_sep = holder_separation_m(fixture, &experiment.loading[0])?;
        let mut prev_rot_rad = experiment.loading[0].rotation_deg.to_radians();

        for (pair, count) in experiment.loading.windows(2).zip(counts) {
            let dt = pair[1].time_s - pair[0].time_s;
            for step in 1..=count {
                let frac = step as f64 / count as f64;
                let waypoint = if step == count {
                    pair[1].clone()
                } else {
                    Waypoint {
                        time_s: dt.mul_add(frac, pair[0].time_s),
                        axial_displacement_m: (pair[1].axial_displacement_m
                            - pair[0].axial_displacement_m)
                            .mul_add(frac, pair[0].axial_displacement_m),
                        rotation_deg: (pair[1].rotation_deg - pair[0].rotation_deg)
                            .mul_add(frac, pair[0].rotation_deg),
                        clamp_slip_m: (pair[1].clamp_slip_m - pair[0].clamp_slip_m)
                            .mul_add(frac, pair[0].clamp_slip_m),
                    }
                };
                let h = waypoint.time_s - current_time;
                if !h.is_finite() || h <= 0.0 {
                    return Err(invalid("cylinder time step is not representable").into());
                }
                let curr_sep = holder_separation_m(fixture, &waypoint)?;
                let curr_rot_rad = waypoint.rotation_deg.to_radians();
                let vz_m_s = (curr_sep - prev_sep) / h;
                let omega_rad_s = (curr_rot_rad - prev_rot_rad) / h;

                self.admit()?;
                let old_velocity = solver.v.clone();
                let guess = self_contact.is_none().then(|| {
                    super::affine_cylinder_guess(
                        &solver.x,
                        n_theta,
                        n_z,
                        curr_sep - prev_sep,
                        curr_rot_rad - prev_rot_rad,
                    )
                });
                set_upper_ring(
                    &mut solver.x,
                    n_theta,
                    n_z,
                    mesh.mesh_radius_m,
                    curr_sep,
                    waypoint.rotation_deg,
                );
                let numerical = solver
                    .step_with_guess(h, guess.as_deref())
                    .map_err(map_trial)?;
                for i in 0..n_theta {
                    let idx = n_z * n_theta + i;
                    let p = solver.x[idx];
                    solver.v[idx] = Vec3::new(-omega_rad_s * p.y, omega_rad_s * p.x, vz_m_s);
                }
                let st = record_state(
                    fixture,
                    &self.input.controls,
                    &solver,
                    &waypoint,
                    h,
                    &old_velocity,
                    Some(numerical),
                )
                .map_err(map_trial)?;
                merge_strain_range(&mut path_strain_range, st.green_strain_range);
                if step == count {
                    waypoint_states.push(st);
                }
                current_time = waypoint.time_s;
                prev_sep = curr_sep;
                prev_rot_rad = curr_rot_rad;
            }
        }

        Ok(ExperimentRun {
            waypoint_states,
            path_strain_range,
            protocol_audit,
        })
    }

    fn solve(
        &mut self,
        experiment: &Experiment,
        fixture: &Fixture,
        parameters: &[f64],
    ) -> Result<ExperimentRun, Refusal> {
        self.execute(experiment, fixture, parameters, self.inherited_domain)
            .map_err(ForwardFailure::refusal)
    }

    fn replay(
        &mut self,
        experiment: &Experiment,
        fixture: &Fixture,
        parameters: &[f64],
        domain: StrainDomain,
    ) -> Result<ExperimentRun, Refusal> {
        let enforced = self.within_inherited_domain(domain)?;
        self.execute(experiment, fixture, parameters, Some(enforced))
            .map_err(ForwardFailure::refusal)
    }
}

fn resolve_baseline_and_validate(input: &Input) -> Result<Baseline, Refusal> {
    input.material.validate()?;
    input.controls.validate()?;
    if input.schema != INPUT_SCHEMA {
        return Err(invalid(format!(
            "unsupported cylinder calibration schema {}; expected {INPUT_SCHEMA}",
            input.schema
        )));
    }
    if !input.admit_cylindrical_shell_model
        || input.method.trim().is_empty()
        || input.fitted_on.trim().is_empty()
        || matches!(input.material.form, Form::Fiber | Form::Yarn | Form::Fill)
        || input.experiments.is_empty()
        || input.experiments.len() > 16
        || input.targets.is_empty()
        || input.targets.len() > 5
    {
        return Err(invalid(
            "cylinder calibration requires schema ftx-cylinder-calibration/1, explicit cylindrical-shell model consent, a sheet material, method/date, 1..=5 targets and 1..=16 experiments",
        ));
    }
    if input.material.membrane_relaxation.is_some()
        || input
            .material
            .structure
            .contains_key("membrane_plasticity_model")
        || input
            .material
            .structure
            .contains_key("bending_plasticity_model")
    {
        return Err(invalid(
            "cylinder scalar calibration cannot erase existing viscoelastic or plastic history laws",
        ));
    }
    let policy = &input.policy;
    if [
        policy.max_rms_scaled,
        policy.max_peak_scaled,
        policy.max_mesh_scaled,
        policy.min_excitation_pivot,
    ]
    .iter()
    .any(|v| !v.is_finite() || *v <= 0.0)
        || policy.min_excitation_pivot >= 1.0
        || !(1..=80).contains(&policy.max_iterations)
        || !(1..=32_768).contains(&policy.max_forward_solves)
    {
        return Err(invalid(
            "invalid cylinder residual, mesh-refinement, excitation or bounded-work policy",
        ));
    }

    let mut target_kinds = BTreeSet::new();
    let mut fits_membrane = false;
    let mut fits_bending = false;
    for target in &input.targets {
        if !target_kinds.insert(target.parameter) {
            return Err(invalid(format!(
                "duplicate cylinder calibration target {}",
                target.parameter.key()
            )));
        }
        let [lo, hi] = target.bounds;
        if !lo.is_finite()
            || !hi.is_finite()
            || hi <= lo
            || !(hi - lo).is_finite()
            || (target.parameter.requires_positive_lower_bound() && lo <= 0.0)
        {
            return Err(invalid(format!(
                "cylinder target {} requires finite bounds [lo, hi] with hi > lo{}",
                target.parameter.key(),
                if target.parameter.requires_positive_lower_bound() {
                    " and lo > 0"
                } else {
                    ""
                }
            )));
        }
        if target.parameter == TargetKind::BendingRigidity {
            fits_bending = true;
        } else {
            fits_membrane = true;
        }
        if !input.replace_existing
            && input
                .material
                .properties
                .contains_key(target.parameter.key())
        {
            return Err(invalid(format!(
                "cylinder calibration would replace existing {}; explicit replace_existing is required",
                target.parameter.key()
            )));
        }
    }
    if fits_membrane && input.material.membrane_response.is_some() {
        return Err(invalid(
            "cylinder scalar membrane identification cannot discard an existing nonlinear membrane_response",
        ));
    }
    if fits_bending && input.material.structure.contains_key("bending_model") {
        return Err(invalid(
            "cylinder scalar bending_rigidity identification cannot discard an existing directional bending_response",
        ));
    }

    let allow_obs = input.allow_estimated_observations;
    let mass_kg_m2 = input.material.mechanical_areal_density(allow_obs)?;
    let resolved = textile_import::catalog()?.resolve(&Selection {
        id: input.material.id.clone(),
        record: Some(input.material.clone()),
        allow_estimates: allow_obs,
        ..Selection::default()
    })?;
    // Untargeted constitutive laws are the same source-qualified authority used by the public
    // forward experiment and the published selection, never a scalar surrogate during fitting.
    let membrane_response = resolved.membrane_response()?;
    let panel_bending = resolved.bending_response()?;
    let reference_moduli = membrane_response
        .as_ref()
        .map(CompiledMembrane::reference_moduli)
        .transpose()?;
    let membrane_value = |index, key, unit| {
        reference_moduli.map_or_else(
            || input.material.require(key, unit, allow_obs),
            |values| Ok(values[index]),
        )
    };
    let find_target = |kind: TargetKind| {
        input
            .targets
            .iter()
            .find(|t| t.parameter == kind)
            .map(|t| t.bounds)
    };
    let warp_bounds = match find_target(TargetKind::WarpStiffness) {
        Some(b) => b,
        None => {
            let v = membrane_value(0, "warp_stiffness", "N/m")?;
            [v, v]
        }
    };
    let weft_bounds = match find_target(TargetKind::WeftStiffness) {
        Some(b) => b,
        None => {
            let v = membrane_value(1, "weft_stiffness", "N/m")?;
            [v, v]
        }
    };
    let nu_bounds = match find_target(TargetKind::PoissonWarpWeft) {
        Some(b) => b,
        None => {
            let v = membrane_value(2, "poisson_warp_weft", "1")?;
            [v, v]
        }
    };
    let shear_bounds = match find_target(TargetKind::ShearStiffness) {
        Some(b) => b,
        None => {
            let v = membrane_value(3, "shear_stiffness", "N/m")?;
            [v, v]
        }
    };
    let bending_bounds = match find_target(TargetKind::BendingRigidity) {
        Some(b) => b,
        None => {
            let v = if input.material.properties.contains_key("bending_rigidity") {
                let b = input
                    .material
                    .require("bending_rigidity", "N*m", allow_obs)?;
                if b < 0.0 {
                    return Err(out_of_range("bending_rigidity must be nonnegative"));
                }
                b
            } else {
                0.0
            };
            [v, v]
        }
    };

    let nu_abs_max = nu_bounds[0].abs().max(nu_bounds[1].abs());
    if warp_bounds[0] <= 0.0
        || weft_bounds[0] <= 0.0
        || shear_bounds[0] <= 0.0
        || nu_abs_max * nu_abs_max * weft_bounds[1] / warp_bounds[0] >= 1.0
    {
        return Err(invalid(
            "the entire declared orthotropic parameter box must be positive-definite",
        ));
    }
    Membrane::orthotropic(
        warp_bounds[0],
        weft_bounds[0],
        nu_bounds[0],
        shear_bounds[0],
        1.0,
    )
    .validate()?;
    Membrane::orthotropic(
        warp_bounds[0],
        weft_bounds[1],
        nu_bounds[1],
        shear_bounds[0],
        1.0,
    )
    .validate()?;

    let needs_self_contact = input.experiments.iter().any(|e| e.fixture.self_contact);
    let self_contact = if needs_self_contact {
        let thickness_m = resolved.unloaded_thickness()?.value;
        let mu_self = input.material.require("friction_self", "1", allow_obs)?;
        if mu_self < 0.0 {
            return Err(out_of_range("friction_self must be nonnegative"));
        }
        Some((thickness_m, mu_self))
    } else {
        None
    };

    let total_observations: usize = input.experiments.iter().map(|e| e.observations.len()).sum();
    if total_observations > 64 {
        return Err(invalid(
            "cylinder calibration admits at most 64 observed states across all experiments",
        ));
    }

    let mut exp_ids = BTreeSet::new();
    let mut obs_ids = BTreeSet::new();
    let mut fit_channels = 0usize;
    for experiment in &input.experiments {
        validate_loading_counts(&experiment.fixture, &experiment.loading, &input.controls)?;
        let mut doubled = experiment.fixture.clone();
        doubled.cells = doubled.cells.map(|n| n * 2);
        doubled.validate().map_err(|_| {
            invalid(
                "inverse cylinder meshes must permit doubling [2 * n_theta, 2 * n_z] within the 64x32 and 1024-cell forward limit",
            )
        })?;
        validate_loading_counts(&doubled, &experiment.loading, &input.controls)?;
        audit_protocol(
            &experiment.fixture,
            &experiment.loading,
            experiment.nominal_protocol.as_ref(),
            None,
        )?;
        if experiment.id.trim().is_empty()
            || !exp_ids.insert(&experiment.id)
            || !input.material.sources.contains_key(&experiment.source)
            || experiment.observations.is_empty()
            || (experiment.evidence == Evidence::Estimate && !allow_obs)
        {
            return Err(invalid(
                "cylinder experiment needs a unique ID, retained material source, observations and admitted evidence",
            ));
        }
        let any_net_compression = experiment.loading.iter().any(|w| {
            experiment.fixture.pretension_displacement_m + w.axial_displacement_m - w.clamp_slip_m
                < -1e-12
        });
        if any_net_compression && bending_bounds[0] <= 0.0 && panel_bending.is_none() {
            return Err(invalid(
                "cylinder axial compression below nominal_free_height_m requires positive bending_rigidity or a BendingTensor across the entire search domain",
            ));
        }
        let mut seen_waypoints = BTreeSet::new();
        for obs in &experiment.observations {
            if obs.id.trim().is_empty()
                || !obs_ids.insert(&obs.id)
                || obs.waypoint_index >= experiment.loading.len()
                || !seen_waypoints.insert(obs.waypoint_index)
                || obs.channel_count() == 0
            {
                return Err(invalid(
                    "cylinder observation needs a unique ID, valid unique waypoint_index and at least one measured channel",
                ));
            }
            for ch in [&obs.force_n, &obs.torque_nm].into_iter().flatten() {
                if !ch.value.is_finite() || !ch.scale.is_finite() || ch.scale <= 0.0 {
                    return Err(invalid(
                        "cylinder force and torque channels require finite values and positive residual scales",
                    ));
                }
            }
            if let Some(radius) = &obs.mean_mid_radius_m
                && (!radius.value.is_finite()
                    || radius.value <= 0.0
                    || !radius.scale.is_finite()
                    || radius.scale <= 0.0)
            {
                return Err(invalid(
                    "cylinder mean_mid_radius_m channel requires positive finite value and scale",
                ));
            }
            if experiment.partition == Partition::Fit {
                fit_channels += obs.channel_count();
            }
        }
    }
    if fit_channels < input.targets.len() {
        return Err(invalid(
            "cylinder calibration requires at least one measured FIT channel per target parameter",
        ));
    }

    for fit in input
        .experiments
        .iter()
        .filter(|e| e.partition == Partition::Fit)
    {
        for check in input
            .experiments
            .iter()
            .filter(|e| e.partition == Partition::Check)
        {
            let fit_source = &input.material.sources[&fit.source].reference;
            let check_source = &input.material.sources[&check.source].reference;
            let same_grain = fit.fixture.warp_axis == check.fixture.warp_axis
                || fit.fixture.warp_axis == check.fixture.warp_axis.map(|v| -v);
            let same_loading = fit.loading.len() == check.loading.len()
                && fit.loading.iter().zip(&check.loading).all(|(a, b)| {
                    a.time_s == b.time_s
                        && a.axial_displacement_m == b.axial_displacement_m
                        && a.rotation_deg == b.rotation_deg
                        && a.clamp_slip_m == b.clamp_slip_m
                });
            if fit_source == check_source
                && fit.fixture.holder_radius_m == check.fixture.holder_radius_m
                && fit.fixture.nominal_free_height_m == check.fixture.nominal_free_height_m
                && fit.fixture.pretension_displacement_m == check.fixture.pretension_displacement_m
                && same_grain
                && same_loading
            {
                return Err(invalid(
                    "FIT and CHECK reuse the same source acquisition, fixture, grain axis and loading protocol; renamed rows or changed meshes do not establish independent held-out evidence",
                ));
            }
        }
    }

    Ok(Baseline {
        mass_kg_m2,
        warp_n_m: 0.5 * (warp_bounds[0] + warp_bounds[1]),
        weft_n_m: 0.5 * (weft_bounds[0] + weft_bounds[1]),
        poisson: 0.5 * (nu_bounds[0] + nu_bounds[1]),
        shear_n_m: 0.5 * (shear_bounds[0] + shear_bounds[1]),
        bending_nm: 0.5 * (bending_bounds[0] + bending_bounds[1]),
        membrane_response,
        panel_bending,
        self_contact,
    })
}

fn physical<const N: usize>(input: &Input, p: [f64; N]) -> [f64; N] {
    std::array::from_fn(|i| {
        let [lo, hi] = input.targets[i].bounds;
        (hi - lo).mul_add(p[i], lo)
    })
}

struct Evaluation {
    residuals: Vec<f64>,
    sse: f64,
}

fn append_observation_residuals(obs: &Observation, state: &State, residuals: &mut Vec<f64>) {
    if let Some(force) = &obs.force_n {
        residuals.push((state.tare_adjusted_force_n - force.value) / force.scale);
    }
    if let Some(torque) = &obs.torque_nm {
        residuals.push((state.tare_adjusted_torque_nm - torque.value) / torque.scale);
    }
    if let Some(radius) = &obs.mean_mid_radius_m {
        residuals.push((state.mid_radius_range_m[2] - radius.value) / radius.scale);
    }
}

fn evaluate_fit<const N: usize>(
    input: &Input,
    p: [f64; N],
    budget: &mut Budget<'_>,
) -> Result<Evaluation, ForwardFailure> {
    let parameters = physical(input, p);
    let mut residuals = Vec::new();
    for experiment in input
        .experiments
        .iter()
        .filter(|e| e.partition == Partition::Fit)
    {
        let run = match budget.execute(
            experiment,
            &experiment.fixture,
            &parameters,
            budget.inherited_domain,
        ) {
            Ok(run) => run,
            Err(failure @ ForwardFailure::Trial(_)) => {
                budget.recovery.refused_forward_solves += 1;
                return Err(failure);
            }
            Err(failure) => return Err(failure),
        };
        for obs in &experiment.observations {
            append_observation_residuals(
                obs,
                &run.waypoint_states[obs.waypoint_index],
                &mut residuals,
            );
        }
    }
    let sse = residuals.iter().map(|v| v * v).sum::<f64>();
    if !sse.is_finite() {
        return Err(invalid("cylinder calibration objective overflow").into());
    }
    Ok(Evaluation { residuals, sse })
}

fn jacobian<const N: usize>(
    input: &Input,
    p: [f64; N],
    budget: &mut Budget<'_>,
    center: Option<&Evaluation>,
) -> Result<Vec<[f64; N]>, Refusal> {
    let mut rows = Vec::new();
    let mut computed_center = None;
    for j in 0..N {
        let mut delta = 1e-4_f64;
        let mut admissible = [None, None];
        let mut refused = None;
        let mut column = None;
        if let Some(middle) = center {
            // Forward/backward finite difference reusing the already-evaluated center point during LM steps.
            for attempt in 0..=8 {
                let first = if p[j] + delta <= 1.0 {
                    (p[j] + delta).min(1.0)
                } else {
                    (p[j] - delta).max(0.0)
                };
                let second = if p[j] + delta <= 1.0 {
                    (p[j] - delta).max(0.0)
                } else {
                    (p[j] + delta).min(1.0)
                };
                for coordinate in [first, second] {
                    if (coordinate - p[j]).abs() == 0.0 {
                        continue;
                    }
                    let mut probe = p;
                    probe[j] = coordinate;
                    match evaluate_fit(input, probe, budget) {
                        Ok(neighbor) => {
                            column = Some(
                                middle
                                    .residuals
                                    .iter()
                                    .zip(&neighbor.residuals)
                                    .map(|(a, b)| (b - a) / (coordinate - p[j]))
                                    .collect::<Vec<_>>(),
                            );
                            break;
                        }
                        Err(ForwardFailure::Trial(r)) => {
                            refused = Some(r);
                        }
                        Err(ForwardFailure::Fatal(r)) => return Err(r),
                    }
                }
                if column.is_some() {
                    break;
                }
                if attempt < 8 {
                    delta *= 0.5;
                    budget.recovery.sensitivity_halvings += 1;
                }
            }
            if column.is_none() {
                return Err(refused.unwrap_or_else(|| {
                    invalid("cylinder sensitivity has no distinct admissible neighboring state")
                }));
            }
        } else {
            // Central difference at the final solution for second-order excitation certification.
            for attempt in 0..=8 {
                let coordinates = [(p[j] - delta).max(0.0), (p[j] + delta).min(1.0)];
                let mut pair = [None, None];
                for (side, coordinate) in coordinates.into_iter().enumerate() {
                    let mut probe = p;
                    probe[j] = coordinate;
                    match evaluate_fit(input, probe, budget) {
                        Ok(value) => pair[side] = Some((coordinate, value)),
                        Err(ForwardFailure::Trial(r)) => {
                            refused = Some(r);
                        }
                        Err(ForwardFailure::Fatal(r)) => return Err(r),
                    }
                }
                if let [Some((lo, left)), Some((hi, right))] = &pair
                    && (hi - lo).abs() > 0.0
                {
                    column = Some(
                        left.residuals
                            .iter()
                            .zip(&right.residuals)
                            .map(|(a, b)| (b - a) / (hi - lo))
                            .collect::<Vec<_>>(),
                    );
                    break;
                }
                for (retained, candidate) in admissible.iter_mut().zip(pair) {
                    if candidate.as_ref().is_some_and(|(q, _)| *q != p[j]) {
                        *retained = candidate;
                    }
                }
                if attempt < 8 {
                    delta *= 0.5;
                    budget.recovery.sensitivity_halvings += 1;
                }
            }
        }
        let column = if let Some(column) = column {
            column
        } else {
            let Some((coordinate, neighbor)) = admissible
                .iter()
                .flatten()
                .min_by(|a, b| (a.0 - p[j]).abs().total_cmp(&(b.0 - p[j]).abs()))
            else {
                return Err(refused.unwrap_or_else(|| {
                    invalid("cylinder sensitivity has no distinct admissible neighboring state")
                }));
            };
            if computed_center.is_none() {
                computed_center =
                    Some(evaluate_fit(input, p, budget).map_err(ForwardFailure::refusal)?);
            }
            let middle = computed_center
                .as_ref()
                .ok_or_else(|| invalid("cylinder sensitivity requires an admitted center state"))?;
            budget.recovery.one_sided_sensitivity_columns += 1;
            middle
                .residuals
                .iter()
                .zip(&neighbor.residuals)
                .map(|(a, b)| (b - a) / (coordinate - p[j]))
                .collect()
        };
        if rows.is_empty() {
            rows = vec![[0.0; N]; column.len()];
        }
        for (row, value) in rows.iter_mut().zip(column) {
            row[j] = value;
        }
    }
    if rows.iter().flatten().any(|v| !v.is_finite()) {
        return Err(invalid("cylinder sensitivity overflow"));
    }
    Ok(rows)
}

fn norms<const N: usize>(j: &[[f64; N]]) -> [f64; N] {
    std::array::from_fn(|k| j.iter().map(|row| row[k] * row[k]).sum::<f64>().sqrt())
}

fn gradient<const N: usize>(p: [f64; N], j: &[[f64; N]], e: &Evaluation) -> f64 {
    let scales = norms(j);
    (0..N)
        .map(|k| {
            let value = j
                .iter()
                .zip(&e.residuals)
                .map(|(row, r)| row[k] * r)
                .sum::<f64>();
            let projected = if (p[k] <= 0.0 && value > 0.0) || (p[k] >= 1.0 && value < 0.0) {
                0.0
            } else {
                value
            };
            if scales[k] == 0.0 {
                0.0
            } else {
                projected.abs() / (scales[k] * (1.0 + e.sse.sqrt()))
            }
        })
        .fold(0.0, f64::max)
}

fn excitation<const N: usize>(j: &[[f64; N]]) -> f64 {
    let scales = norms(j);
    if scales.iter().any(|s| !s.is_finite() || *s <= 1e-10) {
        return 0.0;
    }
    let mut lower = [[0.0; N]; N];
    let mut pivot = 1.0_f64;
    for i in 0..N {
        for k in 0..=i {
            let dot = j
                .iter()
                .map(|row| (row[i] / scales[i]) * (row[k] / scales[k]))
                .sum::<f64>();
            let value = dot - (0..k).map(|n| lower[i][n] * lower[k][n]).sum::<f64>();
            if i == k {
                if !value.is_finite() || value <= 0.0 {
                    return 0.0;
                }
                lower[i][k] = value.sqrt();
                pivot = pivot.min(value);
            } else {
                lower[i][k] = value / lower[k][k];
            }
        }
    }
    pivot
}

fn search<const N: usize>(
    input: &Input,
    initial: f64,
    budget: &mut Budget<'_>,
) -> Result<(Start, [f64; N]), Refusal> {
    let mut p = [initial; N];
    let mut e = evaluate_fit(input, p, budget).map_err(ForwardFailure::refusal)?;
    let mut lambda = 1e-3_f64;
    let mut converged = false;
    let mut iterations = 0;
    for _ in 0..input.policy.max_iterations {
        iterations += 1;
        let j = jacobian(input, p, budget, Some(&e))?;
        if gradient(p, &j, &e) < 1e-6 {
            converged = true;
            break;
        }
        let scales = norms(&j);
        if scales.iter().any(|s| *s <= 1e-10) {
            break;
        }
        let active: [bool; N] = std::array::from_fn(|k| {
            let g = j
                .iter()
                .zip(&e.residuals)
                .map(|(row, r)| row[k] * r)
                .sum::<f64>();
            (p[k] <= 0.0 && g > 0.0) || (p[k] >= 1.0 && g < 0.0)
        });
        let mut accepted = false;
        for _ in 0..10 {
            let mut rows: Vec<[f64; N]> = j
                .iter()
                .map(|row| {
                    std::array::from_fn(|k| if active[k] { 0.0 } else { row[k] / scales[k] })
                })
                .collect();
            let mut target: Vec<f64> = e.residuals.iter().map(|r| -r).collect();
            for k in 0..N {
                let mut row = [0.0; N];
                row[k] = if active[k] { 1.0 } else { lambda.sqrt() };
                rows.push(row);
                target.push(0.0);
            }
            let scaled = lstsq(&rows, &target);
            let mut step: [f64; N] = std::array::from_fn(|k| {
                if active[k] {
                    0.0
                } else {
                    scaled[k] / scales[k]
                }
            });
            let max = step.iter().map(|v| v.abs()).fold(0.0, f64::max);
            if !max.is_finite() {
                return Err(invalid("cylinder inverse step overflow"));
            }
            if max > 0.25 {
                step.iter_mut().for_each(|v| *v *= 0.25 / max);
            }
            let candidate = std::array::from_fn(|k| (p[k] + step[k]).clamp(0.0, 1.0));
            let moved = (0..N)
                .map(|k| (candidate[k] - p[k]).abs())
                .fold(0.0, f64::max);
            if moved < 1e-11 {
                converged = gradient(p, &j, &e) < 1e-4;
                break;
            }
            let trial = match evaluate_fit(input, candidate, budget) {
                Ok(trial) => trial,
                Err(ForwardFailure::Trial(_)) => {
                    lambda *= 10.0;
                    continue;
                }
                Err(ForwardFailure::Fatal(r)) => return Err(r),
            };
            if trial.sse < e.sse {
                p = candidate;
                e = trial;
                lambda = (lambda * 0.2).max(1e-10);
                accepted = true;
                break;
            }
            lambda *= 10.0;
        }
        if !accepted {
            break;
        }
    }
    Ok((
        Start {
            initial_fraction: initial,
            parameters: physical(input, p).to_vec(),
            fit_sse: e.sse,
            converged,
            iterations,
        },
        p,
    ))
}

fn quality(values: &[f64]) -> Quality {
    Quality {
        components: values.len(),
        rms_scaled: (values.iter().map(|v| v * v).sum::<f64>() / values.len() as f64).sqrt(),
        peak_scaled: values.iter().map(|v| v.abs()).fold(0.0, f64::max),
    }
}

/// Identify caller-declared canonical membrane/bending parameters from cylindrical-holder experiments.
///
/// Every trial solves the actual 3D cylindrical-holder model (`R_mesh` isometric chord wrap,
/// `L_0 + Δz_pre + Δz - u_slip`, tare subtraction, and `warp_axis` rotation). Three deterministic
/// starts, local sensitivity excitation, coarse-to-doubled-mesh stability, held-out `CHECK`
/// verification, and `fitted_green_strain_range` replay are publication gates.
///
/// # Errors
/// Invalid geometry, source, evidence or parameter-box input, exhausted `max_forward_solves`,
/// or a non-recoverable forward failure. Inadequate fits return `Ok(Report)` with `accepted = false`
/// and `selection = None`.
pub fn calibrate(input: &Input) -> Result<Report, Refusal> {
    let baseline = resolve_baseline_and_validate(input)?;
    match input.targets.len() {
        1 => calibrate_n::<1>(input, baseline),
        2 => calibrate_n::<2>(input, baseline),
        3 => calibrate_n::<3>(input, baseline),
        4 => calibrate_n::<4>(input, baseline),
        5 => calibrate_n::<5>(input, baseline),
        _ => Err(invalid(
            "cylinder calibration supports 1..=5 target parameters",
        )),
    }
}

fn calibrate_n<const N: usize>(input: &Input, baseline: Baseline) -> Result<Report, Refusal> {
    let inherited_domain = fitted_domain(&input.material)?;
    if inherited_domain.is_some_and(|domain| !domain.contains([0.0; 3])) {
        return Err(invalid(
            "the inherited material domain must admit the undeformed reference state",
        ));
    }

    let mut budget = Budget {
        used: 0,
        limit: input.policy.max_forward_solves,
        inherited_domain,
        baseline,
        input,
        recovery: SearchRecovery::default(),
    };

    let mut starts = Vec::with_capacity(3);
    let mut coordinates = Vec::with_capacity(3);
    for initial in [0.25, 0.5, 0.75] {
        let (start, p) = search::<N>(input, initial, &mut budget)?;
        starts.push(start);
        coordinates.push(p);
    }
    let best = (0..starts.len())
        .min_by(|&a, &b| starts[a].fit_sse.total_cmp(&starts[b].fit_sse))
        .unwrap_or(0);
    let p = coordinates[best];
    let parameters = starts[best].parameters.clone();
    let j = jacobian(input, p, &mut budget, None)?;
    let pivot = excitation(&j);

    let mut reasons = Vec::new();
    if starts.iter().any(|s| !s.converged) {
        reasons.push("not all three bounded starts reached objective stationarity".into());
    }
    if p.iter().any(|v| *v <= 2e-4 || *v >= 1.0 - 2e-4) {
        reasons
            .push("best fit lies at a parameter bound; expand justified bounds and repeat".into());
    }
    if pivot < input.policy.min_excitation_pivot {
        reasons.push(format!(
            "FIT cylinder reactions do not independently excite all {} target parameters (pivot {:.3e} < {:.3e})",
            parameters.len(),
            pivot,
            input.policy.min_excitation_pivot
        ));
    }
    for (i, start) in starts.iter().enumerate() {
        if start.fit_sse <= starts[best].fit_sse + 1e-8 * (1.0 + starts[best].fit_sse)
            && coordinates[i]
                .iter()
                .zip(p)
                .any(|(a, b)| (a - b).abs() > 0.02)
        {
            reasons.push("equivalent FIT minima have materially different parameters".into());
            break;
        }
    }

    let mut predictions = Vec::new();
    let mut fit_errors = Vec::new();
    let mut check_errors = Vec::new();
    let mut check_strain_ranges = Vec::new();
    let mut mesh_peak = 0.0_f64;
    let mut ranges = [[0.0_f64; 2]; 3];

    for experiment in &input.experiments {
        let mut refined = experiment.fixture.clone();
        refined.cells = refined.cells.map(|n| n * 2);
        let coarse_run = budget.solve(experiment, &experiment.fixture, &parameters)?;
        let refined_run = budget.solve(experiment, &refined, &parameters)?;

        if experiment.partition == Partition::Fit {
            merge_strain_range(&mut ranges, coarse_run.path_strain_range);
            merge_strain_range(&mut ranges, refined_run.path_strain_range);
        } else {
            check_strain_ranges.push(coarse_run.path_strain_range);
            check_strain_ranges.push(refined_run.path_strain_range);
        }

        for obs in &experiment.observations {
            let coarse_state = &coarse_run.waypoint_states[obs.waypoint_index];
            let state = refined_run.waypoint_states[obs.waypoint_index].clone();
            let errors = if experiment.partition == Partition::Fit {
                &mut fit_errors
            } else {
                &mut check_errors
            };

            let force_residual_n = obs.force_n.as_ref().map(|ch| {
                mesh_peak = mesh_peak.max(
                    (state.tare_adjusted_force_n - coarse_state.tare_adjusted_force_n).abs()
                        / ch.scale,
                );
                let err = state.tare_adjusted_force_n - ch.value;
                errors.push(err / ch.scale);
                err
            });
            let torque_residual_nm = obs.torque_nm.as_ref().map(|ch| {
                mesh_peak = mesh_peak.max(
                    (state.tare_adjusted_torque_nm - coarse_state.tare_adjusted_torque_nm).abs()
                        / ch.scale,
                );
                let err = state.tare_adjusted_torque_nm - ch.value;
                errors.push(err / ch.scale);
                err
            });
            let mean_mid_radius_residual_m = obs.mean_mid_radius_m.as_ref().map(|ch| {
                mesh_peak = mesh_peak.max(
                    (state.mid_radius_range_m[2] - coarse_state.mid_radius_range_m[2]).abs()
                        / ch.scale,
                );
                let err = state.mid_radius_range_m[2] - ch.value;
                errors.push(err / ch.scale);
                err
            });

            predictions.push(Prediction {
                experiment: experiment.id.clone(),
                observation: obs.id.clone(),
                partition: experiment.partition,
                waypoint_index: obs.waypoint_index,
                protocol_audit: refined_run.protocol_audit.clone(),
                state,
                force_residual_n,
                torque_residual_nm,
                mean_mid_radius_residual_m,
            });
        }
    }

    ranges = budget
        .within_inherited_domain(StrainDomain::new(ranges)?)?
        .ranges();

    let fit = quality(&fit_errors);
    let check = (!check_errors.is_empty()).then(|| quality(&check_errors));

    if check_strain_ranges.iter().any(|actual| {
        actual
            .iter()
            .zip(ranges)
            .any(|(a, fitted)| a[0] < fitted[0] || a[1] > fitted[1])
    }) {
        reasons.push(
            "held-out CHECK cylinder states escape the FIT-only local strain domain; CHECK data cannot enlarge the fitted law's domain".into(),
        );
    }
    if check.is_none() {
        reasons.push(
            "an independent held-out CHECK cylinder experiment is required before publishing a material"
                .into(),
        );
    }
    for (name, q) in [("FIT", Some(&fit)), ("CHECK", check.as_ref())] {
        if let Some(q) = q
            && (!q.rms_scaled.is_finite()
                || !q.peak_scaled.is_finite()
                || q.rms_scaled > input.policy.max_rms_scaled
                || q.peak_scaled > input.policy.max_peak_scaled)
        {
            reasons.push(format!(
                "{name} cylinder force/torque/radius residuals exceed the declared model budget"
            ));
        }
    }
    if !mesh_peak.is_finite() || mesh_peak > input.policy.max_mesh_scaled {
        reasons.push(
            "doubling the cylinder mesh changes predictions beyond the declared numerical budget"
                .into(),
        );
    }

    let mut domain_replay_verified = false;
    if reasons.is_empty() {
        let domain = StrainDomain::new(ranges)?;
        'replay: for experiment in &input.experiments {
            for factor in [1usize, 2usize] {
                let mut fixture = experiment.fixture.clone();
                fixture.cells = fixture.cells.map(|n| n * factor);
                if let Err(refusal) = budget.replay(experiment, &fixture, &parameters, domain) {
                    if refusal.code == RefusalCode::BudgetExceeded {
                        return Err(refusal);
                    }
                    reasons.push(format!(
                        "fitted-domain replay failed for {} at mesh factor {factor}: {}; no domain was widened",
                        experiment.id, refusal.message
                    ));
                    break 'replay;
                }
            }
        }
        domain_replay_verified = reasons.is_empty();
    }

    let digest = sha256_hex(&serde_json::to_vec(input).map_err(invalid)?);
    let material = if reasons.is_empty() {
        let published_domain = budget.within_inherited_domain(StrainDomain::new(ranges)?)?;
        Some(publish(
            input,
            &budget.baseline,
            &digest,
            &parameters,
            published_domain.ranges(),
        )?)
    } else {
        None
    };
    let selection = material
        .as_ref()
        .map(|mat| {
            textile_import::catalog()?
                .resolve(&Selection {
                    id: mat.id.clone(),
                    record: Some(mat.clone()),
                    allow_estimates: input.allow_estimates,
                    ..Selection::default()
                })
                .map(|m| m.frozen_selection())
        })
        .transpose()?;
    let selection_json = selection
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(invalid)?;

    let parameter_descriptors = input
        .targets
        .iter()
        .map(|t| ParameterDescriptor {
            key: t.parameter.key().into(),
            unit: t.parameter.unit().into(),
            bounds: t.bounds,
        })
        .collect();

    Ok(Report {
        schema: REPORT_SCHEMA.into(),
        input: input.clone(),
        input_sha256: digest,
        parameters,
        parameter_descriptors,
        accepted: material.is_some(),
        reasons,
        material,
        selection,
        selection_json,
        starts,
        excitation_pivot: pivot,
        forward_solves: budget.used,
        search_recovery: (budget.recovery.refused_forward_solves > 0)
            .then(|| budget.recovery.clone()),
        fit,
        check,
        mesh_change_peak_scaled: mesh_peak,
        domain_replay_verified,
        fitted_green_strain_range: ranges,
        predictions,
        model_scope: FIT_SCOPE.into(),
    })
}

fn publish(
    input: &Input,
    baseline: &Baseline,
    digest: &str,
    parameters: &[f64],
    ranges: [[f64; 2]; 3],
) -> Result<Material, Refusal> {
    let mut material = input.material.clone();
    let source = format!("cylinder-{digest}");
    if material.sources.contains_key(&source) {
        return Err(invalid("cylinder calibration source key already exists"));
    }
    material.sources.insert(
        source.clone(),
        Source {
            reference: format!("local:ftx-cylinder-calibration/{digest}"),
            accessed_on: input.fitted_on.clone(),
            note: format!(
                "Inverse 3D cylindrical-holder model; complete experiments, pretension/tare protocol audit, sensitivity and doubled-mesh checks retained in {REPORT_SCHEMA} input {digest}. {}",
                input.method
            ),
        },
    );
    let mut conditions = input.conditions.clone();
    conditions.insert(
        "fitted_green_strain_range".into(),
        serde_json::to_string(&ranges).map_err(invalid)?,
    );
    conditions.insert("held_out_checks".into(), "true".into());
    conditions.insert("fixture_model".into(), FIT_SCOPE.into());

    let coeffs = baseline.apply(&input.targets, parameters);
    let fits_membrane = input
        .targets
        .iter()
        .any(|t| t.parameter != TargetKind::BendingRigidity);
    let method = "Bounded inverse 3D cylindrical-holder shell identification using isometric chord-radius clamping, explicit pretension/tare, local sensitivity pivot and held-out doubled-mesh checks. No unloaded contact thickness or skin friction is inferred.";

    if fits_membrane {
        for (key, unit, value) in [
            ("warp_stiffness", "N/m", coeffs.warp_n_m),
            ("weft_stiffness", "N/m", coeffs.weft_n_m),
            ("poisson_warp_weft", "1", coeffs.poisson),
            ("shear_stiffness", "N/m", coeffs.shear_n_m),
        ] {
            material.properties.insert(
                key.into(),
                Quantity {
                    value,
                    unit: unit.into(),
                    evidence: Evidence::Estimate,
                    source: source.clone(),
                    method: method.into(),
                    conditions: conditions.clone(),
                    bounds: None,
                },
            );
        }
    }
    if input
        .targets
        .iter()
        .any(|t| t.parameter == TargetKind::BendingRigidity)
    {
        material.properties.insert(
            "bending_rigidity".into(),
            Quantity {
                value: coeffs.bending_nm,
                unit: "N*m".into(),
                evidence: Evidence::Estimate,
                source: source.clone(),
                method: method.into(),
                conditions,
                bounds: None,
            },
        );
    }
    material.validate()?;
    Ok(material)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cylinder;
    use serde_json::json;

    fn synthetic_base_material(include_unloaded_thickness: bool) -> Material {
        let mut props = json!({
            "areal_density": {
                "value": 0.21,
                "unit": "kg/m2",
                "evidence": "measured",
                "source": "spec",
                "method": "synthetic-coupon-mass"
            },
            "poisson_warp_weft": {
                "value": 0.25,
                "unit": "1",
                "evidence": "measured",
                "source": "spec",
                "method": "synthetic-biaxial-baseline"
            },
            "reported_thickness": {
                "value": 0.00055,
                "unit": "m",
                "evidence": "measured",
                "source": "spec",
                "method": "synthetic-presser-foot-gauge",
                "conditions": {"pressure_pa": "presser_foot_unspecified"}
            }
        });
        if include_unloaded_thickness {
            props["thickness"] = json!({
                "value": 0.0006,
                "unit": "m",
                "evidence": "measured",
                "source": "spec",
                "method": "synthetic-unloaded-optical-gauge",
                "conditions": {"pressure_pa": "0"}
            });
            props["friction_skin"] = json!({
                "value": 0.35,
                "unit": "1",
                "evidence": "measured",
                "source": "spec",
                "method": "synthetic-sled"
            });
        }
        serde_json::from_value(json!({
            "id": "synthetic/cylinder-coupon",
            "name": "Synthetic cylinder coupon",
            "form": "woven",
            "sources": {
                "spec": {
                    "reference": "synthetic:coupon-spec",
                    "accessed_on": "2026-11-11",
                    "note": "Synthetic baseline mass and presser-foot thickness"
                },
                "fit_acq": {
                    "reference": "synthetic:cylinder-fit-run",
                    "accessed_on": "2026-11-11",
                    "note": "Synthetic FIT cylindrical-holder acquisitions"
                },
                "check_acq": {
                    "reference": "synthetic:cylinder-check-run",
                    "accessed_on": "2026-11-11",
                    "note": "Independent synthetic CHECK cylindrical-holder acquisition"
                }
            },
            "properties": props
        }))
        .unwrap()
    }

    fn base_fixture(warp_axis: [f64; 2]) -> Fixture {
        Fixture {
            holder_radius_m: 0.040,
            specimen_width_m: 0.260,
            specimen_height_m: 0.110,
            nominal_free_height_m: 0.050,
            pretension_displacement_m: 0.0005,
            tare_force_n: 0.12,
            tare_torque_nm: -0.002,
            warp_axis,
            cells: [24, 8],
            self_contact: false,
            grip_slip_force_limit_n: None,
            refuse_unmodeled_slip: false,
        }
    }

    fn fixed_law_input(nonlinear_membrane: bool) -> Input {
        let mut material = synthetic_base_material(false);
        let quantity = |value, unit: &str| Quantity {
            value,
            unit: unit.into(),
            evidence: Evidence::Measured,
            source: "spec".into(),
            method: "synthetic independent baseline law".into(),
            conditions: BTreeMap::new(),
            bounds: None,
        };
        for (key, value, unit) in [
            ("warp_stiffness", 900.0, "N/m"),
            ("weft_stiffness", 620.0, "N/m"),
            ("shear_stiffness", 180.0, "N/m"),
            ("bending_rigidity", 0.0, "N*m"),
        ] {
            material
                .properties
                .insert(key.into(), quantity(value, unit));
        }
        let target = if nonlinear_membrane {
            let curve = |values: [[f64; 2]; 5]| {
                json!({"points": values, "source": "spec", "evidence": "measured",
                    "method": "synthetic independent nonlinear law"})
            };
            material.membrane_response = Some(
                serde_json::from_value(json!({
                    "model": "tabulated_green_strain/1",
                    "interpolation": "piecewise_linear",
                    "warp": curve([[-0.3,-270.0],[0.0,0.0],[0.02,18.0],[0.08,180.0],[0.5,1800.0]]),
                    "weft": curve([[-0.3,-186.0],[0.0,0.0],[0.02,12.4],[0.08,124.0],[0.5,1240.0]]),
                    "shear": curve([[-0.5,-180.0],[-0.04,-7.2],[0.0,0.0],[0.04,7.2],[0.5,180.0]]),
                    "coupling": quantity(40.0, "N/m")
                }))
                .unwrap(),
            );
            // A tabulated membrane replaces these scalar observations in ordinary consumers.
            for key in [
                "warp_stiffness",
                "weft_stiffness",
                "poisson_warp_weft",
                "shear_stiffness",
            ] {
                material.properties.remove(key);
            }
            TargetParameter {
                parameter: TargetKind::BendingRigidity,
                bounds: [0.001, 0.005],
            }
        } else {
            material.structure.insert(
                "bending_model".into(),
                ftx_pattern::material_bending::MODEL.into(),
            );
            for (key, value) in [
                ("bending_warp", 0.003),
                ("bending_weft", 0.005),
                ("bending_coupling", 0.0004),
                ("bending_twist", 0.002),
            ] {
                material
                    .properties
                    .insert(key.into(), quantity(value, "N*m"));
            }
            TargetParameter {
                parameter: TargetKind::WarpStiffness,
                bounds: [600.0, 1400.0],
            }
        };
        let mut fixture = base_fixture([0.0, 1.0]);
        fixture.cells = [8, 4];
        fixture.pretension_displacement_m = 0.0;
        let experiment = |id: &str, partition, extension, rotation| Experiment {
            id: id.into(),
            fixture: fixture.clone(),
            loading: vec![Waypoint {
                time_s: 0.0,
                axial_displacement_m: extension,
                rotation_deg: rotation,
                clamp_slip_m: 0.0,
            }],
            nominal_protocol: None,
            source: if partition == Partition::Fit {
                "fit_acq"
            } else {
                "check_acq"
            }
            .into(),
            evidence: Evidence::Measured,
            partition,
            observations: vec![Observation {
                id: format!("{id}-force"),
                waypoint_index: 0,
                force_n: Some(Channel {
                    value: 0.0,
                    scale: 1.0,
                }),
                torque_nm: None,
                mean_mid_radius_m: None,
            }],
        };
        Input {
            schema: INPUT_SCHEMA.into(),
            material,
            method: "Synthetic independent fixed-law integration check".into(),
            conditions: BTreeMap::new(),
            fitted_on: "2026-11-12".into(),
            targets: vec![target],
            experiments: vec![
                experiment("fit", Partition::Fit, 0.002, 3.0),
                experiment("check", Partition::Check, 0.001, 1.0),
            ],
            admit_cylindrical_shell_model: true,
            allow_estimated_observations: nonlinear_membrane,
            replace_existing: true,
            allow_estimates: true,
            controls: Controls {
                static_tolerance_m: 1e-9,
                velocity_tolerance_m_s: 1e-8,
                ..Controls::default()
            },
            policy: Policy {
                max_rms_scaled: 0.2,
                max_peak_scaled: 0.3,
                max_mesh_scaled: 0.3,
                min_excitation_pivot: 1e-4,
                max_iterations: 40,
                max_forward_solves: 2048,
            },
        }
    }

    #[test]
    fn fixed_constitutive_laws_match_public_forward_and_published_replay() {
        for nonlinear in [false, true] {
            let input = fixed_law_input(nonlinear);
            let baseline = resolve_baseline_and_validate(&input).unwrap();
            let values = [if nonlinear { 0.0025 } else { 900.0 }];
            let domain = StrainDomain::new([[-0.25, 0.4]; 3]).unwrap();
            let published = publish(
                &input,
                &baseline,
                "synthetic-fixed-law",
                &values,
                domain.ranges(),
            )
            .unwrap();
            assert_eq!(
                serde_json::to_value(&published.membrane_response).unwrap(),
                serde_json::to_value(&input.material.membrane_response).unwrap(),
            );
            assert_eq!(published.structure, input.material.structure);
            for key in [
                "bending_warp",
                "bending_weft",
                "bending_coupling",
                "bending_twist",
            ] {
                assert_eq!(
                    serde_json::to_value(published.properties.get(key)).unwrap(),
                    serde_json::to_value(input.material.properties.get(key)).unwrap(),
                );
            }
            let mut budget = Budget {
                used: 0,
                limit: 100,
                inherited_domain: None,
                baseline,
                input: &input,
                recovery: SearchRecovery::default(),
            };
            for experiment in &input.experiments {
                for factor in [1, 2] {
                    let mut fixture = experiment.fixture.clone();
                    fixture.cells = fixture.cells.map(|n| n * factor);
                    let fitted = budget.solve(experiment, &fixture, &values).unwrap();
                    let replayed = budget
                        .replay(experiment, &fixture, &values, domain)
                        .unwrap();
                    let forward = cylinder::evaluate(&cylinder::Request {
                        schema: cylinder::REQUEST_SCHEMA.into(),
                        material: Selection {
                            id: published.id.clone(),
                            record: Some(published.clone()),
                            allow_estimates: true,
                            ..Selection::default()
                        },
                        fixture,
                        loading: experiment.loading.clone(),
                        nominal_protocol: None,
                        controls: input.controls.clone(),
                    })
                    .unwrap();
                    for candidate in [&fitted.waypoint_states[0], &replayed.waypoint_states[0]] {
                        let expected = &forward.states[0];
                        assert!(
                            (candidate.tare_adjusted_force_n - expected.tare_adjusted_force_n)
                                .abs()
                                < 1e-9,
                            "nonlinear={nonlinear}, factor={factor}: force changed on publication"
                        );
                        assert!(
                            (candidate.stored_membrane_energy_j
                                - expected.stored_membrane_energy_j)
                                .abs()
                                < 1e-11
                        );
                        assert!(
                            (candidate.stored_bending_energy_j - expected.stored_bending_energy_j)
                                .abs()
                                < 1e-11
                        );
                        for (a, b) in candidate.positions_m.iter().zip(&expected.positions_m) {
                            for (a, b) in a.iter().zip(b) {
                                assert!((a - b).abs() < 1e-10);
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn fixed_laws_keep_evidence_domains_and_target_overwrite_refusals() {
        let mut nonlinear = fixed_law_input(true);
        nonlinear.allow_estimated_observations = false;
        assert!(
            resolve_baseline_and_validate(&nonlinear)
                .err()
                .unwrap()
                .message
                .contains("estimate consent")
        );
        nonlinear.allow_estimated_observations = true;
        nonlinear.targets[0] = TargetParameter {
            parameter: TargetKind::WarpStiffness,
            bounds: [600.0, 1400.0],
        };
        assert!(
            resolve_baseline_and_validate(&nonlinear)
                .err()
                .unwrap()
                .message
                .contains("cannot discard an existing nonlinear")
        );

        let mut narrow = fixed_law_input(true);
        let law = narrow.material.membrane_response.as_mut().unwrap();
        for (curve, stress) in [
            (&mut law.warp, 0.09),
            (&mut law.weft, 0.062),
            (&mut law.shear, 0.018),
        ] {
            curve.points = vec![[-1e-4, -stress], [0.0, 0.0], [1e-4, stress]];
        }
        let baseline = resolve_baseline_and_validate(&narrow).unwrap();
        let mut budget = Budget {
            used: 0,
            limit: 100,
            inherited_domain: None,
            baseline,
            input: &narrow,
            recovery: SearchRecovery::default(),
        };
        let experiment = &narrow.experiments[0];
        assert!(
            budget
                .solve(experiment, &experiment.fixture, &[0.0025])
                .is_err(),
            "a fixed nonlinear law's source domain cannot disappear during a bending fit"
        );

        let mut directional = fixed_law_input(false);
        directional
            .material
            .properties
            .get_mut("bending_warp")
            .unwrap()
            .evidence = Evidence::Estimate;
        assert!(resolve_baseline_and_validate(&directional).is_err());
        directional.allow_estimated_observations = true;
        assert!(resolve_baseline_and_validate(&directional).is_ok());
        directional
            .material
            .properties
            .get_mut("bending_warp")
            .unwrap()
            .source = "absent".into();
        assert!(resolve_baseline_and_validate(&directional).is_err());
        directional
            .material
            .properties
            .get_mut("bending_warp")
            .unwrap()
            .source = "spec".into();
        directional.targets[0] = TargetParameter {
            parameter: TargetKind::BendingRigidity,
            bounds: [0.001, 0.005],
        };
        assert!(
            resolve_baseline_and_validate(&directional)
                .err()
                .unwrap()
                .message
                .contains("cannot discard an existing directional")
        );
    }

    fn generate_truth_state(
        fixture: &Fixture,
        loading: &[Waypoint],
        truth: [f64; 4],
        bending_nm: f64,
    ) -> Vec<State> {
        let mut refined = fixture.clone();
        refined.cells = refined.cells.map(|n| n * 2);
        let mat: Material = serde_json::from_value(json!({
            "id": "synthetic/truth",
            "name": "Synthetic truth",
            "form": "woven",
            "sources": {
                "s": {
                    "reference": "synthetic:truth",
                    "accessed_on": "2026-11-11",
                    "note": "Synthetic truth parameters"
                }
            },
            "properties": {
                "areal_density": {"value": 0.21, "unit": "kg/m2", "evidence": "measured", "source": "s", "method": "synthetic-truth"},
                "warp_stiffness": {"value": truth[0], "unit": "N/m", "evidence": "measured", "source": "s", "method": "synthetic-truth"},
                "weft_stiffness": {"value": truth[1], "unit": "N/m", "evidence": "measured", "source": "s", "method": "synthetic-truth"},
                "poisson_warp_weft": {"value": truth[2], "unit": "1", "evidence": "measured", "source": "s", "method": "synthetic-truth"},
                "shear_stiffness": {"value": truth[3], "unit": "N/m", "evidence": "measured", "source": "s", "method": "synthetic-truth"},
                "bending_rigidity": {"value": bending_nm, "unit": "N*m", "evidence": "measured", "source": "s", "method": "synthetic-truth"}
            }
        }))
        .unwrap();
        let req = cylinder::Request {
            schema: cylinder::REQUEST_SCHEMA.into(),
            material: Selection {
                id: mat.id.clone(),
                record: Some(mat),
                ..Selection::default()
            },
            fixture: refined,
            loading: loading.to_vec(),
            nominal_protocol: None,
            controls: Controls {
                max_step_s: 0.25,
                static_tolerance_m: 1e-9,
                velocity_tolerance_m_s: 1e-8,
                ..Controls::default()
            },
        };
        cylinder::evaluate(&req).unwrap().states
    }

    fn build_synthetic_calibration(include_unloaded_thickness: bool) -> (Input, [f64; 4]) {
        // Truth: E_warp = 900 N/m, E_weft = 620 N/m, nu = 0.25 (fixed baseline), G_12 = 180 N/m, B = 2.5e-3 N*m
        let truth_mem = [900.0, 620.0, 0.25, 180.0];
        let truth_bend = 2.5e-3;

        // Exp 1 (FIT): axial warp [0, 1], +/- twist + extension + axial compression barreling
        let f_warp = base_fixture([0.0, 1.0]);
        let load_warp = vec![
            Waypoint {
                time_s: 0.0,
                axial_displacement_m: 0.0,
                rotation_deg: 0.0,
                clamp_slip_m: 0.0,
            },
            Waypoint {
                time_s: 0.15,
                axial_displacement_m: 0.0015,
                rotation_deg: 6.0,
                clamp_slip_m: 0.0,
            },
            Waypoint {
                time_s: 0.30,
                axial_displacement_m: 0.0020,
                rotation_deg: 8.0,
                clamp_slip_m: 0.0,
            },
            Waypoint {
                time_s: 0.50,
                axial_displacement_m: 0.0020,
                rotation_deg: -6.0,
                clamp_slip_m: 0.0,
            },
            Waypoint {
                time_s: 0.65,
                axial_displacement_m: 0.0020,
                rotation_deg: -8.0,
                clamp_slip_m: 0.0,
            },
            Waypoint {
                time_s: 0.80,
                axial_displacement_m: -0.0018,
                rotation_deg: 2.0,
                clamp_slip_m: 0.0,
            },
            Waypoint {
                time_s: 0.92,
                axial_displacement_m: -0.0028,
                rotation_deg: 3.0,
                clamp_slip_m: 0.0,
            },
            Waypoint {
                time_s: 1.05,
                axial_displacement_m: -0.0035,
                rotation_deg: 4.0,
                clamp_slip_m: 0.0,
            },
        ];
        let states_warp = generate_truth_state(&f_warp, &load_warp, truth_mem, truth_bend);

        // Exp 2 (FIT): axial weft [1, 0], extension + compression + twist
        let f_weft = base_fixture([1.0, 0.0]);
        let load_weft = vec![
            Waypoint {
                time_s: 0.0,
                axial_displacement_m: 0.0,
                rotation_deg: 0.0,
                clamp_slip_m: 0.0,
            },
            Waypoint {
                time_s: 0.15,
                axial_displacement_m: 0.0014,
                rotation_deg: 4.0,
                clamp_slip_m: 0.0,
            },
            Waypoint {
                time_s: 0.30,
                axial_displacement_m: 0.0018,
                rotation_deg: 5.0,
                clamp_slip_m: 0.0,
            },
            Waypoint {
                time_s: 0.48,
                axial_displacement_m: -0.0014,
                rotation_deg: -2.0,
                clamp_slip_m: 0.0,
            },
            Waypoint {
                time_s: 0.62,
                axial_displacement_m: -0.0023,
                rotation_deg: -3.5,
                clamp_slip_m: 0.0,
            },
            Waypoint {
                time_s: 0.75,
                axial_displacement_m: -0.0028,
                rotation_deg: -4.0,
                clamp_slip_m: 0.0,
            },
        ];
        let states_weft = generate_truth_state(&f_weft, &load_weft, truth_mem, truth_bend);

        // Exp 3 (CHECK): 45-degree bias [1/sqrt(2), 1/sqrt(2)], strictly interior strain state
        let inv_sqrt2 = std::f64::consts::FRAC_1_SQRT_2;
        let f_bias = base_fixture([inv_sqrt2, inv_sqrt2]);
        let load_bias = vec![
            Waypoint {
                time_s: 0.0,
                axial_displacement_m: 0.0,
                rotation_deg: 0.0,
                clamp_slip_m: 0.0,
            },
            Waypoint {
                time_s: 0.25,
                axial_displacement_m: 0.0008,
                rotation_deg: 3.0,
                clamp_slip_m: 0.0,
            },
        ];
        let states_bias = generate_truth_state(&f_bias, &load_bias, truth_mem, truth_bend);

        let input = Input {
            schema: INPUT_SCHEMA.into(),
            material: synthetic_base_material(include_unloaded_thickness),
            method:
                "Synthetic 3D cylindrical-holder warp/weft/bias twist-tension-compression recovery"
                    .into(),
            conditions: BTreeMap::from([("temperature_c".into(), "21".into())]),
            fitted_on: "2026-11-11".into(),
            targets: vec![
                TargetParameter {
                    parameter: TargetKind::WarpStiffness,
                    bounds: [400.0, 1500.0],
                },
                TargetParameter {
                    parameter: TargetKind::WeftStiffness,
                    bounds: [300.0, 1200.0],
                },
                TargetParameter {
                    parameter: TargetKind::ShearStiffness,
                    bounds: [60.0, 400.0],
                },
                TargetParameter {
                    parameter: TargetKind::BendingRigidity,
                    bounds: [1.0e-3, 5.0e-3],
                },
            ],
            experiments: vec![
                Experiment {
                    id: "fit-warp".into(),
                    fixture: f_warp,
                    loading: load_warp,
                    nominal_protocol: None,
                    source: "fit_acq".into(),
                    evidence: Evidence::Measured,
                    partition: Partition::Fit,
                    observations: vec![
                        Observation {
                            id: "warp-ext-pos-twist".into(),
                            waypoint_index: 2,
                            force_n: Some(Channel {
                                value: states_warp[2].tare_adjusted_force_n,
                                scale: 1.0,
                            }),
                            torque_nm: Some(Channel {
                                value: states_warp[2].tare_adjusted_torque_nm,
                                scale: 0.03,
                            }),
                            mean_mid_radius_m: None,
                        },
                        Observation {
                            id: "warp-ext-neg-twist".into(),
                            waypoint_index: 4,
                            force_n: Some(Channel {
                                value: states_warp[4].tare_adjusted_force_n,
                                scale: 1.0,
                            }),
                            torque_nm: Some(Channel {
                                value: states_warp[4].tare_adjusted_torque_nm,
                                scale: 0.03,
                            }),
                            mean_mid_radius_m: None,
                        },
                        Observation {
                            id: "warp-comp".into(),
                            waypoint_index: 7,
                            force_n: Some(Channel {
                                value: states_warp[7].tare_adjusted_force_n,
                                scale: 0.10,
                            }),
                            torque_nm: Some(Channel {
                                value: states_warp[7].tare_adjusted_torque_nm,
                                scale: 0.03,
                            }),
                            mean_mid_radius_m: None,
                        },
                    ],
                },
                Experiment {
                    id: "fit-weft".into(),
                    fixture: f_weft,
                    loading: load_weft,
                    nominal_protocol: None,
                    source: "fit_acq".into(),
                    evidence: Evidence::Measured,
                    partition: Partition::Fit,
                    observations: vec![
                        Observation {
                            id: "weft-ext-twist".into(),
                            waypoint_index: 2,
                            force_n: Some(Channel {
                                value: states_weft[2].tare_adjusted_force_n,
                                scale: 1.0,
                            }),
                            torque_nm: Some(Channel {
                                value: states_weft[2].tare_adjusted_torque_nm,
                                scale: 0.03,
                            }),
                            mean_mid_radius_m: None,
                        },
                        Observation {
                            id: "weft-comp".into(),
                            waypoint_index: 5,
                            force_n: Some(Channel {
                                value: states_weft[5].tare_adjusted_force_n,
                                scale: 0.15,
                            }),
                            torque_nm: Some(Channel {
                                value: states_weft[5].tare_adjusted_torque_nm,
                                scale: 0.03,
                            }),
                            mean_mid_radius_m: None,
                        },
                    ],
                },
                Experiment {
                    id: "check-bias".into(),
                    fixture: f_bias,
                    loading: load_bias,
                    nominal_protocol: None,
                    source: "check_acq".into(),
                    evidence: Evidence::Measured,
                    partition: Partition::Check,
                    observations: vec![Observation {
                        id: "bias-holdout".into(),
                        waypoint_index: 1,
                        force_n: Some(Channel {
                            value: states_bias[1].tare_adjusted_force_n,
                            scale: 1.0,
                        }),
                        torque_nm: Some(Channel {
                            value: states_bias[1].tare_adjusted_torque_nm,
                            scale: 0.03,
                        }),
                        mean_mid_radius_m: None,
                    }],
                },
            ],
            admit_cylindrical_shell_model: true,
            allow_estimated_observations: false,
            replace_existing: false,
            allow_estimates: true,
            controls: Controls {
                max_step_s: 0.25,
                static_tolerance_m: 1e-9,
                velocity_tolerance_m_s: 1e-8,
                ..Controls::default()
            },
            policy: Policy {
                max_rms_scaled: 0.35,
                max_peak_scaled: 0.6,
                max_mesh_scaled: 0.6,
                min_excitation_pivot: 1e-4,
                max_iterations: 40,
                max_forward_solves: 8192,
            },
        };
        (
            input,
            [truth_mem[0], truth_mem[1], truth_mem[3], truth_bend],
        )
    }

    #[test]
    fn recovers_known_orthotropic_and_bending_parameters_without_fabricating_thickness() {
        let (input, expected) = build_synthetic_calibration(false);
        let report = calibrate(&input).expect("calibration succeeds");
        assert!(
            report.accepted,
            "unexpected rejection: {:?}",
            report.reasons
        );
        assert!(report.domain_replay_verified);
        assert_eq!(report.starts.len(), 3);
        assert!(report.starts.iter().all(|s| s.converged));
        for (i, (&got, &want)) in report.parameters.iter().zip(&expected).enumerate() {
            let rel = (got - want).abs() / want;
            let tol = if i < 3 { 0.03 } else { 0.08 };
            assert!(
                rel < tol,
                "parameter {i} ({got}) differs from truth ({want}) by {:.2}% (tol {:.0}%)",
                rel * 100.0,
                tol * 100.0
            );
        }
        let published = report.material.as_ref().expect("published material");
        // Reported presser-foot thickness stays untouched; unloaded contact thickness is NOT fabricated!
        assert!(!published.properties.contains_key("thickness"));
        assert!(published.properties.contains_key("reported_thickness"));
        let sel = report.selection.as_ref().expect("frozen selection");
        assert!(sel.allow_estimates);
        let resolved = textile_import::catalog().unwrap().resolve(sel).unwrap();
        assert!(
            resolved.mechanics().is_err(),
            "missing unloaded thickness and friction_skin must keep mechanics() unready"
        );
    }

    #[test]
    fn rejects_missing_check_unexcited_target_corrupted_check_and_respects_estimate_gate() {
        let (base, _) = build_synthetic_calibration(true);

        // 1. With unloaded thickness + friction_skin, allow_estimates = false keeps mechanics() gated
        let mut gated = base.clone();
        gated.allow_estimates = false;
        let rep_gated = calibrate(&gated).unwrap();
        assert!(rep_gated.accepted);
        let sel_gated = rep_gated.selection.as_ref().unwrap();
        assert!(!sel_gated.allow_estimates);
        let snap_gated = textile_import::catalog()
            .unwrap()
            .resolve(sel_gated)
            .unwrap();
        assert!(snap_gated.mechanics().is_err());

        // With allow_estimates = true AND unloaded thickness + friction_skin, mechanics() succeeds
        let rep_ready = calibrate(&base).unwrap();
        assert!(rep_ready.accepted);
        let snap_ready = textile_import::catalog()
            .unwrap()
            .resolve(rep_ready.selection.as_ref().unwrap())
            .unwrap();
        assert!(snap_ready.mechanics().is_ok());

        // 2. Missing CHECK experiment -> accepted == false, no selection published
        let mut no_check = base.clone();
        no_check
            .experiments
            .retain(|e| e.partition == Partition::Fit);
        let rep_no_check = calibrate(&no_check).unwrap();
        assert!(!rep_no_check.accepted);
        assert!(rep_no_check.selection.is_none());
        assert!(
            rep_no_check
                .reasons
                .iter()
                .any(|r| r.contains("held-out CHECK"))
        );

        // 3. Corrupted CHECK observation -> accepted == false
        let mut bad_check = base.clone();
        bad_check.experiments[2].observations[0]
            .force_n
            .as_mut()
            .unwrap()
            .value += 50.0;
        let rep_bad_check = calibrate(&bad_check).unwrap();
        assert!(!rep_bad_check.accepted);
        assert!(rep_bad_check.selection.is_none());
        assert!(
            rep_bad_check
                .reasons
                .iter()
                .any(|r| r.contains("CHECK cylinder"))
        );

        // 4. Unexcited target (twist-only FIT cannot excite weft_stiffness independently of warp/shear)
        let mut unexcited = base.clone();
        unexcited.experiments.retain(|e| e.id != "fit-weft");
        unexcited.experiments[0].loading = vec![
            Waypoint {
                time_s: 0.0,
                axial_displacement_m: 0.0,
                rotation_deg: 0.0,
                clamp_slip_m: 0.0,
            },
            Waypoint {
                time_s: 1.0,
                axial_displacement_m: 0.0,
                rotation_deg: 2.0,
                clamp_slip_m: 0.0,
            },
        ];
        unexcited.experiments[0].observations = vec![
            Observation {
                id: "twist-0".into(),
                waypoint_index: 0,
                force_n: Some(Channel {
                    value: 0.0,
                    scale: 1.0,
                }),
                torque_nm: Some(Channel {
                    value: 0.0,
                    scale: 0.02,
                }),
                mean_mid_radius_m: None,
            },
            Observation {
                id: "twist-1".into(),
                waypoint_index: 1,
                force_n: Some(Channel {
                    value: 1.0,
                    scale: 1.0,
                }),
                torque_nm: Some(Channel {
                    value: 0.05,
                    scale: 0.02,
                }),
                mean_mid_radius_m: None,
            },
        ];
        unexcited.policy.min_excitation_pivot = 0.05;
        let rep_unexcited = calibrate(&unexcited).unwrap();
        assert!(!rep_unexcited.accepted);
        assert!(rep_unexcited.selection.is_none());

        // 5. Forward solve budget exhaustion -> structured BudgetExceeded refusal
        let mut tiny_budget = base;
        tiny_budget.policy.max_forward_solves = 3;
        let err = calibrate(&tiny_budget).unwrap_err();
        assert_eq!(err.code, RefusalCode::BudgetExceeded);
    }
}

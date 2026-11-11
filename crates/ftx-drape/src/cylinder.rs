//! Cylindrical-holder twist, axial compression and stretch-slip forward experiments.
//!
//! Models the cylindrical-holder textile test apparatus (e.g. Gong et al. 2026, Fabric-101 §3.1):
//! a rectangular sheet of width `W >= 2π R` and height `H >= L_0` is wrapped around two coaxial
//! rigid cylindrical holders of radius `R`, clamped with nominal free height `L_0`, mounted with
//! an explicit axial pretension displacement `Δz_pre` (never absorbed into `Δz / L_0`), and driven
//! through a timed sequence of upper-holder rotations `θ(t)`, axial displacements `Δz(t)` and
//! optional explicit upper-clamp axial slip offsets `u_slip(t)` while the lower holder stays fixed.
//!
//! Every triangle's flat 2D grain-frame rest metric is preserved isometrically on the discrete
//! `n_theta`-sided chord cylinder `R_mesh = (π R / n_theta) / sin(π / n_theta)` (`R_mesh -> R` at
//! `O(1 / n_theta^2)`), eliminating polygonal faceting prestrain at zero extension while retaining
//! unwelded source-panel topology across the longitudinal overlap seam so orthotropic
//! [`BendingTensor`] patches and [`PlasticBending`] permanent-wrinkle memory bind without jumping
//! the seam.
//!
//! Units: metres, kilograms, seconds, newtons, newton-metres; cylinder axis along `+z`.
//! Forward experiment only: does not promote raw Fabric-101 release metadata to garment readiness,
//! fabricate contact thickness from presser-foot `reported_thickness`, or claim empirical fit.

use std::f64::consts::PI;

use ftx_core::{Refusal, RefusalCode, Vec2, Vec3, det, receipt::sha256_hex};
use ftx_pattern::{
    material_bending::BendingTensor,
    material_calibration::fitted_domain,
    material_catalog::Selection,
    material_membrane_plasticity::PlasticMembrane,
    material_plasticity::PlasticBending,
    material_relaxation::CompiledRelaxation,
    material_response::{CompiledMembrane, StrainDomain},
    materials::Form,
    textile_import,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{ClothModel, ContactParams, Membrane, SolveReport, Solver, grid};

/// Schema identifier of [`Request`].
pub const REQUEST_SCHEMA: &str = "ftx-cylinder-experiment/1";
/// Schema identifier of [`Report`].
pub const REPORT_SCHEMA: &str = "ftx-cylinder-experiment-report/1";

const DEFAULT_RATE_TOLERANCE_FRACTION: f64 = 0.05;

const fn default_rate_tolerance_fraction() -> f64 {
    DEFAULT_RATE_TOLERANCE_FRACTION
}

fn invalid(message: impl std::fmt::Display) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message.to_string())
}

fn out_of_range(message: impl std::fmt::Display) -> Refusal {
    Refusal::new(RefusalCode::OutOfRange, message.to_string())
}

fn budget(message: impl std::fmt::Display) -> Refusal {
    Refusal::new(RefusalCode::BudgetExceeded, message.to_string())
}

fn vec3_array(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

/// Cylindrical-holder apparatus geometry, sample orientation and clamp boundary policy.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fixture {
    /// Rigid holder outer radius `R` in metres (e.g. `0.040` m for Fabric-101).
    pub holder_radius_m: f64,
    /// Full flat rectangular specimen width in metres (`>= 2π R`; excess is clamped seam overlap).
    pub specimen_width_m: f64,
    /// Full flat rectangular specimen height in metres (`>= nominal_free_height_m`; excess is inside clamps).
    pub specimen_height_m: f64,
    /// Unstretched axial gauge height `L_0` between inner clamp faces in metres (e.g. `0.050` m).
    pub nominal_free_height_m: f64,
    /// Explicit axial displacement `Δz_pre` (m) applied before zeroing the instrument displacement channel.
    /// Mounted separation at `axial_displacement_m == 0` is `nominal_free_height_m + pretension_displacement_m`.
    pub pretension_displacement_m: f64,
    /// Explicit force-cell tare offset (N) subtracted in `tare_adjusted_force_n`.
    #[serde(default)]
    pub tare_force_n: f64,
    /// Explicit torque-cell tare offset (N·m) subtracted in `tare_adjusted_torque_nm`.
    #[serde(default)]
    pub tare_torque_nm: f64,
    /// Unit warp/wale direction `[u, v]` in unwrapped `(circumferential +s, axial +z)` specimen coordinates.
    /// Axial warp is `[0, 1]`; axial weft is `[1, 0]`; `+45°` bias is `[1/√2, 1/√2]`.
    pub warp_axis: [f64; 2],
    /// Even mesh cell counts `[around circumference, along free height]`, `6..=64` by `2..=32`.
    pub cells: [usize; 2],
    /// Enable cloth-cloth IPC barrier and friction on interior folds (requires qualified unloaded `thickness` and `friction_self`).
    #[serde(default)]
    pub self_contact: bool,
    /// Optional positive upper-clamp slip threshold (N). Exceeding it sets `slip_limit_exceeded` (or refuses when `refuse_unmodeled_slip` is true).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grip_slip_force_limit_n: Option<f64>,
    /// Refuse immediately if `upper_force_n` exceeds `grip_slip_force_limit_n` while `clamp_slip_m == 0`.
    #[serde(default)]
    pub refuse_unmodeled_slip: bool,
}

impl Fixture {
    /// Wrapped active circumference `2π R` in metres, excluding the longitudinal seam overlap.
    #[must_use]
    pub fn wrapped_circumference_m(&self) -> f64 {
        2.0 * PI * self.holder_radius_m
    }

    /// Mounted inner-clamp separation `L_0 + Δz_pre` at zero instrument axial displacement, in metres.
    #[must_use]
    pub fn mounted_free_height_m(&self) -> f64 {
        self.nominal_free_height_m + self.pretension_displacement_m
    }

    /// Exact discrete chord radius `R_mesh` (m) whose `n_theta` polygon perimeter equals `2π R`.
    #[must_use]
    pub fn isometric_mesh_radius_m(&self) -> f64 {
        let n_theta = self.cells[0];
        let ds = self.wrapped_circumference_m() / n_theta as f64;
        0.5 * ds / det::sin(PI / n_theta as f64)
    }

    /// Perpendicular facet apothem `R_mesh cos(π / n_theta)` (m) of the discrete `n_theta`-gon prism.
    #[must_use]
    pub fn isometric_apothem_m(&self) -> f64 {
        self.isometric_mesh_radius_m() * det::cos(PI / self.cells[0] as f64)
    }

    /// Validate physical dimensions, grain axis, discretization and tare/slip bounds.
    ///
    /// # Errors
    /// Non-finite/nonpositive dimensions, specimen narrower than `2π R` or shorter than `L_0`,
    /// nonpositive mounted height, non-unit `warp_axis`, or unsupported cell counts.
    pub fn validate(&self) -> Result<(), Refusal> {
        let [n_theta, n_z] = self.cells;
        let [u, v] = self.warp_axis;
        if !self.holder_radius_m.is_finite()
            || self.holder_radius_m <= 0.0
            || !self.specimen_width_m.is_finite()
            || !self.specimen_height_m.is_finite()
            || !self.nominal_free_height_m.is_finite()
            || self.nominal_free_height_m <= 0.0
            || !self.pretension_displacement_m.is_finite()
            || !self.tare_force_n.is_finite()
            || !self.tare_torque_nm.is_finite()
        {
            return Err(invalid(
                "cylinder fixture requires finite positive holder radius and nominal free height, plus finite pretension and tare offsets",
            ));
        }
        let circumference = self.wrapped_circumference_m();
        if self.specimen_width_m + 1e-12 < circumference {
            return Err(invalid(format!(
                "specimen width {} m is shorter than holder circumference 2πR = {} m",
                self.specimen_width_m, circumference
            )));
        }
        if self.specimen_height_m + 1e-12 < self.nominal_free_height_m {
            return Err(invalid(format!(
                "specimen height {} m is shorter than nominal free height {} m",
                self.specimen_height_m, self.nominal_free_height_m
            )));
        }
        if self.mounted_free_height_m() <= 0.0 {
            return Err(invalid(
                "mounted free height (nominal_free_height_m + pretension_displacement_m) must be positive",
            ));
        }
        if let Some(limit) = self.grip_slip_force_limit_n
            && (!limit.is_finite() || limit <= 0.0)
        {
            return Err(invalid(
                "grip_slip_force_limit_n must be finite and positive",
            ));
        }
        if !(6..=64).contains(&n_theta)
            || !(2..=32).contains(&n_z)
            || !n_theta.is_multiple_of(2)
            || !n_z.is_multiple_of(2)
            || n_theta * n_z > 1024
        {
            return Err(invalid(
                "cylinder cells [n_theta, n_z] must be even in 6..=64 and 2..=32 with at most 1024 cells",
            ));
        }
        if !u.is_finite() || !v.is_finite() || (u * u + v * v - 1.0).abs() > 1e-12 {
            return Err(invalid("cylinder warp_axis must be a finite unit vector"));
        }
        Ok(())
    }
}

/// One prescribed upper-holder kinematic boundary waypoint; linear interpolation in time between waypoints.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Waypoint {
    /// Elapsed seconds from the initial state (`0` at the first waypoint; strictly increasing thereafter).
    pub time_s: f64,
    /// Upper-holder axial displacement (m) relative to the mounted pretensioned zero (`L_0 + Δz_pre`).
    /// Positive stretches the specimen upward; negative compresses the holders together.
    pub axial_displacement_m: f64,
    /// Upper-holder rotation angle about `+z` in degrees (right-hand rule; lower holder fixed at `0°`).
    pub rotation_deg: f64,
    /// Explicit cumulative upper-clamp axial pull-out slip in metres (`>= 0`, monotonically nondecreasing).
    /// Reduces effective fabric stretch `z_upper = L_0 + Δz_pre + axial_displacement_m - clamp_slip_m`.
    #[serde(default)]
    pub clamp_slip_m: f64,
}

/// Declared nominal article/instrument rates for automatic protocol-discrepancy auditing (e.g. Fabric-101 `L01`).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NominalProtocol {
    /// Declared twist speed magnitude in degrees per second (e.g. `0.1` in Fabric-101 §3.1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nominal_twist_rate_deg_s: Option<f64>,
    /// Declared axial crosshead speed magnitude in metres per second (e.g. `0.001` for `1 mm/s`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nominal_axial_rate_m_s: Option<f64>,
    /// Declared net compression travel in metres (e.g. `0.015` for `15 mm`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nominal_compression_travel_m: Option<f64>,
    /// Relative rate discrepancy threshold before flagging (`0.05` = 5% by default).
    #[serde(default = "default_rate_tolerance_fraction")]
    pub rate_tolerance_fraction: f64,
}

/// Explicit numerical time-integration, Newton, equilibrium and optional self-contact controls.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Controls {
    /// Maximum backward-Euler time step (s); each interval is subdivided into equal steps.
    pub max_step_s: f64,
    /// Maximum total dynamic steps across the entire loading path, `1..=2048`.
    pub max_steps: usize,
    /// Static displacement tolerance (m) for a memoryless initial equilibrium.
    pub static_tolerance_m: f64,
    /// Dynamic Newton correction tolerance divided by `h` (m/s), matching [`Solver`].
    pub velocity_tolerance_m_s: f64,
    /// Maximum per-free-DOF force/momentum residual (N).
    pub force_tolerance_n: f64,
    /// Newton iteration cap per static or dynamic step, `1..=300`.
    pub max_newton: usize,
    /// IPC barrier activation distance `d_hat` (m) when `fixture.self_contact` is enabled.
    pub contact_dhat_m: f64,
    /// IPC barrier stiffness `kappa` (N/m) when `fixture.self_contact` is enabled.
    pub contact_kappa_n_m: f64,
    /// Lagged Coulomb friction smoothing speed `eps_v` (m/s) when `fixture.self_contact` is enabled.
    pub friction_eps_v_m_s: f64,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            max_step_s: 0.05,
            max_steps: 1024,
            static_tolerance_m: 1e-10,
            velocity_tolerance_m_s: 1e-8,
            force_tolerance_n: 1e-5,
            max_newton: 150,
            contact_dhat_m: 1e-3,
            contact_kappa_n_m: 100.0,
            friction_eps_v_m_s: 1e-3,
        }
    }
}

impl Controls {
    /// Validate numerical tolerances, step budgets and barrier parameters.
    ///
    /// # Errors
    /// Non-finite or nonpositive tolerances/barriers, or out-of-range step/Newton caps.
    pub fn validate(&self) -> Result<(), Refusal> {
        if !self.max_step_s.is_finite()
            || self.max_step_s <= 0.0
            || !(1..=2048).contains(&self.max_steps)
            || !self.static_tolerance_m.is_finite()
            || self.static_tolerance_m <= 0.0
            || !self.velocity_tolerance_m_s.is_finite()
            || self.velocity_tolerance_m_s <= 0.0
            || !self.force_tolerance_n.is_finite()
            || self.force_tolerance_n <= 0.0
            || !(1..=300).contains(&self.max_newton)
            || !self.contact_dhat_m.is_finite()
            || self.contact_dhat_m <= 0.0
            || !self.contact_kappa_n_m.is_finite()
            || self.contact_kappa_n_m <= 0.0
            || !self.friction_eps_v_m_s.is_finite()
            || self.friction_eps_v_m_s <= 0.0
        {
            return Err(invalid(
                "cylinder controls require finite positive tolerances/barriers, max_steps in 1..=2048 and max_newton in 1..=300",
            ));
        }
        Ok(())
    }
}

/// Source-locked cylindrical-holder forward experiment request.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Must equal [`REQUEST_SCHEMA`] (`"ftx-cylinder-experiment/1"`).
    pub schema: String,
    /// Canonical material selection and its explicit estimate-admission policy.
    pub material: Selection,
    /// Cylindrical holder geometry, pretension, tare, grain axis and clamp options.
    pub fixture: Fixture,
    /// `1..=129` waypoints (`1` allowed only for memoryless static equilibrium at `time_s == 0`).
    pub loading: Vec<Waypoint>,
    /// Optional nominal protocol rates/travel for automatic timestamp/pretension discrepancy auditing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nominal_protocol: Option<NominalProtocol>,
    /// Explicit numerical integration and solver controls.
    #[serde(default)]
    pub controls: Controls,
}

/// Quantitative audit comparing actual timestamped cylinder kinematics against nominal protocol assumptions.
#[derive(Clone, Debug, Serialize)]
pub struct ProtocolAudit {
    /// Wrapped active circumference `2π R` (m).
    pub wrapped_circumference_m: f64,
    /// Clamped circumferential seam overlap `specimen_width_m - 2π R` (m).
    pub seam_overlap_width_m: f64,
    /// Total height clamped inside both holders `specimen_height_m - nominal_free_height_m` (m).
    pub clamped_height_margin_m: f64,
    /// Mounted holder separation `nominal_free_height_m + pretension_displacement_m` (m) at zero instrument travel.
    pub mounted_free_height_m: f64,
    /// Median positive twist speed (deg/s) across active positive-twist intervals, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub median_positive_twist_rate_deg_s: Option<f64>,
    /// Median axial compression speed `|Δz / Δt|` (m/s) across active compression intervals, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub median_compression_rate_m_s: Option<f64>,
    /// Peak downward travel below mounted zero `max(0, -min(axial_displacement_m))` (m).
    pub peak_instrument_compression_m: f64,
    /// Peak net compression below unstretched `nominal_free_height_m`, i.e. `max(0, -(Δz_pre + min(Δz)))` (m).
    pub peak_net_compression_from_nominal_m: f64,
    /// Upstream catalog `source_discrepancy` note when recorded on the resolved material.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub material_source_discrepancy: Option<String>,
    /// Explicit discrepancies between actual timestamps/pretension and nominal protocol assumptions.
    pub discrepancies: Vec<String>,
}

/// One accepted static or backward-Euler dynamic state along the cylindrical-holder loading path.
#[derive(Clone, Debug, Serialize)]
pub struct State {
    /// Elapsed time (s).
    pub time_s: f64,
    /// Accepted time step `h` (s); `0` for the initial `t = 0` state.
    pub time_step_s: f64,
    /// Prescribed instrument axial displacement (m) relative to mounted pretensioned zero.
    pub axial_displacement_m: f64,
    /// Prescribed upper-clamp axial slip offset (m).
    pub clamp_slip_m: f64,
    /// Actual inner-clamp separation `L_0 + Δz_pre + axial_displacement_m - clamp_slip_m` (m).
    pub holder_separation_m: f64,
    /// Net axial extension `(holder_separation_m - L_0) / L_0` relative to unstretched `L_0` (never `Δz / L_0` when `Δz_pre != 0`).
    pub true_axial_extension_fraction: f64,
    /// Prescribed upper-holder rotation angle about `+z` (deg).
    pub rotation_deg: f64,
    /// Total 3D reaction forces `[lower, upper]` exerted by the holders on the cloth (N).
    pub holder_reaction_n: [[f64; 3]; 2],
    /// Upper-holder axial reaction force (N, positive in tension, negative in compression).
    pub upper_force_n: f64,
    /// Lower-holder axial reaction force (N).
    pub lower_force_n: f64,
    /// Tare-corrected upper axial force `upper_force_n - tare_force_n` (N).
    pub tare_adjusted_force_n: f64,
    /// Upper-holder axial reaction torque about `+z` (N·m, positive opposing `+z` rotation).
    pub upper_torque_nm: f64,
    /// Lower-holder axial reaction torque about `+z` (N·m).
    pub lower_torque_nm: f64,
    /// Tare-corrected upper axial torque `upper_torque_nm - tare_torque_nm` (N·m).
    pub tare_adjusted_torque_nm: f64,
    /// Whether `upper_force_n` exceeded `fixture.grip_slip_force_limit_n`.
    pub slip_limit_exceeded: bool,
    /// Maximum free-DOF `|m a - f_internal - f_contact|` residual component (N).
    pub free_dynamic_residual_n: f64,
    /// Global linear-momentum balance residual `‖R_lower + R_upper - Σ_free m_i a_i‖` (N).
    pub momentum_balance_residual_n: f64,
    /// Global elastic axial torque conservation residual `|Σ_all (x_i × f_elastic_i)_z|` (N·m).
    pub elastic_torque_balance_residual_nm: f64,
    /// Global holder + free-inertia axial torque balance residual `|T_lower + T_upper - Σ_free m_i (x_i × a_i)_z|` (N·m).
    pub torque_balance_residual_nm: f64,
    /// Mid-height (`j = n_z / 2`) radial distance `[min, max, mean]` from the cylinder axis (m).
    pub mid_radius_range_m: [f64; 3],
    /// Elementwise material-frame `[E_11, E_22, 2E_12]` Green-strain `[min, max]` bounds.
    pub green_strain_range: [[f64; 2]; 3],
    /// Stored membrane elastic/branch/hardening energy (J).
    pub stored_membrane_energy_j: f64,
    /// Stored hinge + source-panel bending energy (J).
    pub stored_bending_energy_j: f64,
    /// Free interior vertex kinetic energy (J).
    pub free_kinetic_energy_j: f64,
    /// Cumulative Maxwell dashpot dissipation (J).
    pub maxwell_dissipation_j: f64,
    /// Cumulative permanent membrane-strain dissipation (J).
    pub membrane_plastic_dissipation_j: f64,
    /// Cumulative permanent bending-curvature dissipation (J).
    pub bending_plastic_dissipation_j: f64,
    /// Solver vertex positions `[x, y, z]` (m) in row-major `(j, i)` order (`n_theta * (n_z + 1)` vertices).
    pub positions_m: Vec<[f64; 3]>,
    /// Solver vertex velocities `[vx, vy, vz]` (m/s).
    pub velocities_m_s: Vec<[f64; 3]>,
    /// Production Maxwell relaxation state when bound.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relaxation: Option<Value>,
    /// Production permanent membrane-strain state when bound.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub membrane_plasticity: Option<Value>,
    /// Production permanent panel-bending state when bound.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bending_plasticity: Option<Value>,
    /// Production Newton diagnostics; `None` only when `t = 0` starts from a history-bearing unrelaxed wrap.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub solve: Option<SolveReport>,
}

/// Complete replayable cylindrical-holder forward experiment report.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// Equals [`REPORT_SCHEMA`] (`"ftx-cylinder-experiment-report/1"`).
    pub schema: String,
    /// Request with `material` frozen to its exact content hash for offline replay.
    pub input: Request,
    /// SHA-256 of `replay_request_json`.
    pub input_sha256: String,
    /// Exact serialized frozen request JSON text.
    pub replay_request_json: String,
    /// Discrete isometric chord radius `R_mesh` (m) of the `n_theta`-gon holder rings.
    pub isometric_mesh_radius_m: f64,
    /// Initial wrapped cylinder positions `[x, y, z]` (m) at unstretched `nominal_free_height_m`.
    pub reference_positions_m: Vec<[f64; 3]>,
    /// Welded cylinder triangle indices (`2 * n_theta * n_z` triangles).
    pub triangles: Vec<[usize; 3]>,
    /// Active free-gauge cylinder mass `areal_density * 2π R * L_0` (kg), excluding clamped overlap/margins.
    pub active_gauge_mass_kg: f64,
    /// First time (s) at which `upper_force_n` exceeded `grip_slip_force_limit_n`, when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_slip_time_s: Option<f64>,
    /// Quantitative protocol, pretension and timestamp rate discrepancy audit.
    pub protocol_audit: ProtocolAudit,
    /// Initial state followed by every accepted integration step in chronological order.
    pub states: Vec<State>,
    /// Total Newton iterations summed over all solves.
    pub newton_iterations: usize,
    /// Explicit physical and no-claim scope of this forward model.
    pub model_scope: String,
    /// Forward cylinder simulation never promotes a material to garment simulation readiness.
    pub mechanics_readiness_promoted: bool,
    /// Forward cylinder simulation never establishes garment fit.
    pub fit_claim: bool,
    /// Synthetic/numerical forward runs never claim empirical validation against unbundled raw lab curves.
    pub empirical_validation_claim: bool,
}

const MODEL_SCOPE: &str = "3D cylindrical-holder shell forward experiment on an isometric chord-radius welded mesh: rigid coaxial lower/upper holder rings, explicit pretension offset (never delta/nominal_height), explicit tare subtraction, grain-rotated orthotropic/nonlinear membrane, interior panel-bending tensor (or scalar hinges; longitudinal weld seam uses scalar bending_rigidity), optional Maxwell/membrane-plastic/bending-plastic histories, optional unloaded-thickness cloth self-contact, and explicit clamp-slip offset/limit auditing. Zero gravity. Excludes inside-clamp frictional pull-out mechanics, clamped-overlap double-layer stiffness, moisture/thermal coupling, parameter identification, automatic garment readiness promotion and physical garment fit.";

struct CylinderMesh {
    n_verts: usize,
    source_rest: Vec<Vec2>,
    source_triangles: Vec<[usize; 3]>,
    weld: Vec<usize>,
    welded_triangles: Vec<[usize; 3]>,
    tri_rest: Vec<[Vec2; 3]>,
    mesh_radius_m: f64,
    reference_positions: Vec<Vec3>,
}

fn build_cylinder_mesh(fixture: &Fixture) -> CylinderMesh {
    let [n_theta, n_z] = fixture.cells;
    let c_wrap = fixture.wrapped_circumference_m();
    let (raw_rest, source_triangles) = grid(
        c_wrap,
        fixture.nominal_free_height_m,
        n_theta,
        n_z,
        Vec2::ZERO,
    );
    let [u, v] = fixture.warp_axis;
    let source_rest: Vec<Vec2> = raw_rest
        .iter()
        .map(|p| Vec2::new(u.mul_add(p.x, v * p.y), (-v).mul_add(p.x, u * p.y)))
        .collect();
    let n_verts = n_theta * (n_z + 1);
    let mut weld = Vec::with_capacity(raw_rest.len());
    for j in 0..=n_z {
        for i in 0..=n_theta {
            weld.push(j * n_theta + (i % n_theta));
        }
    }
    let welded_triangles: Vec<[usize; 3]> = source_triangles
        .iter()
        .map(|t| t.map(|idx| weld[idx]))
        .collect();
    let tri_rest: Vec<[Vec2; 3]> = source_triangles
        .iter()
        .map(|t| t.map(|idx| source_rest[idx]))
        .collect();
    let mesh_radius_m = fixture.isometric_mesh_radius_m();
    let mut reference_positions = Vec::with_capacity(n_verts);
    for j in 0..=n_z {
        let z = fixture.nominal_free_height_m * (j as f64) / (n_z as f64);
        for i in 0..n_theta {
            let angle = 2.0 * PI * (i as f64) / (n_theta as f64);
            let (sin_a, cos_a) = det::sin_cos(angle);
            reference_positions.push(Vec3::new(mesh_radius_m * cos_a, mesh_radius_m * sin_a, z));
        }
    }
    CylinderMesh {
        n_verts,
        source_rest,
        source_triangles,
        weld,
        welded_triangles,
        tri_rest,
        mesh_radius_m,
        reference_positions,
    }
}

fn holder_separation_m(fixture: &Fixture, waypoint: &Waypoint) -> Result<f64, Refusal> {
    if !waypoint.time_s.is_finite()
        || !waypoint.axial_displacement_m.is_finite()
        || !waypoint.rotation_deg.is_finite()
        || !waypoint.clamp_slip_m.is_finite()
        || waypoint.clamp_slip_m < 0.0
    {
        return Err(invalid(
            "cylinder waypoint requires finite time/displacement/rotation and nonnegative clamp_slip_m",
        ));
    }
    if waypoint.rotation_deg.abs() > 360.0 {
        return Err(out_of_range(
            "cylinder rotation_deg magnitude exceeds 360 degrees",
        ));
    }
    let z = fixture.mounted_free_height_m() + waypoint.axial_displacement_m - waypoint.clamp_slip_m;
    if !z.is_finite() || z <= 1e-6 {
        return Err(out_of_range(format!(
            "prescribed holder separation {z} m must remain strictly positive"
        )));
    }
    if z > 3.0 * fixture.nominal_free_height_m {
        return Err(out_of_range(
            "prescribed holder separation exceeds 3x nominal_free_height_m",
        ));
    }
    Ok(z)
}

fn set_upper_ring(
    positions: &mut [Vec3],
    n_theta: usize,
    n_z: usize,
    mesh_radius_m: f64,
    separation_m: f64,
    rotation_deg: f64,
) {
    let theta_rad = rotation_deg.to_radians();
    let start = n_z * n_theta;
    for i in 0..n_theta {
        let angle = 2.0 * PI * (i as f64) / (n_theta as f64) + theta_rad;
        let (sin_a, cos_a) = det::sin_cos(angle);
        positions[start + i] =
            Vec3::new(mesh_radius_m * cos_a, mesh_radius_m * sin_a, separation_m);
    }
}

fn median(mut values: Vec<f64>) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let mid = values.len() / 2;
    if values.len() % 2 == 1 {
        Some(values[mid])
    } else {
        Some(0.5 * (values[mid - 1] + values[mid]))
    }
}

fn audit_protocol(
    fixture: &Fixture,
    loading: &[Waypoint],
    nominal: Option<&NominalProtocol>,
    material_discrepancy: Option<String>,
) -> Result<ProtocolAudit, Refusal> {
    if let Some(nom) = nominal {
        if !nom.rate_tolerance_fraction.is_finite() || nom.rate_tolerance_fraction < 0.0 {
            return Err(invalid(
                "nominal_protocol.rate_tolerance_fraction must be finite and nonnegative",
            ));
        }
        for val in [
            nom.nominal_twist_rate_deg_s,
            nom.nominal_axial_rate_m_s,
            nom.nominal_compression_travel_m,
        ]
        .into_iter()
        .flatten()
        {
            if !val.is_finite() || val <= 0.0 {
                return Err(invalid(
                    "nominal_protocol rates and travel must be finite and positive when supplied",
                ));
            }
        }
    }
    let mut pos_twist_rates = Vec::new();
    let mut comp_rates = Vec::new();
    for pair in loading.windows(2) {
        let dt = pair[1].time_s - pair[0].time_s;
        if dt > 0.0 {
            let d_rot = pair[1].rotation_deg - pair[0].rotation_deg;
            if d_rot > 1e-9 {
                pos_twist_rates.push(d_rot / dt);
            }
            let d_ax = pair[1].axial_displacement_m - pair[0].axial_displacement_m;
            if d_ax < -1e-12 {
                comp_rates.push((-d_ax) / dt);
            }
        }
    }
    let median_twist = median(pos_twist_rates);
    let median_comp = median(comp_rates);
    let min_axial = loading
        .iter()
        .map(|w| w.axial_displacement_m)
        .fold(0.0_f64, f64::min);
    let peak_instrument_compression_m = (-min_axial).max(0.0);
    let min_net_from_nominal = loading
        .iter()
        .map(|w| fixture.pretension_displacement_m + w.axial_displacement_m - w.clamp_slip_m)
        .fold(0.0_f64, f64::min);
    let peak_net_compression_from_nominal_m = (-min_net_from_nominal).max(0.0);

    let mut discrepancies = Vec::new();
    if fixture.pretension_displacement_m.abs() > 0.0 {
        discrepancies.push(format!(
            "pretension_displacement_m = {:.6} mm shifts mounted zero separation to {:.6} mm vs nominal {:.6} mm; delta/nominal_height is rejected",
            fixture.pretension_displacement_m * 1e3,
            fixture.mounted_free_height_m() * 1e3,
            fixture.nominal_free_height_m * 1e3,
        ));
    }
    if let Some(nom) = nominal {
        let tol = nom.rate_tolerance_fraction;
        if let (Some(obs), Some(exp)) = (median_twist, nom.nominal_twist_rate_deg_s)
            && ((obs - exp).abs() / exp) > tol
        {
            discrepancies.push(format!(
                "observed median positive twist rate {:.6} deg/s differs from nominal {:.6} deg/s by {:.2}%",
                obs,
                exp,
                100.0 * (obs - exp) / exp
            ));
        }
        if let (Some(obs), Some(exp)) = (median_comp, nom.nominal_axial_rate_m_s)
            && ((obs - exp).abs() / exp) > tol
        {
            discrepancies.push(format!(
                "observed median compression rate {:.6} mm/s differs from nominal {:.6} mm/s by {:.2}%",
                obs * 1e3,
                exp * 1e3,
                100.0 * (obs - exp) / exp
            ));
        }
        if let Some(exp_travel) = nom.nominal_compression_travel_m
            && (peak_instrument_compression_m - exp_travel).abs() > 1e-6
        {
            discrepancies.push(format!(
                "peak instrument compression {:.6} mm differs from nominal {:.6} mm (net compression from nominal L_0 is {:.6} mm)",
                peak_instrument_compression_m * 1e3,
                exp_travel * 1e3,
                peak_net_compression_from_nominal_m * 1e3,
            ));
        }
    }

    Ok(ProtocolAudit {
        wrapped_circumference_m: fixture.wrapped_circumference_m(),
        seam_overlap_width_m: (fixture.specimen_width_m - fixture.wrapped_circumference_m())
            .max(0.0),
        clamped_height_margin_m: (fixture.specimen_height_m - fixture.nominal_free_height_m)
            .max(0.0),
        mounted_free_height_m: fixture.mounted_free_height_m(),
        median_positive_twist_rate_deg_s: median_twist,
        median_compression_rate_m_s: median_comp,
        peak_instrument_compression_m,
        peak_net_compression_from_nominal_m,
        material_source_discrepancy: material_discrepancy,
        discrepancies,
    })
}

fn subdivisions(input: &Request, has_history: bool) -> Result<Vec<usize>, Refusal> {
    input.fixture.validate()?;
    input.controls.validate()?;
    if input.schema != REQUEST_SCHEMA {
        return Err(invalid(format!(
            "unsupported cylinder schema {}; expected {REQUEST_SCHEMA}",
            input.schema
        )));
    }
    if input.loading.is_empty() || input.loading.len() > 129 {
        return Err(invalid("cylinder loading requires 1..=129 waypoints"));
    }
    if input.loading[0].time_s != 0.0 {
        return Err(invalid("cylinder loading must start at time_s = 0"));
    }
    if input.loading.len() == 1 && has_history {
        return Err(out_of_range(
            "history-bearing materials require an explicit timed loading path (at least 2 waypoints)",
        ));
    }
    let start_sep = holder_separation_m(&input.fixture, &input.loading[0])?;
    if has_history
        && ((start_sep - input.fixture.nominal_free_height_m).abs() > 1e-12
            || input.loading[0].rotation_deg != 0.0
            || input.loading[0].clamp_slip_m != 0.0)
    {
        return Err(out_of_range(
            "history-bearing cylinder runs must start at pristine unstretched separation (axial_displacement_m = -pretension_displacement_m, rotation_deg = 0, clamp_slip_m = 0) and ramp pretension explicitly in time",
        ));
    }
    let mut total_steps = 0usize;
    let mut counts = Vec::with_capacity(input.loading.len().saturating_sub(1));
    for pair in input.loading.windows(2) {
        holder_separation_m(&input.fixture, &pair[1])?;
        let dt = pair[1].time_s - pair[0].time_s;
        if !dt.is_finite() || dt <= 0.0 {
            return Err(invalid("cylinder loading times must increase strictly"));
        }
        if pair[1].clamp_slip_m + 1e-15 < pair[0].clamp_slip_m {
            return Err(invalid(
                "clamp_slip_m is cumulative irreversible pull-out and must not decrease",
            ));
        }
        let raw_count = (dt / input.controls.max_step_s).ceil().max(1.0);
        if !raw_count.is_finite()
            || raw_count > input.controls.max_steps.saturating_sub(total_steps) as f64
        {
            return Err(budget(
                "cylinder experiment exceeds controls.max_steps; increase max_steps or max_step_s",
            ));
        }
        let count = raw_count as usize;
        total_steps += count;
        counts.push(count);
    }
    let [n_theta, n_z] = input.fixture.cells;
    if n_theta * (n_z + 1) * (total_steps + 1) > 524_288 {
        return Err(budget(
            "cylinder experiment exceeds 524288 retained vertex states; coarsen cells or shorten path",
        ));
    }
    Ok(counts)
}

fn record_state(
    fixture: &Fixture,
    controls: &Controls,
    solver: &Solver,
    waypoint: &Waypoint,
    h: f64,
    old_velocity: &[Vec3],
    solve: Option<SolveReport>,
) -> Result<State, Refusal> {
    let [n_theta, n_z] = fixture.cells;
    let sep_m = holder_separation_m(fixture, waypoint)?;
    let elastic_forces = solver.checked_elastic_forces()?;
    let mut total_forces = elastic_forces.clone();
    if let Some(contact) = &solver.contact {
        let terms = contact.terms(&solver.x, true, h > 0.0);
        if !terms.energy.is_finite() {
            return Err(Refusal::new(
                RefusalCode::IntersectingStart,
                "cylinder self-contact barrier energy is non-finite",
            ));
        }
        for block in &terms.blocks {
            for (&(vi, comp), &grad) in block.dofs.iter().zip(&block.grad) {
                let force_comp = -grad;
                match comp {
                    0 => total_forces[vi].x += force_comp,
                    1 => total_forces[vi].y += force_comp,
                    _ => total_forces[vi].z += force_comp,
                }
            }
        }
    }

    let mut holders = [Vec3::ZERO; 2];
    let mut holder_torques_z = [0.0_f64; 2];
    let mut linear_inertia = Vec3::ZERO;
    let mut angular_inertia_z = 0.0_f64;
    let mut free_residual = 0.0_f64;
    let mut kinetic_j = 0.0_f64;
    let mut elastic_axial_torque_sum = 0.0_f64;

    for (i, (&f_tot, &f_el)) in total_forces.iter().zip(&elastic_forces).enumerate() {
        let pos = solver.x[i];
        elastic_axial_torque_sum += pos.x.mul_add(f_el.y, -(pos.y * f_el.x));
        let row = i / n_theta;
        if row == 0 {
            holders[0] -= f_tot;
            holder_torques_z[0] -= pos.x.mul_add(f_tot.y, -(pos.y * f_tot.x));
        } else if row == n_z {
            holders[1] -= f_tot;
            holder_torques_z[1] -= pos.x.mul_add(f_tot.y, -(pos.y * f_tot.x));
        } else {
            let accel = if h > 0.0 {
                (solver.v[i] - old_velocity[i]) / h
            } else {
                Vec3::ZERO
            };
            let f_in = accel * solver.model.mass[i];
            linear_inertia += f_in;
            angular_inertia_z += pos.x.mul_add(f_in.y, -(pos.y * f_in.x));
            let r = f_in - f_tot;
            free_residual = free_residual.max(r.x.abs()).max(r.y.abs()).max(r.z.abs());
            kinetic_j += 0.5 * solver.model.mass[i] * solver.v[i].norm2();
        }
    }

    let momentum_balance = (holders[0] + holders[1] - linear_inertia).norm();
    let elastic_torque_balance = elastic_axial_torque_sum.abs();
    let torque_balance = (holder_torques_z[0] + holder_torques_z[1] - angular_inertia_z).abs();
    let n_scale = solver.x.len() as f64;
    let torque_tol = controls.force_tolerance_n * fixture.holder_radius_m.max(1.0) * n_scale;
    if !free_residual.is_finite()
        || !momentum_balance.is_finite()
        || !elastic_torque_balance.is_finite()
        || !torque_balance.is_finite()
        || (solve.is_some() && free_residual > controls.force_tolerance_n)
        || (solve.is_some() && momentum_balance > controls.force_tolerance_n * n_scale)
        || elastic_torque_balance > torque_tol
    {
        return Err(Refusal::new(
            RefusalCode::NotConverged,
            format!(
                "cylinder residual {free_residual:.3e} N / momentum balance {momentum_balance:.3e} N / elastic torque balance {elastic_torque_balance:.3e} N*m exceeds tolerance",
            ),
        ));
    }

    let upper_force_n = holders[1].z;
    let slip_limit_exceeded = fixture
        .grip_slip_force_limit_n
        .is_some_and(|limit| upper_force_n > limit);
    if slip_limit_exceeded && fixture.refuse_unmodeled_slip && waypoint.clamp_slip_m == 0.0 {
        return Err(out_of_range(format!(
            "upper axial force {upper_force_n:.6} N exceeds grip_slip_force_limit_n {:.6} N without explicit clamp_slip_m",
            fixture.grip_slip_force_limit_n.unwrap_or(0.0)
        )));
    }

    let mut strain_ranges = [[f64::INFINITY, f64::NEG_INFINITY]; 3];
    for element in &solver.model.membranes {
        let [a, b, c] = element.v.map(|idx| solver.x[idx]);
        if (b - a).cross(c - a).norm2() <= 1e-30 {
            return Err(Refusal::new(
                RefusalCode::DegenerateGeometry,
                "cylinder mesh developed a zero-area degenerate triangle",
            ));
        }
        for (range, val) in strain_ranges.iter_mut().zip(element.strain(&solver.x)) {
            range[0] = range[0].min(val);
            range[1] = range[1].max(val);
        }
    }

    let mid_row = n_z / 2;
    let mut r_min = f64::INFINITY;
    let mut r_max = f64::NEG_INFINITY;
    let mut r_sum = 0.0;
    for i in 0..n_theta {
        let p = solver.x[mid_row * n_theta + i];
        let r = det::hypot(p.x, p.y);
        r_min = r_min.min(r);
        r_max = r_max.max(r);
        r_sum += r;
    }
    let mid_radius_range_m = [r_min, r_max, r_sum / n_theta as f64];

    let stored_membrane_energy_j = solver.membrane_energy()?;
    let hinge_energy_j: f64 = solver
        .model
        .hinges
        .iter()
        .enumerate()
        .map(|(i, hinge)| hinge.energy(&solver.x, solver.model.hinge_rigidity(i)))
        .sum();
    let panel_energy_j = solver.panel_bending_energy()?;
    let stored_bending_energy_j = hinge_energy_j + panel_energy_j;
    let maxwell_dissipation_j = solver
        .membrane_relaxation_dissipation_j()?
        .map_or(0.0, |v| v.iter().sum());
    let membrane_plastic_dissipation_j = solver
        .membrane_plastic_dissipation_j()?
        .map_or(0.0, |v| v.iter().sum());
    let bending_plastic_dissipation_j = solver
        .bending_plastic_dissipation_j()?
        .map_or(0.0, |v| v.iter().sum());
    if [
        stored_membrane_energy_j,
        stored_bending_energy_j,
        kinetic_j,
        maxwell_dissipation_j,
        membrane_plastic_dissipation_j,
        bending_plastic_dissipation_j,
    ]
    .iter()
    .any(|v| !v.is_finite())
    {
        return Err(Refusal::new(
            RefusalCode::NonFinite,
            "cylinder energy or dissipation is non-finite",
        ));
    }

    Ok(State {
        time_s: waypoint.time_s,
        time_step_s: h,
        axial_displacement_m: waypoint.axial_displacement_m,
        clamp_slip_m: waypoint.clamp_slip_m,
        holder_separation_m: sep_m,
        true_axial_extension_fraction: (sep_m - fixture.nominal_free_height_m)
            / fixture.nominal_free_height_m,
        rotation_deg: waypoint.rotation_deg,
        holder_reaction_n: holders.map(vec3_array),
        upper_force_n,
        lower_force_n: holders[0].z,
        tare_adjusted_force_n: upper_force_n - fixture.tare_force_n,
        upper_torque_nm: holder_torques_z[1],
        lower_torque_nm: holder_torques_z[0],
        tare_adjusted_torque_nm: holder_torques_z[1] - fixture.tare_torque_nm,
        slip_limit_exceeded,
        free_dynamic_residual_n: free_residual,
        momentum_balance_residual_n: momentum_balance,
        elastic_torque_balance_residual_nm: elastic_torque_balance,
        torque_balance_residual_nm: torque_balance,
        mid_radius_range_m,
        green_strain_range: strain_ranges,
        stored_membrane_energy_j,
        stored_bending_energy_j,
        free_kinetic_energy_j: kinetic_j,
        maxwell_dissipation_j,
        membrane_plastic_dissipation_j,
        bending_plastic_dissipation_j,
        positions_m: solver.x.iter().copied().map(vec3_array).collect(),
        velocities_m_s: solver.v.iter().copied().map(vec3_array).collect(),
        relaxation: solver.relaxation_report()?,
        membrane_plasticity: solver.membrane_plasticity_report()?,
        bending_plasticity: solver.bending_plasticity_report()?,
        solve,
    })
}

/// Solve a single memoryless static cylinder equilibrium directly from explicit orthotropic parameters.
/// `parameters` are `[warp_n_m, weft_n_m, poisson_warp_weft, shear_n_m]`; `bending_nm` is scalar hinge rigidity.
///
/// # Errors
/// Invalid geometry/parameters, axial compression without bending rigidity, or failed equilibrium.
pub fn solve_static(
    fixture: &Fixture,
    parameters: [f64; 4],
    bending_nm: f64,
    panel_bending: Option<BendingTensor>,
    axial_displacement_m: f64,
    rotation_deg: f64,
    controls: &Controls,
) -> Result<State, Refusal> {
    fixture.validate()?;
    controls.validate()?;
    if !bending_nm.is_finite() || bending_nm < 0.0 {
        return Err(out_of_range("bending_nm must be finite and nonnegative"));
    }
    let waypoint = Waypoint {
        time_s: 0.0,
        axial_displacement_m,
        rotation_deg,
        clamp_slip_m: 0.0,
    };
    let sep_m = holder_separation_m(fixture, &waypoint)?;
    if sep_m + 1e-12 < fixture.nominal_free_height_m && bending_nm == 0.0 && panel_bending.is_none()
    {
        return Err(invalid(
            "axial cylinder compression below nominal_free_height_m requires positive bending rigidity or a BendingTensor",
        ));
    }
    let [e1, e2, nu, g12] = parameters;
    let membrane = Membrane::orthotropic(e1, e2, nu, g12, 1.0);
    membrane.validate()?;
    let mesh = build_cylinder_mesh(fixture);
    let mut model = ClothModel::from_triangles(
        mesh.n_verts,
        &mesh.welded_triangles,
        &mesh.tri_rest,
        membrane,
        bending_nm,
        0.1,
    )?;
    if let Some(tensor) = panel_bending {
        model.set_panel_bending(
            &mesh.source_rest,
            &mesh.source_triangles,
            &mesh.weld,
            &vec![Some(tensor); mesh.welded_triangles.len()],
        )?;
    }
    let [n_theta, n_z] = fixture.cells;
    let mut positions = mesh.reference_positions.clone();
    for j in 0..=n_z {
        let frac = j as f64 / n_z as f64;
        let z = sep_m * frac;
        let rot = (rotation_deg * frac).to_radians();
        for i in 0..n_theta {
            let angle = 2.0 * PI * (i as f64) / (n_theta as f64) + rot;
            let (sin_a, cos_a) = det::sin_cos(angle);
            positions[j * n_theta + i] =
                Vec3::new(mesh.mesh_radius_m * cos_a, mesh.mesh_radius_m * sin_a, z);
        }
    }
    let fixed: Vec<[bool; 3]> = (0..mesh.n_verts)
        .map(|idx| {
            let row = idx / n_theta;
            let clamped = row == 0 || row == n_z;
            [clamped, clamped, clamped]
        })
        .collect();
    let mut solver = Solver::new(model, positions, &fixed, Vec3::ZERO);
    solver.tol = controls.static_tolerance_m;
    solver.max_newton = controls.max_newton;
    let rep = solver.static_solve()?;
    let zero_vel = vec![Vec3::ZERO; mesh.n_verts];
    record_state(
        fixture,
        controls,
        &solver,
        &waypoint,
        0.0,
        &zero_vel,
        Some(rep),
    )
}

#[allow(clippy::too_many_arguments)]
fn assemble_solver(
    fixture: &Fixture,
    controls: &Controls,
    mesh: &CylinderMesh,
    membrane: Membrane,
    bending_nm: f64,
    mass_kg_m2: f64,
    response: Option<CompiledMembrane>,
    domain: Option<StrainDomain>,
    panel_bending: Option<BendingTensor>,
    relaxation: Option<CompiledRelaxation>,
    membrane_plasticity: Option<PlasticMembrane>,
    bending_plasticity: Option<PlasticBending>,
    self_contact: Option<(f64, f64)>,
    initial_waypoint: &Waypoint,
) -> Result<(Solver, Option<SolveReport>), Refusal> {
    let n_tris = mesh.welded_triangles.len();
    let mut model = ClothModel::from_triangles(
        mesh.n_verts,
        &mesh.welded_triangles,
        &mesh.tri_rest,
        membrane,
        bending_nm,
        mass_kg_m2,
    )?;
    if model.mass.iter().any(|m| !m.is_finite() || *m <= 0.0)
        || !model.mass.iter().sum::<f64>().is_finite()
    {
        return Err(invalid("cylinder active gauge mass is not representable"));
    }
    if let Some(law) = response {
        model.set_membrane_responses(&vec![Some(law); n_tris])?;
    }
    if let Some(dom) = domain {
        model.set_membrane_domains(&vec![Some(dom); n_tris])?;
    }
    if let Some(tensor) = panel_bending {
        model.set_panel_bending(
            &mesh.source_rest,
            &mesh.source_triangles,
            &mesh.weld,
            &vec![Some(tensor); n_tris],
        )?;
    }

    let [n_theta, n_z] = fixture.cells;
    let sep_0 = holder_separation_m(fixture, initial_waypoint)?;
    let mut initial_positions = mesh.reference_positions.clone();
    for j in 0..=n_z {
        let frac = j as f64 / n_z as f64;
        let z = sep_0 * frac;
        let rot = (initial_waypoint.rotation_deg * frac).to_radians();
        for i in 0..n_theta {
            let angle = 2.0 * PI * (i as f64) / (n_theta as f64) + rot;
            let (sin_a, cos_a) = det::sin_cos(angle);
            initial_positions[j * n_theta + i] =
                Vec3::new(mesh.mesh_radius_m * cos_a, mesh.mesh_radius_m * sin_a, z);
        }
    }
    let fixed: Vec<[bool; 3]> = (0..mesh.n_verts)
        .map(|idx| {
            let row = idx / n_theta;
            let clamped = row == 0 || row == n_z;
            [clamped, clamped, clamped]
        })
        .collect();

    let has_history =
        relaxation.is_some() || membrane_plasticity.is_some() || bending_plasticity.is_some();
    let mut solver = Solver::new(model, initial_positions, &fixed, Vec3::ZERO)
        .with_membrane_histories(
            &vec![relaxation; n_tris],
            &vec![membrane_plasticity; n_tris],
        )?
        .with_bending_plasticity(&vec![bending_plasticity; n_tris])?;
    if let Some((thickness_m, mu_self)) = self_contact {
        solver = solver.with_self_contact(ContactParams {
            dhat: controls.contact_dhat_m,
            thickness: thickness_m,
            kappa: controls.contact_kappa_n_m,
            mu: mu_self,
            eps_v: controls.friction_eps_v_m_s,
        })?;
    }
    let initial_solve = if !has_history {
        solver.tol = controls.static_tolerance_m;
        solver.max_newton = controls.max_newton;
        Some(solver.static_solve()?)
    } else {
        None
    };
    solver.tol = controls.velocity_tolerance_m_s;
    solver.max_newton = controls.max_newton;
    Ok((solver, initial_solve))
}

/// Evaluate a source-qualified cylindrical-holder forward experiment end to end.
///
/// # Errors
/// Invalid geometry/schedule/controls, uncharacterized or estimate-gated sheet properties,
/// `reported_thickness` misused for self-contact, axial compression without bending stiffness,
/// step/memory budget exhaustion, or Newton/balance failure. Atomic: no partial report on error.
pub fn evaluate(input: &Request) -> Result<Report, Refusal> {
    let resolved = textile_import::catalog()?.resolve(&input.material)?;
    let material = resolved.material();
    let policy = resolved.allows_estimates();
    if matches!(material.form, Form::Fiber | Form::Yarn | Form::Fill) {
        return Err(invalid(
            "cylindrical-holder experiments require a characterized sheet, not fiber, yarn or fill",
        ));
    }
    let mass_kg_m2 = material.mechanical_areal_density(policy)?;
    let response = resolved.membrane_response()?;
    let [warp, weft, poisson, shear] = if let Some(law) = &response {
        law.reference_moduli()?
    } else {
        [
            material.require("warp_stiffness", "N/m", policy)?,
            material.require("weft_stiffness", "N/m", policy)?,
            material.require("poisson_warp_weft", "1", policy)?,
            material.require("shear_stiffness", "N/m", policy)?,
        ]
    };
    let membrane = Membrane::orthotropic(warp, weft, poisson, shear, 1.0);
    membrane.validate()?;

    let relaxation = resolved.membrane_relaxation()?;
    let membrane_plasticity = resolved.membrane_plasticity()?;
    let panel_bending = resolved.bending_response()?;
    let bending_plasticity = resolved.bending_plasticity()?;
    let bending_nm = if material.properties.contains_key("bending_rigidity") {
        let b = material.require("bending_rigidity", "N*m", policy)?;
        if b < 0.0 {
            return Err(out_of_range("bending_rigidity must be nonnegative"));
        }
        b
    } else {
        0.0
    };
    if bending_plasticity.is_some() && panel_bending.is_none() {
        return Err(invalid(
            "plastic bending requires a characterized directional BendingTensor",
        ));
    }

    let has_history =
        relaxation.is_some() || membrane_plasticity.is_some() || bending_plasticity.is_some();
    let counts = subdivisions(input, has_history)?;

    let any_net_compression = input.loading.iter().any(|w| {
        input.fixture.pretension_displacement_m + w.axial_displacement_m - w.clamp_slip_m < -1e-12
    });
    if any_net_compression && bending_nm == 0.0 && panel_bending.is_none() {
        return Err(invalid(
            "cylinder axial compression below nominal_free_height_m requires sourced bending_rigidity or BendingTensor; membrane-only wrinkling has no physical wavelength",
        ));
    }

    let self_contact = if input.fixture.self_contact {
        let thickness_m = resolved.unloaded_thickness()?.value;
        let mu_self = material.require("friction_self", "1", policy)?;
        if mu_self < 0.0 {
            return Err(out_of_range("friction_self must be nonnegative"));
        }
        Some((thickness_m, mu_self))
    } else {
        None
    };

    let protocol_audit = audit_protocol(
        &input.fixture,
        &input.loading,
        input.nominal_protocol.as_ref(),
        material.structure.get("source_discrepancy").cloned(),
    )?;

    let mesh = build_cylinder_mesh(&input.fixture);
    let domain = fitted_domain(material)?;
    let (mut solver, initial_solve) = assemble_solver(
        &input.fixture,
        &input.controls,
        &mesh,
        membrane,
        bending_nm,
        mass_kg_m2,
        response,
        domain,
        panel_bending,
        relaxation,
        membrane_plasticity,
        bending_plasticity,
        self_contact,
        &input.loading[0],
    )?;

    let mut newton_iterations = initial_solve.map_or(0, |s| s.iterations);
    let zero_vel = vec![Vec3::ZERO; mesh.n_verts];
    let first_state = record_state(
        &input.fixture,
        &input.controls,
        &solver,
        &input.loading[0],
        0.0,
        &zero_vel,
        initial_solve,
    )?;
    let mut first_slip_time_s = first_state.slip_limit_exceeded.then_some(0.0);
    let mut states = vec![first_state];

    let [n_theta, n_z] = input.fixture.cells;
    let mut current_time = 0.0_f64;
    let mut prev_sep = holder_separation_m(&input.fixture, &input.loading[0])?;
    let mut prev_rot_rad = input.loading[0].rotation_deg.to_radians();

    for (pair, count) in input.loading.windows(2).zip(counts) {
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
                return Err(invalid("cylinder time step is not representable"));
            }
            let curr_sep = holder_separation_m(&input.fixture, &waypoint)?;
            let curr_rot_rad = waypoint.rotation_deg.to_radians();
            let vz_m_s = (curr_sep - prev_sep) / h;
            let omega_rad_s = (curr_rot_rad - prev_rot_rad) / h;

            let old_velocity = solver.v.clone();
            set_upper_ring(
                &mut solver.x,
                n_theta,
                n_z,
                mesh.mesh_radius_m,
                curr_sep,
                waypoint.rotation_deg,
            );
            let numerical = solver.step(h)?;
            for i in 0..n_theta {
                let idx = n_z * n_theta + i;
                let p = solver.x[idx];
                solver.v[idx] = Vec3::new(-omega_rad_s * p.y, omega_rad_s * p.x, vz_m_s);
            }
            newton_iterations += numerical.iterations;
            let st = record_state(
                &input.fixture,
                &input.controls,
                &solver,
                &waypoint,
                h,
                &old_velocity,
                Some(numerical),
            )?;
            if st.slip_limit_exceeded && first_slip_time_s.is_none() {
                first_slip_time_s = Some(st.time_s);
            }
            states.push(st);
            current_time = waypoint.time_s;
            prev_sep = curr_sep;
            prev_rot_rad = curr_rot_rad;
        }
    }

    let mut frozen = input.clone();
    frozen.material = resolved.frozen_selection();
    let replay_request_json = serde_json::to_string(&frozen).map_err(invalid)?;
    let input_sha256 = sha256_hex(replay_request_json.as_bytes());
    let active_gauge_mass_kg = solver.model.mass.iter().sum();

    Ok(Report {
        schema: REPORT_SCHEMA.into(),
        input: frozen,
        input_sha256,
        replay_request_json,
        isometric_mesh_radius_m: mesh.mesh_radius_m,
        reference_positions_m: mesh
            .reference_positions
            .into_iter()
            .map(vec3_array)
            .collect(),
        triangles: mesh.welded_triangles,
        active_gauge_mass_kg,
        first_slip_time_s,
        protocol_audit,
        states,
        newton_iterations,
        model_scope: MODEL_SCOPE.into(),
        mechanics_readiness_promoted: false,
        fit_claim: false,
        empirical_validation_claim: false,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use ftx_pattern::materials::{Evidence, Material, Quantity, Source};

    use super::*;

    fn fabric101_fixture(warp_axis: [f64; 2], n_theta: usize, n_z: usize) -> Fixture {
        Fixture {
            holder_radius_m: 0.040,
            specimen_width_m: 0.260,
            specimen_height_m: 0.110,
            nominal_free_height_m: 0.050,
            pretension_displacement_m: 0.0,
            tare_force_n: 0.0,
            tare_torque_nm: 0.0,
            warp_axis,
            cells: [n_theta, n_z],
            self_contact: false,
            grip_slip_force_limit_n: None,
            refuse_unmodeled_slip: false,
        }
    }

    fn measured_quantity(value: f64, unit: &str) -> Quantity {
        Quantity {
            value,
            unit: unit.into(),
            evidence: Evidence::Measured,
            source: "lab".into(),
            method: "synthetic known-answer verification coupon".into(),
            bounds: None,
            conditions: BTreeMap::new(),
        }
    }

    fn synthetic_sheet_selection(bending_nm: f64, with_unloaded_thickness: bool) -> Selection {
        let mut properties = BTreeMap::from([
            ("areal_density".into(), measured_quantity(0.150, "kg/m2")),
            ("warp_stiffness".into(), measured_quantity(1200.0, "N/m")),
            ("weft_stiffness".into(), measured_quantity(800.0, "N/m")),
            ("poisson_warp_weft".into(), measured_quantity(0.25, "1")),
            ("shear_stiffness".into(), measured_quantity(300.0, "N/m")),
            (
                "bending_rigidity".into(),
                measured_quantity(bending_nm, "N*m"),
            ),
        ]);
        if with_unloaded_thickness {
            properties.insert("thickness".into(), measured_quantity(0.0004, "m"));
            properties.insert("friction_self".into(), measured_quantity(0.35, "1"));
        }
        let record = Material {
            id: "synthetic/cylinder-sheet".into(),
            name: "Synthetic Cylinder Sheet".into(),
            form: Form::Woven,
            composition: Vec::new(),
            composition_complete: false,
            structure: BTreeMap::new(),
            properties,
            membrane_response: None,
            membrane_relaxation: None,
            thermal_response: None,
            optical_faces: None,
            sources: BTreeMap::from([(
                "lab".into(),
                Source {
                    reference: "synthetic://cylinder-known-answer".into(),
                    accessed_on: "2026-11-11".into(),
                    note: "Synthetic constitutive inputs for closed-form cylinder verification."
                        .into(),
                },
            )]),
        };
        Selection {
            id: record.id.clone(),
            record: Some(record),
            ..Selection::default()
        }
    }

    #[test]
    fn isometric_cylinder_has_zero_initial_strain_and_exact_uniaxial_tension() {
        let fixture = fabric101_fixture([0.0, 1.0], 24, 6);
        let controls = Controls {
            static_tolerance_m: 1e-11,
            force_tolerance_n: 1e-8,
            ..Controls::default()
        };
        let rest_state = solve_static(
            &fixture,
            [1500.0, 900.0, 0.0, 250.0],
            0.0,
            None,
            0.0,
            0.0,
            &controls,
        )
        .unwrap();
        assert!(rest_state.upper_force_n.abs() < 1e-11);
        assert!(rest_state.upper_torque_nm.abs() < 1e-13);
        for range in rest_state.green_strain_range {
            assert!(
                range[0].abs() < 1e-14 && range[1].abs() < 1e-14,
                "{range:?}"
            );
        }

        // Exact Green-strain uniaxial tension when Poisson = 0 and B = 0:
        // Ψ = 0.5 * (2π R L_0) * E_1 * E_zz^2, E_zz = ε + 0.5 ε^2, F_z = 2π R E_1 E_zz (1 + ε).
        let dz = 0.001; // 2% axial extension on 50 mm
        let eps = dz / fixture.nominal_free_height_m;
        let e_zz = eps + 0.5 * eps * eps;
        let expected_force_n = fixture.wrapped_circumference_m() * 1500.0 * e_zz * (1.0 + eps);
        let stretched = solve_static(
            &fixture,
            [1500.0, 900.0, 0.0, 250.0],
            0.0,
            None,
            dz,
            0.0,
            &controls,
        )
        .unwrap();
        let rel_err = (stretched.upper_force_n - expected_force_n).abs() / expected_force_n;
        assert!(
            rel_err < 1e-11,
            "got {} N vs expected {} N (rel_err {rel_err:e})",
            stretched.upper_force_n,
            expected_force_n
        );
        assert!((stretched.upper_force_n + stretched.lower_force_n).abs() < 1e-10);

        // Positive Poisson ratio necks the free mid-height cylinder inward under axial tension.
        let necked = solve_static(
            &fixture,
            [1500.0, 900.0, 0.30, 250.0],
            1e-5,
            None,
            dz,
            0.0,
            &controls,
        )
        .unwrap();
        assert!(necked.mid_radius_range_m[2] < fixture.isometric_mesh_radius_m() - 5e-5);
    }

    #[test]
    fn small_angle_cylinder_torsion_matches_closed_form_and_converges_with_circumferential_refinement()
     {
        // Under pure torsion at dz = 0, the -45 deg principal strain is compressive (-gamma/2),
        // which triggers physical tension-field wrinkling as n_theta refines when bending = 0.
        // Applying small axial pretension dz > 0 (matching physical cylindrical-holder mounting)
        // keeps principal Green strains strictly tensile (0 projected Hessian iterations) and yields
        // the exact closed-form discrete torsion + stress-stiffening law for nu = 0, B = 0:
        //   T = (2*pi*R / L_0) * (G_12 * R_apothem^2 + E_axial * E_zz0 * R_mesh^2) * rot_rad.
        let e1 = 1200.0; // N/m
        let e2 = 900.0; // N/m
        let g12 = 400.0; // N/m
        let dz = 0.0005; // 1% pretension (0.5 mm on 50 mm)
        let rot_deg: f64 = 0.02;
        let rot_rad = rot_deg.to_radians();
        let controls = Controls {
            static_tolerance_m: 1e-10,
            force_tolerance_n: 1e-8,
            ..Controls::default()
        };
        let mut continuum_errors = Vec::new();
        for n_theta in [8usize, 16, 32, 64] {
            let fixture = fabric101_fixture([0.0, 1.0], n_theta, 6);
            let eps = dz / fixture.nominal_free_height_m;
            let e_zz0 = eps + 0.5 * eps * eps;
            let st = solve_static(
                &fixture,
                [e1, e2, 0.0, g12],
                0.0,
                None,
                dz,
                rot_deg,
                &controls,
            )
            .unwrap();
            assert_eq!(st.solve.unwrap().projected_iterations, 0);
            let r_mesh = fixture.isometric_mesh_radius_m();
            let r_apothem = fixture.isometric_apothem_m();
            let discrete_torque_nm = (fixture.wrapped_circumference_m()
                / fixture.nominal_free_height_m)
                * (g12 * r_apothem * r_apothem + e1 * e_zz0 * r_mesh * r_mesh)
                * rot_rad;
            let continuum_torque_nm = (2.0 * PI * fixture.holder_radius_m.powi(3)
                / fixture.nominal_free_height_m)
                * (g12 + e1 * e_zz0)
                * rot_rad;
            let discrete_rel = (st.upper_torque_nm - discrete_torque_nm).abs() / discrete_torque_nm;
            assert!(
                discrete_rel < 1e-6,
                "n_theta={n_theta}: got {} vs discrete {discrete_torque_nm} (rel {discrete_rel:e})",
                st.upper_torque_nm
            );
            assert!((st.upper_torque_nm + st.lower_torque_nm).abs() < 1e-11);

            // Switching warp [0,1] -> weft [1,0] replaces E_1 -> E_2 in the axial stress-stiffening term
            // while preserving G_12 * R_apothem^2.
            let weft_fixture = fabric101_fixture([1.0, 0.0], n_theta, 6);
            let st_weft = solve_static(
                &weft_fixture,
                [e1, e2, 0.0, g12],
                0.0,
                None,
                dz,
                rot_deg,
                &controls,
            )
            .unwrap();
            let expected_weft_nm = (fixture.wrapped_circumference_m()
                / fixture.nominal_free_height_m)
                * (g12 * r_apothem * r_apothem + e2 * e_zz0 * r_mesh * r_mesh)
                * rot_rad;
            assert!((st_weft.upper_torque_nm - expected_weft_nm).abs() / expected_weft_nm < 1e-6);

            // For an isotropic sheet with positive bending stiffness (E_1 = E_2 = 1000, nu = 0.25, G_12 = 400),
            // bias [1/sqrt(2), 1/sqrt(2)] matches warp [0,1] to Newton precision.
            let inv_sqrt2 = std::f64::consts::FRAC_1_SQRT_2;
            let iso_warp = solve_static(
                &fixture,
                [1000.0, 1000.0, 0.25, g12],
                1e-4,
                None,
                dz,
                rot_deg,
                &controls,
            )
            .unwrap();
            let iso_bias = solve_static(
                &fabric101_fixture([inv_sqrt2, inv_sqrt2], n_theta, 6),
                [1000.0, 1000.0, 0.25, g12],
                1e-4,
                None,
                dz,
                rot_deg,
                &controls,
            )
            .unwrap();
            assert!((iso_warp.upper_torque_nm - iso_bias.upper_torque_nm).abs() < 1e-11);
            continuum_errors
                .push((st.upper_torque_nm - continuum_torque_nm).abs() / continuum_torque_nm);
        }
        assert!(continuum_errors[1] < 0.30 * continuum_errors[0]);
        assert!(continuum_errors[2] < 0.30 * continuum_errors[1]);
        assert!(continuum_errors[3] < 0.30 * continuum_errors[2]);
        assert!(continuum_errors[3] < 1.6e-3, "{continuum_errors:?}");
    }

    #[test]
    fn fabric101_l01_refuses_uncalibrated_mechanics_and_presser_thickness_then_audits_discrepancies()
     {
        // 1. Raw Fabric-101 release record has density + presser-foot reported_thickness only; must refuse.
        let uncalibrated = Request {
            schema: REQUEST_SCHEMA.into(),
            material: Selection {
                id: "fabric101-2026/l01".into(),
                ..Selection::default()
            },
            fixture: fabric101_fixture([0.0, 1.0], 12, 4),
            loading: vec![Waypoint {
                time_s: 0.0,
                axial_displacement_m: 0.0,
                rotation_deg: 0.0,
                clamp_slip_m: 0.0,
            }],
            nominal_protocol: None,
            controls: Controls::default(),
        };
        let err = evaluate(&uncalibrated).unwrap_err();
        assert!(
            err.message
                .contains("missing textile property warp_stiffness"),
            "{err:?}"
        );

        // 2. Supplying constitutive overrides on L01 while requesting self_contact MUST still refuse
        //    because Fabric-101 only has presser-foot `reported_thickness`, never unloaded `thickness`.
        let mut with_overrides = Selection {
            id: "fabric101-2026/l01".into(),
            sources: BTreeMap::from([(
                "cal".into(),
                Source {
                    reference: "synthetic://l01-test-law".into(),
                    accessed_on: "2026-11-11".into(),
                    note: "Synthetic test moduli to verify reported_thickness isolation.".into(),
                },
            )]),
            overrides: BTreeMap::from([
                (
                    "warp_stiffness".into(),
                    Quantity {
                        source: "cal".into(),
                        ..measured_quantity(900.0, "N/m")
                    },
                ),
                (
                    "weft_stiffness".into(),
                    Quantity {
                        source: "cal".into(),
                        ..measured_quantity(700.0, "N/m")
                    },
                ),
                (
                    "poisson_warp_weft".into(),
                    Quantity {
                        source: "cal".into(),
                        ..measured_quantity(0.2, "1")
                    },
                ),
                (
                    "shear_stiffness".into(),
                    Quantity {
                        source: "cal".into(),
                        ..measured_quantity(180.0, "N/m")
                    },
                ),
                (
                    "bending_rigidity".into(),
                    Quantity {
                        source: "cal".into(),
                        ..measured_quantity(2e-4, "N*m")
                    },
                ),
                (
                    "friction_self".into(),
                    Quantity {
                        source: "cal".into(),
                        ..measured_quantity(0.3, "1")
                    },
                ),
            ]),
            ..Selection::default()
        };
        let mut self_contact_req = uncalibrated.clone();
        self_contact_req.material = with_overrides.clone();
        self_contact_req.fixture.self_contact = true;
        let thick_err = evaluate(&self_contact_req).unwrap_err();
        assert!(
            thick_err
                .message
                .contains("missing textile property thickness"),
            "{thick_err:?}"
        );

        // 3. With self_contact disabled and L01 warp s1 raw protocol rates + 1.14125 mm pretension,
        //    audit catches pretension offset, 0.499854 vs 0.1 deg/s twist, 0.75 vs 1.0 mm/s compression,
        //    and 16.14125 mm instrument compression vs 15.0 mm net travel.
        let l01_fixture = Fixture {
            pretension_displacement_m: 0.001_141_25,
            tare_force_n: 0.12,
            tare_torque_nm: -0.0005,
            grip_slip_force_limit_n: Some(15.0),
            ..fabric101_fixture([0.0, 1.0], 12, 4)
        };
        let l01_req = Request {
            schema: REQUEST_SCHEMA.into(),
            material: std::mem::take(&mut with_overrides),
            fixture: l01_fixture,
            loading: vec![
                Waypoint {
                    time_s: 0.0,
                    axial_displacement_m: 0.0,
                    rotation_deg: 0.0,
                    clamp_slip_m: 0.0,
                },
                Waypoint {
                    time_s: 10.002_920_85,
                    axial_displacement_m: 0.0,
                    rotation_deg: 5.0,
                    clamp_slip_m: 0.0,
                },
                Waypoint {
                    time_s: 20.0,
                    axial_displacement_m: 0.0,
                    rotation_deg: 0.0,
                    clamp_slip_m: 0.0,
                },
                Waypoint {
                    time_s: 41.521_666_67,
                    axial_displacement_m: -0.016_141_25,
                    rotation_deg: 0.0,
                    clamp_slip_m: 0.0,
                },
                Waypoint {
                    time_s: 46.0,
                    axial_displacement_m: 0.0025,
                    rotation_deg: 0.0,
                    clamp_slip_m: 0.0005,
                },
            ],
            nominal_protocol: Some(NominalProtocol {
                nominal_twist_rate_deg_s: Some(0.1),
                nominal_axial_rate_m_s: Some(0.001),
                nominal_compression_travel_m: Some(0.015),
                rate_tolerance_fraction: 0.05,
            }),
            controls: Controls {
                max_step_s: 2.5,
                max_steps: 64,
                velocity_tolerance_m_s: 1e-11,
                ..Controls::default()
            },
        };
        let report = evaluate(&l01_req).unwrap();
        assert_eq!(report.schema, REPORT_SCHEMA);
        assert!(!report.mechanics_readiness_promoted);
        assert!(!report.fit_claim);
        assert!(!report.empirical_validation_claim);
        assert_eq!(
            report.protocol_audit.discrepancies.len(),
            4,
            "{:?}",
            report.protocol_audit.discrepancies
        );
        assert!(
            report
                .protocol_audit
                .material_source_discrepancy
                .as_deref()
                .unwrap()
                .contains("0.499854")
        );
        assert!((report.protocol_audit.peak_net_compression_from_nominal_m - 0.015).abs() < 1e-12);
        // Check mounted state (index 0) reflects 1.14125 mm pretension, NOT zero strain!
        assert!((report.states[0].holder_separation_m - 0.051_141_25).abs() < 1e-12);
        assert!(report.states[0].upper_force_n > 1.0);
        assert!(
            (report.states[0].tare_adjusted_force_n - (report.states[0].upper_force_n - 0.12))
                .abs()
                < 1e-12
        );
        // Check compression state reaches holder_separation_m = 0.035 m (negative compressive reaction).
        let comp_state = report
            .states
            .iter()
            .find(|s| (s.holder_separation_m - 0.035).abs() < 1e-9)
            .expect("peak compression waypoint");
        assert!(comp_state.upper_force_n < 0.0);
        // Check stretch-slip state detected slip threshold and subtracted clamp_slip_m = 0.5 mm.
        let last = report.states.last().unwrap();
        assert!((last.holder_separation_m - (0.051_141_25 + 0.0025 - 0.0005)).abs() < 1e-12);
        assert!(report.first_slip_time_s.is_some());
    }

    #[test]
    fn zero_bending_compression_and_unmodeled_slip_are_refused() {
        let req = Request {
            schema: REQUEST_SCHEMA.into(),
            material: synthetic_sheet_selection(0.0, false),
            fixture: fabric101_fixture([0.0, 1.0], 12, 4),
            loading: vec![
                Waypoint {
                    time_s: 0.0,
                    axial_displacement_m: 0.0,
                    rotation_deg: 0.0,
                    clamp_slip_m: 0.0,
                },
                Waypoint {
                    time_s: 1.0,
                    axial_displacement_m: -0.002,
                    rotation_deg: 0.0,
                    clamp_slip_m: 0.0,
                },
            ],
            nominal_protocol: None,
            controls: Controls {
                max_step_s: 0.5,
                ..Controls::default()
            },
        };
        let err = evaluate(&req).unwrap_err();
        assert!(
            err.message.contains("requires sourced bending_rigidity"),
            "{err:?}"
        );

        let mut slip_req = Request {
            material: synthetic_sheet_selection(1e-4, true),
            fixture: Fixture {
                self_contact: true,
                grip_slip_force_limit_n: Some(5.0),
                refuse_unmodeled_slip: true,
                ..fabric101_fixture([0.0, 1.0], 12, 4)
            },
            loading: vec![Waypoint {
                time_s: 0.0,
                axial_displacement_m: 0.002,
                rotation_deg: 0.0,
                clamp_slip_m: 0.0,
            }],
            ..req
        };
        let slip_err = evaluate(&slip_req).unwrap_err();
        assert!(
            slip_err.message.contains("grip_slip_force_limit_n"),
            "{slip_err:?}"
        );

        slip_req.fixture.refuse_unmodeled_slip = false;
        let ok = evaluate(&slip_req).unwrap();
        assert!(ok.states[0].slip_limit_exceeded);
    }

    #[test]
    fn maxwell_and_plastic_bending_cylinder_enforces_timed_pretension_and_records_dissipation() {
        let mut selected: Selection = serde_json::from_str(include_str!(
            "../../../tests/fixtures/materials/synthetic-plastic-bending.json"
        ))
        .unwrap();
        let maxwell: Selection = serde_json::from_str(include_str!(
            "../../../tests/fixtures/materials/synthetic-relaxation.json"
        ))
        .unwrap();
        selected.record.as_mut().unwrap().membrane_relaxation =
            maxwell.record.unwrap().membrane_relaxation;

        // Estimate-gated history selection must fail when allow_estimates is false.
        let mut denied_sel = selected.clone();
        denied_sel.allow_estimates = false;
        let denied_req = Request {
            schema: REQUEST_SCHEMA.into(),
            material: denied_sel,
            fixture: Fixture {
                pretension_displacement_m: 0.001,
                ..fabric101_fixture([0.0, 1.0], 12, 4)
            },
            loading: vec![
                Waypoint {
                    time_s: 0.0,
                    axial_displacement_m: -0.001,
                    rotation_deg: 0.0,
                    clamp_slip_m: 0.0,
                },
                Waypoint {
                    time_s: 0.2,
                    axial_displacement_m: 0.0,
                    rotation_deg: 0.0,
                    clamp_slip_m: 0.0,
                },
            ],
            nominal_protocol: None,
            controls: Controls {
                max_step_s: 0.1,
                ..Controls::default()
            },
        };
        assert!(evaluate(&denied_req).is_err());

        // Starting a history-bearing cylinder at non-pristine pretensioned height must refuse.
        let mut bad_start = Request {
            material: selected.clone(),
            ..denied_req.clone()
        };
        bad_start.loading[0].axial_displacement_m = 0.0;
        let start_err = evaluate(&bad_start).unwrap_err();
        assert!(
            start_err
                .message
                .contains("pristine unstretched separation"),
            "{start_err:?}"
        );

        // Timed ramp from pristine separation -> pretension -> twist -> axial compression -> return.
        let history_req = Request {
            schema: REQUEST_SCHEMA.into(),
            material: selected,
            fixture: Fixture {
                pretension_displacement_m: 0.001,
                self_contact: true,
                ..fabric101_fixture([0.0, 1.0], 12, 4)
            },
            loading: vec![
                Waypoint {
                    time_s: 0.0,
                    axial_displacement_m: -0.001,
                    rotation_deg: 0.0,
                    clamp_slip_m: 0.0,
                },
                Waypoint {
                    time_s: 0.2,
                    axial_displacement_m: 0.0,
                    rotation_deg: 0.0,
                    clamp_slip_m: 0.0,
                },
                Waypoint {
                    time_s: 0.6,
                    axial_displacement_m: 0.0,
                    rotation_deg: 8.0,
                    clamp_slip_m: 0.0,
                },
                Waypoint {
                    time_s: 1.0,
                    axial_displacement_m: -0.006,
                    rotation_deg: 0.0,
                    clamp_slip_m: 0.0,
                },
                Waypoint {
                    time_s: 1.4,
                    axial_displacement_m: 0.0,
                    rotation_deg: 0.0,
                    clamp_slip_m: 0.0,
                },
            ],
            nominal_protocol: None,
            controls: Controls {
                max_step_s: 0.2,
                max_steps: 32,
                ..Controls::default()
            },
        };
        let report = evaluate(&history_req).unwrap();
        assert!(report.states[0].solve.is_none());
        let last = report.states.last().unwrap();
        assert!(
            last.maxwell_dissipation_j > 0.0,
            "expected positive Maxwell dissipation"
        );
        assert!(
            last.bending_plastic_dissipation_j > 0.0,
            "expected positive plastic bending dissipation"
        );
        assert!(last.relaxation.is_some());
        assert!(last.bending_plasticity.is_some());
        for st in report.states.iter().skip(1) {
            assert!(
                st.free_dynamic_residual_n <= 1e-6,
                "{}",
                st.free_dynamic_residual_n
            );
            assert!(
                st.momentum_balance_residual_n <= 1e-5,
                "{}",
                st.momentum_balance_residual_n
            );
            assert!(
                st.torque_balance_residual_nm <= 1e-5,
                "{}",
                st.torque_balance_residual_nm
            );
        }

        // Frozen replay_request_json reproduces bit-identical serialized output.
        let replay_req: Request = serde_json::from_str(&report.replay_request_json).unwrap();
        let replayed = evaluate(&replay_req).unwrap();
        assert_eq!(
            serde_json::to_string(&report).unwrap(),
            serde_json::to_string(&replayed).unwrap()
        );
    }
}

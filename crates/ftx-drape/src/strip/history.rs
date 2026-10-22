//! Timed, planar clamped coupons through the ordinary material and implicit dynamic solver.
//!
//! A grip path is prescribed geometry, not homogeneous strain. The same accepted Maxwell or
//! permanent-strain memory survives loading, holds and unloading. Only reference sheet mass and
//! membrane observations are consumed; bending/contact/thermal constants are not fabricated.

use ftx_core::{Refusal, RefusalCode, Vec2, Vec3, receipt::sha256_hex};
use ftx_pattern::{
    material_calibration::fitted_domain, material_catalog::Selection, materials::Form,
    textile_import,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Fixture, experiment::invalid};
use crate::{ClothModel, Membrane, SolveReport, Solver, grid};

/// An acquisition boundary; extension varies linearly in time between successive boundaries.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Loading {
    /// Elapsed seconds from the pristine, stationary reference state.
    pub time_s: f64,
    /// Change in grip separation divided by original gauge length; zero to two.
    pub extension_fraction: f64,
}

/// Explicit time and nonlinear work limits; they are never enlarged after a refusal.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Controls {
    /// Maximum backward-Euler step, s; each loading interval is divided into equal steps.
    pub max_step_s: f64,
    /// Maximum number of accepted dynamic steps across the entire history, 1..=1024.
    pub max_steps: usize,
    /// Newton correction tolerance divided by time step, m/s, as in the production solver.
    pub velocity_tolerance_m_s: f64,
    /// Independent free-coordinate momentum residual tolerance, N.
    pub force_tolerance_n: f64,
    /// Newton iteration budget per step, 1..=300.
    pub max_newton: usize,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            max_step_s: 0.01,
            max_steps: 512,
            velocity_tolerance_m_s: 1e-9,
            force_tolerance_n: 1e-6,
            max_newton: 100,
        }
    }
}

/// Source-locked timed experiment, with no static-solve approximation of material history.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Exactly `ftx-strip-history/1`.
    pub schema: String,
    /// Canonical material selection and its independent evidence-admission policy.
    pub material: Selection,
    /// Same ideal, fully gripped geometry and material axes as the elastic strip experiment.
    pub fixture: Fixture,
    /// Two to 65 strictly increasing acquisition times; the first point must be `(0, 0)`.
    pub loading: Vec<Loading>,
    /// Bounded numerical controls, frozen with the rest of the request.
    #[serde(default)]
    pub controls: Controls,
}

/// One accepted step (or the pristine time-zero state). All energies are in joules.
#[derive(Clone, Debug, Serialize)]
pub struct State {
    /// Time since the pristine state, s.
    pub time_s: f64,
    /// Duration of this accepted step; zero for the initial state.
    pub time_step_s: f64,
    /// Actual prescribed grip extension at this time.
    pub extension_fraction: f64,
    /// Membrane reactions at fixed/moving grips, [x,y,z], N; excludes prescribed-node/hardware inertia.
    pub grip_reaction_n: [[f64; 3]; 2],
    /// Moving-grip axial membrane reaction, N (positive in tension).
    pub force_n: f64,
    /// Fixed-grip axial membrane reaction, N.
    pub fixed_grip_force_n: f64,
    /// Largest free in-plane `mass * acceleration - internal_force` component, N.
    pub free_dynamic_residual_n: f64,
    /// Norm of summed membrane grip reactions minus total free-node inertial force, N.
    pub momentum_balance_residual_n: f64,
    /// Mid-gauge chord width between material side points, m.
    pub mid_width_m: f64,
    /// Stored equilibrium, branch and hardening energy; excludes irreversible/numerical loss.
    pub stored_membrane_energy_j: f64,
    /// Kinetic energy of the free interior sheet nodes; prescribed grip inertia is excluded.
    pub free_kinetic_energy_j: f64,
    /// Cumulative accepted physical dashpot work; zero when no Maxwell law is selected.
    pub maxwell_dissipation_j: f64,
    /// Cumulative irreversible permanent-strain work; zero when no plastic law is selected.
    pub plastic_dissipation_j: f64,
    /// Actual elementwise raw [E11,E22,2E12] bounds, not grip-extension conversions.
    pub green_strain_range: [[f64; 2]; 3],
    /// Current planar positions in fixture coordinates, m.
    pub positions_m: Vec<[f64; 3]>,
    /// Accepted interior velocities plus imposed endpoint grip velocities, m/s.
    pub velocities_m_s: Vec<[f64; 3]>,
    /// Ordinary production Maxwell history, including law identities and per-element memory.
    pub relaxation: Option<Value>,
    /// Ordinary production permanent-strain history, including accepted dissipation.
    pub plasticity: Option<Value>,
    /// Production Newton diagnostics; absent only at pristine time zero.
    pub solve: Option<SolveReport>,
}

/// Complete replayable report; a failure at any time publishes no successful partial result.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// Exactly `ftx-strip-history-report/1`.
    pub schema: String,
    /// Exact request with its material frozen for offline replay.
    pub input: Request,
    /// SHA-256 of the serialized frozen input.
    pub input_sha256: String,
    /// Exact Rust-serialized frozen request, preserving numeric/source identity in browser downloads.
    pub replay_request_json: String,
    /// Undeformed fixture coordinates in metres, with no material-frame rotation applied.
    pub reference_positions_m: Vec<[f64; 3]>,
    /// Production triangles in deterministic row-major order.
    pub triangles: Vec<[usize; 3]>,
    /// Sourced reference areal density times gauge area, including constrained edge shares, kg.
    pub reference_mass_kg: f64,
    /// Initial state followed by every accepted integration step, without decimation.
    pub states: Vec<State>,
    /// Total Newton iterations across every accepted step.
    pub newton_iterations: usize,
    /// The physical and numerical scope of this prescribed planar experiment.
    pub model_scope: String,
}

const SCOPE: &str = "Planar membrane dynamics with sourced reference mass, ideal full-width no-slip grips and prescribed piecewise-linear separation; backward-Euler integration preserves the selected Maxwell or permanent-strain history. Compression during unloading is constrained planar behavior, not wrinkling. Excludes gravity, bending, out-of-plane buckling, grip slip/compliance, thermal feedback, joint Maxwell/plastic laws, parameter identification, experimental qualification and garment fit. Material dissipation excludes backward-Euler numerical loss; no external-work or total-energy balance is inferred from endpoint reactions. Refine time step and mesh independently.";

fn vector(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

fn budget(message: impl std::fmt::Display) -> Refusal {
    Refusal::new(RefusalCode::BudgetExceeded, message.to_string())
}

fn subdivisions(input: &Request) -> Result<Vec<usize>, Refusal> {
    let c = &input.controls;
    input.fixture.validate()?;
    if input.schema != "ftx-strip-history/1"
        || !(2..=65).contains(&input.loading.len())
        || !c.max_step_s.is_finite()
        || c.max_step_s <= 0.0
        || !(1..=1024).contains(&c.max_steps)
        || !c.velocity_tolerance_m_s.is_finite()
        || c.velocity_tolerance_m_s <= 0.0
        || !c.force_tolerance_n.is_finite()
        || c.force_tolerance_n <= 0.0
        || !(1..=300).contains(&c.max_newton)
    {
        return Err(invalid(
            "invalid strip-history schema, loading count or numerical controls",
        ));
    }
    if input.loading[0].time_s != 0.0 || input.loading[0].extension_fraction != 0.0 {
        return Err(invalid(
            "strip history must start at pristine time 0, extension 0",
        ));
    }
    for node in &input.loading {
        if !node.time_s.is_finite()
            || !node.extension_fraction.is_finite()
            || !(0.0..=2.0).contains(&node.extension_fraction)
        {
            return Err(invalid(
                "loading times must be finite and extensions in 0..2",
            ));
        }
    }
    let mut total = 0usize;
    let mut counts = Vec::new();
    for pair in input.loading.windows(2) {
        let dt = pair[1].time_s - pair[0].time_s;
        if !dt.is_finite() || dt <= 0.0 {
            return Err(invalid("strip loading times must increase strictly"));
        }
        let count = (dt / c.max_step_s).ceil().max(1.0);
        if !count.is_finite() || count > c.max_steps.saturating_sub(total) as f64 {
            return Err(budget(
                "strip history exceeds its complete-path step budget",
            ));
        }
        let count = count as usize;
        total += count;
        counts.push(count);
    }
    let [nx, ny] = input.fixture.cells;
    if (nx + 1) * (ny + 1) * (total + 1) > 262_144 {
        return Err(budget(
            "strip history exceeds 262144 retained vertex states; reduce mesh or path length",
        ));
    }
    Ok(counts)
}

struct Recorder<'a> {
    fixture: &'a Fixture,
    controls: &'a Controls,
}

impl Recorder<'_> {
    fn state(
        &self,
        solver: &Solver,
        node: &Loading,
        h: f64,
        old_velocity: &[Vec3],
        solve: Option<SolveReport>,
    ) -> Result<State, Refusal> {
        let [nx, ny] = self.fixture.cells;
        let forces = solver.checked_elastic_forces()?;
        let mut grips = [Vec3::ZERO; 2];
        let mut inertia = Vec3::ZERO;
        let mut residual = 0.0_f64;
        let mut kinetic = 0.0;
        for (i, &force) in forces.iter().enumerate() {
            match i / (nx + 1) {
                0 => grips[0] -= force,
                row if row == ny => grips[1] -= force,
                _ => {
                    let acceleration = if h > 0.0 {
                        (solver.v[i] - old_velocity[i]) / h
                    } else {
                        Vec3::ZERO
                    };
                    let local = acceleration * solver.model.mass[i];
                    inertia += local;
                    let r = local - force;
                    residual = residual.max(r.x.abs()).max(r.y.abs());
                    kinetic += 0.5 * solver.model.mass[i] * solver.v[i].norm2();
                }
            }
        }
        let balance = (grips[0] + grips[1] - inertia).norm();
        if !residual.is_finite()
            || !balance.is_finite()
            || residual > self.controls.force_tolerance_n
            || balance > self.controls.force_tolerance_n * solver.x.len() as f64
        {
            return Err(Refusal::new(
                RefusalCode::NotConverged,
                format!(
                    "strip dynamic residual {residual} N / summed momentum residual {balance} N exceeds declared tolerance"
                ),
            ));
        }
        let mut ranges = [[f64::INFINITY, f64::NEG_INFINITY]; 3];
        for element in &solver.model.membranes {
            let [a, b, c] = element.v.map(|i| solver.x[i]);
            if (b - a).cross(c - a).z <= 0.0 {
                return Err(invalid(
                    "strip history developed an inverted or degenerate planar element",
                ));
            }
            for (range, value) in ranges.iter_mut().zip(element.strain(&solver.x)) {
                range[0] = range[0].min(value);
                range[1] = range[1].max(value);
            }
        }
        let maxwell = solver
            .membrane_relaxation_dissipation_j()?
            .map_or(0.0, |v| v.iter().sum());
        let plastic = solver
            .membrane_plastic_dissipation_j()?
            .map_or(0.0, |v| v.iter().sum());
        let stored = solver.membrane_energy()?;
        if [kinetic, maxwell, plastic, stored]
            .iter()
            .any(|v| !v.is_finite())
        {
            return Err(invalid("strip history energy is not representable"));
        }
        let left = (ny / 2) * (nx + 1);
        let width = (solver.x[left + nx] - solver.x[left]).norm();
        if !width.is_finite() || solver.v.iter().any(|v| !v.is_finite()) {
            return Err(invalid("strip width or velocity is not representable"));
        }
        Ok(State {
            time_s: node.time_s,
            time_step_s: h,
            extension_fraction: node.extension_fraction,
            grip_reaction_n: grips.map(vector),
            force_n: grips[1].y,
            fixed_grip_force_n: grips[0].y,
            free_dynamic_residual_n: residual,
            momentum_balance_residual_n: balance,
            mid_width_m: width,
            stored_membrane_energy_j: stored,
            free_kinetic_energy_j: kinetic,
            maxwell_dissipation_j: maxwell,
            plastic_dissipation_j: plastic,
            green_strain_range: ranges,
            positions_m: solver.x.iter().copied().map(vector).collect(),
            velocities_m_s: solver.v.iter().copied().map(vector).collect(),
            relaxation: solver.relaxation_report()?,
            plasticity: solver.membrane_plasticity_report()?,
            solve,
        })
    }
}

/// Run the entire timed acquisition with the actual source-qualified material and solver.
/// A material-only change changes mechanics/history/inertia without changing the fixture or code.
///
/// # Errors
/// Invalid schedule, missing mass/membrane evidence, unsupported joint laws, exhausted work budget,
/// failed Newton/momentum/domain checks or any late material-history failure. No partial success.
pub fn evaluate(input: &Request) -> Result<Report, Refusal> {
    let counts = subdivisions(input)?;
    let resolved = textile_import::catalog()?.resolve(&input.material)?;
    let material = resolved.material();
    if matches!(material.form, Form::Fiber | Form::Yarn | Form::Fill) {
        return Err(invalid(
            "timed strip experiments require a characterized sheet",
        ));
    }
    let mass = material.require("areal_density", "kg/m2", resolved.allows_estimates())?;
    let response = resolved.membrane_response()?;
    let [warp, weft, poisson, shear] = if let Some(law) = &response {
        law.reference_moduli()?
    } else {
        [
            material.require("warp_stiffness", "N/m", resolved.allows_estimates())?,
            material.require("weft_stiffness", "N/m", resolved.allows_estimates())?,
            material.require("poisson_warp_weft", "1", resolved.allows_estimates())?,
            material.require("shear_stiffness", "N/m", resolved.allows_estimates())?,
        ]
    };
    let membrane = Membrane::orthotropic(warp, weft, poisson, shear, 1.0);
    membrane.validate()?;
    let relaxation = resolved.membrane_relaxation()?;
    let plasticity = resolved.membrane_plasticity()?;
    let [nx, ny] = input.fixture.cells;
    let history_scalars = material.membrane_relaxation.as_ref().map_or(0, |law| {
        4 + 3 * (law.branches.len() + law.orthotropic_branches.len())
    }) + if plasticity.is_some() { 18 } else { 0 };
    let states = counts.iter().sum::<usize>() + 1;
    let retained_scalars = states * (6 * (nx + 1) * (ny + 1) + 2 * nx * ny * history_scalars);
    if retained_scalars > 2_097_152 {
        return Err(budget(
            "strip history exceeds 2097152 retained geometry/history values; reduce mesh or path length",
        ));
    }
    let (rest, triangles) = grid(
        input.fixture.width_m,
        input.fixture.gauge_length_m,
        nx,
        ny,
        Vec2::ZERO,
    );
    let [u, v] = input.fixture.warp_axis;
    let material_rest = rest
        .iter()
        .map(|p| Vec2::new(u * p.x + v * p.y, -v * p.x + u * p.y))
        .collect();
    let mut model = ClothModel::new(material_rest, &triangles, membrane, 0.0, mass)?;
    if model.mass.iter().any(|m| !m.is_finite() || *m <= 0.0)
        || !model.mass.iter().sum::<f64>().is_finite()
    {
        return Err(invalid("strip reference mass is not representable"));
    }
    if let Some(law) = response {
        model.set_membrane_responses(&vec![Some(law); triangles.len()])?;
    }
    if let Some(domain) = fitted_domain(material)? {
        model.set_membrane_domains(&vec![Some(domain); triangles.len()])?;
    }
    let reference: Vec<_> = rest.iter().map(|p| Vec3::new(p.x, p.y, 0.0)).collect();
    let fixed: Vec<_> = (0..reference.len())
        .map(|i| {
            let grip = i / (nx + 1) == 0 || i / (nx + 1) == ny;
            [grip, grip, true]
        })
        .collect();
    let mut solver = Solver::new(model, reference.clone(), &fixed, Vec3::ZERO)
        .with_relaxation(&vec![relaxation; triangles.len()])?
        .with_membrane_plasticity(&vec![plasticity; triangles.len()])?;
    solver.tol = input.controls.velocity_tolerance_m_s;
    solver.max_newton = input.controls.max_newton;
    let recorder = Recorder {
        fixture: &input.fixture,
        controls: &input.controls,
    };
    let mut states = vec![recorder.state(&solver, &input.loading[0], 0.0, &solver.v, None)?];
    let mut iterations = 0usize;
    let mut current_time = 0.0;
    for (pair, count) in input.loading.windows(2).zip(counts) {
        let dt = pair[1].time_s - pair[0].time_s;
        let slope = (pair[1].extension_fraction - pair[0].extension_fraction) / dt;
        let grip_velocity = input.fixture.gauge_length_m * slope;
        if !slope.is_finite() || !grip_velocity.is_finite() {
            return Err(invalid("prescribed grip velocity is not representable"));
        }
        for step in 1..=count {
            let fraction = step as f64 / count as f64;
            let node = if step == count {
                pair[1].clone()
            } else {
                Loading {
                    time_s: pair[0].time_s + dt * fraction,
                    extension_fraction: pair[0].extension_fraction
                        + (pair[1].extension_fraction - pair[0].extension_fraction) * fraction,
                }
            };
            let h = node.time_s - current_time;
            if !h.is_finite() || h <= 0.0 {
                return Err(invalid("strip-history time step is not representable"));
            }
            let old_velocity = solver.v.clone();
            // Only prescribed boundary coordinates move before the step; free positions retain
            // their accepted values and the existing implicit prediction. The solver's own
            // transactional step preserves history on a refusal; this whole request is atomic.
            for position in &mut solver.x[ny * (nx + 1)..] {
                position.y = input.fixture.gauge_length_m * (1.0 + node.extension_fraction);
            }
            let numerical = solver.step(h)?;
            // Fixed-coordinate velocities do not enter the solve. Retain the actual imposed
            // velocity for inspection, rather than the solver's fixed-coordinate zero increment.
            for velocity in &mut solver.v[ny * (nx + 1)..] {
                velocity.y = grip_velocity;
            }
            iterations += numerical.iterations;
            states.push(recorder.state(&solver, &node, h, &old_velocity, Some(numerical))?);
            current_time = node.time_s;
        }
    }
    let mut frozen = input.clone();
    frozen.material = resolved.frozen_selection();
    let replay_request_json = serde_json::to_string(&frozen).map_err(invalid)?;
    let digest = sha256_hex(replay_request_json.as_bytes());
    Ok(Report {
        schema: "ftx-strip-history-report/1".into(),
        input: frozen,
        input_sha256: digest,
        replay_request_json,
        reference_positions_m: reference.into_iter().map(vector).collect(),
        triangles,
        reference_mass_kg: solver.model.mass.iter().sum(),
        states,
        newton_iterations: iterations,
        model_scope: SCOPE.into(),
    })
}

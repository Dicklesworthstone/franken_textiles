//! Displacement-controlled tensile strips using the production cloth membrane and Newton solver.
//!
//! The two full-width ends are ideal no-slip grips. Interior in-plane displacement is free; all
//! out-of-plane displacement is constrained. This is an explicitly planar, quasistatic, elastic
//! experiment model, not a homogeneous-strain conversion of grip force. It does not identify
//! bending, wrinkling, grip slip, rate dependence or hysteresis.

use ftx_core::{Refusal, RefusalCode, Vec2, Vec3, receipt::sha256_hex};
use ftx_pattern::{
    material_calibration::fitted_domain,
    material_catalog::Selection,
    material_response::{CompiledMembrane, StrainDomain},
    materials::Form,
    textile_import,
};
use serde::{Deserialize, Serialize};

use crate::{ClothModel, Membrane, Solver, grid};

/// Geometry of the free strip between the inner grip faces, in metres.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fixture {
    /// Initial distance between the grips, excluding material inside the clamps.
    pub gauge_length_m: f64,
    /// Undeformed strip width, fully fixed in each grip.
    pub width_m: f64,
    /// Unit warp/wale axis in strip coordinates (x across width, y along the loading direction).
    /// Warp loading is [0,1], weft loading is [1,0]. No angle convention is inferred.
    pub warp_axis: [f64; 2],
    /// Even cell counts [across width, along length]; refinement doubles each count.
    pub cells: [usize; 2],
}

/// Explicit numerical controls. Force equilibrium is checked independently of Newton step size.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Controls {
    /// Newton displacement tolerance, metres.
    pub tolerance_m: f64,
    /// Maximum absolute free-vertex reaction and summed grip-force imbalance, newtons.
    pub force_tolerance_n: f64,
    /// Newton iterations allowed for each independent strip equilibrium.
    pub max_newton: usize,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            tolerance_m: 1e-11,
            force_tolerance_n: 1e-7,
            max_newton: 100,
        }
    }
}

/// One converged equilibrium, including nonuniform local strains and actual grip reactions.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct State {
    /// Change in grip separation divided by the original gauge length.
    pub extension_fraction: f64,
    /// Tensile reaction at the moving grip (positive in tension), N.
    pub force_n: f64,
    /// Signed axial reaction at the fixed grip, N.
    pub fixed_grip_force_n: f64,
    /// Largest free in-plane force component, N.
    pub free_force_residual_n: f64,
    /// Norm of the sum of the two full grip reaction vectors, N.
    pub grip_balance_residual_n: f64,
    /// Distance between the two side-edge material points at half the original gauge length, m.
    pub mid_width_m: f64,
    /// Membrane strain energy, J; gravity, inertia, bending and contact are absent.
    pub energy_j: f64,
    /// Observed elementwise [E_warp,E_weft,2E_warp_weft] bounds, not grip strains.
    pub green_strain_range: [[f64; 2]; 3],
    /// Actual physical element area-ratio bounds when the selected law has an area response.
    /// Omitted for legacy separable laws; this is not a box-corner estimate from strain extrema.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area_ratio_range: Option<[f64; 2]>,
    /// Accepted positions in strip coordinates, metres, row-major mesh order.
    pub positions_m: Vec<[f64; 3]>,
    /// Number of Newton iterations used by the actual production solver.
    pub newton_iterations: usize,
}

/// Source-locked forward experiment; only membrane properties are required.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// `ftx-strip-experiment/1`.
    pub schema: String,
    /// Existing catalog/embedded material authority and ordinary estimate policy.
    pub material: Selection,
    /// Declared ideal fixture geometry and discretization.
    pub fixture: Fixture,
    /// Independent nonnegative tensile extension states; at most 64.
    pub extensions: Vec<f64>,
    /// Explicit solver controls.
    #[serde(default)]
    pub controls: Controls,
}

/// Replayable results; source resolution never requires unrelated contact or thermal properties.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    /// `ftx-strip-experiment-report/1`.
    pub schema: String,
    /// Same request with its material frozen for offline replay.
    pub input: Request,
    /// Hash of the exact serialized frozen input.
    pub input_sha256: String,
    /// Independent accepted equilibria in request order.
    pub states: Vec<State>,
    /// Explicit assumptions and scope of the experiment model.
    pub model_scope: String,
}

pub(super) const SCOPE: &str = "Planar quasistatic elastic membrane; ideal full-width no-slip grips; free interior in-plane contraction. No out-of-plane buckling, bending, grip slip, rate dependence, hysteresis, statistical confidence or empirical garment validation.";

pub(super) fn invalid(message: impl std::fmt::Display) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message.to_string())
}

impl Fixture {
    pub(super) fn validate(&self) -> Result<(), Refusal> {
        let [nx, ny] = self.cells;
        let [u, v] = self.warp_axis;
        if !self.gauge_length_m.is_finite()
            || !self.width_m.is_finite()
            || self.gauge_length_m <= 0.0
            || self.width_m <= 0.0
            || !(2..=32).contains(&nx)
            || !(2..=32).contains(&ny)
            || nx % 2 != 0
            || ny % 2 != 0
            || nx * ny > 512
            || !u.is_finite()
            || !v.is_finite()
            || (u * u + v * v - 1.0).abs() > 1e-12
        {
            return Err(invalid(
                "strip needs positive finite dimensions, a unit warp axis and even 2..32 cell counts with at most 512 cells",
            ));
        }
        Ok(())
    }
}

impl Controls {
    pub(super) fn validate(&self) -> Result<(), Refusal> {
        if !self.tolerance_m.is_finite()
            || self.tolerance_m <= 0.0
            || !self.force_tolerance_n.is_finite()
            || self.force_tolerance_n <= 0.0
            || !(1..=300).contains(&self.max_newton)
        {
            return Err(invalid(
                "strip solver tolerances must be finite and positive; Newton budget must be 1..300",
            ));
        }
        Ok(())
    }
}

/// Solve the actual clamped geometry with the solver's existing orthotropic scalar law.
/// Parameters are [warp N/m, weft N/m, major Poisson ratio, shear N/m]. No material is fabricated.
///
/// # Errors
/// Invalid geometry/coefficients, nonconvergence, force imbalance or a flipped planar element.
pub fn solve(
    fixture: &Fixture,
    parameters: [f64; 4],
    extension: f64,
    controls: &Controls,
) -> Result<State, Refusal> {
    let [e1, e2, nu, shear] = parameters;
    let membrane = Membrane::orthotropic(e1, e2, nu, shear, 1.0);
    solve_law(fixture, membrane, None, None, extension, controls)
}

pub(super) fn solve_law(
    fixture: &Fixture,
    membrane: Membrane,
    response: Option<&CompiledMembrane>,
    domain: Option<StrainDomain>,
    extension: f64,
    controls: &Controls,
) -> Result<State, Refusal> {
    fixture.validate()?;
    controls.validate()?;
    membrane.validate()?;
    if !extension.is_finite() || !(0.0..=2.0).contains(&extension) {
        return Err(invalid(
            "strip extension must be finite and in 0..2; compression requires a buckling-capable experiment model",
        ));
    }
    let [nx, ny] = fixture.cells;
    let (strip_rest, tris) = grid(fixture.width_m, fixture.gauge_length_m, nx, ny, Vec2::ZERO);
    let [u, v] = fixture.warp_axis;
    let rest = strip_rest
        .iter()
        .map(|p| Vec2::new(u * p.x + v * p.y, -v * p.x + u * p.y))
        .collect();
    // Auxiliary positive mass satisfies ClothModel's storage invariant. It is NEVER consumed:
    // static solve, zero gravity and zero bending; no density observation is inferred or published.
    let mut model = ClothModel::new(rest, &tris, membrane, 0.0, 1.0)?;
    if let Some(response) = response {
        model.set_membrane_responses(&vec![Some(response.clone()); tris.len()])?;
    }
    if let Some(domain) = domain {
        model.set_membrane_domains(&vec![Some(domain); tris.len()])?;
    }
    let positions: Vec<_> = strip_rest
        .iter()
        .map(|p| Vec3::new(p.x, p.y * (1.0 + extension), 0.0))
        .collect();
    let fixed: Vec<_> = (0..positions.len())
        .map(|i| {
            let grip = i / (nx + 1) == 0 || i / (nx + 1) == ny;
            [grip, grip, true]
        })
        .collect();
    let mut solver = Solver::new(model, positions, &fixed, Vec3::ZERO);
    solver.tol = controls.tolerance_m;
    solver.max_newton = controls.max_newton;
    let numerical = solver.static_solve()?;
    let forces = solver.checked_elastic_forces()?;
    let mut lower = Vec3::ZERO;
    let mut upper = Vec3::ZERO;
    let mut residual = 0.0_f64;
    for (i, force) in forces.iter().enumerate() {
        match i / (nx + 1) {
            0 => lower -= *force,
            j if j == ny => upper -= *force,
            _ => residual = residual.max(force.x.abs()).max(force.y.abs()),
        }
    }
    let balance = (lower + upper).norm();
    if residual > controls.force_tolerance_n || balance > controls.force_tolerance_n {
        return Err(Refusal::new(
            RefusalCode::NotConverged,
            format!(
                "strip equilibrium residual {residual} N / grip imbalance {balance} N exceeds {} N",
                controls.force_tolerance_n
            ),
        ));
    }
    let mut ranges = [[f64::INFINITY, f64::NEG_INFINITY]; 3];
    let mut area_range = response
        .and_then(CompiledMembrane::area_ratio_range)
        .map(|_| [f64::INFINITY, f64::NEG_INFINITY]);
    for element in &solver.model.membranes {
        let [a, b, c] = element.v.map(|i| solver.x[i]);
        if (b - a).cross(c - a).z <= 0.0 {
            return Err(invalid(
                "clamped strip developed an inverted or degenerate planar element",
            ));
        }
        let strain = element.strain(&solver.x);
        if let Some(range) = &mut area_range {
            let value = ftx_pattern::material_response::physical_area_ratio(strain)
                .ok_or_else(|| invalid("strip developed an invalid physical area metric"))?;
            range[0] = range[0].min(value);
            range[1] = range[1].max(value);
        }
        for (range, value) in ranges.iter_mut().zip(strain) {
            range[0] = range[0].min(value);
            range[1] = range[1].max(value);
        }
    }
    let mid = ny / 2 * (nx + 1);
    let width = (solver.x[mid + nx] - solver.x[mid]).norm();
    let energy = solver.model.elastic_energy(&solver.x);
    if !width.is_finite() || !energy.is_finite() || !upper.y.is_finite() {
        return Err(invalid("nonfinite strip reaction, width or energy"));
    }
    Ok(State {
        extension_fraction: extension,
        force_n: upper.y,
        fixed_grip_force_n: lower.y,
        free_force_residual_n: residual,
        grip_balance_residual_n: balance,
        mid_width_m: width,
        energy_j: energy,
        green_strain_range: ranges,
        area_ratio_range: area_range,
        positions_m: solver.x.iter().map(|p| [p.x, p.y, p.z]).collect(),
        newton_iterations: numerical.iterations,
    })
}

/// Evaluate a frozen source-bearing membrane through ideal clamped-strip equilibria.
///
/// # Errors
/// Missing/admission-refused membrane data, unsupported history, domain escape or failed equilibrium.
pub fn evaluate(input: &Request) -> Result<Report, Refusal> {
    if input.schema != "ftx-strip-experiment/1"
        || input.extensions.is_empty()
        || input.extensions.len() > 64
    {
        return Err(invalid(
            "strip experiment schema or extension count (1..64) is invalid",
        ));
    }
    let resolved = textile_import::catalog()?.resolve(&input.material)?;
    let material = resolved.material();
    if matches!(material.form, Form::Fiber | Form::Yarn | Form::Fill) {
        return Err(invalid(
            "clamped strip experiments require a sheet material; fiber, yarn and loose fill cannot supply a sheet law",
        ));
    }
    if material.membrane_relaxation.is_some()
        || material.structure.contains_key("membrane_plasticity_model")
    {
        return Err(invalid(
            "quasistatic elastic strip experiments cannot choose a duration or erase irreversible material history",
        ));
    }
    let response = resolved.membrane_response()?;
    let parameters = if let Some(law) = &response {
        law.reference_moduli()?
    } else {
        [
            material.require("warp_stiffness", "N/m", resolved.allows_estimates())?,
            material.require("weft_stiffness", "N/m", resolved.allows_estimates())?,
            material.require("poisson_warp_weft", "1", resolved.allows_estimates())?,
            material.require("shear_stiffness", "N/m", resolved.allows_estimates())?,
        ]
    };
    let membrane = Membrane::orthotropic(
        parameters[0],
        parameters[1],
        parameters[2],
        parameters[3],
        1.0,
    );
    let domain = fitted_domain(material)?;
    let states = input
        .extensions
        .iter()
        .map(|&extension| {
            solve_law(
                &input.fixture,
                membrane,
                response.as_ref(),
                domain,
                extension,
                &input.controls,
            )
        })
        .collect::<Result<_, _>>()?;
    let mut frozen = input.clone();
    frozen.material = resolved.frozen_selection();
    let digest = sha256_hex(&serde_json::to_vec(&frozen).map_err(invalid)?);
    Ok(Report {
        schema: "ftx-strip-experiment-report/1".into(),
        input: frozen,
        input_sha256: digest,
        states,
        model_scope: SCOPE.into(),
    })
}

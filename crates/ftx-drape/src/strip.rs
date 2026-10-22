//! Planar, fully gripped tensile coupons using the production membrane and static solver.
//!
//! Grip displacement is a boundary condition, not a homogeneous material strain. All interior
//! in-plane coordinates relax; both end edges retain their transverse coordinates. This is a
//! membrane-only experiment: no gravity, grip slip, wrinkling, bending or material history.

use ftx_core::{Refusal, RefusalCode, Vec2, Vec3, det};
use ftx_pattern::{material_catalog::ResolvedMaterial, materials::Form};
use serde::{Deserialize, Serialize};

use crate::{ClothModel, Membrane, SolveReport, Solver, grid};

mod experiment;
/// Bounded identification from independent source-bearing clamped-strip experiments.
pub mod fit;
/// Explicit material-following virtual extensometers for declared strip observations.
pub mod measurement;
/// Independent-equilibrium experiment report; `Report` remains the continuation-path report.
pub use experiment::Report as ExperimentReport;
pub use experiment::{Controls, Fixture, Request, State, evaluate, solve};

/// Undeformed free gauge; grip lengths outside the gauge are not simulated.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Geometry {
    /// Distance between the inner grip edges, metres.
    pub gauge_m: f64,
    /// Free-gauge width, metres.
    pub width_m: f64,
    /// Warp/wale angle from the positive loading axis, in degrees, in [-180, 180].
    pub warp_angle_deg: f64,
    /// Cells along the gauge, 2..=64.
    pub length_cells: usize,
    /// Cells across the width, 2..=64; at most 1024 vertices overall.
    pub width_cells: usize,
}

/// Bounded work and independent equilibrium admission; smaller values are not silently relaxed.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Options {
    /// Newton correction tolerance in metres, distinct from the force acceptance test.
    pub correction_tolerance_m: f64,
    /// Maximum absolute free-coordinate force component, N, after each accepted solve.
    pub force_tolerance_n: f64,
    /// Newton iterations per station, 1..=200.
    pub max_newton: usize,
    /// Sum of Newton iterations over the complete path, 1..=10000.
    pub max_total_newton: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            correction_tolerance_m: 1e-10,
            force_tolerance_n: 1e-7,
            max_newton: 100,
            max_total_newton: 2000,
        }
    }
}

/// One converged equilibrium, retaining clamp reactions and actual nonuniform material strains.
#[derive(Clone, Debug, Serialize)]
pub struct Sample {
    /// Imposed engineering extension (current grip separation / free gauge minus one).
    pub extension_ratio: f64,
    /// External reactions required at the fixed and moving grips, in N, [axial, transverse, z].
    pub grip_reaction_n: [Vec3; 2],
    /// Axial reaction at the moving grip, N. This is not second-Piola stress.
    pub axial_force_n: f64,
    /// Largest absolute free-coordinate force component, recomputed from physical forces.
    pub free_residual_n: f64,
    /// Sum of both external grip reactions; its discrepancy is bounded by free-force tolerance.
    pub reaction_balance_n: Vec3,
    /// Reference-area-integrated membrane energy, J.
    pub energy_j: f64,
    /// Per-axis minimum and maximum raw Green strains [E11, E22, 2 E12] across all triangles.
    pub strain_range: [[f64; 2]; 3],
    /// Equilibrium positions in the coupon frame, metres. The loading direction is +x.
    pub positions_m: Vec<Vec3>,
    /// Diagnostics from the existing static Newton solver.
    pub solve: SolveReport,
}

/// A complete path; a late failure returns a refusal, never a successful partial response.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// `ftx-clamped-strip/1`.
    pub schema: &'static str,
    /// Exact normalized source/policy identity used by the production material resolver.
    pub material_sha256: String,
    /// Portable original material and admission policy, not a fitted material.
    pub material: ftx_pattern::material_catalog::Selection,
    /// Authored finite-gauge boundary conditions.
    pub geometry: Geometry,
    /// Actual numerical policy.
    pub options: Options,
    /// Undeformed positions in the coupon frame, metres.
    pub reference_positions_m: Vec<Vec3>,
    /// Production constant-strain triangles in deterministic row order.
    pub triangles: Vec<[usize; 3]>,
    /// Samples in input order; unloading is memoryless and does not identify hysteresis.
    pub samples: Vec<Sample>,
    /// All accepted Newton iterations, across the entire path.
    pub newton_iterations: usize,
    /// Scope of the result, separate from measured coupon or garment agreement.
    pub no_claim: &'static str,
}

fn invalid(message: &str) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

impl Geometry {
    fn validate(&self) -> Result<(), Refusal> {
        if !self.gauge_m.is_finite()
            || !self.width_m.is_finite()
            || !(1e-5..=10.0).contains(&self.gauge_m)
            || !(1e-5..=10.0).contains(&self.width_m)
            || !self.warp_angle_deg.is_finite()
            || !(-180.0..=180.0).contains(&self.warp_angle_deg)
            || !(2..=64).contains(&self.length_cells)
            || !(2..=64).contains(&self.width_cells)
            || (self.length_cells + 1) * (self.width_cells + 1) > 1024
        {
            return Err(invalid(
                "strip needs a finite 1e-5..10 m gauge/width, angle in [-180,180], 2..64 cells per axis and at most 1024 vertices",
            ));
        }
        Ok(())
    }
}

impl Options {
    fn validate(&self) -> Result<(), Refusal> {
        if !self.correction_tolerance_m.is_finite()
            || !(1e-14..=1e-4).contains(&self.correction_tolerance_m)
            || !self.force_tolerance_n.is_finite()
            || !(1e-12..=1.0).contains(&self.force_tolerance_n)
            || !(1..=200).contains(&self.max_newton)
            || !(1..=10000).contains(&self.max_total_newton)
        {
            return Err(invalid(
                "invalid strip correction/force tolerance or Newton work limit",
            ));
        }
        Ok(())
    }
}

/// Predict the full edge-clamped strip response with free transverse contraction in its interior.
/// Only the selected memoryless membrane law is required; missing mass/bending/contact properties
/// are not guessed. A unit lumped mass is internal to the static zero-gravity assembly and has no
/// role in the objective, reaction or output. No dynamic or out-of-plane state is published.
///
/// # Errors
/// Unsupported history/physical form, invalid source policy or geometry, 0..1 extension violation,
/// work exhaustion, unconverged equilibrium, inverted elements or out-of-domain physical strains.
pub fn simulate(
    material: &ResolvedMaterial,
    geometry: &Geometry,
    extension_ratio: &[f64],
    options: &Options,
) -> Result<Report, Refusal> {
    geometry.validate()?;
    options.validate()?;
    if extension_ratio.is_empty()
        || extension_ratio.len() > 128
        || extension_ratio
            .iter()
            .any(|e| !e.is_finite() || !(0.0..=1.0).contains(e))
    {
        return Err(invalid(
            "strip requires 1..128 explicit nonnegative extensions no greater than one",
        ));
    }
    let m = material.material();
    if matches!(m.form, Form::Fiber | Form::Yarn | Form::Fill)
        || m.membrane_relaxation.is_some()
        || m.structure.contains_key("membrane_plasticity_model")
        || m.structure.contains_key("bending_plasticity_model")
    {
        return Err(invalid(
            "planar static strip requires a memoryless sheet membrane; it cannot discard a declared history law",
        ));
    }
    let response = material.membrane_response()?;
    let membrane = if let Some(law) = &response {
        let [a, b, c, shear] = law.origin_stiffness();
        Membrane {
            q: [[a, c, 0.0], [c, b, 0.0], [0.0, 0.0, shear]],
            compression: [1.0; 2],
            transition: 1e-3,
        }
    } else {
        let get = |name, unit| m.require(name, unit, material.allows_estimates());
        Membrane::orthotropic(
            get("warp_stiffness", "N/m")?,
            get("weft_stiffness", "N/m")?,
            get("poisson_warp_weft", "1")?,
            get("shear_stiffness", "N/m")?,
            1.0,
        )
    };
    membrane.validate()?;
    let (reference, triangles) = grid(
        geometry.gauge_m,
        geometry.width_m,
        geometry.length_cells,
        geometry.width_cells,
        Vec2::new(0.0, -0.5 * geometry.width_m),
    );
    let (sin, cos) = det::sin_cos(geometry.warp_angle_deg * (std::f64::consts::PI / 180.0));
    let rest = reference
        .iter()
        .map(|p| Vec2::new(cos * p.x + sin * p.y, -sin * p.x + cos * p.y))
        .collect();
    let mut model = ClothModel::new(rest, &triangles, membrane, 0.0, 1.0)?;
    if let Some(law) = response {
        model.set_membrane_responses(&vec![Some(law); triangles.len()])?;
    }
    if let Some(domain) = ftx_pattern::material_calibration::fitted_domain(m)? {
        model.set_membrane_domains(&vec![Some(domain); triangles.len()])?;
    }
    let nx = geometry.length_cells;
    let fixed: Vec<_> = (0..reference.len())
        .map(|i| {
            let end = i % (nx + 1) == 0 || i % (nx + 1) == nx;
            [end, end, true]
        })
        .collect();
    let reference_positions_m: Vec<_> =
        reference.iter().map(|p| Vec3::new(p.x, p.y, 0.0)).collect();
    let mut solver = Solver::new(model, reference_positions_m.clone(), &fixed, Vec3::ZERO);
    solver.tol = options.correction_tolerance_m;
    let mut report = Report {
        schema: "ftx-clamped-strip/1",
        material_sha256: material.sha256().into(),
        material: material.frozen_selection(),
        geometry: geometry.clone(),
        options: options.clone(),
        reference_positions_m,
        triangles,
        samples: Vec::new(),
        newton_iterations: 0,
        no_claim: "Planar memoryless membrane with full-width no-slip grips and free interior in-plane coordinates. No wrinkling, gravity, bending, grip compliance, rate, hysteresis, fitted law, empirical coupon or garment qualification. Refine the mesh and loading path independently.",
    };
    let mut previous = 0.0;
    for &extension in extension_ratio {
        let remaining = options.max_total_newton - report.newton_iterations;
        if remaining == 0 {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "strip exhausted its total Newton budget; no partial path published",
            ));
        }
        solver.max_newton = remaining.min(options.max_newton);
        // Affine continuation is ONLY the starting guess. Interior x and y are subsequently free.
        for (i, p) in solver.x.iter_mut().enumerate() {
            p.x *= (1.0 + extension) / (1.0 + previous);
            if i % (nx + 1) == 0 {
                p.x = 0.0;
                p.y = reference[i].y;
            }
            if i % (nx + 1) == nx {
                p.x = geometry.gauge_m * (1.0 + extension);
                p.y = reference[i].y;
            }
        }
        let solve = solver.static_solve()?;
        if !solve.converged {
            return Err(Refusal::new(
                RefusalCode::NotConverged,
                "strip static solve did not converge",
            ));
        }
        report.newton_iterations += solve.iterations;
        for tri in &report.triangles {
            let [a, b, c] = tri.map(|i| solver.x[i]);
            if (b - a).cross(c - a).z <= 0.0 {
                return Err(Refusal::new(
                    RefusalCode::DegenerateGeometry,
                    "strip contains an inverted or collapsed planar triangle",
                ));
            }
        }
        let forces = solver.checked_elastic_forces()?;
        let mut grip_reaction_n = [Vec3::ZERO; 2];
        let mut residual: f64 = 0.0;
        for (i, &f) in forces.iter().enumerate() {
            match i % (nx + 1) {
                0 => grip_reaction_n[0] -= f,
                k if k == nx => grip_reaction_n[1] -= f,
                _ => residual = residual.max(f.x.abs()).max(f.y.abs()),
            }
        }
        let balance = grip_reaction_n[0] + grip_reaction_n[1];
        if residual > options.force_tolerance_n
            || balance.norm() > options.force_tolerance_n * reference.len() as f64
        {
            return Err(Refusal::new(
                RefusalCode::NotConverged,
                format!(
                    "strip reaction equilibrium failed: free residual {residual} N, grip imbalance {} N",
                    balance.norm()
                ),
            ));
        }
        let mut strain_range = [[f64::INFINITY, f64::NEG_INFINITY]; 3];
        for element in &solver.model.membranes {
            for (range, strain) in strain_range.iter_mut().zip(element.strain(&solver.x)) {
                range[0] = range[0].min(strain);
                range[1] = range[1].max(strain);
            }
        }
        let energy = solver.model.elastic_energy(&solver.x);
        if !energy.is_finite() || !grip_reaction_n.iter().all(|v| v.is_finite()) {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "strip reaction or energy is not finite",
            ));
        }
        report.samples.push(Sample {
            extension_ratio: extension,
            axial_force_n: grip_reaction_n[1].x,
            grip_reaction_n,
            free_residual_n: residual,
            reaction_balance_n: balance,
            energy_j: energy,
            strain_range,
            positions_m: solver.x.clone(),
            solve,
        });
        previous = extension;
    }
    Ok(report)
}

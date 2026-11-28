//! Source-bound Rung-2 knit garment draping and continuum patch diagnostics.
//!
//! Preserves admitted stitch topology and intrinsic gauge metrics, binds the selected material's
//! mass, contact and constitutive laws, and advances the requested physical schedule. The patch
//! diagnostic samples that same continuum model; it does not run an independent yarn-level RVE.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode, Vec3, det};
use ftx_pattern::knit::KnitDoc;
use ftx_pattern::material_bending::BendingTensor;
use ftx_pattern::material_catalog::ResolvedMaterial;
use ftx_pattern::materials::{Evidence, Mechanics};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::cloth::{ClothModel, Solver};
use crate::contact::Body;
use crate::elements::Membrane;
use crate::material::KnitMaterial;
use crate::run::{DrapeBody, DrapeOptions, GRAVITY};
use crate::stepping::SubdivisionOptions;

use super::constitutive::KnitShellAssessment;
use super::surface::{KnitSurfaceMesh, KnitSurfaceTopology, mesh_knit_doc};

const MAX_KNIT_DRAPE_VERTICES: usize = 12_000;
const MAX_RETAINED_VERTEX_FRAMES: usize = 2_000_000;
const QUIET_SPEED_M_S: f64 = 1e-3;
const QUIET_GRAVITY_STEPS: usize = 3;

/// Options controlling [`drape_knit_doc`].
#[derive(Clone, Debug)]
pub struct KnitDrapeOptions {
    /// Solver, contact, and step options.
    pub drape_opts: DrapeOptions,
    /// Actual admitted source record, including independently sourced mass and contact values.
    /// Its explicit coupled law selects membrane, optional bending and optional hysteresis.
    pub material: ResolvedMaterial,
}

/// Completed Rung-2 knit garment drape result (`FTX-K17`).
#[derive(Clone, Debug)]
pub struct KnitDrapeResult {
    /// Exact source stitch IDs, topology and intrinsic gauge charts consumed by the solver.
    pub surface: KnitSurfaceMesh,
    /// Source material identity, active physical inputs and explicit model limits.
    pub material_report: Value,
    /// Final solver state after contact/gravity steps.
    pub solver: Solver,
    /// Welded triangle connectivity indexing `solver.x`.
    pub triangles: Vec<[usize; 3]>,
    /// Sealed continuum assessment receipt; unassessed mechanisms remain explicit in the report.
    pub assessment: KnitShellAssessment,
    /// Trajectory snapshots (initial + one per accepted step).
    pub frames: Vec<Vec<Vec3>>,
    /// Number of dynamic steps executed.
    pub steps_run: usize,
    /// Executed zero-gravity preparation intervals; no seam closure is inferred.
    pub sew_steps_run: usize,
    /// Executed gravity intervals, including any explicitly requested settling tail.
    pub drape_steps_run: usize,
    /// Consecutive accepted gravity substeps with every vertex at or below 1 mm/s.
    pub quiet_gravity_steps: usize,
    /// Total Newton iterations across executed steps.
    pub newton_iterations: usize,
    /// Number of unconverged steps.
    pub unconverged_steps: usize,
    /// Maximum vertex speed on the final step (m/s).
    pub max_speed_m_s: f64,
    /// True only after the declared gravity phase has a three-step quiet tail.
    pub settled: bool,
    /// True if draped as a closed tubular garment rather than a curved open panel.
    pub is_tubular: bool,
    /// Course index span `[min_course, max_course]`.
    pub course_range: [u32; 2],
    /// Wale index span `[min_wale, max_wale]`.
    pub wale_range: [i32; 2],
}

/// Source-matched continuum patch diagnostic. This is not an independent yarn-level solve.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnitPatchAuditReport {
    /// Exact selected source material identity.
    pub material_sha256: String,
    /// Explicit physical scope of this calculation.
    pub scope: String,
    /// Exact topological selection: `"all"`, `"boundary"`, or `"selvedge"`.
    pub zone: String,
    /// Number of converged solver triangles aggregated into this patch.
    pub selected_triangles: usize,
    /// Reference-area-weighted macro membrane Green strain `[E11, E22, 2*E12]`.
    pub macro_strain: [f64; 3],
    /// Reference-area-weighted physical curvature `[k11, k22, 2*k12]` in 1/m.
    pub macro_curvature: [f64; 3],
    /// Linear span `L_patch` in metres.
    pub patch_span_m: f64,
    /// Wale pitch in metres.
    pub wale_pitch_m: f64,
    /// Equilibrium membrane plus bound panel-bending energy per reference area, in J/m².
    pub energy_density_j_m2: f64,
    /// Maximum positive principal continuum stress resultant times wale pitch, in Newtons.
    /// This dimensionally consistent estimate is not an independently resolved yarn force.
    pub estimated_peak_yarn_tension_n: f64,
    /// Unavailable without a source optical law; never invented from a reference RVE.
    pub pore_open_fraction: Option<f64>,
    /// Remaining jamming margin (`< 0.05` triggers `KnitJamming`).
    pub jamming_margin: f64,
    /// Evidence basis of the underlying constitutive card.
    pub evidence: Evidence,
    /// Always `false`: software audit never authorizes live hardware execution.
    pub hardware_execution_authorized: bool,
    /// Always `false`: software audit never validates physical knitting.
    pub physical_knitting_validated: bool,
    /// Always `false`: software audit never claims wearer fit.
    pub fit_claimed: bool,
}

struct KnitMaterialInputs {
    law: KnitMaterial,
    tensor: Option<BendingTensor>,
    mechanics: Mechanics,
    options: DrapeOptions,
    evidence: Evidence,
    report: Value,
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

fn combine_evidence(first: Evidence, second: Evidence) -> Evidence {
    match (first, second) {
        (Evidence::Estimate, _) | (_, Evidence::Estimate) => Evidence::Estimate,
        (Evidence::Published, _) | (_, Evidence::Published) => Evidence::Published,
        _ => Evidence::Measured,
    }
}

fn with_tensor_evidence(
    record: &ResolvedMaterial,
    mut evidence: Evidence,
    tensor: Option<BendingTensor>,
) -> Evidence {
    if tensor.is_some() {
        for key in [
            "bending_warp",
            "bending_weft",
            "bending_coupling",
            "bending_twist",
            "preferred_curvature_warp",
            "preferred_curvature_weft",
            "preferred_curvature_twist",
        ] {
            if let Some(quantity) = record.material().properties.get(key) {
                evidence = combine_evidence(evidence, quantity.evidence);
            }
        }
    }
    evidence
}

fn resolve_material(
    record: &ResolvedMaterial,
    options: &DrapeOptions,
) -> Result<KnitMaterialInputs, Refusal> {
    options.validate()?;
    if options.spacing != DrapeOptions::default().spacing {
        return Err(invalid("KnitDoc surfaces retain one vertex per source loop; mesh spacing cannot override that topology")
            .repair("omit --spacing; change the authored stitch topology/gauge, or use the sewn-panel mesh pathway"));
    }
    if options.compression != DrapeOptions::default().compression
        || options.compression_model != DrapeOptions::default().compression_model
    {
        return Err(invalid(
            "legacy compression overrides do not apply to the selected coupled knit law",
        )
        .repair(
            "retain the source coupled material response and omit the legacy compression override",
        ));
    }
    let compiled = record.coupled_knit()?.ok_or_else(|| {
        invalid("KnitDoc draping requires an explicitly selected source-bearing coupled knit law")
            .repair("select a material record with knit_response and independently sourced sheet mass/contact properties")
    })?;
    let law = KnitMaterial::from_compiled(&compiled)?;
    let mechanics = record.mechanics_for_contacts(options.self_contact.is_some())?;
    let tensor = record.bending_response()?;
    if record
        .material()
        .retained_liquid_mass(record.allows_estimates())?
        .is_some()
    {
        return Err(invalid(
            "retained-liquid mechanics are not admitted for this knit surface pathway",
        ));
    }
    if options.stick_slip.is_some() || options.self_stick_slip.is_some() {
        return Err(invalid(
            "uniform stick-slip overrides cannot replace the selected material's friction evidence",
        ));
    }
    if options.construction.is_active()
        || options.mesh != crate::garment::MeshOptions::default()
        || options.start_self_repulsion.is_some()
        || options.elastic_ea != DrapeOptions::default().elastic_ea
    {
        return Err(invalid(
            "KnitDoc surfaces do not admit woven construction multipliers, adaptive remeshing, start repulsion or casing-elastic overrides",
        ));
    }
    if options.elastic_hold_steps != 0 || options.static_equilibrium.is_some() {
        return Err(invalid(
            "KnitDoc draping does not yet emit elastic-hold or separate static-equilibrium studies",
        ));
    }
    if law.hysteresis.is_some() && options.integrator != crate::TimeIntegrator::ImplicitEuler {
        return Err(invalid(
            "source-selected knit hysteresis requires ImplicitEuler",
        ));
    }
    if (law.bending.is_some() || tensor.is_some())
        && options.bending_model == crate::BendingModel::DiscreteShells
    {
        return Err(invalid(
            "source-selected panel bending requires the quadratic patch formulation",
        ));
    }
    if law.bending.is_some() && options.damping.rayleigh_bending_s > 0.0 {
        return Err(invalid(
            "Rayleigh bending damping is not characterized for asymmetric knit bending",
        ));
    }
    let mut effective = *options;
    effective.contact.thickness = 0.5 * mechanics.thickness_m;
    effective.contact.mu = mechanics.friction_skin;
    if let Some(contact) = &mut effective.self_contact {
        contact.thickness = mechanics.thickness_m;
        contact.mu = mechanics.friction_self.ok_or_else(|| {
            invalid("self-contact requires independently sourced same-sheet friction")
        })?;
    }
    effective.validate()?;
    let mut evidence = compiled.evidence();
    for key in [
        "areal_density",
        "thickness",
        "bending_rigidity",
        "friction_skin",
        "friction_self",
    ] {
        if key == "friction_self" && effective.self_contact.is_none() {
            continue;
        }
        if let Some(quantity) = record.material().properties.get(key) {
            evidence = combine_evidence(evidence, quantity.evidence);
        }
    }
    evidence = with_tensor_evidence(record, evidence, tensor);
    let report = json!({
        "basis":"characterized_record", "record":record, "record_sha256":record.sha256(),
        "mechanics":mechanics, "knit_law_sha256":compiled.fingerprint(), "evidence":evidence,
        "knit_response":compiled, "body_offset_m":effective.contact.thickness,
        "self_contact_enabled":effective.self_contact.is_some(),
        "contact_policy":"source material owns thickness and friction; caller owns numerical tolerances",
        "material_axes":"u: course growth (wale); v: negative ordered course contour",
        "surface_basis":"actual source loop centres and parent-connected triangles; no extra half-stitch boundary extension",
        "mesh_resolution":"source_loop_centres",
        "not_simulated":["yarn-resolved RVE audit","pore optics","snagging and laddering","physical garment fit"],
        "fit_claimed":false,
    });
    Ok(KnitMaterialInputs {
        law,
        tensor,
        mechanics,
        options: effective,
        evidence,
        report,
    })
}

fn check_material_gauge(doc: &KnitDoc, record: &ResolvedMaterial) -> Result<(), Refusal> {
    if doc
        .metadata
        .get("material_sha256")
        .is_some_and(|hash| hash != record.sha256())
    {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            "KnitDoc material identity differs from the consumed material record",
        ));
    }
    for (key, density) in [
        ("knit_wale_density", doc.gauge.wales_per_100mm * 10.0),
        ("knit_course_density", doc.gauge.courses_per_100mm * 10.0),
    ] {
        if record.material().properties.contains_key(key) {
            let source = record
                .material()
                .require(key, "1/m", record.allows_estimates())?;
            if source <= 0.0 || (source - density).abs() > 1e-12 * source.abs().max(density.abs()) {
                return Err(Refusal::new(
                    RefusalCode::VerificationMismatch,
                    format!("KnitDoc gauge differs from selected material {key}"),
                ));
            }
        }
    }
    Ok(())
}

struct Placement {
    positions: Vec<Vec3>,
    fixed: Vec<[bool; 3]>,
    report: Value,
}

fn place_surface(
    surface: &KnitSurfaceMesh,
    body: &DrapeBody,
    opts: &DrapeOptions,
) -> Result<Placement, Refusal> {
    let frame = body
        .frames
        .frame("torso")
        .ok_or_else(|| invalid("knit placement requires an actual torso frame"))?;
    let anchor_mm = body
        .frames
        .landmark_h("torso", "snp")
        .ok_or_else(|| invalid("knit placement requires the source torso snp landmark"))?;
    Refusal::finite("torso snp landmark", anchor_mm)?;
    if !frame.origin.is_finite()
        || [frame.up, frame.front, frame.lat]
            .iter()
            .any(|axis| !axis.is_finite() || (axis.norm2() - 1.0).abs() > 1e-8)
        || frame.up.dot(frame.front).abs() > 1e-8
        || frame.up.dot(frame.lat).abs() > 1e-8
        || frame.front.dot(frame.lat).abs() > 1e-8
        || (frame.up.cross(frame.front) - frame.lat).norm() > 1e-8
    {
        return Err(invalid(
            "knit placement needs a finite orthonormal source torso frame",
        ));
    }
    let origin = frame.origin * 0.001;
    let top = origin + frame.up * (anchor_mm * 0.001);
    let mut front_extent = f64::NEG_INFINITY;
    for vertex in body.collision.0 {
        if !vertex.is_finite() {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "nonfinite source body vertex",
            ));
        }
        front_extent = front_extent.max((*vertex - origin).dot(frame.front));
    }
    if !front_extent.is_finite() {
        return Err(invalid("knit placement requires a nonempty collision body"));
    }
    let is_tube = surface.topology == KnitSurfaceTopology::Tube;
    let max_distance = surface
        .vertices
        .iter()
        .map(|v| v.course_distance_m)
        .fold(0.0, f64::max);
    let center_wale = 0.5 * (f64::from(surface.wale_range[0]) + f64::from(surface.wale_range[1]));
    let clearance = opts.contact.thickness + opts.contact.dhat;
    let mut positions = Vec::with_capacity(surface.vertices.len());
    let anchor_course = if surface.course_growth_upward {
        surface.course_range[1]
    } else {
        surface.course_range[0]
    };
    let anchors: Vec<_> = surface
        .vertices
        .iter()
        .enumerate()
        .filter(|(_, v)| v.course == anchor_course)
        .map(|(i, _)| i)
        .collect();
    if anchors.len() < 2 {
        return Err(invalid(
            "knit placement requires two source loops on its support course",
        ));
    }
    let supports = if is_tube {
        [anchors[0], anchors[anchors.len() / 2]]
    } else {
        [anchors[0], *anchors.last().unwrap()]
    };
    let mut fixed = vec![[false; 3]; surface.vertices.len()];
    for &i in &supports {
        fixed[i] = [true; 3];
    }
    for vertex in &surface.vertices {
        let position = if is_tube {
            let count = vertex.course_stitches as f64;
            let radius = surface.wale_pitch_m / (2.0 * det::sin(std::f64::consts::PI / count));
            // Material +u follows course growth and +v opposes the ordered contour. Reversing
            // growth must also reverse the contour embedding to keep the physical normal outward.
            let orientation = if surface.course_growth_upward {
                1.0
            } else {
                -1.0
            };
            let theta = orientation * std::f64::consts::TAU * vertex.course_position as f64 / count;
            let (sin, cos) = det::sin_cos(theta);
            let height = if surface.course_growth_upward {
                vertex.course_distance_m - max_distance
            } else {
                -vertex.course_distance_m
            };
            top + frame.up * height + (frame.front * cos + frame.lat * sin) * radius
        } else {
            top + frame.up * (vertex.course_distance_m - max_distance)
                + frame.lat
                    * ((f64::from(vertex.logical_wale) - center_wale) * surface.wale_pitch_m)
                + frame.front * (front_extent + clearance)
        };
        if !position.is_finite() {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "unrepresentable knit source placement",
            ));
        }
        positions.push(position);
    }
    let report = json!({
        "model":if is_tube {"source_gauge_torso_rings"} else {"source_gauge_front_hanging_panel"},
        "part":"torso", "landmark":"snp", "support_stitch_ids":supports.map(|i| surface.vertices[i].stitch_id),
        "tube_radius_resized":false, "dimensions_resized":false,
        "course_growth_upward":surface.course_growth_upward,
        "scope":"numerical initial placement with two explicit fixed source loops; no dressing or physical shoulder attachment claim",
    });
    Ok(Placement {
        positions,
        fixed,
        report,
    })
}

/// Drapes a validated `ftx-knit/1` [`KnitDoc`] on a [`DrapeBody`] using Rung-2 homogenized knit
/// constitutive laws and emits a sealed Seam-4 `DrapeUncertaintyReceipt` for the admitted continuum
/// diagnostics. The material report identifies mechanisms that have not been assessed.
///
/// # Errors
/// Invalid document/options, non-finite geometry, body-intersection at placement, jammed start,
/// or non-converged Newton steps.
pub fn drape_knit_doc(
    doc: &KnitDoc,
    body: &DrapeBody,
    opts: &KnitDrapeOptions,
) -> Result<KnitDrapeResult, Refusal> {
    doc.validate()?;
    check_material_gauge(doc, &opts.material)?;
    let mut material = resolve_material(&opts.material, &opts.drape_opts)?;
    let options = material.options;
    let surface = mesh_knit_doc(doc, MAX_KNIT_DRAPE_VERTICES)?;
    let nominal_steps = options
        .sew_steps
        .checked_add(options.drape_steps)
        .ok_or_else(|| {
            Refusal::new(
                RefusalCode::BudgetExceeded,
                "knit physical schedule overflows",
            )
        })?;
    let max_steps = nominal_steps
        .checked_add(options.max_settle_steps)
        .ok_or_else(|| {
            Refusal::new(
                RefusalCode::BudgetExceeded,
                "knit settling schedule overflows",
            )
        })?;
    if max_steps
        .checked_add(1)
        .and_then(|n| n.checked_mul(surface.vertices.len()))
        .is_none_or(|n| n > MAX_RETAINED_VERTEX_FRAMES)
    {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "requested knit trajectory exceeds the 2,000,000 retained vertex-frame bound; no steps or source vertices were dropped",
        ));
    }
    let placement = place_surface(&surface, body, &options)?;
    let mechanics = material.mechanics;
    let base_membrane = Membrane::orthotropic(
        mechanics.warp_n_m,
        mechanics.weft_n_m,
        mechanics.poisson,
        mechanics.shear_n_m,
        options.compression,
    );
    base_membrane.validate()?;
    let mut model = ClothModel::from_triangles(
        surface.vertices.len(),
        &surface.triangles,
        &surface.tri_rest_m,
        base_membrane,
        mechanics.bending_nm,
        mechanics.areal_kg_m2,
    )?;
    model.compression_model = options.compression_model;
    model.bending_model = options.bending_model;
    model
        .set_coupled_knit_membranes(&vec![Some(material.law.membrane); surface.triangles.len()])?;
    let weld: Vec<_> = (0..surface.vertices.len()).collect();
    model.set_intrinsic_panel_bending(
        &surface.triangles,
        &surface.tri_rest_m,
        &weld,
        &vec![material.tensor; surface.triangles.len()],
        &vec![material.law.bending; surface.triangles.len()],
    )?;
    let body_collider = Body::new(
        body.collision.0.to_vec(),
        body.collision.1.to_vec(),
        options.contact.dhat,
    )?;
    let mut solver = Solver::new(
        model,
        placement.positions.clone(),
        &placement.fixed,
        Vec3::ZERO,
    )
    .with_contact(body_collider, options.contact)?;
    if let Some(contact) = options.self_contact {
        solver = solver.with_self_contact(contact)?;
    }
    if let Some(bank) = &material.law.hysteresis {
        solver = solver.with_knit_hysteresis(&vec![Some(bank.clone()); surface.triangles.len()])?;
    }
    solver.tol = options.tol;
    solver.pcg_iters = options.pcg_iters;
    solver.newton_strategy = options.newton_strategy;
    solver.integrator = options.integrator;
    solver.damping = options.damping;
    if let Some(policy) = options.adaptive_barrier {
        let mean_mass = solver.model.mass.iter().sum::<f64>() / solver.model.mass.len() as f64;
        let stiffness = 0.5 * (material.law.membrane.q11 + material.law.membrane.q22);
        let gravity = if options.drape_steps + options.max_settle_steps > 0 {
            GRAVITY
        } else {
            0.0
        };
        solver
            .contact
            .as_mut()
            .unwrap()
            .set_adaptive_barrier(policy, mean_mass, stiffness, gravity)?;
    }
    solver.check_constitutive_models()?;
    solver.check_integrator_history()?;
    if options.max_settle_steps > 0 {
        solver.check_settling_history()?;
    }
    solver.model.check_membrane_domains(&solver.x)?;
    solver.model.check_panel_bending_domains(&solver.x)?;
    let mut frames = Vec::with_capacity(max_steps + 1);
    frames.push(placement.positions);
    let mut newton_iterations = 0usize;
    let mut max_speed_m_s = 0.0_f64;
    let mut quiet_gravity_steps = 0usize;
    let mut drape_steps_run = 0usize;
    let mut partitions = Vec::with_capacity(max_steps);
    let mut remaining_trials = options.time_subdivision.map_or(max_steps, |p| p.max_trials);
    for interval in 0..max_steps {
        if interval >= nominal_steps && quiet_gravity_steps >= QUIET_GRAVITY_STEPS {
            break;
        }
        let gravity = interval >= options.sew_steps;
        solver.gravity = if gravity {
            Vec3::new(0.0, 0.0, -GRAVITY)
        } else {
            Vec3::ZERO
        };
        solver.friction_rounds = options.lag_rounds[usize::from(gravity)];
        if remaining_trials == 0 {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "knit time-subdivision trial budget exhausted",
            ));
        }
        let subdivision = options.time_subdivision.map_or(
            SubdivisionOptions {
                max_halvings: 0,
                max_trials: 1,
            },
            |p| SubdivisionOptions {
                max_trials: remaining_trials,
                ..p
            },
        );
        let (report, speeds) =
            solver.step_subdivided_with(options.step, subdivision, |state, _| {
                let mut peak = 0.0_f64;
                for velocity in &state.v {
                    let speed = velocity.norm();
                    Refusal::finite("knit vertex speed", speed)?;
                    peak = peak.max(speed);
                }
                Ok(peak)
            })?;
        remaining_trials -= report.trials;
        for (step, speed) in report.accepted.iter().zip(speeds) {
            newton_iterations += step.solve.iterations;
            max_speed_m_s = speed;
            if gravity {
                quiet_gravity_steps = if speed <= QUIET_SPEED_M_S {
                    quiet_gravity_steps + 1
                } else {
                    0
                };
            }
        }
        drape_steps_run += usize::from(gravity);
        partitions.push(report);
        frames.push(solver.x.clone());
        let more = interval + 1 < nominal_steps
            || (interval + 1 < max_steps && quiet_gravity_steps < QUIET_GRAVITY_STEPS);
        if more && let Some(contact) = &mut solver.contact {
            contact.adapt_barrier_stiffness(&solver.x);
        }
    }
    let assessment = solver.assess_knit_shell(
        surface.pattern_sha256.clone(),
        opts.material.sha256().to_string(),
        None,
        material.evidence,
    )?;
    material.report["gauge"] = json!(doc.gauge);
    material.report["source_wale_density_present"] = json!(
        opts.material
            .material()
            .properties
            .contains_key("knit_wale_density")
    );
    material.report["source_course_density_present"] = json!(
        opts.material
            .material()
            .properties
            .contains_key("knit_course_density")
    );
    material.report["placement"] = placement.report;
    material.report["time_intervals"] = json!(partitions);
    material.report["total_mass_kg"] = json!(solver.model.mass.iter().sum::<f64>());
    material.report["settling_basis"] = json!(
        "at least three accepted gravity substeps with every vertex speed <= 1 mm/s; converged Newton solves alone do not establish settling"
    );
    material.report["complete_knit_assessment"] = json!(false);
    material.report["unassessed_mechanisms"] = json!([
        "source-qualified pore optics",
        "rib snap-through from a rib-specific constitutive model",
        "free-boundary edge curl distinguished from ordinary garment curvature",
        "yarn migration, snagging and laddering",
        "independent yarn-resolved patch validation"
    ]);
    let steps_run = frames.len() - 1;
    Ok(KnitDrapeResult {
        solver,
        triangles: surface.triangles.clone(),
        assessment,
        frames,
        steps_run,
        sew_steps_run: options.sew_steps,
        drape_steps_run,
        quiet_gravity_steps,
        newton_iterations,
        unconverged_steps: 0,
        max_speed_m_s,
        settled: drape_steps_run > 0 && quiet_gravity_steps >= QUIET_GRAVITY_STEPS,
        is_tubular: surface.topology == KnitSurfaceTopology::Tube,
        course_range: surface.course_range,
        wale_range: surface.wale_range,
        surface,
        material_report: material.report,
    })
}

/// Samples the actual bound coupled law over an exact topological selection. Nonlinear energies
/// and extrema are evaluated per triangle before aggregation; no averaged-strain surrogate or
/// reference RVE is substituted for the selected material.
///
/// # Errors
/// Unsupported anatomical labels or vector-Hessian curvature diagnostics, empty selection,
/// mismatched source laws/compression mappings, non-finite solver state, or jammed local patch.
/// Anatomical zones require source region bindings not inferred here.
pub fn audit_solver_knit_patch(
    solver: &Solver,
    zone: &str,
    material: &ResolvedMaterial,
    wale_pitch_m: f64,
) -> Result<KnitPatchAuditReport, Refusal> {
    let zone = zone.trim().to_ascii_lowercase();
    if !matches!(zone.as_str(), "all" | "boundary" | "selvedge") {
        return Err(invalid("patch audit admits all, boundary, or selvedge; anatomical zone names require explicit source region bindings")
            .repair("select all or an actual topological boundary; do not infer collar, hem, cuff or armhole from global height"));
    }
    Refusal::finite("wale_pitch_m", wale_pitch_m)?;
    if wale_pitch_m <= 0.0 {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            "wale_pitch_m must be strictly positive",
        ));
    }
    if solver.model.membranes.is_empty() {
        return Err(Refusal::new(
            RefusalCode::DegenerateGeometry,
            "cannot audit patch on empty cloth model",
        ));
    }
    if solver.x.len() != solver.model.rest.len()
        || solver
            .model
            .membranes
            .iter()
            .any(|e| e.v.iter().any(|&v| v >= solver.x.len()))
    {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            "patch audit requires cloth positions matching triangle topology",
        ));
    }
    if solver.x.iter().any(|p| !p.is_finite()) {
        return Err(Refusal::new(
            RefusalCode::NonFinite,
            "patch audit requires finite cloth vertex positions",
        ));
    }
    let compiled = material.coupled_knit()?.ok_or_else(|| {
        invalid("patch audit requires the actual source-bearing coupled knit material")
    })?;
    let law = KnitMaterial::from_compiled(&compiled)?;
    let tensor = material.bending_response()?;
    if tensor.is_some_and(|law| !law.uses_normal()) {
        return Err(Refusal::new(
            RefusalCode::UnqualifiedProcess,
            "vector-Hessian bending has a valid drape energy, but this patch audit does not provide its physical normal-curvature diagnostic",
        )
        .repair("omit --audit-patch to retain the selected vector-Hessian drape; no substitute normal-projected law is inferred"));
    }

    // Boundary means an edge incident on one triangle, independent of its current height.
    let mut edge_counts: BTreeMap<(usize, usize), usize> = BTreeMap::new();
    for e in &solver.model.membranes {
        for k in 0..3 {
            let (a, b) = (e.v[k], e.v[(k + 1) % 3]);
            *edge_counts.entry((a.min(b), a.max(b))).or_insert(0) += 1;
        }
    }
    let mut selected: Vec<usize> = Vec::new();
    for (ti, e) in solver.model.membranes.iter().enumerate() {
        let is_boundary = (0..3).any(|k| {
            let (a, b) = (e.v[k], e.v[(k + 1) % 3]);
            edge_counts.get(&(a.min(b), a.max(b))).copied().unwrap_or(0) == 1
        });
        if zone == "all" || is_boundary {
            if solver.model.coupled_knit_membrane(ti) != Some(&law.membrane) {
                return Err(Refusal::new(
                    RefusalCode::VerificationMismatch,
                    format!(
                        "selected triangle {ti} is not bound to the selected coupled material law"
                    ),
                ));
            }
            if solver.model.compression.get(ti) != Some(&[1.0, 1.0]) {
                return Err(Refusal::new(
                    RefusalCode::VerificationMismatch,
                    format!(
                        "selected triangle {ti} has a modified or missing compression mapping; the source-law audit requires [1, 1]"
                    ),
                ));
            }
            selected.push(ti);
        }
    }
    if selected.is_empty() {
        return Err(invalid("requested topological patch selection is empty"));
    }

    let selected_set: BTreeSet<usize> = selected.iter().copied().collect();
    let mut mean_strain = [0.0_f64; 3];
    let mut reference_area = 0.0_f64;
    let mut energy = 0.0_f64;
    let mut peak_tension = 0.0_f64;
    let mut jamming_margin = 1.0_f64;
    let mut bb_min = [f64::INFINITY; 3];
    let mut bb_max = [f64::NEG_INFINITY; 3];
    for &ti in &selected {
        let e = &solver.model.membranes[ti];
        let s = e.strain(&solver.x);
        let evaluated = law.membrane.evaluate(s)?;
        reference_area += e.area;
        energy += evaluated.energy_density * e.area;
        jamming_margin = jamming_margin.min(evaluated.jamming_margin);
        let [s11, s22, s12] = evaluated.stress;
        let principal = 0.5 * (s11 + s22) + det::sqrt(0.25 * (s11 - s22).powi(2) + s12 * s12);
        peak_tension = peak_tension.max(principal.max(0.0) * wale_pitch_m);
        for k in 0..3 {
            mean_strain[k] += s[k] * e.area;
        }
        for &vi in &e.v {
            let p = solver.x[vi];
            bb_min[0] = bb_min[0].min(p.x);
            bb_min[1] = bb_min[1].min(p.y);
            bb_min[2] = bb_min[2].min(p.z);
            bb_max[0] = bb_max[0].max(p.x);
            bb_max[1] = bb_max[1].max(p.y);
            bb_max[2] = bb_max[2].max(p.z);
        }
    }
    if !reference_area.is_finite() || reference_area <= 0.0 {
        return Err(invalid(
            "selected patch has no finite positive reference area",
        ));
    }
    for s in &mut mean_strain {
        *s /= reference_area;
    }

    let mut mean_curvature = [0.0_f64; 3];
    let mut curvature_area = 0.0_f64;
    let mut covered = BTreeSet::new();
    let patch_tris = solver.model.panel_bending_triangles();
    for (pi, patch) in solver.model.panel_bending_patches().iter().enumerate() {
        let tri_idx = *patch_tris
            .get(pi)
            .ok_or_else(|| invalid("panel bending patch has no source triangle binding"))?;
        if !selected_set.contains(&tri_idx) {
            continue;
        }
        let active = solver.model.panel_knit_bending(pi);
        if active != law.bending.as_ref() || (active.is_none() && tensor != Some(patch.tensor())) {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                format!("selected panel patch {pi} differs from the selected source bending law"),
            ));
        }
        patch.check_domain(&solver.x)?;
        let kp = patch.physical_curvature(&solver.x)?;
        let area = patch.reference_area();
        energy += active.map_or_else(
            || patch.energy(&solver.x),
            |bending| bending.evaluate_from_kappa(kp).energy_density * area,
        );
        for k in 0..3 {
            mean_curvature[k] += kp[k] * area;
        }
        curvature_area += area;
        covered.insert(tri_idx);
    }
    if (law.bending.is_some() || tensor.is_some()) && covered != selected_set {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            "selected patch is missing the source material's panel bending binding",
        ));
    }
    if curvature_area > 0.0 {
        for c in &mut mean_curvature {
            *c /= curvature_area;
        }
    }

    let dx = bb_max[0] - bb_min[0];
    let dy = bb_max[1] - bb_min[1];
    let dz = bb_max[2] - bb_min[2];
    let patch_span_m = det::sqrt(dx * dx + dy * dy + dz * dz);
    for (label, value) in [
        ("patch span", patch_span_m),
        ("patch energy", energy),
        ("patch tension estimate", peak_tension),
        ("patch curvature area", curvature_area),
    ] {
        Refusal::finite(label, value)?;
    }

    Ok(KnitPatchAuditReport {
        material_sha256: material.sha256().to_string(),
        scope: "source-matched equilibrium continuum membrane and panel bending; excludes scalar seam hinges, history storage, contact, optical pores and independent yarn mechanics".to_string(),
        zone,
        selected_triangles: selected.len(),
        macro_strain: mean_strain,
        macro_curvature: mean_curvature,
        patch_span_m,
        wale_pitch_m,
        energy_density_j_m2: energy / reference_area,
        estimated_peak_yarn_tension_n: peak_tension,
        pore_open_fraction: None,
        jamming_margin,
        evidence: with_tensor_evidence(material, compiled.evidence(), tensor),
        hardware_execution_authorized: false,
        physical_knitting_validated: false,
        fit_claimed: false,
    })
}

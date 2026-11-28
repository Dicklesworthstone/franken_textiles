//! Rung-2 knit garment draping & local Rung-1 patch auditing (`FTX-K09`, `FTX-K10`, `FTX-K17`).
//!
//! Converts a validated `ftx-knit/1` [`KnitDoc`] into a deterministic [`ClothModel`] bound with
//! [`CoupledKnitMembrane`], [`AsymmetricKnitBending`], and [`KnitHysteresisBank`], drapes it in
//! contact with a [`DrapeBody`], evaluates all 5 `C011` honesty flags into a sealed Seam-4
//! [`ftx_pattern::knit::DrapeUncertaintyReceipt`], and supports `--audit-patch <zone>` via
//! [`audit_local_knit_patch`].

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode, Vec2, Vec3, det};
use ftx_pattern::knit::{Bed, KnitDoc, WearerAgeClass};
use ftx_pattern::materials::Evidence;
use serde::{Deserialize, Serialize};

use crate::cloth::{ClothModel, Solver};
use crate::contact::Body;
use crate::elements::Membrane;
use crate::run::{DrapeBody, DrapeOptions, GRAVITY};

use super::constitutive::{
    AsymmetricKnitBending, CoupledKnitMembrane, KnitHysteresisBank, KnitShellAssessment,
};
use super::poincloux::{LocalAuditPatchInput, RveHomogenizedKnitCard, audit_local_knit_patch};

/// Options controlling [`drape_knit_doc`].
#[derive(Clone, Debug)]
pub struct KnitDrapeOptions {
    /// Solver, contact, and step options.
    pub drape_opts: DrapeOptions,
    /// Active source material SHA-256 bound into the sealed uncertainty receipt.
    pub material_sha256: String,
    /// Evidence basis of the constitutive laws (`Evidence::Estimate` unless coupon-measured).
    pub evidence: Evidence,
    /// Optional offline RVE homogenized knit card; falls back to reference stockinette estimate when `None`.
    pub rve_card: Option<RveHomogenizedKnitCard>,
}

/// Completed Rung-2 knit garment drape result (`FTX-K17`).
#[derive(Clone, Debug)]
pub struct KnitDrapeResult {
    /// Final solver state after contact/gravity steps.
    pub solver: Solver,
    /// Welded triangle connectivity indexing `solver.x`.
    pub triangles: Vec<[usize; 3]>,
    /// Sealed `C011` knit shell assessment & `ftx-drape-uncertainty/1` receipt.
    pub assessment: KnitShellAssessment,
    /// Trajectory snapshots (initial + one per accepted step).
    pub frames: Vec<Vec<Vec3>>,
    /// Number of dynamic steps executed.
    pub steps_run: usize,
    /// Total Newton iterations across executed steps.
    pub newton_iterations: usize,
    /// Number of unconverged steps.
    pub unconverged_steps: usize,
    /// Maximum vertex speed on the final step (m/s).
    pub max_speed_m_s: f64,
    /// True if all steps converged cleanly.
    pub settled: bool,
    /// True if draped as a closed tubular garment rather than a curved open panel.
    pub is_tubular: bool,
    /// Course index span `[min_course, max_course]`.
    pub course_range: [u32; 2],
    /// Wale index span `[min_wale, max_wale]`.
    pub wale_range: [i32; 2],
}

/// Result of a post-drape local Rung-1 Kirchhoff–Love patch audit (`--audit-patch <zone>`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnitPatchAuditReport {
    /// Audited zone label (e.g. `"selvedge"`, `"collar"`, `"hem"`, `"armhole"`).
    pub zone: String,
    /// Number of converged solver triangles aggregated into this patch.
    pub selected_triangles: usize,
    /// Mean macro membrane Green strain `[E11, E22, 2*E12]`.
    pub macro_strain: [f64; 3],
    /// Mean macro physical curvature `[k11, k22, 2*k12]` in 1/m.
    pub macro_curvature: [f64; 3],
    /// Linear span `L_patch` in metres.
    pub patch_span_m: f64,
    /// Wale pitch in metres.
    pub wale_pitch_m: f64,
    /// Patch energy density in J/m².
    pub energy_density_j_m2: f64,
    /// Estimated peak yarn-level tensile force in Newtons.
    pub estimated_peak_yarn_tension_n: f64,
    /// Open-pore grin-through fraction under local stretch.
    pub pore_open_fraction: f64,
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

fn default_reference_rve_card(evidence: Evidence) -> RveHomogenizedKnitCard {
    let mut ev = evidence;
    if ev == Evidence::Measured {
        // RVE cards are strictly MODEL_FIT / Estimate until backed by physical RVE validation.
        ev = Evidence::Estimate;
    }
    RveHomogenizedKnitCard {
        schema: "ftx-knit-rve-card/1".to_string(),
        id: "reference-stockinette-rve-estimate".to_string(),
        frankensim_rve_sha256: "reference-stockinette-estimate-v1".to_string(),
        evidence: ev,
        hill_mandel_residual: 1.0e-7,
        membrane: CoupledKnitMembrane::reference_stockinette_estimate(),
        bending: AsymmetricKnitBending::reference_stockinette_estimate(),
        rest_pore_fraction: 0.06,
        pore_strain_slope: 0.35,
    }
}

/// Drapes a validated `ftx-knit/1` [`KnitDoc`] on a [`DrapeBody`] using Rung-2 homogenized knit
/// constitutive laws and emits a sealed Seam-4 `DrapeUncertaintyReceipt` (`FTX-K09`, `FTX-K17`).
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
    opts.drape_opts.validate()?;
    if let Some(card) = &opts.rve_card {
        card.validate()?;
    }
    let pattern_sha256 = doc.canonical_sha256()?;

    const MAX_DRAPE_GRID_SPAN: i64 = 100_000;
    let mut c_min = u32::MAX;
    let mut c_max = u32::MIN;
    let mut w_min = i32::MAX;
    let mut w_max = i32::MIN;
    let mut front_cells: BTreeSet<(u32, i32)> = BTreeSet::new();
    let mut has_tubular_overlap = false;
    for st in &doc.stitches {
        c_min = c_min.min(st.course);
        c_max = c_max.max(st.course);
        w_min = w_min.min(st.wale);
        w_max = w_max.max(st.wale);
        if matches!(st.bed, Bed::Front | Bed::FrontSlider) {
            front_cells.insert((st.course, st.wale));
        }
    }
    for st in &doc.stitches {
        if matches!(st.bed, Bed::Back | Bed::BackSlider)
            && front_cells.contains(&(st.course, st.wale))
        {
            has_tubular_overlap = true;
            break;
        }
    }
    let course_span_u64 = u64::from(c_max).saturating_sub(u64::from(c_min)) + 1;
    let wale_span_i64 = i64::from(w_max) - i64::from(w_min) + 1;
    if course_span_u64 > MAX_DRAPE_GRID_SPAN as u64 || wale_span_i64 > MAX_DRAPE_GRID_SPAN {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            format!(
                "knit drape stitch span ({course_span_u64} courses x {wale_span_i64} wales) exceeds {MAX_DRAPE_GRID_SPAN}-cell drape bound"
            ),
        ));
    }
    let n_courses = (course_span_u64 as u32).max(2);
    let n_wales = (wale_span_i64 as u32).max(2);

    let wale_pitch_m = doc.gauge.blocked_wale_pitch_mm() * 0.001;
    let course_height_m = doc.gauge.blocked_course_height_mm() * 0.001;
    // Double circumferential rest width ONLY for genuine double-layer tubes where both beds are
    // occupied at the same `(course, wale)` — never for a flat panel with a 1x1 rib hem!
    let rest_width_m =
        (f64::from(n_wales) * wale_pitch_m * if has_tubular_overlap { 2.0 } else { 1.0 }).max(0.04);
    let rest_height_m = (f64::from(n_courses) * course_height_m).max(0.04);

    let (body_verts, body_tris) = body.collision;
    if body_verts.is_empty() || body_tris.is_empty() {
        return Err(Refusal::new(
            RefusalCode::DegenerateGeometry,
            "knit drape requires a non-empty collision body mesh",
        ));
    }

    let mut body_z_min = f64::INFINITY;
    let mut body_z_max = f64::NEG_INFINITY;
    let mut sum_xy = [0.0_f64; 2];
    for v in body_verts {
        Refusal::finite("body_vertex.x", v.x)?;
        Refusal::finite("body_vertex.y", v.y)?;
        Refusal::finite("body_vertex.z", v.z)?;
        body_z_min = body_z_min.min(v.z);
        body_z_max = body_z_max.max(v.z);
        sum_xy[0] += v.x;
        sum_xy[1] += v.y;
    }
    let cx = sum_xy[0] / (body_verts.len() as f64);
    let cy = sum_xy[1] / (body_verts.len() as f64);

    // Anchor the garment vertical span in the torso region of the body collider.
    let torso_top_z = body
        .frames
        .frame("torso")
        .and_then(|f| {
            body.frames
                .landmark_h("torso", "snp")
                .map(|h| (f.origin.z + h) * 0.001)
        })
        .filter(|z| z.is_finite() && *z > body_z_min + 0.04 && *z <= body_z_max + 0.05)
        .unwrap_or((body_z_max - 0.05).max(body_z_min + 0.05));
    let z_top = torso_top_z.min(body_z_max - 0.01).max(body_z_min + 0.01);
    let z_bot = z_top - rest_height_m;
    let placed_height_m = rest_height_m;

    let mut r_body_max = 0.0_f64;
    let mut y_front_min = f64::INFINITY;
    for v in body_verts {
        if v.z >= z_bot - 0.03 && v.z <= z_top + 0.03 {
            let dx = v.x - cx;
            let dy = v.y - cy;
            let r = det::sqrt(dx * dx + dy * dy);
            r_body_max = r_body_max.max(r);
            y_front_min = y_front_min.min(v.y);
        }
    }
    if !r_body_max.is_finite() || r_body_max <= 1e-4 {
        for v in body_verts {
            let dx = v.x - cx;
            let dy = v.y - cy;
            r_body_max = r_body_max.max(det::sqrt(dx * dx + dy * dy));
            y_front_min = y_front_min.min(v.y);
        }
    }

    let clearance_m = opts.drape_opts.contact.thickness + opts.drape_opts.contact.dhat + 0.008;
    let r_clear = r_body_max + clearance_m;
    let r_rest = rest_width_m / std::f64::consts::TAU;
    // Wrap as a closed tubular shell only when both beds actually overlap into a double-layer tube
    // AND the garment's rest circumference fits around the body within unjammed stretch (<= 12%).
    let is_tubular = has_tubular_overlap
        && doc.wearer_age_class != WearerAgeClass::CalibrationSwatchOnly
        && r_clear <= 1.12 * r_rest;

    let nx = (n_wales as usize).clamp(8, 20);
    let ny = (n_courses as usize).clamp(6, 16);

    let mut source_rest = Vec::with_capacity((nx + 1) * (ny + 1));
    for iy in 0..=ny {
        let v_frac = (iy as f64) / (ny as f64);
        for ix in 0..=nx {
            let u_frac = (ix as f64) / (nx as f64);
            source_rest.push(Vec2::new(u_frac * rest_width_m, v_frac * rest_height_m));
        }
    }
    let idx = |ix: usize, iy: usize| iy * (nx + 1) + ix;
    let mut source_tris = Vec::with_capacity(2 * nx * ny);
    for iy in 0..ny {
        for ix in 0..nx {
            let a = idx(ix, iy);
            let b = idx(ix + 1, iy);
            let c = idx(ix + 1, iy + 1);
            let d = idx(ix, iy + 1);
            source_tris.push([a, b, c]);
            source_tris.push([a, c, d]);
        }
    }

    let (n_welded, weld, x0, fixed) = if is_tubular {
        let mut weld = vec![0usize; (nx + 1) * (ny + 1)];
        let mut x0 = vec![Vec3::ZERO; nx * (ny + 1)];
        let mut fixed = vec![[false; 3]; nx * (ny + 1)];
        let r_place = r_rest.max(r_clear);
        for iy in 0..=ny {
            let v_frac = (iy as f64) / (ny as f64);
            let z = z_bot + v_frac * placed_height_m;
            for ix in 0..=nx {
                let w = iy * nx + (ix % nx);
                weld[idx(ix, iy)] = w;
                if ix < nx {
                    let theta = std::f64::consts::TAU * (ix as f64) / (nx as f64);
                    let (sin_t, cos_t) = det::sin_cos(theta);
                    x0[w] = Vec3::new(cx + r_place * cos_t, cy + r_place * sin_t, z);
                    // Pin two opposite top-collar shoulder anchor points so a sleeveless tube
                    // hangs realistically rather than sliding off a straight-walled body mesh.
                    if iy == ny && (ix == 0 || ix == nx / 2) {
                        fixed[w] = [true, true, true];
                    }
                }
            }
        }
        (nx * (ny + 1), weld, x0, fixed)
    } else {
        let weld: Vec<usize> = (0..(nx + 1) * (ny + 1)).collect();
        let mut x0 = vec![Vec3::ZERO; (nx + 1) * (ny + 1)];
        let mut fixed = vec![[false; 3]; (nx + 1) * (ny + 1)];
        let y_place = y_front_min - clearance_m - 0.005;
        // Place flat panels at their true rest width so wide panels never start artificially compressed.
        let span_x = rest_width_m;
        for iy in 0..=ny {
            let v_frac = (iy as f64) / (ny as f64);
            let z = z_bot + v_frac * placed_height_m;
            for ix in 0..=nx {
                let u_frac = (ix as f64) / (nx as f64);
                let w = idx(ix, iy);
                x0[w] = Vec3::new(cx + (u_frac - 0.5) * span_x, y_place, z);
                if iy == ny && (ix == 0 || ix == nx) {
                    fixed[w] = [true, true, true];
                }
            }
        }
        ((nx + 1) * (ny + 1), weld, x0, fixed)
    };

    let welded_tris: Vec<[usize; 3]> = source_tris.iter().map(|t| t.map(|v| weld[v])).collect();
    let tri_rest: Vec<[Vec2; 3]> = source_tris
        .iter()
        .map(|t| t.map(|v| source_rest[v]))
        .collect();

    let card = opts
        .rve_card
        .clone()
        .unwrap_or_else(|| default_reference_rve_card(opts.evidence));
    let mem_law = card.membrane;
    let bend_law = card.bending;
    let base_membrane = Membrane::orthotropic(mem_law.q11, mem_law.q22, 0.25, mem_law.q66, 1.0);
    let areal_kg_m2 = doc
        .gauge
        .yarn_tex
        .map_or(0.22, |tex| (tex * 1.2e-3).clamp(0.08, 0.65));

    let mut model = ClothModel::from_triangles(
        n_welded,
        &welded_tris,
        &tri_rest,
        base_membrane,
        1e-5,
        areal_kg_m2,
    )?;
    model.set_coupled_knit_membranes(&vec![Some(mem_law); welded_tris.len()])?;
    model.set_knit_panel_bending(
        &source_rest,
        &source_tris,
        &weld,
        &vec![Some(bend_law); welded_tris.len()],
    )?;

    let body_collider = Body::new(
        body_verts.to_vec(),
        body_tris.to_vec(),
        opts.drape_opts.contact.dhat.max(1e-3),
    )?;
    let mut solver = Solver::new(model, x0.clone(), &fixed, Vec3::new(0.0, 0.0, -GRAVITY))
        .with_contact(body_collider, opts.drape_opts.contact)?
        .with_knit_hysteresis(&vec![
            Some(KnitHysteresisBank::reference_jersey_estimate());
            welded_tris.len()
        ])?;
    solver.tol = opts.drape_opts.tol;
    solver.pcg_iters = opts.drape_opts.pcg_iters;
    solver.newton_strategy = opts.drape_opts.newton_strategy;
    solver.integrator = opts.drape_opts.integrator;
    solver.damping = opts.drape_opts.damping;
    solver.friction_rounds = opts.drape_opts.lag_rounds[1];

    solver.model.check_membrane_domains(&solver.x)?;
    solver.model.check_panel_bending_domains(&solver.x)?;

    let steps = opts.drape_opts.drape_steps.clamp(4, 24);
    let mut frames = Vec::with_capacity(steps + 1);
    frames.push(x0);
    let mut newton_iterations = 0usize;
    let mut unconverged_steps = 0usize;
    let mut max_speed_m_s = 0.0_f64;

    for _ in 0..steps {
        let rep = solver.step(opts.drape_opts.step)?;
        newton_iterations += rep.iterations;
        if !rep.converged {
            unconverged_steps += 1;
        }
        let mut step_max_speed = 0.0_f64;
        for v in &solver.v {
            let speed = v.norm();
            Refusal::finite("solver.v.norm", speed)?;
            if speed > step_max_speed {
                step_max_speed = speed;
            }
        }
        max_speed_m_s = step_max_speed;
        frames.push(solver.x.clone());
    }

    // Never stamp Evidence::Measured when draping with an RVE/reference-stockinette estimate card
    // or the unmeasured reference hysteresis bank (`C011` honesty boundary).
    let effective_evidence = Evidence::Estimate;
    let assessment = solver.assess_knit_shell(
        pattern_sha256,
        opts.material_sha256.clone(),
        Some(&card),
        effective_evidence,
    )?;

    Ok(KnitDrapeResult {
        solver,
        triangles: welded_tris,
        assessment,
        frames,
        steps_run: steps,
        newton_iterations,
        unconverged_steps,
        max_speed_m_s,
        settled: unconverged_steps == 0,
        is_tubular,
        course_range: [c_min, c_max],
        wale_range: [w_min, w_max],
    })
}

/// Audits a named local garment zone on a converged [`Solver`] via [`audit_local_knit_patch`] (`--audit-patch <zone>`).
///
/// # Errors
/// Invalid zone parameters, empty knit triangles, non-finite solver state, or jammed local patch.
pub fn audit_solver_knit_patch(
    solver: &Solver,
    zone: &str,
    card: Option<&RveHomogenizedKnitCard>,
    wale_pitch_m: f64,
) -> Result<KnitPatchAuditReport, Refusal> {
    if zone.trim().is_empty() {
        return Err(Refusal::new(
            RefusalCode::InvalidDocument,
            "--audit-patch requires a non-empty zone name (e.g. selvedge, collar, hem, armhole)",
        ));
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
    let has_any_knit =
        (0..solver.model.membranes.len()).any(|i| solver.model.coupled_knit_membrane(i).is_some());
    if !has_any_knit && card.is_none() {
        return Err(Refusal::new(
            RefusalCode::InvalidDocument,
            "--audit-patch requires a knit document (`ftx-knit/1`) or coupled knit material pattern",
        ));
    }

    let effective_card = card
        .cloned()
        .unwrap_or_else(|| default_reference_rve_card(Evidence::Estimate));
    effective_card.validate()?;

    // Identify boundary triangles deterministically for edge/selvedge/hem zones.
    let mut edge_counts: BTreeMap<(usize, usize), usize> = BTreeMap::new();
    for e in &solver.model.membranes {
        for k in 0..3 {
            let (a, b) = (e.v[k], e.v[(k + 1) % 3]);
            *edge_counts.entry((a.min(b), a.max(b))).or_insert(0) += 1;
        }
    }
    let mut z_min = f64::INFINITY;
    let mut z_max = f64::NEG_INFINITY;
    for p in &solver.x {
        Refusal::finite("solver.x.x", p.x)?;
        Refusal::finite("solver.x.y", p.y)?;
        Refusal::finite("solver.x.z", p.z)?;
        z_min = z_min.min(p.z);
        z_max = z_max.max(p.z);
    }
    let z_span = (z_max - z_min).max(1e-4);

    let zone_lc = zone.trim().to_ascii_lowercase();
    let mut selected: Vec<usize> = Vec::new();
    for (ti, e) in solver.model.membranes.iter().enumerate() {
        let z_tri = (solver.x[e.v[0]].z + solver.x[e.v[1]].z + solver.x[e.v[2]].z) / 3.0;
        let z_rel = (z_tri - z_min) / z_span;
        let is_boundary = (0..3).any(|k| {
            let (a, b) = (e.v[k], e.v[(k + 1) % 3]);
            edge_counts.get(&(a.min(b), a.max(b))).copied().unwrap_or(0) == 1
        });
        let include = if zone_lc.contains("selvedge") || zone_lc.contains("edge") {
            is_boundary || z_rel <= 0.25 || z_rel >= 0.75
        } else if zone_lc.contains("hem") || zone_lc.contains("cuff") {
            z_rel <= 0.25
        } else if zone_lc.contains("collar") || zone_lc.contains("neck") {
            z_rel >= 0.75
        } else if zone_lc.contains("armhole") || zone_lc.contains("peak") {
            z_rel >= 0.55
        } else {
            true
        };
        if include {
            selected.push(ti);
        }
    }
    if selected.is_empty() {
        selected.extend(0..solver.model.membranes.len());
    }

    let selected_set: BTreeSet<usize> = selected.iter().copied().collect();
    let mut mean_strain = [0.0_f64; 3];
    let mut bb_min = [f64::INFINITY; 3];
    let mut bb_max = [f64::NEG_INFINITY; 3];
    for &ti in &selected {
        let e = &solver.model.membranes[ti];
        let s = e.strain(&solver.x);
        for k in 0..3 {
            mean_strain[k] += s[k];
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
    let n_sel = selected.len() as f64;
    for s in &mut mean_strain {
        *s /= n_sel;
    }

    let mut mean_curvature = [0.0_f64; 3];
    let mut curv_count = 0usize;
    let patch_tris = solver.model.panel_bending_triangles();
    for (pi, patch) in solver.model.panel_bending_patches().iter().enumerate() {
        let tri_idx = patch_tris.get(pi).copied().unwrap_or(pi);
        if !selected_set.contains(&tri_idx) {
            continue;
        }
        let kp = if solver.model.panel_knit_bending(pi).is_some() {
            patch.physical_curvature(&solver.x)?
        } else if patch.tensor().uses_normal() {
            patch.normal_curvature(&solver.x)?
        } else {
            continue;
        };
        for k in 0..3 {
            mean_curvature[k] += kp[k];
        }
        curv_count += 1;
    }
    if curv_count > 0 {
        for c in &mut mean_curvature {
            *c /= curv_count as f64;
        }
    }

    let dx = bb_max[0] - bb_min[0];
    let dy = bb_max[1] - bb_min[1];
    let dz = bb_max[2] - bb_min[2];
    let patch_span_m = det::sqrt(dx * dx + dy * dy + dz * dz).max(0.01);

    let input = LocalAuditPatchInput {
        macro_strain: mean_strain,
        macro_curvature: mean_curvature,
        patch_span_m,
    };
    let audited = audit_local_knit_patch(&effective_card, &input, wale_pitch_m)?;

    Ok(KnitPatchAuditReport {
        zone: zone.to_string(),
        selected_triangles: selected.len(),
        macro_strain: mean_strain,
        macro_curvature: mean_curvature,
        patch_span_m,
        wale_pitch_m,
        energy_density_j_m2: audited.energy_density_j_m2,
        estimated_peak_yarn_tension_n: audited.estimated_peak_yarn_tension_n,
        pore_open_fraction: audited.pore_open_fraction,
        jamming_margin: audited.jamming_margin,
        evidence: effective_card.evidence,
        hardware_execution_authorized: false,
        physical_knitting_validated: false,
        fit_claimed: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::PartFrames;
    use crate::start::PartFrame;
    use ftx_pattern::knit::{
        CarrierId, GaugeSpec, MachineProfile, ParametricPanelSpec, RaglanPulloverSpec,
        draft_seamless_raglan_tube, draft_shaped_panel,
    };

    struct EmptyFrames;

    impl PartFrames for EmptyFrames {
        fn frame(&self, _part: &str) -> Option<PartFrame> {
            None
        }

        fn landmark_h(&self, _part: &str, _name: &str) -> Option<f64> {
            None
        }

        fn section_radius(&self, _part: &str, _h: f64) -> f64 {
            0.0
        }
    }

    fn cylinder_body(radius_m: f64, height_m: f64) -> (Vec<Vec3>, Vec<[usize; 3]>) {
        let rings = 6usize;
        let segs = 12usize;
        let mut verts = Vec::new();
        let mut tris = Vec::new();
        for r in 0..=rings {
            let z = (r as f64 / rings as f64) * height_m;
            for s in 0..segs {
                let theta = std::f64::consts::TAU * (s as f64) / (segs as f64);
                let (st, ct) = det::sin_cos(theta);
                verts.push(Vec3::new(radius_m * ct, radius_m * st, z));
            }
        }
        for r in 0..rings {
            for s in 0..segs {
                let s1 = (s + 1) % segs;
                let a = r * segs + s;
                let b = r * segs + s1;
                let c = (r + 1) * segs + s1;
                let d = (r + 1) * segs + s;
                tris.push([a, b, c]);
                tris.push([a, c, d]);
            }
        }
        (verts, tris)
    }

    #[test]
    fn flat_panel_with_rib_hem_stays_flat_and_patch_audit_rejects_nan() {
        let profile = MachineProfile::kniterate_7e();
        let gauge = GaugeSpec {
            wales_per_100mm: 22.0,
            courses_per_100mm: 30.0,
            needle_pitch_mm: profile.needle_pitch_mm,
            yarn_tex: Some(200.0),
            thickness_mm: Some(2.0),
            relaxed_after_wash: true,
        };
        // Flat front panel WITH 1x1 rib hem occupies both Bed::Front and Bed::Back at alternating
        // wales, NEVER both beds at the same (course, wale) -> must NOT be doubled into a tube!
        let panel = draft_shaped_panel(
            &ParametricPanelSpec {
                name: "ribbed_flat_panel".to_string(),
                wearer_age_class: WearerAgeClass::Child3To14,
                hem_width_mm: 240.0,
                chest_width_mm: 260.0,
                length_mm: 260.0,
                armhole_depth_mm: 80.0,
                armhole_inset_total_mm: 30.0,
                neck_width_mm: 0.0,
                neck_depth_mm: 0.0,
                rib_hem_courses: 6,
                cam_setting: 7.0,
                yarn_id: "merino-200tex".to_string(),
                carrier_id: CarrierId("1".to_string()),
                has_hood_or_neck_drawstring: false,
            },
            &gauge,
            &profile,
        )
        .unwrap();

        let (verts, tris) = cylinder_body(0.08, 0.55);
        let frames = EmptyFrames;
        let body = DrapeBody {
            frames: &frames,
            collision: (&verts, &tris),
            display: (&verts, &tris),
            arm_deg: ftx_body::Pose::A_POSE_DEG,
            arm_pose: None,
        };
        let mut drape_opts = DrapeOptions::default();
        drape_opts.drape_steps = 4;
        let opts = KnitDrapeOptions {
            drape_opts,
            material_sha256: "synthetic-material-sha".to_string(),
            evidence: Evidence::Estimate,
            rve_card: None,
        };
        let mut res = drape_knit_doc(&panel, &body, &opts).unwrap();
        assert!(
            !res.is_tubular,
            "flat panel with 1x1 rib hem must NOT be falsely wrapped as a closed tube"
        );
        let audit = audit_solver_knit_patch(
            &res.solver,
            "selvedge",
            None,
            gauge.blocked_wale_pitch_mm() * 0.001,
        )
        .unwrap();
        assert!(!audit.hardware_execution_authorized);
        assert!(!audit.fit_claimed);
        assert!(audit.selected_triangles > 0);

        // Corrupting a vertex to NaN must fail closed with NonFinite rather than being swallowed.
        res.solver.x[0].z = f64::NAN;
        assert_eq!(
            audit_solver_knit_patch(
                &res.solver,
                "selvedge",
                None,
                gauge.blocked_wale_pitch_mm() * 0.001,
            )
            .unwrap_err()
            .code,
            RefusalCode::NonFinite
        );

        // Contrast: a true circular raglan tube (both beds occupied at the same (course, wale))
        // wraps around the cylinder as a closed tubular shell.
        let tube = draft_seamless_raglan_tube(
            &RaglanPulloverSpec {
                name: "circular_raglan_tube".to_string(),
                wearer_age_class: WearerAgeClass::Child3To14,
                neck_girth_mm: 320.0,
                chest_girth_mm: 560.0,
                yoke_depth_mm: 100.0,
                body_length_below_arm_mm: 160.0,
                collar_rib_courses: 0,
                hem_rib_courses: 0,
                cam_setting: 7.0,
                yarn_id: "merino-200tex".to_string(),
                carrier_id: CarrierId("1".to_string()),
                has_hood_or_neck_drawstring: false,
            },
            &gauge,
            &profile,
        )
        .unwrap();
        let tube_res = drape_knit_doc(&tube, &body, &opts).unwrap();
        assert!(
            tube_res.is_tubular,
            "true circular double-bed tube must drape as a closed tubular shell"
        );
    }

    #[test]
    fn drape_knit_doc_keeps_estimate_evidence_and_preserves_tall_panel_rest_height() {
        let profile = MachineProfile::kniterate_7e();
        let gauge = GaugeSpec {
            wales_per_100mm: 20.0,
            courses_per_100mm: 25.0,
            needle_pitch_mm: profile.needle_pitch_mm,
            yarn_tex: Some(200.0),
            thickness_mm: Some(2.0),
            relaxed_after_wash: true,
        };
        // Tall 600 mm flat panel on a 550 mm body must keep its full 0.60 m initial height
        // rather than silently squashing vertically into the collider height.
        let panel = draft_shaped_panel(
            &ParametricPanelSpec {
                name: "tall_tunic_panel".to_string(),
                wearer_age_class: WearerAgeClass::Adult,
                hem_width_mm: 300.0,
                chest_width_mm: 300.0,
                length_mm: 600.0,
                armhole_depth_mm: 0.0,
                armhole_inset_total_mm: 0.0,
                neck_width_mm: 0.0,
                neck_depth_mm: 0.0,
                rib_hem_courses: 0,
                cam_setting: 7.0,
                yarn_id: "merino-200tex".to_string(),
                carrier_id: CarrierId("1".to_string()),
                has_hood_or_neck_drawstring: false,
            },
            &gauge,
            &profile,
        )
        .unwrap();

        let (verts, tris) = cylinder_body(0.08, 0.55);
        let frames = EmptyFrames;
        let body = DrapeBody {
            frames: &frames,
            collision: (&verts, &tris),
            display: (&verts, &tris),
            arm_deg: ftx_body::Pose::A_POSE_DEG,
            arm_pose: None,
        };
        let mut drape_opts = DrapeOptions::default();
        drape_opts.drape_steps = 4;
        // Even if caller passes Evidence::Measured without a measured RVE card, receipt MUST remain Estimate!
        let opts = KnitDrapeOptions {
            drape_opts,
            material_sha256: "synthetic-material-sha".to_string(),
            evidence: Evidence::Measured,
            rve_card: None,
        };
        let mut res = drape_knit_doc(&panel, &body, &opts).unwrap();
        assert_eq!(
            res.assessment.receipt.evidence,
            Evidence::Estimate,
            "unmeasured reference stockinette RVE drape must never stamp Evidence::Measured"
        );
        let z0_min = res.frames[0]
            .iter()
            .map(|p| p.z)
            .fold(f64::INFINITY, f64::min);
        let z0_max = res.frames[0]
            .iter()
            .map(|p| p.z)
            .fold(f64::NEG_INFINITY, f64::max);
        let expected_courses = panel
            .stitches
            .iter()
            .map(|s| s.course)
            .max()
            .unwrap()
            .saturating_add(1);
        let expected_h = f64::from(expected_courses) * gauge.blocked_course_height_mm() * 0.001;
        assert!(
            ((z0_max - z0_min) - expected_h).abs() < 1e-9,
            "initial panel height {} must match rest height {}",
            z0_max - z0_min,
            expected_h
        );

        // Truncated vertex buffer must fail closed with OutOfRange instead of panicking.
        res.solver.x.pop();
        assert_eq!(
            audit_solver_knit_patch(
                &res.solver,
                "selvedge",
                None,
                gauge.blocked_wale_pitch_mm() * 0.001,
            )
            .unwrap_err()
            .code,
            RefusalCode::OutOfRange
        );
    }
}

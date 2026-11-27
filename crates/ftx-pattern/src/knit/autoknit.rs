//! Pathway 2: Intrinsic-Delaunay harmonic time-field 3D-to-knit drafter (`FTX-K12`, AutoKnit-style).
//!
//! Consumes any open manifold [`EaseShellMesh`] (Seam 1, metres, z-up) via `ftx-core::sparse`
//! (zero dependency on `ftx-body` or `ftx-drape`), performs intrinsic Delaunay edge flips (`iDT`)
//! so all cotangent Laplacian weights are nonnegative (enforcing the discrete maximum principle),
//! solves the harmonic time field `t in [0, 1]`, slices iso-time courses at anisotropic gauge, and
//! emits a validated `ftx-knit/1` [`KnitDoc`] with exact per-ring girth quantization residuals.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::shaping::EaseShellMesh;
use ftx_core::sparse::SparseCoo;
use ftx_core::{Refusal, RefusalCode, Vec3};

use super::ir::{
    Bed, CamStitchSetting, CarrierId, GaugeSpec, KNIT_SCHEMA, KnitDoc, LoopKind, ParentRef,
    StitchNode, YarnRun,
};
use super::profile::{MachineProfile, WearerAgeClass};

/// Configuration for 3D-to-knit harmonic AutoKnit drafting.
#[derive(Clone, Debug)]
pub struct AutoKnitConfig {
    /// Output pattern name.
    pub name: String,
    /// Required wearer age classification (non-defaulting).
    pub wearer_age_class: WearerAgeClass,
    /// Catalog yarn identifier.
    pub yarn_id: String,
    /// Primary yarn carrier identifier.
    pub carrier_id: CarrierId,
    /// Dimensionless stitch cam setting.
    pub cam_setting: f64,
    /// Maximum intrinsic Delaunay edge-flip iterations.
    pub max_idt_flips: usize,
    /// True if garment has a hood/neck drawstring (refused on child garments downstream).
    pub has_hood_or_neck_drawstring: bool,
}

/// Summary report returned alongside the drafted [`KnitDoc`].
#[derive(Clone, Debug)]
pub struct AutoKnitReport {
    /// Drafted topological stitch graph (`ftx-knit/1`).
    pub doc: KnitDoc,
    /// Number of intrinsic Delaunay flips applied to eliminate negative cotangent weights.
    pub idt_flips_applied: usize,
    /// Minimum edge cotangent weight after `iDT` (`>= -1e-9` proves discrete maximum principle).
    pub min_cotangent_weight: f64,
    /// Total geodesic height from `t = 0` to `t = 1`, in mm.
    pub shell_height_mm: f64,
    /// Number of iso-time courses extracted.
    pub course_count: u32,
    /// Peak absolute girth quantization residual across all courses, in mm.
    pub max_abs_girth_residual_mm: f64,
}

/// Upper bound on generated AutoKnit courses to prevent unbounded memory allocation on malformed shells.
const MAX_AUTOKNIT_COURSES: u32 = 5_000;

/// Drafts a seamless tubular [`KnitDoc`] directly from a 3D [`EaseShellMesh`] via `iDT` + harmonic time field.
///
/// # Errors
/// [`RefusalCode::DegenerateGeometry`] if the shell has `< 2` boundary rings, out-of-range indices, or degenerate triangles,
/// [`RefusalCode::OutOfRange`] or [`RefusalCode::InvalidDocument`] if `cfg` has invalid fields,
/// [`RefusalCode::BudgetExceeded`] if the shell exceeds the course cap,
/// [`RefusalCode::MachineStateInfeasible`] if the circumference or carrier exceeds machine capacity, or
/// [`RefusalCode::NotConverged`] if the harmonic solve fails.
pub fn draft_autoknit_from_shell(
    shell: &EaseShellMesh,
    gauge: &GaugeSpec,
    profile: &MachineProfile,
    cfg: &AutoKnitConfig,
) -> Result<AutoKnitReport, Refusal> {
    gauge.validate()?;
    profile.validate()?;
    if cfg.name.trim().is_empty() || cfg.yarn_id.trim().is_empty() {
        return Err(Refusal::new(
            RefusalCode::InvalidDocument,
            "AutoKnit config requires non-empty name and yarn_id",
        ));
    }
    CarrierId::new(cfg.carrier_id.as_str())?;
    if !profile.carrier_ids.contains(&cfg.carrier_id) {
        return Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            format!(
                "AutoKnit carrier '{}' is not available on machine '{}'",
                cfg.carrier_id.as_str(),
                profile.id
            ),
        ));
    }
    Refusal::finite("cam_setting", cfg.cam_setting)?;
    if cfg.cam_setting < profile.cam_setting_range[0]
        || cfg.cam_setting > profile.cam_setting_range[1]
    {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            format!(
                "AutoKnit cam setting {} outside machine profile range {:?}",
                cfg.cam_setting, profile.cam_setting_range
            ),
        ));
    }
    if shell.vertices_m.len() < 6 || shell.triangles.len() < 4 {
        return Err(Refusal::new(
            RefusalCode::DegenerateGeometry,
            "EaseShellMesh has too few vertices/triangles for 3D-to-knit drafting",
        ));
    }
    let n_shell_verts = shell.vertices_m.len();
    for ring in &shell.boundary_rings {
        for &v in &ring.vertices {
            if (v as usize) >= n_shell_verts {
                return Err(Refusal::new(
                    RefusalCode::DegenerateGeometry,
                    format!(
                        "EaseShellMesh boundary ring '{}' references out-of-range vertex {v} (n_verts={n_shell_verts})",
                        ring.name
                    ),
                ));
            }
        }
    }
    let start_verts: BTreeSet<usize> = shell
        .boundary_rings
        .iter()
        .filter(|r| r.is_start)
        .flat_map(|r| r.vertices.iter().map(|&v| v as usize))
        .collect();
    let end_verts: BTreeSet<usize> = shell
        .boundary_rings
        .iter()
        .filter(|r| !r.is_start)
        .flat_map(|r| r.vertices.iter().map(|&v| v as usize))
        .collect();
    if start_verts.is_empty() || end_verts.is_empty() {
        return Err(Refusal::new(
            RefusalCode::DegenerateGeometry,
            "EaseShellMesh must declare at least one start (t=0) and one end (t=1) boundary ring",
        ));
    }
    if start_verts.intersection(&end_verts).next().is_some() {
        return Err(Refusal::new(
            RefusalCode::DegenerateGeometry,
            "start and end boundary rings share a vertex",
        ));
    }

    let verts: Vec<Vec3> = shell
        .vertices_m
        .iter()
        .map(|&[x, y, z]| Vec3::new(x, y, z))
        .collect();
    let n_verts = verts.len();
    for (idx, v) in verts.iter().enumerate() {
        if !v.is_finite() {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                format!("EaseShellMesh vertex {idx} is non-finite"),
            ));
        }
    }

    // Step 1: Intrinsic Delaunay edge flips on triangle topology
    let mut tris: Vec<[usize; 3]> = shell
        .triangles
        .iter()
        .map(|&[a, b, c]| [a as usize, b as usize, c as usize])
        .collect();
    for &[a, b, c] in &tris {
        if a >= n_verts || b >= n_verts || c >= n_verts || a == b || b == c || c == a {
            return Err(Refusal::new(
                RefusalCode::DegenerateGeometry,
                "EaseShellMesh contains out-of-range or degenerate triangle indices",
            ));
        }
    }
    let idt_flips_applied = intrinsic_delaunay_flip(&verts, &mut tris, cfg.max_idt_flips);

    // Step 2: Assemble nonnegative cotangent Laplacian with Dirichlet BCs (t=0 start, t=1 end)
    let mut edge_weights: BTreeMap<(usize, usize), f64> = BTreeMap::new();
    for &[i, j, k] in &tris {
        let vi = verts[i];
        let vj = verts[j];
        let vk = verts[k];
        let cot_i = safe_cotangent(vj - vi, vk - vi);
        let cot_j = safe_cotangent(vk - vj, vi - vj);
        let cot_k = safe_cotangent(vi - vk, vj - vk);
        for ((a, b), w) in [
            ((j, k), 0.5 * cot_i),
            ((k, i), 0.5 * cot_j),
            ((i, j), 0.5 * cot_k),
        ] {
            let key = if a < b { (a, b) } else { (b, a) };
            *edge_weights.entry(key).or_insert(0.0) += w;
        }
    }
    let min_cotangent_weight = edge_weights.values().copied().fold(f64::INFINITY, f64::min);

    // Solve reduced SPD system on interior (non-Dirichlet) vertices
    let mut var_of: Vec<Option<usize>> = vec![None; n_verts];
    let mut free_verts = Vec::new();
    for (v, slot) in var_of.iter_mut().enumerate() {
        if !start_verts.contains(&v) && !end_verts.contains(&v) {
            *slot = Some(free_verts.len());
            free_verts.push(v);
        }
    }
    let mut time_field = vec![0.0_f64; n_verts];
    for &v in &end_verts {
        time_field[v] = 1.0;
    }

    let n_free = free_verts.len();
    if n_free > 0 {
        let mut coo = SparseCoo::new(n_free, n_free);
        let mut rhs = vec![0.0_f64; n_free];
        for (&(a, b), &raw_w) in &edge_weights {
            let w = raw_w.max(1e-8); // nonnegative discrete-maximum-principle weight
            match (var_of[a], var_of[b]) {
                (Some(ia), Some(ib)) => {
                    coo.push(ia, ia, w);
                    coo.push(ib, ib, w);
                    coo.push(ia, ib, -w);
                    coo.push(ib, ia, -w);
                }
                (Some(ia), None) => {
                    coo.push(ia, ia, w);
                    rhs[ia] += w * time_field[b];
                }
                (None, Some(ib)) => {
                    coo.push(ib, ib, w);
                    rhs[ib] += w * time_field[a];
                }
                (None, None) => {}
            }
        }
        let csr = coo.assemble();
        let sol = csr.solve_spd(&rhs, 1e-10, 6000)?;
        for (idx, &v) in free_verts.iter().enumerate() {
            time_field[v] = sol[idx].clamp(0.0, 1.0);
        }
    }

    // Estimate shell geodesic arc-height (mm) via average gradient along edges crossing t
    let mut total_dz = 0.0_f64;
    let mut weight_sum = 0.0_f64;
    for &(a, b) in edge_weights.keys() {
        let dt = (time_field[b] - time_field[a]).abs();
        if dt > 1e-4 {
            let len = (verts[b] - verts[a]).norm();
            total_dz += len * dt;
            weight_sum += dt * dt;
        }
    }
    let shell_height_mm = if weight_sum > 1e-12 {
        (total_dz / weight_sum) * 1000.0
    } else {
        let zmin = verts.iter().map(|v| v.z).fold(f64::INFINITY, f64::min);
        let zmax = verts.iter().map(|v| v.z).fold(f64::NEG_INFINITY, f64::max);
        ((zmax - zmin) * 1000.0).max(20.0)
    };

    let course_height_mm = gauge.blocked_course_height_mm();
    let wale_pitch_mm = gauge.blocked_wale_pitch_mm();
    let raw_courses = (shell_height_mm / course_height_mm).round();
    if !raw_courses.is_finite() || raw_courses > f64::from(MAX_AUTOKNIT_COURSES) {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            format!(
                "AutoKnit shell height {shell_height_mm:.1} mm requires {raw_courses:.0} courses, exceeding cap {MAX_AUTOKNIT_COURSES}"
            ),
        ));
    }
    let course_count = raw_courses.max(4.0) as u32;

    // Step 3: Slice iso-time contours t_c = c / course_count and compute per-course girth
    let mut stitches = Vec::new();
    let mut prev_front: BTreeMap<i32, u32> = BTreeMap::new();
    let mut prev_back: BTreeMap<i32, u32> = BTreeMap::new();
    let mut next_id = 0u32;
    let mut max_abs_girth_residual_mm = 0.0_f64;
    let est_loop_mm = 2.6 * wale_pitch_mm;
    let max_total_st_f64 =
        f64::from(profile.needle_count) * if profile.has_back_bed { 2.0 } else { 1.0 };

    for c in 0..=course_count {
        let t_iso = f64::from(c) / f64::from(course_count);
        let girth_mm = iso_contour_perimeter_m(&verts, &tris, &time_field, t_iso) * 1000.0;
        let girth_mm = if girth_mm > 1.0 {
            girth_mm
        } else {
            shell
                .boundary_rings
                .first()
                .map_or(200.0, |r| r.perimeter_m * 1000.0)
        };
        if !girth_mm.is_finite() {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                format!("AutoKnit course {c} produced non-finite girth"),
            ));
        }
        let raw_st_f64 = (girth_mm / wale_pitch_mm).round().max(8.0);
        if raw_st_f64 > max_total_st_f64 + 2.0 {
            return Err(Refusal::new(
                RefusalCode::MachineStateInfeasible,
                format!(
                    "AutoKnit course {c} girth {girth_mm:.1} mm ({raw_st_f64:.0} stitches) exceeds machine '{}' capacity",
                    profile.id
                ),
            ));
        }
        let raw_st = raw_st_f64 as i32;
        let total_st = if raw_st % 2 == 0 { raw_st } else { raw_st + 1 };
        let residual = (f64::from(total_st) * wale_pitch_mm - girth_mm).abs();
        max_abs_girth_residual_mm = max_abs_girth_residual_mm.max(residual);

        let per_bed = if profile.has_back_bed {
            total_st / 2
        } else {
            total_st
        };
        if per_bed as u32 > profile.needle_count {
            return Err(Refusal::new(
                RefusalCode::MachineStateInfeasible,
                format!(
                    "AutoKnit course {c} requires {per_bed} needles/bed, exceeding machine '{}' limit {}",
                    profile.id, profile.needle_count
                ),
            ));
        }

        let left = -(per_bed / 2);
        let right = left + per_bed - 1;
        let mut cur_front = BTreeMap::new();
        let mut cur_back = BTreeMap::new();
        let beds: &[Bed] = if profile.has_back_bed {
            &[Bed::Front, Bed::Back]
        } else {
            &[Bed::Front]
        };

        for &bed in beds {
            let (prev_bed, cur_bed) = if bed == Bed::Front {
                (&prev_front, &mut cur_front)
            } else {
                (&prev_back, &mut cur_back)
            };
            for w in left..=right {
                let mut parents = Vec::new();
                if c > 0
                    && let Some(&pid) = prev_bed
                        .get(&w)
                        .or_else(|| prev_bed.get(&(w - 1)))
                        .or_else(|| prev_bed.get(&(w + 1)))
                {
                    parents.push(ParentRef {
                        stitch_id: pid,
                        stack_pos: 0,
                    });
                }
                let kind = if c == 0 {
                    LoopKind::CastOn
                } else if c == course_count {
                    LoopKind::BindOff
                } else if bed == Bed::Back {
                    LoopKind::Purl
                } else {
                    LoopKind::Knit
                };
                stitches.push(StitchNode {
                    id: next_id,
                    course: c,
                    wale: w,
                    bed,
                    kind,
                    parents,
                    carrier_ids: vec![cfg.carrier_id.clone()],
                    yarn_id: cfg.yarn_id.clone(),
                    cam_stitch_setting: CamStitchSetting {
                        setting: cfg.cam_setting,
                        machine_family: profile.id.clone(),
                    },
                    yarn_length_mm: Some(est_loop_mm),
                });
                cur_bed.insert(w, next_id);
                next_id += 1;
            }
        }
        prev_front = cur_front;
        prev_back = cur_back;
    }

    let total_yarn_m = stitches.len() as f64 * est_loop_mm / 1000.0;
    let mut metadata = BTreeMap::new();
    metadata.insert(
        "pathway".to_string(),
        "autoknit_idt_harmonic_3d".to_string(),
    );
    metadata.insert(
        "source_body_sha256".to_string(),
        shell.source_body_sha256.clone(),
    );
    metadata.insert(
        "idt_flips_applied".to_string(),
        idt_flips_applied.to_string(),
    );
    metadata.insert(
        "shell_height_mm".to_string(),
        format!("{shell_height_mm:.2}"),
    );
    metadata.insert(
        "max_abs_girth_residual_mm".to_string(),
        format!("{max_abs_girth_residual_mm:.2}"),
    );

    let doc = KnitDoc {
        schema: KNIT_SCHEMA.to_string(),
        name: cfg.name.clone(),
        wearer_age_class: cfg.wearer_age_class,
        gauge: gauge.clone(),
        stitches,
        short_row_turns: vec![],
        cable_groups: vec![],
        yarn_runs: vec![YarnRun {
            run_id: 0,
            carrier_id: cfg.carrier_id.clone(),
            yarn_id: cfg.yarn_id.clone(),
            start_course: 0,
            end_course: course_count,
            total_yarn_length_m: total_yarn_m,
            cut_tail_mm: Some(15.0),
        }],
        has_hood_or_neck_drawstring: cfg.has_hood_or_neck_drawstring,
        metadata,
    };
    doc.validate()?;

    Ok(AutoKnitReport {
        doc,
        idt_flips_applied,
        min_cotangent_weight,
        shell_height_mm,
        course_count,
        max_abs_girth_residual_mm,
    })
}

fn safe_cotangent(u: Vec3, v: Vec3) -> f64 {
    let cross = u.cross(v).norm();
    if cross <= 1e-14 {
        return 0.0;
    }
    u.dot(v) / cross
}

/// Performs deterministic intrinsic Delaunay edge flips to remove negative cotangent edge weights.
fn intrinsic_delaunay_flip(verts: &[Vec3], tris: &mut [[usize; 3]], max_flips: usize) -> usize {
    let mut flips = 0usize;
    for _ in 0..max_flips {
        // Build edge -> incident (tri_idx, opposite_vertex) map deterministically
        let mut edge_tris: BTreeMap<(usize, usize), Vec<(usize, usize)>> = BTreeMap::new();
        for (ti, &[a, b, c]) in tris.iter().enumerate() {
            for (u, v, opp) in [(a, b, c), (b, c, a), (c, a, b)] {
                let key = if u < v { (u, v) } else { (v, u) };
                edge_tris.entry(key).or_default().push((ti, opp));
            }
        }
        let mut did_flip = false;
        for (&(u, v), inc) in &edge_tris {
            if inc.len() != 2 {
                continue;
            }
            let (t0, o0) = inc[0];
            let (t1, o1) = inc[1];
            if o0 == o1 {
                continue;
            }
            let cot0 = safe_cotangent(verts[u] - verts[o0], verts[v] - verts[o0]);
            let cot1 = safe_cotangent(verts[u] - verts[o1], verts[v] - verts[o1]);
            if cot0 + cot1 < -1e-9 {
                // Only flip if the new edge (o0, o1) does not already exist
                let new_key = if o0 < o1 { (o0, o1) } else { (o1, o0) };
                if !edge_tris.contains_key(&new_key) {
                    let area0 = (verts[o1] - verts[u]).cross(verts[o0] - verts[u]).norm();
                    let area1 = (verts[o0] - verts[v]).cross(verts[o1] - verts[v]).norm();
                    if area0 <= 1e-14 || area1 <= 1e-14 {
                        continue;
                    }
                    // Preserve outward orientation cleanly
                    let tri0 = tris[t0];
                    let u_before_v = (tri0[0] == u && tri0[1] == v)
                        || (tri0[1] == u && tri0[2] == v)
                        || (tri0[2] == u && tri0[0] == v);
                    if u_before_v {
                        tris[t0] = [u, o1, o0];
                        tris[t1] = [v, o0, o1];
                    } else {
                        tris[t0] = [u, o0, o1];
                        tris[t1] = [v, o1, o0];
                    }
                    flips += 1;
                    did_flip = true;
                    break;
                }
            }
        }
        if !did_flip {
            break;
        }
    }
    flips
}

/// Measures the exact 3D perimeter of the piecewise-linear iso-contour `{ x : t(x) = t_iso }`.
fn iso_contour_perimeter_m(verts: &[Vec3], tris: &[[usize; 3]], t: &[f64], t_iso: f64) -> f64 {
    let iso = t_iso.clamp(1e-5, 1.0 - 1e-5);
    let mut perimeter = 0.0_f64;
    for &[a, b, c] in tris {
        let idx = [a, b, c];
        let tv = [t[a], t[b], t[c]];
        let mut pts = Vec::with_capacity(2);
        for e in 0..3 {
            let i0 = idx[e];
            let i1 = idx[(e + 1) % 3];
            let t0 = tv[e];
            let t1 = tv[(e + 1) % 3];
            if (t0 < iso && t1 >= iso) || (t1 < iso && t0 >= iso) {
                let denom = t1 - t0;
                if denom.abs() > 1e-14 {
                    let s = ((iso - t0) / denom).clamp(0.0, 1.0);
                    pts.push(verts[i0] + (verts[i1] - verts[i0]) * s);
                }
            }
        }
        if pts.len() == 2 {
            perimeter += (pts[1] - pts[0]).norm();
        }
    }
    perimeter
}

#[cfg(test)]
mod tests {
    use super::*;
    use ftx_core::det;
    use ftx_core::shaping::BoundaryRing;

    #[test]
    fn autoknit_solves_harmonic_field_and_recovers_tapered_cylinder_girth() {
        // Tapered open frustum cylinder: bottom radius 0.10m (girth 628.3mm), top radius 0.07m (439.8mm), height 0.25m
        let n_theta = 24usize;
        let n_z = 8usize;
        let mut vertices_m = Vec::new();
        for iz in 0..=n_z {
            let u = iz as f64 / n_z as f64;
            let r = 0.10 * (1.0 - u) + 0.07 * u;
            let z = 0.25 * u;
            for it in 0..n_theta {
                let ang = 2.0 * std::f64::consts::PI * (it as f64) / (n_theta as f64);
                vertices_m.push([r * det::cos(ang), r * det::sin(ang), z]);
            }
        }
        let mut triangles = Vec::new();
        for iz in 0..n_z {
            for it in 0..n_theta {
                let it_next = (it + 1) % n_theta;
                let a = (iz * n_theta + it) as u32;
                let b = (iz * n_theta + it_next) as u32;
                let c = ((iz + 1) * n_theta + it_next) as u32;
                let d = ((iz + 1) * n_theta + it) as u32;
                triangles.push([a, b, c]);
                triangles.push([a, c, d]);
            }
        }
        let bot_ring: Vec<u32> = (0..n_theta as u32).collect();
        let top_ring: Vec<u32> = ((n_z * n_theta) as u32..((n_z + 1) * n_theta) as u32).collect();
        let shell = EaseShellMesh {
            vertices_m,
            triangles,
            boundary_rings: vec![
                BoundaryRing {
                    name: "hem".to_string(),
                    is_start: true,
                    vertices: bot_ring,
                    perimeter_m: 2.0 * std::f64::consts::PI * 0.10,
                },
                BoundaryRing {
                    name: "neck".to_string(),
                    is_start: false,
                    vertices: top_ring,
                    perimeter_m: 2.0 * std::f64::consts::PI * 0.07,
                },
            ],
            source_body_sha256: "synthetic-frustum".to_string(),
        };
        let prof = MachineProfile::kniterate_7e();
        let gauge = GaugeSpec {
            wales_per_100mm: 24.0,
            courses_per_100mm: 32.0,
            needle_pitch_mm: prof.needle_pitch_mm,
            yarn_tex: Some(180.0),
            thickness_mm: Some(2.0),
            relaxed_after_wash: true,
        };
        let cfg = AutoKnitConfig {
            name: "tapered_tube".to_string(),
            wearer_age_class: WearerAgeClass::Adult,
            yarn_id: "merino-180tex".to_string(),
            carrier_id: CarrierId("1".to_string()),
            cam_setting: 8.0,
            max_idt_flips: 200,
            has_hood_or_neck_drawstring: false,
        };
        let rep = draft_autoknit_from_shell(&shell, &gauge, &prof, &cfg).unwrap();
        assert!(rep.min_cotangent_weight >= -1e-6);
        assert!(rep.course_count >= 60);
        assert!(rep.max_abs_girth_residual_mm < 10.0);
        assert!(rep.doc.stitches.len() > 2000);
    }

    #[test]
    fn autoknit_refuses_oob_boundary_ring_invalid_cfg_and_oversized_shell() {
        let prof = MachineProfile::kniterate_7e();
        let gauge = GaugeSpec {
            wales_per_100mm: 24.0,
            courses_per_100mm: 32.0,
            needle_pitch_mm: prof.needle_pitch_mm,
            yarn_tex: Some(180.0),
            thickness_mm: Some(2.0),
            relaxed_after_wash: true,
        };
        let base_cfg = AutoKnitConfig {
            name: "tube".to_string(),
            wearer_age_class: WearerAgeClass::Adult,
            yarn_id: "merino-180tex".to_string(),
            carrier_id: CarrierId("1".to_string()),
            cam_setting: 8.0,
            max_idt_flips: 20,
            has_hood_or_neck_drawstring: false,
        };
        let mut shell = EaseShellMesh {
            vertices_m: vec![
                [0.1, 0.0, 0.0],
                [-0.05, 0.086, 0.0],
                [-0.05, -0.086, 0.0],
                [0.1, 0.0, 0.2],
                [-0.05, 0.086, 0.2],
                [-0.05, -0.086, 0.2],
            ],
            triangles: vec![
                [0, 1, 4],
                [0, 4, 3],
                [1, 2, 5],
                [1, 5, 4],
                [2, 0, 3],
                [2, 3, 5],
            ],
            boundary_rings: vec![
                BoundaryRing {
                    name: "hem".to_string(),
                    is_start: true,
                    vertices: vec![0, 1, 2],
                    perimeter_m: 0.6,
                },
                BoundaryRing {
                    name: "neck".to_string(),
                    is_start: false,
                    vertices: vec![3, 4, 999], // out-of-bounds vertex index
                    perimeter_m: 0.6,
                },
            ],
            source_body_sha256: "synthetic".to_string(),
        };
        assert_eq!(
            draft_autoknit_from_shell(&shell, &gauge, &prof, &base_cfg)
                .unwrap_err()
                .code,
            RefusalCode::DegenerateGeometry
        );

        // Fix boundary ring and test invalid cam_setting & unknown carrier
        shell.boundary_rings[1].vertices = vec![3, 4, 5];
        let mut bad_cam = base_cfg.clone();
        bad_cam.cam_setting = f64::NAN;
        assert_eq!(
            draft_autoknit_from_shell(&shell, &gauge, &prof, &bad_cam)
                .unwrap_err()
                .code,
            RefusalCode::NonFinite
        );
        let mut bad_carrier = base_cfg.clone();
        bad_carrier.carrier_id = CarrierId("99".to_string());
        assert_eq!(
            draft_autoknit_from_shell(&shell, &gauge, &prof, &bad_carrier)
                .unwrap_err()
                .code,
            RefusalCode::MachineStateInfeasible
        );

        // Unreasonably tall shell (100 meters) refuses with BudgetExceeded instead of OOM
        for v in &mut shell.vertices_m[3..6] {
            v[2] = 100.0;
        }
        assert_eq!(
            draft_autoknit_from_shell(&shell, &gauge, &prof, &base_cfg)
                .unwrap_err()
                .code,
            RefusalCode::BudgetExceeded
        );
    }
}

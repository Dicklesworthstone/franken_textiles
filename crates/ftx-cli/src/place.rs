//! `ftx place PATTERN --measurements M --out NEW_DIR [--model PATH] [--body-fit REPORT] [--spacing-mm MM]
//! [--arm-deg DEG] [--legs standing|seated|squat] [--leg-fraction F] [--skinning lbs|dqs] [--res WxH]`: where a pattern
//! starts on the body, as pictures and numbers.
//!
//! It builds the same mesh, isometric wrap, pre-sew and relaxation as `ftx drape` (spec 04 §2–4) and
//! stops before contact admission and dynamics. It therefore answers in seconds and also shows a start
//! the solver would refuse. `wrap/` is each placed piece on its cylinder; `start/` is the pre-sewn,
//! relaxed start. Both are directories `ftx render` reads, already rendered here. A drape refused at its
//! start writes the same two stages from the pattern and body it was given ([`start_stills`]).
//!
//! No-claim boundary: nothing here is a drape, a fit or an admission. `ftx drape` remains the only
//! authority on whether a start is accepted. Lengths in the report are millimetres; OBJ files are
//! metres, z up, wearer facing −y.

use std::path::{Path, PathBuf};

use ftx_body::{BodyFrames, SkinningMode, clothing_skin, pose_deltas, skin_mesh};
use ftx_core::intersection::{indexed_triangle_pair, validate_mesh};
use ftx_core::{Refusal, RefusalCode, Vec3};
use ftx_drape::scene::{
    PartFrames, casings_at_cut_length, elastic_casings, placements, wrapped_positions,
};
use ftx_drape::start::RELAX_TARGET_P95;
use ftx_drape::start::{edge_strain, face_edges};
use ftx_drape::{GarmentMesh, NearestTree, WrapPlacement, build_garment, start_state};
use ftx_pattern::Pattern;
use serde_json::{Map, Value, json};

use super::{Args, bad_input, body_source, known_options, render, required, workspace};

/// One colour per source piece, in first-use order; a mirrored (`.L`) copy is a lighter tint of its pair.
const PIECE_COLOURS: [[f64; 3]; 10] = [
    [0.12, 0.47, 0.71],
    [1.00, 0.50, 0.05],
    [0.17, 0.63, 0.17],
    [0.84, 0.15, 0.16],
    [0.58, 0.40, 0.74],
    [0.55, 0.34, 0.29],
    [0.89, 0.47, 0.76],
    [0.09, 0.75, 0.81],
    [0.74, 0.74, 0.13],
    [0.35, 0.35, 0.35],
];
const PIECE_COLOUR_NAMES: [&str; 10] = [
    "blue", "orange", "green", "red", "purple", "brown", "pink", "cyan", "olive", "grey",
];
/// Cloth vertices inside the collision body are painted this colour in `start/` and `wrap/`.
const INSIDE_BODY: [f64; 3] = [1.0, 0.0, 1.0];
/// Vertices of start-state cloth triangles that cross another cloth triangle are painted this colour.
const CROSSING_CLOTH: [f64; 3] = [1.0, 0.86, 0.0];
/// Wrap-stage seam travel that saturates the red end of the `wrap/` strain-view ramp (m).
const GAP_RAMP_M: f64 = 0.15;
/// Overlapping-triangle budget of the drape's static body-surface audit (`ftx_drape::scene::build_scene`).
const BODY_AUDIT_PAIRS: usize = 1_000_000;
/// Edge strain that saturates the `start/` strain view. A kinematic start is far outside the ±3 %
/// ramp used for a finished drape.
const START_STRAIN_RAMP: f64 = 0.15;
const TORSO_LANDMARKS: [&str; 14] = [
    "floor",
    "ankle",
    "calf",
    "knee",
    "crotch",
    "hip",
    "waist",
    "chest",
    "across_back",
    "shoulder",
    "snp",
    "c7",
    "head_top",
    "head_centre",
];

/// One decimal place. A value that rounds to nothing is 0, not −0, so a report never prints `-0.0`.
fn round1(v: f64) -> f64 {
    let rounded = (v * 10.0).round() / 10.0;
    if rounded == 0.0 { 0.0 } else { rounded }
}

/// How far the deepest of these vertices is inside the body, in millimetres, from signed distances in
/// metres (negative inside). 0 when none is inside, never −0.
fn deepest_mm(depth: &[f64]) -> f64 {
    round1(1000.0 * depth.iter().copied().fold(0.0, f64::min).abs())
}

/// `(piece index, mirrored copy)` of each instance, pieces numbered in first-use order.
fn piece_slots(instances: &[String]) -> Vec<(usize, bool)> {
    let mut stems: Vec<&str> = Vec::new();
    instances
        .iter()
        .map(|id| {
            let (stem, left) = match id.rsplit_once('.') {
                Some((stem, "L")) => (stem, true),
                Some((stem, "R" | "C")) => (stem, false),
                _ => (id.as_str(), false),
            };
            let slot = stems.iter().position(|s| *s == stem).unwrap_or_else(|| {
                stems.push(stem);
                stems.len() - 1
            });
            (slot, left)
        })
        .collect()
}

fn slot_colour((slot, left): (usize, bool)) -> [f64; 3] {
    let c = PIECE_COLOURS[slot % PIECE_COLOURS.len()];
    if left {
        c.map(|v| v + (1.0 - v) * 0.45)
    } else {
        c
    }
}

/// Per-vertex colours: the instance colour, [`INSIDE_BODY`] where the vertex is inside the body, or
/// [`CROSSING_CLOTH`] where it belongs to a triangle that crosses another cloth triangle.
fn piece_colours(
    mesh: &GarmentMesh,
    slots: &[(usize, bool)],
    depth: &[f64],
    crossing: &[bool],
) -> Vec<[f64; 3]> {
    mesh.owner
        .iter()
        .enumerate()
        .map(|(v, &k)| {
            if depth[v] < 0.0 {
                INSIDE_BODY
            } else if crossing[v] {
                CROSSING_CLOTH
            } else {
                slot_colour(slots[k])
            }
        })
        .collect()
}

/// Every pair of start-state cloth triangles that intersect beyond a shared vertex or edge, as pairs
/// of source face indices. The test runs on the mesh `ftx drape --self-contact-mm` audits first: one
/// vertex per sewing group, collapsed triangles dropped, so a sewn seam is adjacency, not a crossing.
///
/// # Errors
/// Whatever [`validate_mesh`] refuses (a non-finite vertex or a zero-area welded triangle).
fn cloth_crossings(
    faces: &[[usize; 3]],
    groups: &[Vec<usize>],
    x: &[Vec3],
) -> Result<Vec<(usize, usize)>, Refusal> {
    let mut weld: Vec<usize> = (0..x.len()).collect();
    for group in groups {
        if let Some(&first) = group.first() {
            for &v in group {
                weld[v] = first;
            }
        }
    }
    let (mut triangles, mut source) = (Vec::new(), Vec::new());
    for (f, face) in faces.iter().enumerate() {
        let t = face.map(|v| weld[v]);
        if t[0] != t[1] && t[1] != t[2] && t[0] != t[2] {
            triangles.push(t);
            source.push(f);
        }
    }
    validate_mesh(x, &triangles)?;
    let bounds: Vec<([f64; 3], [f64; 3])> = triangles
        .iter()
        .map(|t| {
            let p = t.map(|v| [x[v].x, x[v].y, x[v].z]);
            let fold = |pick: fn(f64, f64) -> f64| {
                [0, 1, 2].map(|axis| pick(pick(p[0][axis], p[1][axis]), p[2][axis]))
            };
            (fold(f64::min), fold(f64::max))
        })
        .collect();
    // Sweep along x; exact tests only where the boxes overlap on all three axes.
    let mut order: Vec<usize> = (0..triangles.len()).collect();
    order.sort_by(|&a, &b| bounds[a].0[0].total_cmp(&bounds[b].0[0]));
    let mut pairs = Vec::new();
    for (i, &a) in order.iter().enumerate() {
        for &b in &order[i + 1..] {
            if bounds[b].0[0] > bounds[a].1[0] {
                break;
            }
            let apart = (1..3).any(|axis| {
                bounds[a].0[axis] > bounds[b].1[axis] || bounds[b].0[axis] > bounds[a].1[axis]
            });
            if !apart && indexed_triangle_pair(x, triangles[a], triangles[b]).is_some() {
                pairs.push((source[a].min(source[b]), source[a].max(source[b])));
            }
        }
    }
    pairs.sort_unstable();
    Ok(pairs)
}

/// Per-vertex colours for the wrap stage: grey where nothing is sewn, green to red by how far the
/// vertex must travel to its seam partner.
fn travel_colours(mesh: &GarmentMesh, x: &[Vec3]) -> Vec<[f64; 3]> {
    let mut travel = vec![-1.0_f64; x.len()];
    for &(a, b) in &mesh.pairs {
        let gap = (x[a] - x[b]).norm();
        travel[a] = travel[a].max(gap);
        travel[b] = travel[b].max(gap);
    }
    travel
        .into_iter()
        .map(|gap| {
            if gap < 0.0 {
                [0.72, 0.72, 0.72]
            } else {
                let t = (gap / GAP_RAMP_M).min(1.0);
                [0.15 + 0.85 * t, 0.75 * (1.0 - t), 0.12]
            }
        })
        .collect()
}

/// The mean in-plane strain of one instance in its own pattern axes, fitted by least squares to its
/// edge strains: an edge at angle φ to pattern x stretches by `εx·cos²φ + εy·sin²φ + γ·sinφ·cosφ`.
/// `None` when the edges do not span the plane.
fn mean_strain(edges: &[(f64, f64, f64)]) -> Option<[f64; 3]> {
    let (mut a, mut b) = ([[0.0_f64; 3]; 3], [0.0_f64; 3]);
    for &(dx, dy, e) in edges {
        let l2 = dx * dx + dy * dy;
        if l2 <= 0.0 {
            continue;
        }
        let basis = [dx * dx / l2, dy * dy / l2, dx * dy / l2];
        for i in 0..3 {
            b[i] += basis[i] * e;
            for j in 0..3 {
                a[i][j] += basis[i] * basis[j];
            }
        }
    }
    let det3 = |m: &[[f64; 3]; 3]| {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };
    let det = det3(&a);
    if det.abs() < 1e-9 {
        return None;
    }
    let mut out = [0.0; 3];
    for (k, slot) in out.iter_mut().enumerate() {
        let mut m = a;
        for row in 0..3 {
            m[row][k] = b[row];
        }
        *slot = det3(&m) / det;
    }
    Some(out)
}

/// Edge strain of the relaxed start: per-vertex colours on a ±[`START_STRAIN_RAMP`] ramp (blue
/// compressed, white unstrained, red stretched) and one summary per instance. This is the strain of
/// a kinematic start, not of a draped garment.
fn start_strain(mesh: &GarmentMesh, x: &[Vec3]) -> (Vec<[f64; 3]>, Vec<Value>) {
    let edges = face_edges(&mesh.faces);
    let strain = edge_strain(x, &mesh.verts2d, &edges);
    let (mut sum, mut count) = (vec![0.0; x.len()], vec![0_usize; x.len()]);
    let mut by_instance: Vec<Vec<(f64, f64, f64)>> = vec![Vec::new(); mesh.instances.len()];
    for (&(a, b), &e) in edges.iter().zip(&strain) {
        sum[a] += e;
        sum[b] += e;
        count[a] += 1;
        count[b] += 1;
        let (p, q) = (mesh.verts2d[a], mesh.verts2d[b]);
        by_instance[mesh.owner[a]].push((q.x - p.x, q.y - p.y, e));
    }
    let colours = sum
        .iter()
        .zip(&count)
        .map(|(&s, &n)| {
            let mean = if n == 0 { 0.0 } else { s / n as f64 };
            let t = (mean / START_STRAIN_RAMP).clamp(-1.0, 1.0);
            if t >= 0.0 {
                [1.0, 1.0 - t, 1.0 - t]
            } else {
                [1.0 + t, 1.0 + t, 1.0]
            }
        })
        .collect();
    let pct = |v: f64| round1(100.0 * v);
    let summaries = by_instance
        .iter()
        .map(|edges| {
            let mut sorted: Vec<f64> = edges.iter().map(|e| e.2).collect();
            sorted.sort_by(f64::total_cmp);
            let at = |q: f64| {
                sorted
                    .get(((sorted.len().saturating_sub(1)) as f64 * q).round() as usize)
                    .copied()
                    .map_or(Value::Null, |v| json!(pct(v)))
            };
            let fit = mean_strain(edges);
            json!({
                "edges": edges.len(),
                "p05_pct": at(0.05),
                "median_pct": at(0.5),
                "p95_pct": at(0.95),
                "mean_along_pattern_x_pct": fit.map(|f| pct(f[0])),
                "mean_along_pattern_y_pct": fit.map(|f| pct(f[1])),
                "mean_shear_pct": fit.map(|f| pct(f[2])),
            })
        })
        .collect();
    (colours, summaries)
}

fn instance_report(
    mesh: &GarmentMesh,
    places: &[WrapPlacement],
    slots: &[(usize, bool)],
    wrap_depth: &[f64],
    start_depth: &[f64],
    strain: &[Value],
) -> Vec<Value> {
    mesh.instances
        .iter()
        .enumerate()
        .map(|(k, id)| {
            let (lo, hi) = mesh.inst_range[k];
            let p = &places[k];
            // Angles are reported in the part frame of the right-hand copy: a mirrored instance is
            // wrapped as that copy and then reflected in x (spec 04 §3.4).
            let drafted_x = |x: f64| if p.mirrored { -x } else { x };
            let theta = |x: f64| p.theta_deg + ((drafted_x(x) - p.x0) / p.radius).to_degrees();
            let (mut t0, mut t1, mut h0, mut h1) = (
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
            );
            for v in &mesh.verts2d[lo..hi] {
                let t = theta(v.x);
                t0 = t0.min(t);
                t1 = t1.max(t);
                h0 = h0.min(p.h0 + v.y);
                h1 = h1.max(p.h0 + v.y);
            }
            let inside = |depth: &[f64]| depth[lo..hi].iter().filter(|d| **d < 0.0).count();
            let deepest = |depth: &[f64]| deepest_mm(&depth[lo..hi]);
            json!({
                "instance": id,
                "colour": PIECE_COLOUR_NAMES[slots[k].0 % PIECE_COLOUR_NAMES.len()],
                "lighter_tint": slots[k].1,
                "mirrored": p.mirrored,
                "vertices": hi - lo,
                "radius_mm": round1(p.radius),
                "anchor": {"theta_deg": p.theta_deg, "x0_mm": p.x0, "y0_height_mm": round1(p.h0)},
                "theta_deg": [round1(t0), round1(t1)],
                "height_mm": [round1(h0), round1(h1)],
                "inside_body": {
                    "wrap_vertices": inside(wrap_depth),
                    "wrap_deepest_mm": deepest(wrap_depth),
                    "start_vertices": inside(start_depth),
                    "start_deepest_mm": deepest(start_depth),
                },
                "start_edge_strain": strain[k],
            })
        })
        .collect()
}

fn seam_report(mesh: &GarmentMesh, wrapped: &[Vec3]) -> Vec<Value> {
    mesh.seams
        .iter()
        .map(|seam| {
            let gaps: Vec<f64> = seam
                .pairs
                .iter()
                .map(|&(a, b)| 1000.0 * (wrapped[a] - wrapped[b]).norm())
                .collect();
            let owner = |path: &[usize]| {
                path.first()
                    .map_or("", |&v| mesh.instances[mesh.owner[v]].as_str())
            };
            let mean = if gaps.is_empty() {
                0.0
            } else {
                gaps.iter().sum::<f64>() / gaps.len() as f64
            };
            json!({
                "seam": seam.id,
                "a": owner(&seam.side_a),
                "b": owner(&seam.side_b),
                "pairs": gaps.len(),
                "wrap_travel_mm": {
                    "mean": round1(mean),
                    "max": round1(gaps.iter().copied().fold(0.0, f64::max)),
                },
            })
        })
        .collect()
}

/// The landmark heights and section radii an author needs to choose `y0_landmark`, `theta_deg` and
/// `radius_mm`, straight from the frames the wrap uses.
fn body_report(frames: &dyn PartFrames) -> Value {
    let vector = |v: Vec3| json!([round1(v.x), round1(v.y), round1(v.z)]);
    let mut parts = Map::new();
    for (part, names) in [
        ("torso", &TORSO_LANDMARKS[..]),
        ("arm_R", &["shoulder_tip", "elbow", "wrist"][..]),
        ("arm_L", &["shoulder_tip", "elbow", "wrist"][..]),
        (
            "leg_R",
            &["waist", "hip", "crotch", "knee", "calf", "ankle"][..],
        ),
        (
            "leg_L",
            &["waist", "hip", "crotch", "knee", "calf", "ankle"][..],
        ),
    ] {
        let Some(frame) = frames.frame(part) else {
            continue;
        };
        let mut landmarks = Map::new();
        for name in names {
            if let Some(h) = frames.landmark_h(part, name) {
                landmarks.insert(
                    (*name).to_string(),
                    json!({"h_mm": round1(h), "section_radius_mm": round1(frames.section_radius(part, h))}),
                );
            }
        }
        parts.insert(
            part.to_string(),
            json!({
                "origin_mm": vector(frame.origin),
                "up": [frame.up.x, frame.up.y, frame.up.z],
                "theta_0": [frame.front.x, frame.front.y, frame.front.z],
                "theta_90": [frame.lat.x, frame.lat.y, frame.lat.z],
                "landmarks": landmarks,
            }),
        );
    }
    Value::Object(parts)
}

fn write_stage(
    dir: &Path,
    body: &[u8],
    mesh: &GarmentMesh,
    x: &[Vec3],
    pieces: &[[f64; 3]],
    second: &[[f64; 3]],
    (width, height): (usize, usize),
) -> Result<Vec<String>, Refusal> {
    std::fs::create_dir(dir).map_err(|e| workspace::io_error(dir, e))?;
    workspace::write_new(&dir.join("body.obj"), body)?;
    workspace::write_new(
        &dir.join("garment.obj"),
        super::obj(x, &mesh.faces, Some(pieces)).as_bytes(),
    )?;
    workspace::write_new(
        &dir.join("garment_strain.obj"),
        super::obj(x, &mesh.faces, Some(second)).as_bytes(),
    )?;
    let mut written = Vec::new();
    for (name, png) in render::images(dir, width, height)? {
        let path = dir.join(&name);
        workspace::write_new(&path, &png)?;
        written.push(path.display().to_string());
    }
    Ok(written)
}

fn write_refused_map(
    refusal: &mut Refusal,
    dir: &Path,
    mesh: &GarmentMesh,
    phase: &str,
    accepted_steps: usize,
    (width, height): (usize, usize),
) {
    let mut map = ftx_drape::animation::pattern_mesh_map(mesh);
    map["refused_pose"] = json!({ "phase": phase, "accepted_steps": accepted_steps, "width": width, "height": height });
    if let Err(error) = workspace::write_json(&dir.join("mesh_map.json"), &map) {
        refusal.repairs.push(format!(
            "The diagnostic source-edge map could not be saved: {}",
            error.message
        ));
    }
}

/// When a drape refuses once its start has been admitted: draws the cloth state the refusal is about under
/// `OUT/refused` (the files and stills of a `place` stage: coloured by piece, and by edge strain on the
/// start-state ramp) and puts where to look first among the refusal's repairs. That state is the last
/// accepted one when a step refuses, or the recorded frame that crosses the displayed wearer.
///
/// No claim: it is not a drape or a fit. Inside-body marking needs the collision body and is not drawn. If
/// the pictures cannot be written the refusal is returned unchanged: its pictures never replace or delay a
/// refusal.
pub(super) fn refused_stills(
    mut refusal: Refusal,
    state: Option<&ftx_drape::run::RefusedRun>,
    out: &Path,
    body_obj: &[u8],
) -> Refusal {
    let Some(state) = state else { return refusal };
    let Ok(size) = render::resolution(None) else {
        return refusal;
    };
    let dir = out.join("refused");
    let slots = piece_slots(&state.mesh.instances);
    let n = state.positions.len();
    let pieces = piece_colours(&state.mesh, &slots, &vec![0.0; n], &vec![false; n]);
    let (strain, _) = start_strain(&state.mesh, &state.positions);
    if write_stage(
        &dir,
        body_obj,
        &state.mesh,
        &state.positions,
        &pieces,
        &strain,
        size,
    )
    .is_ok()
    {
        write_refused_map(
            &mut refusal,
            &dir,
            &state.mesh,
            state.phase,
            state.accepted_steps,
            size,
        );
        refusal.repairs.insert(
            0,
            format!(
                "Look at {}/still_*.png: the cloth after {} accepted {} step(s), the state this refusal is \
                 about. It is not a drape or a fit; still_strain_* shows edge strain from blue (15 % short) \
                 through white to red (15 % stretched).",
                dir.display(),
                state.accepted_steps,
                state.phase
            ),
        );
    }
    refusal
}

/// What a start refusal tells its reader to open, with the counts behind the two warning colours taken
/// from the placement report written beside the stills.
fn start_repair(dir: &Path, placement: &Value) -> String {
    let dir = dir.display();
    let mut text = format!(
        "Look at {dir}/start/still_*.png (the pre-sewn start) and {dir}/wrap/still_*.png (each piece on \
         its cylinder), drawn as ftx place draws them."
    );
    // A body that is not admitted comes first: nothing below repairs it. The audit's own words are
    // quoted because it also refuses over its pair budget or the body's topology, not only a surface
    // that crosses itself.
    if let Some(message) = placement["body_surface"]["refusal"]["message"].as_str() {
        text.push_str(&format!(
            " The posed body itself was not admitted ({message}): change the pose first, since no \
             placement repairs that."
        ));
    }
    text.push_str(&format!(
        " Magenta marks cloth vertices inside the body ({} in the start)",
        placement["start"]["vertices_inside_body"]
            .as_u64()
            .unwrap_or(0)
    ));
    if let Some(pairs) = placement["cloth_contact"]["crossing_triangle_pairs"].as_u64() {
        text.push_str(&format!(
            "; yellow marks cloth triangles that cross other cloth ({pairs} pair(s)"
        ));
        // The report lists piece pairs with the most crossings first.
        let named: Vec<String> = placement["cloth_contact"]["by_pieces"]
            .as_array()
            .into_iter()
            .flatten()
            .take(4)
            .map(|row| {
                format!(
                    "{} × {}: {}",
                    row["a"].as_str().unwrap_or("?"),
                    row["b"].as_str().unwrap_or("?"),
                    row["triangle_pairs"]
                )
            })
            .collect();
        if !named.is_empty() {
            text.push_str(&format!(": {}", named.join("; ")));
        }
        text.push(')');
    }
    // A start is also refused with both counts at zero: the drape's admission tests thickness, the marks
    // test only penetration and crossing.
    text.push_str(&format!(
        ". A start is also refused for layers that only touch (with cloth contact on) and for cloth too \
         near the body with no vertex inside it; neither carries a mark, so go by the refusal's message \
         for those. {dir}/placement.json \
         lists each piece's angles, heights and radius. No dynamics ran: this is not a drape or a fit."
    ));
    text
}

/// When a drape refuses its start (`intersecting_start`: the posed body, cloth inside it, cloth crossing
/// cloth) there is no solver state to show. What was refused is what `ftx place` draws, so the two
/// placement stages of the drape's own pattern and body are written under `OUT/refused` and named first
/// among the repairs. If they cannot be produced the refusal is returned unchanged.
pub(super) fn start_stills(mut refusal: Refusal, staging: &Staging, out: &Path) -> Refusal {
    let dir = out.join("refused");
    let Ok(size) = render::resolution(None) else {
        return refusal;
    };
    if std::fs::create_dir(&dir).is_ok()
        && let Ok((placement, mesh)) = stages(staging, size, &dir)
    {
        write_refused_map(
            &mut refusal,
            &dir.join("start"),
            &mesh,
            "placement",
            0,
            size,
        );
        refusal.repairs.insert(0, start_repair(&dir, &placement));
    }
    refusal
}

/// What the two placement stages are drawn from: a resolved pattern, the material options that shape its
/// mesh, and a posed body. `ftx place` builds these from its command line. A drape refused at its start
/// hands over its own, so the pictures show the start the solver was given.
pub(super) struct Staging<'a> {
    /// The pattern as the solver meshes it: an outfit already merged, a fabric override already applied.
    pub pattern: &'a Pattern,
    /// How the report names that pattern: the path it was read from.
    pub label: &'a str,
    /// Mesh edge length (mm).
    pub spacing: f64,
    /// Elastic stiffness EA (N), which sets a casing's cut length.
    pub elastic_ea: f64,
    pub frames: &'a dyn PartFrames,
    /// The collision body's vertices (m) and triangles: hands and feet left out.
    pub collision: (&'a [Vec3], &'a [[usize; 3]]),
    /// The whole displayed body surface, drawn behind the cloth.
    pub display: (&'a [Vec3], &'a [[usize; 3]]),
    pub skinning: SkinningMode,
}

/// Writes `out/wrap`, `out/start` and `out/placement.json` into the existing, empty directory `out`, with
/// stills of `size` pixels, and returns the report and the exact mesh used for both stages.
fn stages(
    staging: &Staging,
    size: (usize, usize),
    out: &Path,
) -> Result<(Value, GarmentMesh), Refusal> {
    let Staging {
        pattern: source,
        label,
        spacing,
        elastic_ea,
        frames,
        collision: skin,
        display: full,
        skinning,
    } = *staging;
    // The same scope rule as a drape: an unplaced attachment refuses instead of vanishing.
    let scope = ftx_drape::garment::simulation_scope(source)?;

    // An elastic casing is meshed at its cut length, exactly as the drape does.
    let elastics = elastic_casings(source, elastic_ea)?;
    let cut_length;
    let pattern = if elastics.is_empty() {
        source
    } else {
        cut_length = casings_at_cut_length(source, &elastics);
        &cut_length
    };
    let mesh = build_garment(pattern, spacing)?;
    if mesh.verts2d.is_empty() {
        return Err(Refusal::new(
            RefusalCode::InvalidDocument,
            "no piece has a fold-up placement: nothing to place",
        )
        .repair("Give each simulated piece a placement record (spec 04 §3.1)."));
    }
    let places = placements(pattern, &mesh, frames)?;
    let wrapped = wrapped_positions(&mesh, &places);
    if let Some(v) = wrapped.iter().position(|p| !p.is_finite()) {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            format!(
                "{}: placement produces a non-finite wrapped position",
                mesh.instances[mesh.owner[v]]
            ),
        )
        .repair(
            "Check the wrap radius, angle, anchor and body frame against the pattern dimensions.",
        ));
    }
    // The drape refuses a posed body whose own surface crosses itself (arms against the trunk, a deep
    // squat) before it places any cloth. Report that here instead of stopping: the pictures still help.
    let body_surface = match ftx_drape::collider::audit_surface(skin.0, skin.1, BODY_AUDIT_PAIRS) {
        Ok(_) => json!({"admitted": true}),
        Err(refusal) => json!({"admitted": false, "refusal": refusal}),
    };
    let tree = NearestTree::new(skin.0.to_vec(), skin.1.to_vec())?;
    let start = start_state(&mesh, wrapped.clone(), &tree);
    let depth = |x: &[Vec3]| -> Vec<f64> { x.iter().map(|p| tree.signed_distance(*p)).collect() };
    let (wrap_depth, start_depth) = (depth(&wrapped), depth(&start.relaxed));

    let slots = piece_slots(&mesh.instances);
    let (strain_colours, strain_summaries) = start_strain(&mesh, &start.relaxed);
    // Cloth crossing cloth in the start: what refuses a drape with self-contact before it begins.
    let mut crossing = vec![false; mesh.verts2d.len()];
    let cloth_contact = match cloth_crossings(&mesh.faces, &start.groups, &start.relaxed) {
        Ok(pairs) => {
            let mut between = std::collections::BTreeMap::<(&str, &str), usize>::new();
            for &(a, b) in &pairs {
                for face in [mesh.faces[a], mesh.faces[b]] {
                    for v in face {
                        crossing[v] = true;
                    }
                }
                let (p, q) = (
                    mesh.instances[mesh.owner[mesh.faces[a][0]]].as_str(),
                    mesh.instances[mesh.owner[mesh.faces[b][0]]].as_str(),
                );
                *between.entry((p.min(q), p.max(q))).or_default() += 1;
            }
            let mut by_pieces: Vec<_> = between.into_iter().collect();
            by_pieces.sort_by_key(|row| std::cmp::Reverse(row.1));
            json!({
                "crossing_triangle_pairs": pairs.len(),
                "by_pieces": by_pieces.iter().map(|((a, b), n)| json!({"a": a, "b": b, "triangle_pairs": n})).collect::<Vec<_>>(),
                "basis": "start-state triangles, one vertex per sewing group, that intersect beyond a shared vertex or edge; thickness clearance is not tested here",
            })
        }
        Err(refusal) => json!({"unavailable": refusal}),
    };
    let body_obj = super::obj(full.0, full.1, None).into_bytes();
    let wrap_stills = write_stage(
        &out.join("wrap"),
        &body_obj,
        &mesh,
        &wrapped,
        &piece_colours(&mesh, &slots, &wrap_depth, &vec![false; mesh.verts2d.len()]),
        &travel_colours(&mesh, &wrapped),
        size,
    )?;
    let start_stills = write_stage(
        &out.join("start"),
        &body_obj,
        &mesh,
        &start.relaxed,
        &piece_colours(&mesh, &slots, &start_depth, &crossing),
        &strain_colours,
        size,
    )?;

    let p95 = start.block_p95.last().copied().unwrap_or(f64::NAN);
    let inside = |d: &[f64]| d.iter().filter(|v| **v < 0.0).count();
    let mut report = json!({
        "pattern": label,
        "spacing_mm": spacing,
        "vertices": mesh.verts2d.len(),
        "simulation_scope": scope,
        "instances": instance_report(
            &mesh,
            &places,
            &slots,
            &wrap_depth,
            &start_depth,
            &strain_summaries,
        ),
        "seams": seam_report(&mesh, &wrapped),
        "start": {
            "relax_iterations": start.relax_iterations,
            "plain_edge_strain_p95": p95,
            "relax_target_p95": RELAX_TARGET_P95,
            "reached_target": p95 <= RELAX_TARGET_P95,
            "vertices_inside_body": inside(&start_depth),
        },
        "wrap": {"vertices_inside_body": inside(&wrap_depth)},
        "body_surface": body_surface,
        "cloth_contact": cloth_contact,
        "body": body_report(frames),
        "views": {
            "wrap": "pieces on their cylinders; still_front/side/back/three_quarter coloured by piece, still_strain_* by the distance each seam vertex travels to its partner (green 0 mm to red 150 mm, grey unsewn)",
            "start": "pre-sewn and relaxed; coloured by piece, still_strain_* by edge strain (blue −15 % compressed, white 0, red +15 % stretched)",
            "magenta": "cloth vertex inside the collision body",
            "yellow": "start-state cloth triangle that crosses another cloth triangle",
        },
        "stills": {"wrap": wrap_stills, "start": start_stills},
        "no_claim": "No contact admission and no dynamics ran. This is not a drape or a fit; ftx drape decides whether the start is accepted.",
    });
    if !skinning.is_lbs() {
        report["skinning"] = json!(skinning.as_str());
    }
    workspace::write_json(&out.join("placement.json"), &report)?;
    Ok((report, mesh))
}

pub(super) fn run(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(
        args,
        "place",
        &[
            "measurements",
            "out",
            "model",
            "body-fit",
            "spacing-mm",
            "arm-deg",
            "legs",
            "leg-fraction",
            "skinning",
            "res",
        ],
    )?;
    let opts = super::drape_options(args)?;
    let pose = super::drape_pose(args)?;
    let skinning = super::parse_skinning(args)?;
    let pattern_path = args
        .positional
        .get(1)
        .ok_or_else(|| bad_input("ftx place needs a pattern file"))?;
    let (raw, _) = super::read_json(pattern_path)?;
    let source = Pattern::new(raw);
    // Refuse an unplaced attachment before the body is fitted; the stages apply the same rule.
    ftx_drape::garment::simulation_scope(&source)?;
    let material = ftx_drape::material::resolve(&source, &opts)?;
    let spacing = material.options.spacing;
    let (meas, _) = super::read_json(required(args, "measurements")?)?;
    let meas = super::current_measurements(meas)?;
    let size = render::resolution(args.options.get("res").map(String::as_str))?;
    let out = PathBuf::from(required(args, "out")?);
    super::reserve_output(&out, "place")?;

    let body_source::PreparedBody { model, params, .. } =
        body_source::prepare_bound(args, Some(&meas), None)?;
    let rest = model.rest(&params)?;
    let posed =
        model.pose_with_skinning(&rest, &pose_deltas(&model, &rest, &pose, 1.0)?, skinning)?;
    let skin = clothing_skin(&model, &posed.vertices)?;
    let full = skin_mesh(&posed.vertices, &model.data().faces)?;
    let frames = BodyFrames::new(&model, &posed)?;
    let (report, _) = stages(
        &Staging {
            pattern: &source,
            label: pattern_path,
            spacing,
            elastic_ea: material.options.elastic_ea,
            frames: &frames,
            collision: (&skin.vertices, &skin.triangles),
            display: (&full.vertices, &full.triangles),
            skinning,
        },
        size,
        &out,
    )?;

    let mut text = format!(
        "{} vertices at {spacing} mm; start strain p95 {:.3} after {} iterations (target {RELAX_TARGET_P95}); inside body: wrap {}, start {}\n",
        report["vertices"],
        report["start"]["plain_edge_strain_p95"]
            .as_f64()
            .unwrap_or(f64::NAN),
        report["start"]["relax_iterations"],
        report["wrap"]["vertices_inside_body"],
        report["start"]["vertices_inside_body"],
    );
    if let Some(message) = report["body_surface"]["refusal"]["message"].as_str() {
        text.push_str(&format!(
            "  BODY POSE NOT ADMITTED by the drape's surface audit: {message}\n"
        ));
    }
    if let Some(pairs) = report["cloth_contact"]["crossing_triangle_pairs"].as_u64() {
        text.push_str(&format!(
            "  cloth crosses cloth in {pairs} triangle pair(s) of the start"
        ));
        for row in report["cloth_contact"]["by_pieces"]
            .as_array()
            .into_iter()
            .flatten()
            .take(6)
        {
            text.push_str(&format!(
                "; {} × {}: {}",
                row["a"].as_str().unwrap_or(""),
                row["b"].as_str().unwrap_or(""),
                row["triangle_pairs"]
            ));
        }
        text.push('\n');
    }
    for i in report["instances"].as_array().into_iter().flatten() {
        text.push_str(&format!(
            "  {:<24} {:<7} R {:>6} mm  θ {:>7}..{:<7}  h {:>7}..{:<7} mm  inside wrap/start {}/{}  start strain x {} % y {} % (p95 {} %)\n",
            i["instance"].as_str().unwrap_or(""),
            i["colour"].as_str().unwrap_or(""),
            i["radius_mm"],
            i["theta_deg"][0],
            i["theta_deg"][1],
            i["height_mm"][0],
            i["height_mm"][1],
            i["inside_body"]["wrap_vertices"],
            i["inside_body"]["start_vertices"],
            i["start_edge_strain"]["mean_along_pattern_x_pct"],
            i["start_edge_strain"]["mean_along_pattern_y_pct"],
            i["start_edge_strain"]["p95_pct"],
        ));
    }
    let mut seams: Vec<&Value> = report["seams"].as_array().into_iter().flatten().collect();
    seams.sort_by(|a, b| {
        b["wrap_travel_mm"]["max"]
            .as_f64()
            .unwrap_or(0.0)
            .total_cmp(&a["wrap_travel_mm"]["max"].as_f64().unwrap_or(0.0))
    });
    text.push_str("  seams that travel furthest from the wrap:\n");
    for s in seams.iter().take(8) {
        text.push_str(&format!(
            "    {:<28} {} ↔ {}: mean {} mm, max {} mm\n",
            s["seam"].as_str().unwrap_or(""),
            s["a"].as_str().unwrap_or(""),
            s["b"].as_str().unwrap_or(""),
            s["wrap_travel_mm"]["mean"],
            s["wrap_travel_mm"]["max"],
        ));
    }
    text.push_str(&format!(
        "stills: {}/wrap and {}/start\n",
        out.display(),
        out.display()
    ));
    Ok((report, 0, text))
}

#[cfg(test)]
mod tests {
    use super::{cloth_crossings, deepest_mm, mean_strain, piece_slots, slot_colour};
    use ftx_core::Vec3;

    #[test]
    fn a_refused_drape_leaves_stills_of_its_last_accepted_state_and_says_where() {
        use ftx_core::{Refusal, RefusalCode};
        use serde_json::json;

        let pattern = ftx_pattern::Pattern::new(json!({
            "schema": "fabric-pattern/1", "units": "mm",
            "pieces": [{
                "id": "panel", "cut": {"qty": 1}, "grain": [[10, 10], [10, 40]],
                "vertices": {"a": [0, 0], "b": [120, 0], "c": [120, 80], "d": [0, 80]},
                "edges": [
                    {"id": "bottom", "from": "a", "to": "b"}, {"id": "right", "from": "b", "to": "c"},
                    {"id": "top", "from": "c", "to": "d"}, {"id": "left", "from": "d", "to": "a"}
                ],
                "placement": {"part": "torso"}
            }],
            "seams": []
        }));
        let mesh = ftx_drape::build_garment(&pattern, 20.0).unwrap();
        // The panel standing upright in front of a one-triangle "body", in metres.
        let positions = mesh
            .verts2d
            .iter()
            .map(|p| Vec3::new(p.x * 0.001, -0.2, 1.0 + p.y * 0.001))
            .collect();
        let state = ftx_drape::run::RefusedRun {
            mesh,
            positions,
            phase: "sew",
            accepted_steps: 2,
        };
        let body = b"v -0.2 0 0.8\nv 0.3 0 0.8\nv 0.05 0 1.3\nf 1 2 3\n";
        let refusal = || {
            Refusal::new(RefusalCode::NotConverged, "sewing step 3 did not converge")
                .repair("try --time-subdivision")
        };
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let out = std::env::temp_dir().join(format!("ftx-refused-{}-{time}", std::process::id()));
        std::fs::create_dir_all(&out).unwrap();

        let shown = super::refused_stills(refusal(), Some(&state), &out, body);
        assert_eq!(shown.code, RefusalCode::NotConverged);
        assert_eq!(shown.message, "sewing step 3 did not converge");
        assert_eq!(shown.repairs.len(), 2);
        assert!(
            shown.repairs[0].contains("refused/still_*.png"),
            "{}",
            shown.repairs[0]
        );
        assert!(shown.repairs[0].contains("after 2 accepted sew step(s)"));
        assert!(shown.repairs[0].contains("not a drape or a fit"));
        assert_eq!(shown.repairs[1], "try --time-subdivision");
        let (map, _) = crate::fold::document(&out.join("refused/mesh_map.json")).unwrap();
        assert_eq!(
            map["refused_pose"],
            json!({"phase":"sew","accepted_steps":2,"width":960,"height":540})
        );
        assert!(map.get("frames").is_none());
        for name in [
            "body.obj",
            "garment.obj",
            "garment_strain.obj",
            "still_front.png",
            "still_strain_back.png",
        ] {
            let file = out.join("refused").join(name);
            assert!(
                std::fs::metadata(&file).is_ok_and(|m| m.len() > 0),
                "{}",
                file.display()
            );
        }

        // Nothing to show, or nowhere to write it: the refusal comes back exactly as it was.
        for unchanged in [
            super::refused_stills(refusal(), None, &out, body),
            super::refused_stills(refusal(), Some(&state), &out.join("missing/parent"), body),
            // `refused/` already exists here, and a run's files are never overwritten.
            super::refused_stills(refusal(), Some(&state), &out, body),
        ] {
            assert_eq!(unchanged.repairs, ["try --time-subdivision"]);
        }
    }

    #[test]
    fn a_refused_start_is_drawn_from_the_drapes_own_pattern_and_body() {
        use ftx_core::{Refusal, RefusalCode};
        use ftx_drape::PartFrame;
        use ftx_drape::scene::PartFrames;
        use serde_json::json;

        struct Form;
        impl PartFrames for Form {
            fn frame(&self, _: &str) -> Option<PartFrame> {
                Some(PartFrame {
                    origin: Vec3::ZERO,
                    up: Vec3::new(0., 0., 1.),
                    front: Vec3::new(0., -1., 0.),
                    lat: Vec3::new(1., 0., 0.),
                })
            }
            fn landmark_h(&self, _: &str, _: &str) -> Option<f64> {
                Some(20.)
            }
            fn section_radius(&self, _: &str, _: f64) -> f64 {
                30.
            }
        }
        let pattern = ftx_pattern::Pattern::new(json!({
            "schema": "fabric-pattern/1", "units": "mm", "seams": [],
            "pieces": [{
                "id": "panel", "cut": {"qty": 1}, "grain": [[0, 0], [0, 20]],
                "vertices": {"a": [0, 0], "b": [20, 0], "c": [20, 20], "d": [0, 20]},
                "edges": [
                    {"id": "bottom", "from": "a", "to": "b"}, {"id": "right", "from": "b", "to": "c"},
                    {"id": "top", "from": "c", "to": "d"}, {"id": "left", "from": "d", "to": "a"}
                ],
                "placement": {"part": "torso", "radius_mm": 40.0, "x0": 10}
            }]
        }));
        // A closed box 60 mm square about the torso axis, in metres, wound outward: the panel's
        // cylinder (40 mm) clears it.
        let (a, z0, z1) = (0.03, -0.02, 0.14);
        let vertices = [
            Vec3::new(-a, -a, z0),
            Vec3::new(a, -a, z0),
            Vec3::new(a, a, z0),
            Vec3::new(-a, a, z0),
            Vec3::new(-a, -a, z1),
            Vec3::new(a, -a, z1),
            Vec3::new(a, a, z1),
            Vec3::new(-a, a, z1),
        ];
        let triangles = [
            [0, 2, 1],
            [0, 3, 2],
            [4, 5, 6],
            [4, 6, 7],
            [0, 1, 5],
            [0, 5, 4],
            [1, 2, 6],
            [1, 6, 5],
            [2, 3, 7],
            [2, 7, 6],
            [3, 0, 4],
            [3, 4, 7],
        ];
        let staging = super::Staging {
            pattern: &pattern,
            label: "outfit.json",
            spacing: 10.0,
            elastic_ea: ftx_drape::run::DrapeOptions::default().elastic_ea,
            frames: &Form,
            collision: (&vertices, &triangles),
            display: (&vertices, &triangles),
            skinning: ftx_body::SkinningMode::LinearBlend,
        };
        let refusal = || {
            Refusal::new(
                RefusalCode::IntersectingStart,
                "cloth triangles 3 and 9 intersect",
            )
            .repair("separate the layers")
        };
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let out = std::env::temp_dir().join(format!("ftx-start-{}-{time}", std::process::id()));
        std::fs::create_dir_all(&out).unwrap();

        let shown = super::start_stills(refusal(), &staging, &out);
        assert_eq!(shown.code, RefusalCode::IntersectingStart);
        assert_eq!(shown.message, "cloth triangles 3 and 9 intersect");
        assert_eq!(shown.repairs.len(), 2, "{:?}", shown.repairs);
        let dir = out.join("refused");
        assert!(
            shown.repairs[0].starts_with(&format!(
                "Look at {}/start/still_*.png (the pre-sewn start)",
                dir.display()
            )),
            "{}",
            shown.repairs[0]
        );
        // Nothing crosses here, and the text says the marks do not cover every refused start.
        assert!(shown.repairs[0].contains(
            "Magenta marks cloth vertices inside the body (0 in the start); yellow marks cloth \
             triangles that cross other cloth (0 pair(s)). A start is also refused for layers that \
             only touch (with cloth contact on)"
        ));
        assert_eq!(shown.repairs[1], "separate the layers");
        // The report describes the pattern and body that were handed over, not a second reading of them.
        let (placement, _) = crate::fold::document(&dir.join("placement.json")).unwrap();
        assert_eq!(placement["pattern"], "outfit.json");
        assert_eq!(placement["spacing_mm"], 10.0);
        assert_eq!(placement["instances"][0]["instance"], "panel");
        let (map, _) = crate::fold::document(&dir.join("start/mesh_map.json")).unwrap();
        assert_eq!(map["vertices"], placement["vertices"]);
        assert_eq!(map["refused_pose"]["phase"], "placement");
        assert_eq!(map["refused_pose"]["accepted_steps"], 0);
        assert!(
            map["edges"]
                .as_object()
                .unwrap()
                .keys()
                .any(|edge| edge.starts_with("panel/"))
        );
        assert_eq!(placement["body_surface"]["admitted"], true);
        assert!(placement.get("skinning").is_none());
        for name in [
            "wrap/garment.obj",
            "wrap/still_front.png",
            "start/body.obj",
            "start/still_strain_back.png",
        ] {
            let file = dir.join(name);
            assert!(
                std::fs::metadata(&file).is_ok_and(|m| m.len() > 0),
                "{}",
                file.display()
            );
        }

        // Already drawn, or nowhere to draw it: the refusal comes back exactly as it was.
        for unchanged in [
            super::start_stills(refusal(), &staging, &out),
            super::start_stills(refusal(), &staging, &out.join("missing/parent")),
        ] {
            assert_eq!(unchanged.repairs, ["separate the layers"]);
        }
    }

    #[test]
    fn a_start_refusal_says_where_the_start_is_drawn_and_what_the_colours_count() {
        use serde_json::json;

        let dir = std::path::Path::new("out/refused");
        let crossing = super::start_repair(
            dir,
            &json!({
                "start": {"vertices_inside_body": 7},
                "cloth_contact": {"crossing_triangle_pairs": 31, "by_pieces": [
                    {"a": "front.L", "b": "front.R", "triangle_pairs": 27},
                    {"a": "back", "b": "front.L", "triangle_pairs": 4},
                ]},
                "body_surface": {"admitted": true},
            }),
        );
        assert!(crossing.starts_with("Look at out/refused/start/still_*.png"));
        assert!(crossing.contains("out/refused/wrap/still_*.png"));
        assert!(crossing.contains(
            "ftx place draws them. Magenta marks cloth vertices inside the body (7 in the start); \
             yellow marks cloth triangles that cross other cloth (31 pair(s): front.L × front.R: 27; \
             back × front.L: 4). A start is also refused for layers that only touch"
        ));
        assert!(
            crossing.contains("neither carries a mark, so go by the refusal's message for those.")
        );
        assert!(crossing.contains("out/refused/placement.json"));
        assert!(crossing.ends_with("not a drape or a fit."));
        assert!(!crossing.contains("posed body"));

        // A body the audit did not admit is reported in the audit's own words; an unavailable crossing
        // check is not reported as zero crossings.
        let pose = super::start_repair(
            dir,
            &json!({
                "start": {"vertices_inside_body": 0},
                "cloth_contact": {"unavailable": {"code": "degenerate_geometry"}},
                "body_surface": {"admitted": false, "refusal": {"code": "budget_exceeded",
                    "message": "body surface audit exceeds its 1000000 candidate-pair budget"}},
            }),
        );
        // The body comes before the cloth marks: nothing after it repairs a pose.
        assert!(pose.contains(
            "ftx place draws them. The posed body itself was not admitted (body surface audit exceeds \
             its 1000000 candidate-pair budget): change the pose first, since no placement repairs \
             that. Magenta marks cloth vertices inside the body (0 in the start). A start is also \
             refused for layers that only touch"
        ));
        assert!(!pose.contains("yellow"));
    }

    #[test]
    fn depth_inside_the_body_is_a_positive_length_and_none_is_plain_zero() {
        assert_eq!(deepest_mm(&[0.004, -0.0123, -0.002, 0.0]), 12.3);
        for clear in [&[0.004, 0.0][..], &[][..], &[-0.0][..]] {
            let depth = deepest_mm(clear);
            assert_eq!(depth, 0.0);
            assert!(depth.is_sign_positive(), "a report must not print -0.0");
        }
        // The same holds for any rounded reading, such as a slightly negative mean shear.
        assert!(super::round1(-0.04).is_sign_positive());
        assert_eq!(super::round1(-0.06), -0.1);
    }

    #[test]
    fn a_sewn_seam_is_adjacency_and_a_pierced_panel_is_a_crossing() {
        let p = |x: f64, y: f64, z: f64| Vec3::new(x, y, z);
        // Face 0 lies in z = 0. Face 1 pierces its interior. Face 2 is sewn to face 0 along one edge
        // through duplicate vertices, as two pattern pieces are before welding.
        let x = [
            p(0.0, 0.0, 0.0),
            p(1.0, 0.0, 0.0),
            p(0.0, 1.0, 0.0),
            p(0.2, 0.2, -1.0),
            p(0.3, 0.2, 1.0),
            p(0.2, 0.3, 1.0),
            p(0.0, 0.0, 0.0),
            p(1.0, 0.0, 0.0),
            p(0.5, -1.0, 0.2),
        ];
        let faces = [[0, 1, 2], [3, 4, 5], [6, 7, 8]];
        let sewn = [vec![0, 6], vec![1, 7]];
        assert_eq!(cloth_crossings(&faces, &sewn, &x).unwrap(), [(0, 1)]);
        // Unsewn, the coincident edge is a contact between differently indexed vertices.
        assert_eq!(cloth_crossings(&faces, &[], &x).unwrap(), [(0, 1), (0, 2)]);
    }

    #[test]
    fn mean_strain_recovers_a_uniform_strain_from_hex_edges() {
        // The three edge directions of the garment's hex grid, under one known in-plane strain.
        let (ex, ey, shear) = (0.08, -0.02, 0.01);
        let edges: Vec<(f64, f64, f64)> = [0.0_f64, 60.0, 120.0]
            .iter()
            .map(|deg| {
                let (s, c) = deg.to_radians().sin_cos();
                (12.0 * c, 12.0 * s, ex * c * c + ey * s * s + shear * s * c)
            })
            .collect();
        let fit = mean_strain(&edges).expect("three directions span the plane");
        for (got, want) in fit.iter().zip([ex, ey, shear]) {
            assert!((got - want).abs() < 1e-12, "{got} vs {want}");
        }
    }

    #[test]
    fn mean_strain_is_absent_when_edges_share_one_direction() {
        assert!(mean_strain(&[(1.0, 0.0, 0.1), (2.0, 0.0, 0.1), (0.0, 0.0, 0.3)]).is_none());
    }

    #[test]
    fn a_mirrored_copy_takes_the_lighter_tint_of_its_pair() {
        let ids = ["front.R", "front.L", "collar", "back.C"].map(String::from);
        let slots = piece_slots(&ids);
        assert_eq!(slots, [(0, false), (0, true), (1, false), (2, false)]);
        let (right, left) = (slot_colour(slots[0]), slot_colour(slots[1]));
        assert!(left.iter().zip(right).all(|(l, r)| *l > r));
    }
}

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

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ftx_body::{
    BodyFrames, HandCollider, SkinningMode, ThumbPose, clothing_skin_with_hands, skin_mesh,
};
use ftx_core::intersection::{indexed_triangle_pair, validate_mesh};
use ftx_core::{Refusal, RefusalCode, Vec3};
use ftx_drape::collider::SurfaceAudit;
use ftx_drape::render::Shading;
use ftx_drape::scene::{
    PartFrames, casings_at_cut_length, elastic_casings, placements, wrapped_positions,
};
use ftx_drape::start::{RELAX_MAX_ITERS, RELAX_TARGET_P95};
use ftx_drape::start::{edge_strain, face_edges};
use ftx_drape::{
    GarmentMesh, MeshOptions, NearestTree, SelfRepulsionOptions, WrapPlacement, build_garment,
    build_garment_with_options, mesh_quality, start_state, start_state_with_repulsion,
};
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
pub(super) const BODY_AUDIT_PAIRS: usize = 1_000_000;
/// Edge strain that saturates the `start/` strain view. A kinematic start is far outside the ±3 %
/// ramp used for a finished drape.
const START_STRAIN_RAMP: f64 = 0.15;
/// How far round the cloth triangle a crossing refusal names its pictures mark cloth, in millimetres. One
/// triangle alone does not show: on the 92 cm bolero at 12 mm spacing it was 3 to 4 pixels wide in an 840
/// by 620 whole-figure still and went unseen.
const CROSSING_MARK_MM: f64 = 30.0;
/// Share of an instance's vertices at or past [`START_STRAIN_RAMP`], in percent, from which a start is
/// called tight there. Measured on the 92 cm bolero of the fitting skill: a 170 mm cuff that has to sit
/// over the open hand (hull girth 174 to 180 mm) leaves 3.0 and 3.3 % of the two sleeves at the ramp and
/// the first sewing step never converges; the shortened sleeve that drapes leaves 0.0 %, and ordinary
/// seam pull at the shoulders leaves 0.3 to 1.3 % of the back. Those are 12 mm meshes. It does not carry
/// to a finer one: the same long sleeve at 6 mm, refused from a checkpoint at 1225 iterations (p95 0.219),
/// named the back and both fronts (8, 6.4 and 7.1 %, by the shoulder and neck seams) and not the sleeves.
const TIGHT_START_PCT: f64 = 2.0;
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
///
/// Each summary also gives `stretched_past_ramp_pct`, the share of the instance's vertices drawn fully
/// red, and `stretched_nearest_edge`, the pattern edge most of those vertices lie nearest in the flat
/// pattern (null when none is red).
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
    // The vertices drawn fully red: mean edge strain at or past the ramp. Their share of an instance and
    // the pattern edge most of them lie nearest say, in the pattern's own terms, where a start is too
    // small for what it surrounds.
    let red = |v: usize| count[v] > 0 && sum[v] / count[v] as f64 >= START_STRAIN_RAMP;
    let nearest_edge = |k: usize, v: usize| {
        let at = mesh.verts2d[v];
        mesh.index
            .iter()
            .filter(|((instance, _), _)| *instance == mesh.instances[k])
            .filter_map(|((_, edge), along)| {
                // Squared distances order the same and need no platform square root.
                along
                    .iter()
                    .map(|&w| {
                        let (dx, dy) = (mesh.verts2d[w].x - at.x, mesh.verts2d[w].y - at.y);
                        dx * dx + dy * dy
                    })
                    .min_by(f64::total_cmp)
                    .map(|distance| (distance, edge))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, edge)| edge)
    };
    let stretched: Vec<(f64, Option<&String>)> = mesh
        .inst_range
        .iter()
        .enumerate()
        .map(|(k, &(lo, hi))| {
            let mut votes: BTreeMap<&String, usize> = BTreeMap::new();
            let mut reds = 0_usize;
            for v in (lo..hi).filter(|&v| red(v)) {
                reds += 1;
                if let Some(edge) = nearest_edge(k, v) {
                    *votes.entry(edge).or_default() += 1;
                }
            }
            // The first of equally voted edges in the map's order, so the answer does not depend on a hash.
            let most = votes
                .iter()
                .fold(None, |best: Option<(&String, usize)>, (&edge, &n)| {
                    if best.is_some_and(|b| b.1 >= n) {
                        best
                    } else {
                        Some((edge, n))
                    }
                });
            (
                reds as f64 / hi.saturating_sub(lo).max(1) as f64,
                most.map(|m| m.0),
            )
        })
        .collect();
    let summaries = by_instance
        .iter()
        .zip(&stretched)
        .map(|(edges, &(red_share, red_edge))| {
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
                "stretched_past_ramp_pct": pct(red_share),
                "stretched_nearest_edge": red_edge,
            })
        })
        .collect();
    (colours, summaries)
}

fn instance_report(
    pattern: &Pattern,
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
            let source = pattern
                .instance(id)
                .map(|instance| &pattern.raw()["pieces"][instance.piece_index]["placement"]);
            // The final mirror is part of wrap_points, not PartFrame. Report the actual world
            // frame so its origin and basis reconstruct the displayed wrap on either side.
            let world_vector = |mut v: Vec3| {
                if p.mirrored {
                    v.x = -v.x;
                }
                json!([v.x, v.y, v.z])
            };
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
            let mut report = json!({
                "instance": id,
                "colour": PIECE_COLOUR_NAMES[slots[k].0 % PIECE_COLOUR_NAMES.len()],
                "lighter_tint": slots[k].1,
                "mirrored": p.mirrored,
                "vertices": hi - lo,
                "radius_mm": round1(p.radius),
                "anchor": {"theta_deg": p.theta_deg, "x0_mm": p.x0, "y0_height_mm": round1(p.h0)},
                "theta_deg": [round1(t0), round1(t1)],
                "height_mm": [round1(h0), round1(h1)],
                "world_frame": {
                    "origin_mm": world_vector(p.frame.origin),
                    "up": world_vector(p.frame.up),
                    "theta_0": world_vector(p.frame.front),
                    "theta_90": world_vector(p.frame.lat),
                    "h0_mm": p.h0,
                    "radius_mm": p.radius,
                    "x0_mm": p.x0,
                    "theta_deg": p.theta_deg,
                    "drafted_x_sign": if p.mirrored { -1 } else { 1 },
                    "own_side_clamp": if p.own_side {
                        Some(if p.own_side_left ^ p.mirrored { "x >= 1 mm" } else { "x <= -1 mm" })
                    } else { None },
                },
                "inside_body": {
                    "wrap_vertices": inside(wrap_depth),
                    "wrap_deepest_mm": deepest(wrap_depth),
                    "start_vertices": inside(start_depth),
                    "start_deepest_mm": deepest(start_depth),
                },
                "start_edge_strain": strain[k],
            });
            if let Some(source) = source {
                for key in ["frame_offset_mm", "frame_rotation"] {
                    if let Some(value) = source.get(key) {
                        report[key] = value.clone();
                    }
                }
            }
            report
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

/// Report the same displayed-coordinate crossing check used by the drape without admitting a
/// kinematic preview to dynamics. A malformed surface stays unavailable, never crossing-free.
fn display_contact_report(
    full: (&[Vec3], &[[usize; 3]]),
    faces: &[[usize; 3]],
    wrapped: &[Vec3],
    relaxed: &[Vec3],
) -> Value {
    let mut contact = match ftx_drape::animation::ReplayContact::new(full.0, full.1) {
        Ok(contact) => contact,
        Err(refusal) => return json!({"unavailable": refusal}),
    };
    let mut check = |points: &[Vec3], frame| match contact.check(points, faces, frame) {
        Ok(()) => json!({"crossing_free": true}),
        Err(refusal) if refusal.code == RefusalCode::IntersectingTrajectory => {
            json!({"crossing_free": false, "refusal": refusal})
        }
        Err(refusal) => json!({"unavailable": refusal}),
    };
    json!({
        "wrap": check(wrapped, ftx_drape::animation::FOLD_FRAMES - 1),
        "start": check(relaxed, ftx_drape::animation::FOLD_FRAMES + ftx_drape::animation::ZIP_FRAMES - 1),
        "basis": "proper cloth/body and cloth/cloth crossings at displayed f32 precision in these preview stages; includes the full displayed body, not only the collision proxy; touching is allowed; no thickness clearance, dynamic admission or later trajectory check",
    })
}

fn write_stage(
    dir: &Path,
    body: &[u8],
    mesh: &GarmentMesh,
    x: &[Vec3],
    pieces: &[[f64; 3]],
    second: &[[f64; 3]],
    ((width, height), shading): ((usize, usize), Shading),
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
    for (name, png) in render::shaded(dir, width, height, shading)? {
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
    crossing_faces: Option<[usize; 2]>,
    (width, height): (usize, usize),
) {
    let mut map = ftx_drape::animation::pattern_mesh_map(mesh);
    map["refused_pose"] = json!({ "phase": phase, "accepted_steps": accepted_steps, "width": width, "height": height });
    if let Some(faces) = crossing_faces {
        map["refused_pose"]["crossing_faces"] = json!(faces);
    }
    if let Err(error) = workspace::write_json(&dir.join("mesh_map.json"), &map) {
        refusal.repairs.push(format!(
            "The diagnostic source-edge map could not be saved: {}",
            error.message
        ));
    }
}

/// When a drape retains a rejected start or trajectory state: draws that cloth under
/// `OUT/refused` (the files and stills of a `place` stage: coloured by piece, and by edge strain on the
/// start-state ramp) and puts where to look first among the refusal's repairs. That state is the last
/// accepted one when a step refuses, the untangled cloth start that failed contact admission,
/// or the recorded frame that crosses the displayed wearer.
///
/// `size` is the width and height in pixels the run's own stills were asked for, so a pixel read off one
/// of these pictures names the same place as in the stills of a round that completes.
///
/// When that state is the start itself (no sewing step accepted), a second repair says what the run knows
/// is wrong with the start: see [`start_condition`].
///
/// No claim: it is not a drape or a fit. Inside-body marking needs the collision body and is not drawn. If
/// the pictures cannot be written (a full disk, an existing `refused/`), the first repair says so with the
/// error in place of where to look, and what the run knows about its start is still said: the pictures
/// never replace or delay a refusal, and their absence is not hidden.
pub(super) fn refused_stills(
    mut refusal: Refusal,
    state: Option<&ftx_drape::run::RefusedRun>,
    out: &Path,
    body_obj: &[u8],
    size: (usize, usize),
) -> Refusal {
    let Some(state) = state else { return refusal };
    let dir = out.join("refused");
    let slots = piece_slots(&state.mesh.instances);
    let n = state.positions.len();
    // A full-display refusal names its cloth by triangle index, which tells a reader nothing: mark that
    // cloth in the pictures and say whose it is.
    let valid_start_crossing = state
        .start_crossing_faces
        .filter(|faces| faces.iter().all(|&f| f < state.mesh.faces.len()));
    let named = if let Some(faces) = valid_start_crossing {
        faces.to_vec()
    } else if refusal.code == RefusalCode::IntersectingTrajectory {
        named_cloth_triangles(&refusal.message, state.mesh.faces.len())
    } else {
        Vec::new()
    };
    // Cloth crossing the body takes the colour `place` gives cloth inside the body; cloth crossing cloth
    // takes its crossing colour.
    let on_body = refusal.message.contains("displayed body triangle");
    let reach = CROSSING_MARK_MM * 0.001;
    let mut near = vec![false; n];
    for &triangle in &named {
        for corner in state.mesh.faces[triangle] {
            let at = state.positions[corner];
            for (mark, p) in near.iter_mut().zip(&state.positions) {
                let (dx, dy, dz) = (p.x - at.x, p.y - at.y, p.z - at.z);
                *mark |= dx * dx + dy * dy + dz * dz <= reach * reach;
            }
        }
    }
    let named_in = (!named.is_empty()).then(|| {
        let owners = triangle_instances(&state.mesh, &named).join(" and ");
        if refusal.message.contains("touch or overlap") {
            format!(
                "the cloth within {CROSSING_MARK_MM} mm of the touching or overlapping cloth the \
                 refusal names, which is in {owners}"
            )
        } else {
            let (triangles, are) = if named.len() == 1 {
                ("triangle", "is")
            } else {
                ("triangles", "are")
            };
            format!(
                "the cloth within {CROSSING_MARK_MM} mm of the cloth {triangles} the refusal names, which \
                 {are} in {owners}"
            )
        }
    });
    let depth: Vec<f64> = near
        .iter()
        .map(|&mark| if mark && on_body { -1.0 } else { 0.0 })
        .collect();
    let crossing: Vec<bool> = near.iter().map(|&mark| mark && !on_body).collect();
    let pieces = piece_colours(&state.mesh, &slots, &depth, &crossing);
    let (strain, summaries) = start_strain(&state.mesh, &state.positions);
    let description = if state.phase == "start" {
        "the untangled start rejected at cloth contact admission".to_string()
    } else {
        format!(
            "the cloth after {} accepted {} step(s)",
            state.accepted_steps, state.phase
        )
    };
    match write_stage(
        &dir,
        body_obj,
        &state.mesh,
        &state.positions,
        &pieces,
        &strain,
        (size, Shading::Standard),
    ) {
        Ok(_) => {
            write_refused_map(
                &mut refusal,
                &dir,
                &state.mesh,
                state.phase,
                state.accepted_steps,
                valid_start_crossing,
                size,
            );
            refusal.repairs.insert(
                0,
                format!(
                    "Look at {}/still_*.png: {description}, the state this refusal \
                     is about. It is not a drape or a fit; still_strain_* shows edge strain from blue (15 % \
                     short) through white to red (15 % stretched).{}",
                    dir.display(),
                    named_in.as_ref().map_or(String::new(), |named_in| format!(
                        " {} marks {named_in}; the check stops at the first crossing it finds, so \
                         there can be others.",
                        if on_body { "Magenta" } else { "Yellow" }
                    ))
                ),
            );
        }
        // Said, not hidden: a full disk is when a reader most needs to know why `refused/` is missing
        // or partial (seen: a quota left a 0-byte body.obj and nothing else).
        Err(error) => refusal.repairs.insert(
            0,
            format!(
                "The pictures of the state this refusal is about ({description}) \
                 could not be written: {}.{}",
                error.message,
                named_in.as_ref().map_or(String::new(), |named_in| format!(
                    " They would have marked {named_in}."
                ))
            ),
        ),
    }
    // Only the start itself or the state before any sewing step. Later states are stretched by seams
    // that are still closing, which says nothing about the size of a piece. It is text, so it does not
    // depend on the pictures having been written.
    if (state.phase == "sew" || state.phase == "start") && state.accepted_steps == 0 {
        let tight = tight_instances(
            state
                .mesh
                .instances
                .iter()
                .map(String::as_str)
                .zip(&summaries),
        );
        if let Some(text) = start_condition(state.start_fallback, &tight, state.start_unrelaxed_p95)
        {
            refusal.repairs.insert(1, text);
        }
    }
    refusal
}

/// How to tell unfinished relaxation from a small piece, said wherever a start that stopped above its
/// relaxation target is reported as tight. It claims no cause for what remains: on the one garment
/// measured, the coarser mesh reached the target and the stretch by the seams was gone.
const UNRELAXED_CHECK: &str = " To tell them apart, look at ftx place with a coarser --spacing-mm: if the \
     relaxation reaches its target there, stretch that remains is not unfinished relaxation.";

/// What is wrong with the start a refused drape began from, when the run can tell: it is an earlier and
/// more strained relaxation checkpoint (`fallback`: its iterations, then those of the start that was not
/// admitted for cloth contact), it is stretched past the strain ramp in the `tight` instances, or both.
/// `None` when neither holds. It names what to change in the pattern, because no solver budget does.
///
/// `unrelaxed` is the start's p95 strain when its relaxation ran its full [`RELAX_MAX_ITERS`] and the
/// start is still above [`RELAX_TARGET_P95`]. Stretch next to a seam is then not evidence of a small
/// piece, and the text says so and how to tell (measured: the 92 cm bolero at 6 mm spacing stops at 0.194
/// and shows 4 to 6 % of its back and fronts at the ramp by the shoulder and neck seams, where at 12 mm it
/// reaches the target in 600 iterations and shows 0.3 %).
fn start_condition(
    fallback: Option<(usize, usize)>,
    tight: &[String],
    unrelaxed: Option<f64>,
) -> Option<String> {
    if fallback.is_none() && tight.is_empty() {
        return None;
    }
    let mut text = match fallback {
        Some((used, rejected)) => format!(
            "This start is the relaxation checkpoint at {used} iterations, not the fully relaxed start at \
             {rejected}: that start failed contact admission against cloth layers or the displayed body, so \
             the drape began from an earlier and more strained state."
        ),
        None => "Before any sewing step this start was already strained.".to_string(),
    };
    if !tight.is_empty() {
        // Name the subject: after the sentence above, "it" could be read as the start that was not used.
        let subject = if fallback.is_some() {
            "The checkpoint"
        } else {
            "It"
        };
        text.push_str(&format!(
            " {subject} is stretched 15 % or more (red in still_strain_*) in {}. If that cloth has to pass \
             round a wider part of the body there (a cuff over the hand, a hem over the hip), end the \
             piece short of that part or widen it at that edge.",
            tight.join("; ")
        ));
        // A checkpoint is above the target by construction; it gets its own sentence below.
        if let (None, Some(p95)) = (fallback, unrelaxed) {
            text.push_str(&format!(
                " The start's relaxation ran its full {RELAX_MAX_ITERS} iterations and the start is at \
                 p95 strain {p95:.3}, above the {RELAX_TARGET_P95} target, so stretch next to a seam can \
                 be unfinished relaxation and not a piece that is too small.{UNRELAXED_CHECK}"
            ));
        }
    }
    if fallback.is_some() {
        text.push_str(if tight.is_empty() {
            " ftx place draws the fully relaxed start: look there for cloth contact or a displayed-body crossing."
        } else {
            // Measured: the long-sleeved bolero at 6 mm, begun from the checkpoint at 1225 iterations (p95
            // 0.219), had 6 to 8 % of its back and fronts at the ramp by the shoulder and neck seams.
            " A checkpoint is less relaxed than the full start, so stretch next to a seam can be \
             unfinished relaxation and not a piece that is too small. ftx place draws the fully relaxed \
             start and applies the same measure to it: look there for what is still stretched, and for \
             cloth contact or a displayed-body crossing."
        });
    }
    text.push_str(" No solver budget changes the start.");
    Some(text)
}

/// The cloth triangles a full-display refusal names, as indices into the garment's faces: its message is
/// "... cloth triangle A crosses displayed body triangle B" or "... cloth triangle A crosses cloth triangle
/// B" (`ftx_drape::animation::ReplayContact::check`, whose `faces` are the garment's). Empty for any other
/// message, and an index outside this garment is dropped. That check stops at its first crossing, so these
/// are one place where cloth crosses, not every place.
fn named_cloth_triangles(message: &str, faces: usize) -> Vec<usize> {
    const KEY: &str = "cloth triangle ";
    let mut found = Vec::new();
    let mut rest = message;
    while let Some(at) = rest.find(KEY) {
        rest = &rest[at + KEY.len()..];
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if let Ok(triangle) = rest[..digits].parse::<usize>()
            && triangle < faces
            && !found.contains(&triangle)
        {
            found.push(triangle);
        }
    }
    found
}

/// The piece instances that own `triangles`, in their order and without repeats: what a reader can act on
/// where a refusal gives only a triangle index.
fn triangle_instances<'a>(mesh: &'a GarmentMesh, triangles: &[usize]) -> Vec<&'a str> {
    let mut names: Vec<&str> = Vec::new();
    for &triangle in triangles {
        let name = mesh.instances[mesh.owner[mesh.faces[triangle][0]]].as_str();
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// Adds to each crossing stage of a [`display_contact_report`] the piece instances that own the cloth
/// triangles its message names, as `cloth_instances`.
fn with_crossing_pieces(mut contact: Value, mesh: &GarmentMesh) -> Value {
    for stage in ["wrap", "start"] {
        let named = contact[stage]["refusal"]["message"]
            .as_str()
            .map(|message| named_cloth_triangles(message, mesh.faces.len()))
            .unwrap_or_default();
        if !named.is_empty() {
            contact[stage]["cloth_instances"] = json!(triangle_instances(mesh, &named));
        }
    }
    if contact["start"]["refusal"]["message"]
        .as_str()
        .is_some_and(|message| message.contains("displayed body triangle"))
    {
        contact["start"]["note"] = json!(
            "This preview is not untangled: a drape first pushes its start out of the collision body, so \
             a start that crosses here can still drape. Where the part crossed is left out of the \
             collider (fingers beyond the --hands cap), a drape can only begin from an earlier relaxation \
             checkpoint that does not cross it; --hands full keeps the whole hand."
        );
    }
    contact
}

/// The instances of a start that are tight, as its reader needs them named: the instance, the share of
/// it drawn fully red and the pattern edge that red lies nearest. Takes each instance with its
/// [`start_strain`] summary. Empty when none reaches [`TIGHT_START_PCT`].
fn tight_instances<'a>(rows: impl Iterator<Item = (&'a str, &'a Value)>) -> Vec<String> {
    rows.filter_map(|(id, summary)| {
        let share = summary["stretched_past_ramp_pct"]
            .as_f64()
            .filter(|&share| share >= TIGHT_START_PCT)?;
        Some(match summary["stretched_nearest_edge"].as_str() {
            Some(edge) => format!("{id} ({share} % of it, nearest its edge {edge})"),
            None => format!("{id} ({share} % of it)"),
        })
    })
    .collect()
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

/// A start can be refused by the posed-body audit, by cloth inside or too near the body, or by cloth
/// layers crossing or touching. The refused geometry is what `ftx place` draws, so the two
/// placement stages of the drape's own pattern and body are written under `OUT/refused` and named first
/// among the repairs. If they cannot be produced the refusal is returned unchanged.
pub(super) fn start_stills(
    mut refusal: Refusal,
    staging: &Staging,
    out: &Path,
    size: (usize, usize),
) -> Refusal {
    let dir = out.join("refused");
    if std::fs::create_dir(&dir).is_ok()
        && let Ok((placement, mesh)) = stages(staging, size, Shading::Standard, &dir)
    {
        write_refused_map(
            &mut refusal,
            &dir.join("start"),
            &mesh,
            "placement",
            0,
            None,
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
    /// Opt-in curvature/gather-adaptive meshing and bounded Lloyd CVT options.
    pub mesh: MeshOptions,
    /// Opt-in spatial-hash cloth–cloth self-repulsion during start-state relaxation.
    pub start_self_repulsion: Option<SelfRepulsionOptions>,
    /// Elastic stiffness EA (N), which sets a casing's cut length.
    pub elastic_ea: f64,
    pub frames: &'a dyn PartFrames,
    /// The collision body's vertices (m) and triangles: feet left out, and hands as `hands` says.
    pub collision: (&'a [Vec3], &'a [[usize; 3]]),
    /// The whole displayed body surface, drawn behind the cloth.
    pub display: (&'a [Vec3], &'a [[usize; 3]]),
    pub skinning: SkinningMode,
    /// Which hand collider `collision` was built with; reported when it is not the reference's.
    pub hands: HandCollider,
    pub arm_pose: Option<ftx_body::drape::ArmPose>,
    pub thumbs: Option<ThumbPose>,
    pub thumb_audit: Option<&'a Result<SurfaceAudit, Refusal>>,
}

fn body_surface_report(surface: (&[Vec3], &[[usize; 3]])) -> Value {
    match ftx_drape::collider::audit_surface(surface.0, surface.1, BODY_AUDIT_PAIRS) {
        Ok(_) => json!({"admitted": true}),
        Err(refusal) => json!({"admitted": false, "refusal": refusal}),
    }
}

/// Writes `out/wrap`, `out/start` and `out/placement.json` into the existing, empty directory `out`, with
/// stills of `size` pixels, and returns the report and the exact mesh used for both stages.
fn stages(
    staging: &Staging,
    size: (usize, usize),
    shading: Shading,
    out: &Path,
) -> Result<(Value, GarmentMesh), Refusal> {
    let Staging {
        pattern: source,
        label,
        spacing,
        mesh: mesh_options,
        start_self_repulsion,
        elastic_ea,
        frames,
        collision: skin,
        display: full,
        skinning,
        hands,
        arm_pose,
        thumbs,
        thumb_audit,
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
    let mesh = if mesh_options.is_default() {
        build_garment(pattern, spacing)?
    } else {
        build_garment_with_options(pattern, spacing, mesh_options)?
    };
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
    let body_surface = body_surface_report(skin);
    let tree = NearestTree::new(skin.0.to_vec(), skin.1.to_vec())?;
    let start = match start_self_repulsion {
        Some(repulsion) => {
            start_state_with_repulsion(&mesh, wrapped.clone(), &tree, Some(repulsion))?
        }
        None => start_state(&mesh, wrapped.clone(), &tree),
    };
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
        (size, shading),
    )?;
    let start_stills = write_stage(
        &out.join("start"),
        &body_obj,
        &mesh,
        &start.relaxed,
        &piece_colours(&mesh, &slots, &start_depth, &crossing),
        &strain_colours,
        (size, shading),
    )?;

    let p95 = start.block_p95.last().copied().unwrap_or(f64::NAN);
    let inside = |d: &[f64]| d.iter().filter(|v| **v < 0.0).count();
    let mut report = json!({
        "pattern": label,
        "spacing_mm": spacing,
        "vertices": mesh.verts2d.len(),
        "simulation_scope": scope,
        "instances": instance_report(
            pattern,
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
        "display_contact": with_crossing_pieces(
            display_contact_report(full, &mesh.faces, &wrapped, &start.relaxed),
            &mesh,
        ),
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
    if !hands.is_wrist() {
        report["hand_collider"] = json!(hands.as_str());
    }
    if let Some(thumbs) = thumbs {
        report["body"]["thumb_pose"] = json!(thumbs);
    }
    if let Some(arms) = arm_pose {
        report["body"]["arm_pose"] = json!(arms);
    }
    if let Some(audit) = thumb_audit {
        report["authored_hand_surface"] = match audit {
            Ok(audit) => json!({"admitted": true, "pairs_tested": audit.pairs_tested}),
            Err(refusal) => json!({"admitted": false, "refusal": refusal}),
        };
    }
    if !mesh_options.is_default() {
        report["mesh"] = json!({
            "options": mesh_options,
            "quality": mesh_quality(&mesh),
        });
    }
    if let Some(repulsion) = start_self_repulsion {
        report["start_self_repulsion"] = json!(repulsion);
    }
    if shading != Shading::Standard {
        report["shading"] = json!(shading.name());
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
            "min-spacing-mm",
            "cvt-iters",
            "max-turning-angle-rad",
            "gather-refinement",
            "start-self-repulsion",
            "start-repulsion-mm",
            "start-repulsion-stiffness",
            "start-repulsion-period",
            "start-repulsion-max-checks",
            "arm-deg",
            "left-arm-deg",
            "right-arm-deg",
            "thumb-pose",
            "legs",
            "leg-fraction",
            "skinning",
            "hands",
            "res",
            "shading",
        ],
    )?;
    let opts = super::drape_options(args)?;
    let pose = super::drape_pose(args)?;
    let thumbs = super::parse_thumb_pose(args)?;
    let skinning = super::parse_skinning(args)?;
    let hands = super::parse_hands(args)?;
    let size = render::resolution(args.options.get("res").map(String::as_str))?;
    let shading = render::shading(args.options.get("shading").map(String::as_str))?;
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
    let out = PathBuf::from(required(args, "out")?);
    super::reserve_output(&out, "place")?;

    let body_source::PreparedBody { model, params, .. } =
        body_source::prepare_bound(args, Some(&meas), None)?;
    let rest = model.rest(&params)?;
    let posed = super::pose_fitted_body(&model, &rest, &pose, skinning, thumbs)?;
    let skin = clothing_skin_with_hands(&model, &posed.vertices, hands)?;
    let full = skin_mesh(&posed.vertices, &model.data().faces)?;
    let thumb_audit = super::audit_authored_hands(&model, &posed, &skin, hands, thumbs);
    let frames = BodyFrames::new(&model, &posed)?;
    let posed_frames = ftx_drape::scene::PosedFrames::new(&frames, pose.arm_pose);
    let (report, _) = stages(
        &Staging {
            pattern: &source,
            label: pattern_path,
            spacing,
            mesh: material.options.mesh,
            start_self_repulsion: material.options.start_self_repulsion,
            elastic_ea: material.options.elastic_ea,
            frames: &posed_frames,
            collision: (&skin.vertices, &skin.triangles),
            display: (&full.vertices, &full.triangles),
            skinning,
            hands,
            arm_pose: pose.arm_pose,
            thumbs,
            thumb_audit: thumb_audit.as_ref(),
        },
        size,
        shading,
        &out,
    )?;

    let arms_note = pose.arm_pose.map_or_else(String::new, |arms| {
        format!(
            "; arms L {}°/R {}°",
            ftx_export::text::fnum(arms.left_abduction_deg, 2),
            ftx_export::text::fnum(arms.right_abduction_deg, 2),
        )
    });
    let skinning_note = if skinning.is_lbs() {
        String::new()
    } else {
        format!("; skinning {}", skinning.as_str())
    };
    let hands_note = if hands.is_wrist() {
        String::new()
    } else {
        format!("; hand collider {}", hands.as_str())
    };
    let shading_note = if shading == Shading::Standard {
        String::new()
    } else {
        format!("; shading {}", shading.name())
    };
    let mut text = format!(
        "{} vertices at {spacing} mm; start strain p95 {:.3} after {} iterations (target {RELAX_TARGET_P95}); inside body: wrap {}, start {}{arms_note}{skinning_note}{hands_note}{shading_note}\n",
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
    if let Some(message) = report["authored_hand_surface"]["refusal"]["message"].as_str() {
        text.push_str(&format!(
            "  AUTHORED FULL-HAND POSE NOT ADMITTED: {message}\n"
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
    for stage in ["wrap", "start"] {
        let check = &report["display_contact"][stage];
        if let Some(message) = check["refusal"]["message"].as_str() {
            let pieces: Vec<&str> = check["cloth_instances"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            let pieces = if pieces.is_empty() {
                String::new()
            } else {
                format!(" (cloth of {})", pieces.join(" and "))
            };
            text.push_str(&format!(
                "  FULL DISPLAY CROSSING in {stage}: {message}{pieces}\n"
            ));
        } else if let Some(message) = check["unavailable"]["message"].as_str() {
            text.push_str(&format!(
                "  full display check unavailable in {stage}: {message}\n"
            ));
        }
    }
    if let Some(message) = report["display_contact"]["unavailable"]["message"].as_str() {
        text.push_str(&format!("  full display check unavailable: {message}\n"));
    }
    if let Some(note) = report["display_contact"]["start"]["note"].as_str() {
        text.push_str(&format!("    {note}\n"));
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
    let tight = tight_instances(
        report["instances"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|i| Some((i["instance"].as_str()?, &i["start_edge_strain"]))),
    );
    if !tight.is_empty() {
        // The first line above gives the p95 and the target; repeat the consequence here, where it applies.
        let unrelaxed = if report["start"]["reached_target"] == false {
            format!(
                " The relaxation stopped above its target, so stretch next to a seam can be unfinished \
                 relaxation and not a piece that is too small.{UNRELAXED_CHECK}"
            )
        } else {
            String::new()
        };
        text.push_str(&format!(
            "  TIGHT START: stretched 15 % or more (red in start/still_strain_*) in {}. If that cloth has to \
             pass round a wider part of the body there, a drape may not be able to begin: end the piece \
             short of that part or widen it at that edge.{unrelaxed}\n",
            tight.join("; ")
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
    use super::{
        cloth_crossings, deepest_mm, mean_strain, named_cloth_triangles, piece_slots, slot_colour,
        start_condition,
    };
    use ftx_core::Vec3;

    /// Width and height from a PNG's header.
    fn png_size(path: &std::path::Path) -> (usize, usize) {
        let bytes = std::fs::read(path).unwrap();
        assert!(
            bytes.starts_with(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"),
            "{}",
            path.display()
        );
        let field = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        (field(16), field(20))
    }

    #[test]
    fn placement_preview_reports_display_only_obstacles_and_unavailable_checks() {
        use serde_json::json;

        // An independently specified upright panel and a finger triangle that crosses its
        // interior. The collision proxy omits the finger entirely.
        let panel = [
            Vec3::new(-0.02, 0., 0.),
            Vec3::new(0.02, 0., 0.),
            Vec3::new(0., 0., 0.04),
        ];
        let finger = [
            Vec3::new(-0.01, -0.01, 0.01),
            Vec3::new(0.01, 0.01, 0.01),
            Vec3::new(0., 0.01, 0.03),
        ];
        let triangles = [[0, 1, 2]];
        let clear = super::display_contact_report((&[], &[]), &triangles, &panel, &panel);
        assert_eq!(clear["wrap"], json!({"crossing_free": true}));
        assert_eq!(clear["start"], json!({"crossing_free": true}));
        let moved: Vec<_> = panel.iter().map(|p| *p + Vec3::new(0., 0.03, 0.)).collect();
        let checked =
            super::display_contact_report((&finger, &triangles), &triangles, &panel, &moved);
        assert_eq!(checked["wrap"]["crossing_free"], false);
        assert!(
            checked["wrap"]["refusal"]["message"]
                .as_str()
                .unwrap()
                .contains("displayed body")
        );
        assert_eq!(checked["start"]["crossing_free"], true);
        let malformed =
            super::display_contact_report((&finger, &[[0, 1, 3]]), &triangles, &panel, &panel);
        assert!(malformed.get("unavailable").is_some());
        assert!(malformed.get("start").is_none());
        let malformed_cloth =
            super::display_contact_report((&finger, &triangles), &[[0, 1, 3]], &panel, &panel);
        assert!(malformed_cloth["start"].get("unavailable").is_some());
        assert!(malformed_cloth["start"].get("crossing_free").is_none());
    }

    #[test]
    fn placement_report_reconstructs_transformed_wrap_on_both_sides() {
        use ftx_drape::scene::{PartFrames, placements, wrapped_positions};
        use ftx_drape::{PartFrame, build_garment};
        use serde_json::{Value, json};

        struct Form;
        impl PartFrames for Form {
            fn frame(&self, _: &str) -> Option<PartFrame> {
                Some(PartFrame {
                    origin: Vec3::new(-100., 12., 400.),
                    up: Vec3::new(0.6, 0., 0.8),
                    front: Vec3::new(0., -1., 0.),
                    lat: Vec3::new(0.8, 0., -0.6),
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
                "id": "sleeve", "cut": {"qty": 2},
                "vertices": {"a": [0, 0], "b": [20, 0], "c": [20, 20], "d": [0, 20]},
                "edges": [
                    {"id": "bottom", "from": "a", "to": "b"}, {"id": "right", "from": "b", "to": "c"},
                    {"id": "top", "from": "c", "to": "d"}, {"id": "left", "from": "d", "to": "a"}
                ],
                "placement": {"part": "arm", "radius_mm": 40., "x0": 10., "y0": 7.,
                    "frame_offset_mm": [8., -3., 2.],
                    "frame_rotation": {"axis": [1., 0., 0.], "angle_deg": 12.}}
            }]
        }));
        let mesh = build_garment(&pattern, 10.).unwrap();
        let places = placements(&pattern, &mesh, &Form).unwrap();
        let positions = wrapped_positions(&mesh, &places);
        let depth = vec![0.; mesh.verts2d.len()];
        let report = super::instance_report(
            &pattern,
            &mesh,
            &places,
            &piece_slots(&mesh.instances),
            &depth,
            &depth,
            &vec![Value::Null; mesh.instances.len()],
        );
        assert_eq!(mesh.instances, ["sleeve.R", "sleeve.L"]);
        let vector = |v: &Value| {
            Vec3::new(
                v[0].as_f64().unwrap(),
                v[1].as_f64().unwrap(),
                v[2].as_f64().unwrap(),
            )
        };
        for (k, item) in report.iter().enumerate() {
            assert_eq!(item["frame_offset_mm"], json!([8., -3., 2.]));
            assert_eq!(item["frame_rotation"]["angle_deg"], 12.);
            let frame = &item["world_frame"];
            assert!(frame["own_side_clamp"].is_null());
            let (lo, hi) = mesh.inst_range[k];
            for (i, actual) in positions.iter().enumerate().take(hi).skip(lo) {
                let q = mesh.verts2d[i];
                let x = frame["drafted_x_sign"].as_f64().unwrap() * q.x;
                let radius = frame["radius_mm"].as_f64().unwrap();
                let angle = frame["theta_deg"].as_f64().unwrap().to_radians()
                    + (x - frame["x0_mm"].as_f64().unwrap()) / radius;
                let (sin, cos) = ftx_core::det::sin_cos(angle);
                let recovered = vector(&frame["origin_mm"])
                    + vector(&frame["up"]) * (frame["h0_mm"].as_f64().unwrap() + q.y)
                    + (vector(&frame["theta_90"]) * sin + vector(&frame["theta_0"]) * cos) * radius;
                assert!((recovered * 0.001 - *actual).norm() < 1e-15);
            }
        }
        assert_eq!(
            report[0]["world_frame"]["origin_mm"][0].as_f64(),
            report[1]["world_frame"]["origin_mm"][0]
                .as_f64()
                .map(|x| -x)
        );
    }

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
            start_crossing_faces: None,
            start_fallback: None,
            start_unrelaxed_p95: None,
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

        // Not the default 960 by 540: the pictures are drawn at the size the caller asked for.
        let size = (420, 300);
        let shown = super::refused_stills(refusal(), Some(&state), &out, body, size);
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
            json!({"phase":"sew","accepted_steps":2,"width":420,"height":300})
        );
        assert_eq!(png_size(&out.join("refused/still_front.png")), size);
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

        // A full-display refusal names its cloth by triangle index. The pictures mark that triangle and
        // the first repair says whose it is, so a reader need not know the mesh.
        let crossing_message =
            "physical frame 87: cloth triangle 1 crosses displayed body triangle 0";
        let crossing_out = out.join("crossing");
        std::fs::create_dir_all(&crossing_out).unwrap();
        let crossing = super::refused_stills(
            Refusal::new(RefusalCode::IntersectingTrajectory, crossing_message)
                .repair("move the hand or foot away from the garment"),
            Some(&state),
            &crossing_out,
            body,
            size,
        );
        assert_eq!(crossing.repairs.len(), 2, "{:?}", crossing.repairs);
        assert!(
            crossing.repairs[0].ends_with(
                " Magenta marks the cloth within 30 mm of the cloth triangle the refusal names, which \
                 is in panel; the check stops at the first crossing it finds, so there can be others."
            ),
            "{}",
            crossing.repairs[0]
        );
        let coloured = |file: &std::path::Path, colour: &str| -> Vec<usize> {
            std::fs::read_to_string(file)
                .unwrap()
                .lines()
                .filter(|line| line.starts_with("v "))
                .enumerate()
                .filter(|(_, line)| line.ends_with(colour))
                .map(|(vertex, _)| vertex)
                .collect()
        };
        let (magenta, yellow) = (" 1 0 1", " 1 0.86 0");
        // The named triangle and the cloth round it, enough to see in a whole-figure still, and nothing
        // further than 30 mm from one of its corners.
        let marked = coloured(&crossing_out.join("refused/garment.obj"), magenta);
        let within = |corners: [usize; 3], vertex: usize| {
            corners.iter().any(|&corner| {
                let (p, q) = (state.positions[vertex], state.positions[corner]);
                let (dx, dy, dz) = (p.x - q.x, p.y - q.y, p.z - q.z);
                dx * dx + dy * dy + dz * dz <= 0.03 * 0.03 + 1e-12
            })
        };
        let corners = state.mesh.faces[1];
        assert!(corners.iter().all(|corner| marked.contains(corner)));
        assert!(marked.len() > 3 && marked.len() < state.positions.len());
        assert!(marked.iter().all(|&vertex| within(corners, vertex)));
        assert!(coloured(&crossing_out.join("refused/garment.obj"), yellow).is_empty());
        // The ordinary refusal above marked nothing.
        for colour in [magenta, yellow] {
            assert!(coloured(&out.join("refused/garment.obj"), colour).is_empty());
        }
        // Cloth crossing cloth takes the other colour, and both triangles are marked and owned.
        let cloth_out = out.join("cloth");
        std::fs::create_dir_all(&cloth_out).unwrap();
        let cloth = super::refused_stills(
            Refusal::new(
                RefusalCode::IntersectingTrajectory,
                "physical frame 90: cloth triangle 0 crosses cloth triangle 1",
            ),
            Some(&state),
            &cloth_out,
            body,
            size,
        );
        assert!(
            cloth.repairs[0].ends_with(
                " Yellow marks the cloth within 30 mm of the cloth triangles the refusal names, which \
                 are in panel; the check stops at the first crossing it finds, so there can be others."
            ),
            "{}",
            cloth.repairs[0]
        );
        let marked = coloured(&cloth_out.join("refused/garment.obj"), yellow);
        for corners in [state.mesh.faces[0], state.mesh.faces[1]] {
            assert!(corners.iter().all(|corner| marked.contains(corner)));
        }
        assert!(coloured(&cloth_out.join("refused/garment.obj"), magenta).is_empty());
        // With nowhere to write, the piece is still named.
        let unwritten = super::refused_stills(
            Refusal::new(RefusalCode::IntersectingTrajectory, crossing_message),
            Some(&state),
            &crossing_out,
            body,
            size,
        );
        assert!(
            unwritten.repairs[0].ends_with(
                " They would have marked the cloth within 30 mm of the cloth triangle the refusal \
                 names, which is in panel."
            ),
            "{}",
            unwritten.repairs[0]
        );
        // `ftx place` puts the same owner beside its own display check, per stage.
        let contact = super::with_crossing_pieces(
            json!({
                "start": {"crossing_free": false, "refusal": {"message": crossing_message}},
                "wrap": {"crossing_free": true},
            }),
            &state.mesh,
        );
        assert_eq!(contact["start"]["cloth_instances"], json!(["panel"]));
        assert!(
            contact["start"]["note"]
                .as_str()
                .unwrap()
                .contains("not untangled")
        );
        assert!(contact["wrap"].get("cloth_instances").is_none());
        assert_eq!(
            super::with_crossing_pieces(
                json!({"unavailable": {"message": "bad mesh"}}),
                &state.mesh
            ),
            json!({"unavailable": {"message": "bad mesh"}})
        );

        // Nothing to show: the refusal comes back exactly as it was.
        assert_eq!(
            super::refused_stills(refusal(), None, &out, body, size).repairs,
            ["try --time-subdivision"]
        );
        // Nowhere to write it: the refusal says so where it would have said where to look, and keeps
        // its own repairs.
        for unwritten in [
            super::refused_stills(
                refusal(),
                Some(&state),
                &out.join("missing/parent"),
                body,
                size,
            ),
            // `refused/` already exists here, and a run's files are never overwritten.
            super::refused_stills(refusal(), Some(&state), &out, body, size),
        ] {
            assert_eq!(unwritten.repairs.len(), 2, "{:?}", unwritten.repairs);
            assert!(
                unwritten.repairs[0].starts_with(
                    "The pictures of the state this refusal is about (the cloth after 2 accepted sew \
                     step(s)) could not be written: "
                ),
                "{}",
                unwritten.repairs[0]
            );
            assert_eq!(unwritten.repairs[1], "try --time-subdivision");
        }
    }

    #[test]
    fn a_start_self_contact_refusal_marks_untangled_pair_touch_and_start_condition() {
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
        // Stretched slightly in x so `start_condition` fires on an unrelaxed `"start"` refusal.
        let positions: Vec<Vec3> = mesh
            .verts2d
            .iter()
            .map(|p| Vec3::new(p.x * 0.001 * 1.4, -0.2, 1.0 + p.y * 0.001))
            .collect();
        let body = b"v -0.2 0 0.8\nv 0.3 0 0.8\nv 0.05 0 1.3\nf 1 2 3\n";
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let out = std::env::temp_dir().join(format!("ftx-start-sc-{}-{time}", std::process::id()));
        let size = (320, 200);
        let coloured = |file: &std::path::Path, colour: &str| -> Vec<usize> {
            std::fs::read_to_string(file)
                .unwrap()
                .lines()
                .filter(|line| line.starts_with("v "))
                .enumerate()
                .filter(|(_, line)| line.ends_with(colour))
                .map(|(vertex, _)| vertex)
                .collect()
        };
        let (magenta, yellow) = (" 1 0 1", " 1 0.86 0");

        // 1. Triangle-crossing start refusal: raw welded indices (1991, 2111) in the message are
        // ignored in favour of source `start_crossing_faces`, yellow marks the pair, and an
        // unrelaxed start still emits its start-condition repair.
        let cross_dir = out.join("cross");
        std::fs::create_dir_all(&cross_dir).unwrap();
        let state = ftx_drape::run::RefusedRun {
            mesh: mesh.clone(),
            positions: positions.clone(),
            phase: "start",
            accepted_steps: 0,
            start_crossing_faces: Some([0, 1]),
            start_fallback: None,
            start_unrelaxed_p95: Some(0.08),
        };
        let shown = super::refused_stills(
            Refusal::new(
                RefusalCode::IntersectingStart,
                "cloth triangles 1991 and 2111 intersect beyond their shared mesh feature",
            )
            .repair("separate the overlapping cloth layers before enabling self-contact"),
            Some(&state),
            &cross_dir,
            body,
            size,
        );
        assert_eq!(shown.repairs.len(), 3, "{:?}", shown.repairs);
        assert!(
            shown.repairs[0].contains("the untangled start rejected at cloth contact admission")
                && shown.repairs[0].contains(
                    "Yellow marks the cloth within 30 mm of the cloth triangles the refusal names, \
                     which are in panel;"
                ),
            "{}",
            shown.repairs[0]
        );
        assert!(
            shown.repairs[1].contains("Before any sewing step this start was already strained")
                && shown.repairs[1].contains("panel"),
            "{}",
            shown.repairs[1]
        );
        let (map, _) = crate::fold::document(&cross_dir.join("refused/mesh_map.json")).unwrap();
        assert_eq!(
            map["refused_pose"],
            json!({
                "phase": "start",
                "accepted_steps": 0,
                "width": 320,
                "height": 200,
                "crossing_faces": [0, 1]
            })
        );
        let marked = coloured(&cross_dir.join("refused/garment.obj"), yellow);
        for corners in [mesh.faces[0], mesh.faces[1]] {
            assert!(corners.iter().all(|c| marked.contains(c)));
        }
        assert!(coloured(&cross_dir.join("refused/garment.obj"), magenta).is_empty());

        // 2. Thickness-gap touch/overlap start refusal: honest touch/overlap wording and yellow mark.
        let touch_dir = out.join("touch");
        std::fs::create_dir_all(&touch_dir).unwrap();
        let touch_state = ftx_drape::run::RefusedRun {
            mesh: mesh.clone(),
            positions: positions.clone(),
            phase: "start",
            accepted_steps: 0,
            start_crossing_faces: Some([1, 0]),
            start_fallback: None,
            start_unrelaxed_p95: None,
        };
        let touched = super::refused_stills(
            Refusal::new(
                RefusalCode::IntersectingStart,
                "cloth layers touch or overlap at SelfVertexTri { cv: 4, ct: 0 } (gap -0.0005 m)",
            ),
            Some(&touch_state),
            &touch_dir,
            body,
            size,
        );
        assert!(
            touched.repairs[0].contains(
                "Yellow marks the cloth within 30 mm of the touching or overlapping cloth the \
                 refusal names, which is in panel;"
            ),
            "{}",
            touched.repairs[0]
        );
        assert!(!coloured(&touch_dir.join("refused/garment.obj"), yellow).is_empty());

        // 3. Out-of-bounds `start_crossing_faces` never panics and is omitted from mesh_map.json.
        let oob_dir = out.join("oob");
        std::fs::create_dir_all(&oob_dir).unwrap();
        let oob_state = ftx_drape::run::RefusedRun {
            mesh,
            positions,
            phase: "start",
            accepted_steps: 0,
            start_crossing_faces: Some([9999, 9998]),
            start_fallback: None,
            start_unrelaxed_p95: None,
        };
        let oob = super::refused_stills(
            Refusal::new(
                RefusalCode::IntersectingStart,
                "cloth triangles 9999 and 9998",
            ),
            Some(&oob_state),
            &oob_dir,
            body,
            size,
        );
        assert!(
            !oob.repairs[0].contains("Yellow marks"),
            "{}",
            oob.repairs[0]
        );
        let (oob_map, _) = crate::fold::document(&oob_dir.join("refused/mesh_map.json")).unwrap();
        assert!(oob_map["refused_pose"].get("crossing_faces").is_none());
    }

    #[test]
    fn a_start_stretched_along_one_edge_is_named_by_piece_and_pattern_edge() {
        use ftx_core::{Refusal, RefusalCode};
        use serde_json::json;

        let pattern = ftx_pattern::Pattern::new(json!({
            "schema": "fabric-pattern/1", "units": "mm",
            "pieces": [{
                "id": "panel", "cut": {"qty": 1}, "grain": [[10, 10], [10, 40]],
                "vertices": {"a": [0, 0], "b": [200, 0], "c": [200, 160], "d": [0, 160]},
                "edges": [
                    {"id": "bottom", "from": "a", "to": "b"}, {"id": "right", "from": "b", "to": "c"},
                    {"id": "top", "from": "c", "to": "d"}, {"id": "left", "from": "d", "to": "a"}
                ],
                "placement": {"part": "torso"}
            }],
            "seams": []
        }));
        let mesh = ftx_drape::build_garment(&pattern, 10.0).unwrap();
        // Upright in front of a one-triangle "body", in metres. `widen` draws the lowest 20 mm that much
        // wider than it was cut: a hem forced round something larger than itself.
        let standing = |widen: f64| -> Vec<Vec3> {
            mesh.verts2d
                .iter()
                .map(|p| {
                    let scale = if p.y < 20.0 { widen } else { 1.0 };
                    Vec3::new((p.x - 100.0) * scale * 0.001, -0.2, 1.0 + p.y * 0.001)
                })
                .collect()
        };

        // The summary a placement report carries: nothing red when the cloth is as it was cut.
        let (_, loose) = super::start_strain(&mesh, &standing(1.0));
        assert_eq!(loose[0]["stretched_past_ramp_pct"], 0.0);
        assert!(loose[0]["stretched_nearest_edge"].is_null());
        let (_, tight) = super::start_strain(&mesh, &standing(1.5));
        assert!(
            tight[0]["stretched_past_ramp_pct"].as_f64().unwrap() >= super::TIGHT_START_PCT,
            "{}",
            tight[0]
        );
        assert_eq!(tight[0]["stretched_nearest_edge"], "bottom");

        let body = b"v -0.2 0 0.8\nv 0.3 0 0.8\nv 0.05 0 1.3\nf 1 2 3\n";
        let refusal = || {
            Refusal::new(RefusalCode::NotConverged, "Newton did not converge")
                .repair("increase the Newton iteration budget")
        };
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let out = std::env::temp_dir().join(format!("ftx-tight-{}-{time}", std::process::id()));
        let shown = |name: &str, widen: f64, accepted_steps: usize, start_fallback| {
            let dir = out.join(name);
            std::fs::create_dir_all(&dir).unwrap();
            let state = ftx_drape::run::RefusedRun {
                mesh: mesh.clone(),
                positions: standing(widen),
                phase: "sew",
                accepted_steps,
                start_crossing_faces: None,
                start_fallback,
                // The relaxation reached its target, so the diagnosis carries no caveat about it.
                start_unrelaxed_p95: None,
            };
            super::refused_stills(refusal(), Some(&state), &dir, body, (320, 200))
        };

        // Refused before any sewing step, with the hem stretched: the piece and its edge come right after
        // where to look, ahead of the solver's own advice.
        let tight = shown("tight", 1.5, 0, None);
        assert_eq!(tight.repairs.len(), 3, "{:?}", tight.repairs);
        assert!(tight.repairs[0].contains("still_*.png"));
        assert!(
            tight.repairs[1].starts_with(
                "Before any sewing step this start was already strained. It is stretched 15 % or more"
            ) && tight.repairs[1].contains(" in panel (")
                && tight.repairs[1].contains(" % of it, nearest its edge bottom). If that cloth")
                && tight.repairs[1].ends_with("No solver budget changes the start."),
            "{}",
            tight.repairs[1]
        );
        assert_eq!(tight.repairs[2], "increase the Newton iteration budget");
        // The diagnosis is text, so it survives pictures that cannot be written (`refused/` now exists).
        let unwritten = shown("tight", 1.5, 0, None);
        assert_eq!(unwritten.repairs.len(), 3, "{:?}", unwritten.repairs);
        assert!(unwritten.repairs[0].contains("could not be written: "));
        assert_eq!(unwritten.repairs[1], tight.repairs[1]);

        // A start that is an earlier checkpoint says so, with both iteration counts and where the fully
        // relaxed one can be seen, whether or not anything is red.
        let fallback = shown("fallback", 1.0, 0, Some((400, 600)));
        assert_eq!(fallback.repairs.len(), 3, "{:?}", fallback.repairs);
        assert!(
            fallback.repairs[1].starts_with(
                "This start is the relaxation checkpoint at 400 iterations, not the fully relaxed start \
                 at 600:"
            ) && fallback.repairs[1].contains("ftx place draws the fully relaxed start")
                && !fallback.repairs[1].contains("stretched 15 %"),
            "{}",
            fallback.repairs[1]
        );
        let both = shown("both", 1.5, 0, Some((400, 600)));
        assert!(
            both.repairs[1].contains("checkpoint at 400 iterations")
                && both.repairs[1].contains("nearest its edge bottom)"),
            "{}",
            both.repairs[1]
        );

        // Cloth as it was cut says nothing about size, and neither does a state reached after a step:
        // seams that are still closing stretch cloth too.
        assert_eq!(shown("loose", 1.0, 0, None).repairs.len(), 2);
        assert_eq!(shown("later", 1.5, 1, Some((400, 600))).repairs.len(), 2);
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
            mesh: ftx_drape::MeshOptions::default(),
            start_self_repulsion: None,
            elastic_ea: ftx_drape::run::DrapeOptions::default().elastic_ea,
            frames: &Form,
            collision: (&vertices, &triangles),
            display: (&vertices, &triangles),
            skinning: ftx_body::SkinningMode::LinearBlend,
            hands: ftx_body::HandCollider::Palm,
            arm_pose: None,
            thumbs: None,
            thumb_audit: None,
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

        let size = (420, 300);
        let shown = super::start_stills(refusal(), &staging, &out, size);
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
        // Both stages are drawn at the asked size, and the map a pixel is looked up in says the same.
        assert_eq!(map["refused_pose"]["width"], 420);
        assert_eq!(map["refused_pose"]["height"], 300);
        assert_eq!(png_size(&dir.join("start/still_front.png")), size);
        assert_eq!(png_size(&dir.join("wrap/still_front.png")), size);
        assert!(
            map["edges"]
                .as_object()
                .unwrap()
                .keys()
                .any(|edge| edge.starts_with("panel/"))
        );
        assert_eq!(placement["body_surface"]["admitted"], true);
        assert!(placement.get("skinning").is_none());
        // A collider that is not the reference's is named; this staging said its palms were kept.
        assert_eq!(placement["hand_collider"], "palm");
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
            super::start_stills(refusal(), &staging, &out, size),
            super::start_stills(refusal(), &staging, &out.join("missing/parent"), size),
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

    #[test]
    fn a_crossing_refusal_names_its_cloth_triangles() {
        let body = "physical frame 87: cloth triangle 7867 crosses displayed body triangle 4360";
        assert_eq!(named_cloth_triangles(body, 8000), [7867]);
        // An index outside this garment names nothing here, so nothing is marked.
        assert!(named_cloth_triangles(body, 7867).is_empty());
        let cloth = "physical frame 69: cloth triangle 45 crosses cloth triangle 11649";
        assert_eq!(named_cloth_triangles(cloth, 12000), [45, 11649]);
        assert_eq!(named_cloth_triangles(cloth, 100), [45]);
        // One triangle named twice is one triangle; any other message names none.
        assert_eq!(
            named_cloth_triangles("cloth triangle 3 crosses cloth triangle 3", 9),
            [3]
        );
        assert!(
            named_cloth_triangles("Newton did not converge after 300 iterations", 9).is_empty()
        );
        assert!(named_cloth_triangles("cloth triangle crosses the body", 9).is_empty());
    }

    #[test]
    fn start_condition_names_the_checkpoint_on_fallback_and_keeps_it_without_fallback() {
        assert!(start_condition(None, &[], None).is_none());
        // A relaxation that stopped above its target is not, alone, a reason to speak.
        assert!(start_condition(None, &[], Some(0.194)).is_none());

        let tight = vec!["sleeve.R (22 % of it, nearest its edge cuff)".to_string()];
        let no_fb = start_condition(None, &tight, None).unwrap();
        assert!(no_fb.contains("Before any sewing step this start was already strained."));
        assert!(no_fb.contains("It is stretched 15 % or more"));
        assert!(!no_fb.contains("The checkpoint is stretched"));
        assert!(!no_fb.contains("ftx place draws the fully relaxed start"));
        assert!(!no_fb.contains("unfinished relaxation"));
        assert!(!no_fb.contains("--spacing-mm"));

        // With the relaxation stopped at its cap, stretch by a seam is not evidence of a small piece,
        // and the text says how to find out which it is.
        let capped = start_condition(None, &tight, Some(0.194)).unwrap();
        assert!(
            capped.contains(
                "The start's relaxation ran its full 1600 iterations and the start is at p95 strain \
                 0.194, above the 0.12 target, so stretch next to a seam can be unfinished relaxation and \
                 not a piece that is too small. To tell them apart, look at ftx place with a coarser \
                 --spacing-mm: if the relaxation reaches its target there, stretch that remains is not \
                 unfinished relaxation. No solver budget changes the start."
            ),
            "{capped}"
        );
        assert!(capped.ends_with("No solver budget changes the start."));
        // A checkpoint gets its own caveat, never the capped start's sentence or its mesh check. (The run
        // never passes both; the function must still not say both.)
        let checkpoint = start_condition(Some((200, 800)), &tight, Some(0.136)).unwrap();
        assert!(!checkpoint.contains("ran its full"));
        assert!(!checkpoint.contains("--spacing-mm"));

        let fb_only = start_condition(Some((200, 800)), &[], None).unwrap();
        assert!(
            fb_only.contains("checkpoint at 200 iterations, not the fully relaxed start at 800")
        );
        assert!(
            fb_only.contains("failed contact admission against cloth layers or the displayed body")
        );
        assert!(!fb_only.contains("is stretched 15 % or more"));
        assert!(!fb_only.contains("unfinished relaxation"));
        assert!(fb_only.contains(
            "ftx place draws the fully relaxed start: look there for cloth contact or a displayed-body crossing."
        ));

        let fb_tight = start_condition(Some((200, 800)), &tight, None).unwrap();
        assert!(fb_tight.contains("The checkpoint is stretched 15 % or more"));
        assert!(!fb_tight.contains("It is stretched 15 % or more"));
        // What a checkpoint's stretch can be, and where the fully relaxed start's own measure is.
        assert!(
            fb_tight.ends_with(
                " A checkpoint is less relaxed than the full start, so stretch next to a seam can be \
                 unfinished relaxation and not a piece that is too small. ftx place draws the fully \
                 relaxed start and applies the same measure to it: look there for what is still \
                 stretched, and for cloth contact or a displayed-body crossing. No solver budget changes the start."
            ),
            "{fb_tight}"
        );
        let stretch_pos = fb_tight.find("The checkpoint is stretched").unwrap();
        let place_pos = fb_tight
            .find("ftx place draws the fully relaxed start")
            .unwrap();
        assert!(stretch_pos < place_pos);
    }

    #[test]
    fn place_accepts_adaptive_mesh_and_start_self_repulsion_options() {
        let args = crate::parse(
            &[
                "place",
                "absent-pattern.json",
                "--measurements",
                "absent-meas.json",
                "--out",
                "absent-out",
                "--min-spacing-mm",
                "6",
                "--cvt-iters",
                "4",
                "--start-self-repulsion",
                "--start-repulsion-mm",
                "5",
                "--start-repulsion-stiffness",
                "0.6",
                "--start-repulsion-period",
                "5",
                "--start-repulsion-max-checks",
                "500000",
            ]
            .map(String::from),
        )
        .unwrap();
        let err = super::run(&args).unwrap_err();
        assert_eq!(err.code, ftx_core::RefusalCode::Io, "{}", err.message);
        assert!(
            !err.message.contains("has no option") && !err.message.contains("unknown option"),
            "ftx place must recognize mesh and start-self-repulsion options: {}",
            err.message
        );

        let legible_args = crate::parse(
            &[
                "place",
                "absent-pattern.json",
                "--measurements",
                "absent-meas.json",
                "--out",
                "absent-out",
                "--shading",
                "legible",
            ]
            .map(String::from),
        )
        .unwrap();
        let legible_err = super::run(&legible_args).unwrap_err();
        assert_eq!(
            legible_err.code,
            ftx_core::RefusalCode::Io,
            "{}",
            legible_err.message
        );
        assert!(
            !legible_err.message.contains("has no option")
                && !legible_err.message.contains("unknown option")
                && !legible_err.message.contains("--shading"),
            "ftx place must recognize --shading legible: {}",
            legible_err.message
        );

        let bad_shading = crate::parse(
            &[
                "place",
                "absent-pattern.json",
                "--measurements",
                "absent-meas.json",
                "--out",
                "absent-out",
                "--shading",
                "glossy",
            ]
            .map(String::from),
        )
        .unwrap();
        let bad_shading_err = super::run(&bad_shading).unwrap_err();
        assert_eq!(bad_shading_err.code, ftx_core::RefusalCode::InvalidDocument);
        assert!(
            bad_shading_err.message.contains("standard or legible"),
            "{}",
            bad_shading_err.message
        );
    }

    #[test]
    fn place_stages_render_legible_and_standard_stills_deterministically() {
        use super::Staging;
        use ftx_body::{HandCollider, SkinningMode};
        use ftx_drape::{MeshOptions, render::Shading, scene::PartFrames, start::PartFrame};
        use ftx_pattern::Pattern;
        use serde_json::json;

        struct BoxFrames;
        impl PartFrames for BoxFrames {
            fn frame(&self, part: &str) -> Option<PartFrame> {
                (part == "torso").then_some(PartFrame {
                    origin: Vec3::new(0.0, 0.0, 0.5),
                    up: Vec3::new(0.0, 0.0, 1.0),
                    front: Vec3::new(0.0, -1.0, 0.0),
                    lat: Vec3::new(1.0, 0.0, 0.0),
                })
            }
            fn landmark_h(&self, _: &str, _: &str) -> Option<f64> {
                Some(0.5)
            }
            fn section_radius(&self, _: &str, _: f64) -> f64 {
                0.08
            }
        }
        let pattern = Pattern::new(json!({
            "schema": "fabric-pattern/1",
            "units": "mm",
            "pieces": [{
                "id": "panel",
                "cut": {"qty": 1},
                "vertices": {"a": [0, 0], "b": [80, 0], "c": [80, 60], "d": [0, 60]},
                "edges": [
                    {"id": "bottom", "from": "a", "to": "b"},
                    {"id": "right", "from": "b", "to": "c"},
                    {"id": "top", "from": "c", "to": "d"},
                    {"id": "left", "from": "d", "to": "a"}
                ],
                "notches": [],
                "grain": [[40, 10], [40, 50]],
                "placement": {"part": "torso", "radius_mm": 90.0, "theta_deg": 0.0, "x0": 40.0}
            }],
            "seams": []
        }));
        let body_verts = vec![
            Vec3::new(-0.04, 0.0, 0.0),
            Vec3::new(0.04, 0.0, 0.0),
            Vec3::new(0.0, 0.04, 1.0),
        ];
        let body_tris = vec![[0, 1, 2]];
        let frames = BoxFrames;
        let staging = Staging {
            pattern: &pattern,
            label: "panel.json",
            spacing: 25.0,
            mesh: MeshOptions::default(),
            start_self_repulsion: None,
            elastic_ea: 0.0,
            frames: &frames,
            collision: (&body_verts, &body_tris),
            display: (&body_verts, &body_tris),
            skinning: SkinningMode::LinearBlend,
            hands: HandCollider::Wrist,
            arm_pose: None,
            thumbs: None,
            thumb_audit: None,
        };
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("ftx-place-shading-{}-{stamp}", std::process::id()));
        let std_dir = root.join("std");
        let leg_dir = root.join("leg");
        std::fs::create_dir_all(&std_dir).unwrap();
        std::fs::create_dir_all(&leg_dir).unwrap();

        let (std_rep, _) = super::stages(&staging, (32, 24), Shading::Standard, &std_dir).unwrap();
        let (leg_rep, _) = super::stages(&staging, (32, 24), Shading::Legible, &leg_dir).unwrap();
        assert!(std_rep.get("shading").is_none());
        assert_eq!(leg_rep["shading"], "legible");
        for stage in ["wrap", "start"] {
            let std_files = std_rep["stills"][stage].as_array().unwrap();
            let leg_files = leg_rep["stills"][stage].as_array().unwrap();
            assert_eq!(std_files.len(), 6);
            assert_eq!(leg_files.len(), 6);
            assert!(
                std_files
                    .iter()
                    .all(|p| !p.as_str().unwrap().ends_with("_legible.png"))
            );
            assert!(
                leg_files
                    .iter()
                    .all(|p| p.as_str().unwrap().ends_with("_legible.png"))
            );
            let a = std::fs::read(std_files[0].as_str().unwrap()).unwrap();
            let b = std::fs::read(leg_files[0].as_str().unwrap()).unwrap();
            assert_eq!(&b[..8], b"\x89PNG\r\n\x1a\n");
            assert_ne!(a, b);
        }
    }
}

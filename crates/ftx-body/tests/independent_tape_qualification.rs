//! Independent geometric qualification of all 21 `TAPE_FIELDS` functionals and posed-collider
//! volume/self-intersection diagnostics (`ftx-onn.16`).
//!
//! Zero plane-slicing, loop-assembly, polygon-containment, convex-hull or polyline-integration code
//! is imported from `ftx_body::tape`. Instead this suite implements:
//! 1. Direct oriented-plane triangle slicing + `BTreeMap` adjacency loop chaining;
//! 2. Dan Sunday's integer winding-number point-in-polygon test;
//! 3. Jarvis-march (gift-wrapping) 2D convex hull;
//! 4. Closed-form analytic verification on a 5-shell stepped rectangular mannequin covering every
//!    one of the 21 `TAPE_FIELDS` functionals, plus missing-section refusal controls;
//! 5. Admitted `anny-0.6.1-default.ftxbody` cross-checks across 6 diverse ages/phenotypes (126
//!    readings), landmark perturbation sensitivity bounds, and posed-collider LBS volume-collapse /
//!    self-intersection sweeps across standing, arms-up, seated, and squat postures.

use std::collections::{BTreeMap, BTreeSet};

use ftx_body::{
    AnnyData, AnnyModel, BodyFrames, DEFAULT_POSE_AUDIT_PAIR_BUDGET, FREE, Frame, Legs,
    N_PHENOTYPE, N_TAPE, Phenotype, Pose, PosedBody, TAPE_FIELDS, TapeTarget, Wearer,
    age_parameter_for_years, clothing_skin, default_model_path, fit_stature, fit_tape, is_girth,
    partial_tape_readings, pose_deltas, skin_mesh, tape_readings, tape_targets,
};
use ftx_core::RefusalCode;
use ftx_core::Vec3;
use ftx_core::receipt::sha256_hex;
use serde_json::json;

// ---------------------------------------------------------------------------
// Independent geometric primitives (zero code shared with `ftx_body::tape`)
// ---------------------------------------------------------------------------

fn plane_loops(
    vertices: &[Vec3],
    faces: &[[usize; 3]],
    origin: Vec3,
    axis_u: Vec3,
    axis_v: Vec3,
    normal: Vec3,
    level: f64,
) -> Vec<Vec<[f64; 2]>> {
    let height = |idx: usize| (vertices[idx] - origin).dot(normal);
    let above = |idx: usize| height(idx) >= level;

    let mut raw_segments: Vec<[(usize, usize); 2]> = Vec::new();
    let mut adjacency: BTreeMap<(usize, usize), Vec<usize>> = BTreeMap::new();
    for tri in faces {
        let mut crossed = Vec::with_capacity(2);
        for k in 0..3 {
            let a = tri[k];
            let b = tri[(k + 1) % 3];
            if above(a) != above(b) {
                crossed.push((a.min(b), a.max(b)));
            }
        }
        if crossed.len() == 2 {
            let seg_idx = raw_segments.len();
            let pair = [crossed[0], crossed[1]];
            raw_segments.push(pair);
            for edge in pair {
                adjacency.entry(edge).or_default().push(seg_idx);
            }
        }
    }

    let edge_point = |(a, b): (usize, usize)| -> [f64; 2] {
        let ha = height(a);
        let hb = height(b);
        let t = (level - ha) / (hb - ha);
        let p = vertices[a] + (vertices[b] - vertices[a]) * t - origin;
        [p.dot(axis_u), p.dot(axis_v)]
    };

    let mut visited = vec![false; raw_segments.len()];
    let mut rings = Vec::new();
    for start in 0..raw_segments.len() {
        if visited[start] {
            continue;
        }
        visited[start] = true;
        let first_edge = raw_segments[start][0];
        let mut cur_edge = raw_segments[start][1];
        let mut ring = vec![edge_point(first_edge)];
        let closed = loop {
            ring.push(edge_point(cur_edge));
            let next_seg = adjacency
                .get(&cur_edge)
                .and_then(|list| list.iter().copied().find(|&s| !visited[s]));
            let Some(seg_idx) = next_seg else {
                break false;
            };
            visited[seg_idx] = true;
            let [e0, e1] = raw_segments[seg_idx];
            cur_edge = if e0 == cur_edge { e1 } else { e0 };
            if cur_edge == first_edge {
                break true;
            }
        };
        if closed && ring.len() >= 3 {
            rings.push(ring);
        }
    }
    rings
}

fn shoelace_signed_area(ring: &[[f64; 2]]) -> f64 {
    let mut acc = 0.0;
    for i in 0..ring.len() {
        let a = ring[i];
        let b = ring[(i + 1) % ring.len()];
        acc += a[0] * b[1] - b[0] * a[1];
    }
    0.5 * acc
}

/// Dan Sunday's winding-number inclusion test (`wn != 0`), independent of ray-parity crossing.
fn winding_contains(ring: &[[f64; 2]], pt: [f64; 2]) -> bool {
    let is_left = |a: [f64; 2], b: [f64; 2], p: [f64; 2]| -> f64 {
        (b[0] - a[0]) * (p[1] - a[1]) - (p[0] - a[0]) * (b[1] - a[1])
    };
    let mut wn: i32 = 0;
    for i in 0..ring.len() {
        let a = ring[i];
        let b = ring[(i + 1) % ring.len()];
        if a[1] <= pt[1] {
            if b[1] > pt[1] && is_left(a, b, pt) > 0.0 {
                wn += 1;
            }
        } else if b[1] <= pt[1] && is_left(a, b, pt) < 0.0 {
            wn -= 1;
        }
    }
    wn != 0
}

/// Jarvis march (gift-wrapping) 2D convex hull in CCW order starting at lowest `(x, y)`,
/// dropping collinear interior points by selecting the farthest point along extreme rays.
fn jarvis_convex_hull(points: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut unique = points.to_vec();
    unique.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    unique.dedup();
    if unique.len() <= 1 {
        return unique;
    }
    let cross = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| -> f64 {
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    };
    let dist2 = |a: [f64; 2], b: [f64; 2]| -> f64 {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        dx * dx + dy * dy
    };
    let start = 0usize;
    let mut hull_indices = Vec::new();
    let mut cur = start;
    loop {
        hull_indices.push(cur);
        let mut next = usize::MAX;
        for cand in 0..unique.len() {
            if cand == cur {
                continue;
            }
            if next == usize::MAX {
                next = cand;
                continue;
            }
            let turn = cross(unique[cur], unique[next], unique[cand]);
            if turn < 0.0
                || (turn == 0.0
                    && dist2(unique[cur], unique[cand]) > dist2(unique[cur], unique[next]))
            {
                next = cand;
            }
        }
        if next == usize::MAX || next == start || hull_indices.len() > unique.len() {
            break;
        }
        cur = next;
    }
    // If all points were collinear, keep only the two endpoints.
    if hull_indices.len() > 2
        && hull_indices.iter().all(|&idx| {
            cross(
                unique[hull_indices[0]],
                unique[hull_indices[1]],
                unique[idx],
            ) == 0.0
        })
    {
        return vec![unique[0], *unique.last().unwrap()];
    }
    hull_indices.into_iter().map(|i| unique[i]).collect()
}

fn polyline_len(pts: &[[f64; 2]]) -> f64 {
    pts.windows(2)
        .map(|w| ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2)).sqrt())
        .sum()
}

fn closed_perimeter(pts: &[[f64; 2]]) -> f64 {
    if pts.len() < 2 {
        return 0.0;
    }
    let mut sum = polyline_len(pts);
    let a = pts[pts.len() - 1];
    let b = pts[0];
    sum += ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
    sum
}

fn outermost_containing(loops: &[Vec<[f64; 2]>], center: [f64; 2]) -> Option<&Vec<[f64; 2]>> {
    loops
        .iter()
        .filter(|ring| winding_contains(ring, center))
        .max_by(|a, b| {
            shoelace_signed_area(a)
                .abs()
                .total_cmp(&shoelace_signed_area(b).abs())
        })
}

fn nearest_or_containing(loops: &[Vec<[f64; 2]>], center: [f64; 2]) -> Option<&Vec<[f64; 2]>> {
    if !center[0].is_finite() || !center[1].is_finite() {
        return None;
    }
    outermost_containing(loops, center).or_else(|| {
        let ring_dist = |ring: &Vec<[f64; 2]>| -> f64 {
            let mut best = f64::INFINITY;
            for i in 0..ring.len() {
                let a = ring[i];
                let b = ring[(i + 1) % ring.len()];
                let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                let l2 = dx * dx + dy * dy;
                let t = if l2 > 0.0 {
                    (((center[0] - a[0]) * dx + (center[1] - a[1]) * dy) / l2).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let d = ((a[0] + t * dx - center[0]).powi(2) + (a[1] + t * dy - center[1]).powi(2))
                    .sqrt();
                best = best.min(d);
            }
            best
        };
        loops
            .iter()
            .min_by(|a, b| ring_dist(a).total_cmp(&ring_dist(b)))
    })
}

fn line_crossings(ring: &[[f64; 2]], axis: usize, val: f64) -> Vec<[f64; 2]> {
    let other = 1 - axis;
    let mut out = Vec::new();
    for i in 0..ring.len() {
        let a = ring[i];
        let b = ring[(i + 1) % ring.len()];
        if (a[axis] >= val) != (b[axis] >= val) {
            let t = (val - a[axis]) / (b[axis] - a[axis]);
            let mut p = [0.0; 2];
            p[axis] = val;
            p[other] = a[other] + t * (b[other] - a[other]);
            out.push(p);
        }
    }
    out
}

/// Independent computation of all 21 `TAPE_FIELDS` functionals from `AnnyModel`, `rest_height_m`,
/// `PosedBody`, and `BodyFrames` without touching `ftx_body::tape`.
fn independent_tape_readings_21(
    model: &AnnyModel,
    faces: &[[usize; 3]],
    rest_height_m: f64,
    posed: &PosedBody,
    frames: &BodyFrames,
) -> [Option<f64>; N_TAPE] {
    let mm: Vec<Vec3> = posed.vertices.iter().map(|v| *v * 1000.0).collect();
    let lm = |name: &str| {
        frames
            .landmarks
            .get(name)
            .copied()
            .filter(|v| v.is_finite())
    };

    let world_origin = Vec3::ZERO;
    let x_axis = Vec3::new(1.0, 0.0, 0.0);
    let y_axis = Vec3::new(0.0, 1.0, 0.0);
    let z_axis = Vec3::new(0.0, 0.0, 1.0);
    let torso_axis = [frames.torso.origin.x, frames.torso.origin.y];

    let horizontal_torso_girth = |h_above_floor: f64| -> Option<f64> {
        let loops = plane_loops(
            &mm,
            faces,
            world_origin,
            x_axis,
            y_axis,
            z_axis,
            frames.floor_z + h_above_floor,
        );
        let ring = outermost_containing(&loops, torso_axis)?;
        Some(closed_perimeter(&jarvis_convex_hull(ring)))
    };

    let part_girth_near = |f: Frame, h: f64| -> Option<f64> {
        let loops = plane_loops(&mm, faces, f.origin, f.lat, f.front, f.up, h);
        let ring = nearest_or_containing(&loops, [0.0, 0.0])?;
        Some(closed_perimeter(&jarvis_convex_hull(ring)))
    };

    let sweep_extreme = |lo: f64, hi: f64, want_max: bool, eval: &dyn Fn(f64) -> Option<f64>| {
        let mut best: Option<f64> = None;
        let mut h = lo;
        while h <= hi {
            let v = eval(h)?;
            best = Some(best.map_or(v, |b| if want_max { b.max(v) } else { b.min(v) }));
            h += 2.0;
        }
        best
    };

    let chest = lm("chest").and_then(|h| horizontal_torso_girth(h - 10.0));
    let waist = lm("waist").and_then(horizontal_torso_girth);
    let hip = (|| {
        let (w, c) = (lm("waist")?, lm("crotch")?);
        sweep_extreme(c + 0.2 * (w - c), w, true, &horizontal_torso_girth)
    })();

    let lh = |name: &str| frames.landmark_h("leg_R", name).filter(|v| v.is_finite());
    let leg_eval = |h: f64| part_girth_near(frames.leg_r, h);
    let arm_eval = |h: f64| part_girth_near(frames.arm_r, h);

    let thigh = lh("crotch").and_then(|c| sweep_extreme(c - 40.0, c - 5.0, true, &leg_eval));
    let knee = lh("knee").and_then(leg_eval);
    let calf = (|| {
        let (k, a) = (lh("knee")?, lh("ankle")?);
        let shin = k - a;
        sweep_extreme(a + 0.3 * shin, k - 0.1 * shin, true, &leg_eval)
    })();
    let ankle = (|| {
        let (k, a) = (lh("knee")?, lh("ankle")?);
        sweep_extreme(a + 10.0, a + 0.25 * (k - a), false, &leg_eval)
    })();
    let upper_arm = arm_eval(frames.tip_h - 0.275 * frames.arm_length);
    let wrist = arm_eval(frames.tip_h - frames.arm_length);

    let back_neck_to_waist = (|| {
        let (w, c7) = (lm("waist")?, lm("c7")?);
        let mut total = 0.0;
        let mut prev: Option<(f64, f64)> = None;
        let mut h = w;
        loop {
            let loops = plane_loops(
                &mm,
                faces,
                world_origin,
                x_axis,
                y_axis,
                z_axis,
                frames.floor_z + h,
            );
            let ring = outermost_containing(&loops, torso_axis)?;
            let y = line_crossings(ring, 0, torso_axis[0])
                .into_iter()
                .map(|p| p[1])
                .fold(None, |m: Option<f64>, y| Some(m.map_or(y, |b| b.max(y))))?;
            if let Some((ph, py)) = prev {
                total += ((h - ph).powi(2) + (y - py).powi(2)).sqrt();
            }
            prev = Some((h, y));
            if h >= c7 {
                break;
            }
            h = (h + 2.0).min(c7);
        }
        Some(total)
    })();

    let neck_and_snp = (|| {
        let f = frames.neck;
        let loops = plane_loops(&mm, faces, f.origin, f.lat, f.front, f.up, 0.0);
        let ring = outermost_containing(&loops, [0.0, 0.0])?;
        let side = ring.iter().copied().min_by(|a, b| a[0].total_cmp(&b[0]))?;
        let snp = f.origin + f.lat * side[0] + f.front * side[1];
        Some((closed_perimeter(&jarvis_convex_hull(ring)), snp))
    })();

    let head = (|| {
        let top = lm("head_top")?;
        let hh = top - lm("c7")?;
        let axis = [frames.head_bone.x, frames.head_bone.y];
        let eval = |d: f64| -> Option<f64> {
            let loops = plane_loops(
                &mm,
                faces,
                world_origin,
                x_axis,
                y_axis,
                z_axis,
                frames.floor_z + top - d,
            );
            let ring = nearest_or_containing(&loops, axis)?;
            Some(closed_perimeter(&jarvis_convex_hull(ring)))
        };
        sweep_extreme(0.15 * hh, 0.50 * hh, true, &eval)
    })();

    let shoulder_and_back = neck_and_snp.and_then(|(_, snp)| {
        let j = frames.shoulder_joint_r;
        let coronal_loops = plane_loops(
            &mm,
            faces,
            Vec3::new(0.0, j.y, 0.0),
            x_axis,
            z_axis,
            y_axis,
            0.0,
        );
        let section = outermost_containing(&coronal_loops, [j.x, j.z])?;
        let top_at = |x: f64| -> Option<f64> {
            line_crossings(section, 0, x)
                .into_iter()
                .map(|p| p[1])
                .filter(|&z| z <= snp.z + 5.0)
                .fold(None, |m: Option<f64>, z| Some(m.map_or(z, |b| b.max(z))))
        };
        let acromion_z = top_at(j.x)?;
        let mut pts = vec![[snp.x, snp.z]];
        let mut x = snp.x - 2.0;
        while x > j.x {
            pts.push([x, top_at(x)?]);
            x -= 2.0;
        }
        pts.push([j.x, acromion_z]);
        let shoulder_len = polyline_len(&pts);

        let back_len = (|| {
            let loops = plane_loops(
                &mm,
                faces,
                world_origin,
                x_axis,
                y_axis,
                z_axis,
                acromion_z - 15.0,
            );
            let hull = jarvis_convex_hull(outermost_containing(&loops, torso_axis)?);
            if hull.len() < 3 {
                return None;
            }
            let max_y_at = |x: f64| -> Option<[f64; 2]> {
                line_crossings(&hull, 0, x)
                    .into_iter()
                    .max_by(|a, b| a[1].total_cmp(&b[1]))
            };
            let opp_x = 2.0 * frames.torso.origin.x - j.x;
            let a = max_y_at(j.x.min(opp_x))?;
            let b = max_y_at(j.x.max(opp_x))?;
            let left_of =
                |p: [f64; 2]| (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]) > 0.0;
            let mut chain: Vec<[f64; 2]> = hull
                .into_iter()
                .filter(|&p| p[0] > a[0] && p[0] < b[0] && left_of(p))
                .collect();
            chain.sort_by(|p, q| p[0].total_cmp(&q[0]));
            chain.insert(0, a);
            chain.push(b);
            Some(polyline_len(&chain))
        })();
        Some((shoulder_len, back_len))
    });

    let total_crotch = (|| {
        let waist_z = frames.floor_z + lm("waist")?;
        let x0 = frames.torso.origin.x;
        let mut cloud = Vec::new();
        for dx in [-15.0, -7.5, 0.0, 7.5, 15.0] {
            let loops = plane_loops(
                &mm,
                faces,
                Vec3::new(x0, 0.0, 0.0),
                y_axis,
                z_axis,
                x_axis,
                dx,
            );
            if let Some(ring) = outermost_containing(&loops, [frames.torso.origin.y, waist_z]) {
                cloud.extend(ring.iter().copied().filter(|p| p[1] <= waist_z));
                cloud.extend(line_crossings(ring, 1, waist_z));
            }
        }
        let hull = jarvis_convex_hull(&cloud);
        if hull.len() < 3 {
            return None;
        }
        let waist_corners: Vec<usize> = (0..hull.len())
            .filter(|&i| hull[i][1] >= waist_z - 1e-9)
            .collect();
        let front = *waist_corners
            .iter()
            .min_by(|&&i, &&k| hull[i][0].total_cmp(&hull[k][0]))?;
        let back = *waist_corners
            .iter()
            .max_by(|&&i, &&k| hull[i][0].total_cmp(&hull[k][0]))?;
        let mut arc = vec![hull[front]];
        let mut idx = front;
        while idx != back {
            idx = (idx + 1) % hull.len();
            arc.push(hull[idx]);
        }
        Some(polyline_len(&arc))
    })();

    let outseam = (|| {
        let waist_z = frames.floor_z + lm("waist")?;
        let crotch_z = frames.floor_z + lm("crotch")?;
        let x0 = frames.torso.origin.x;
        let loops = plane_loops(&mm, faces, world_origin, x_axis, y_axis, z_axis, waist_z);
        let side = outermost_containing(&loops, torso_axis)?
            .iter()
            .copied()
            .min_by(|a, b| a[0].total_cmp(&b[0]))?;
        let top = [side[0], waist_z];

        // Dominant-bone vertex region classification directly from `model.data()` (independent of
        // `BodyFrames`'s private point caches).
        let data = model.data();
        let parents = &data.header.labels.bone_parents;
        let subtree = |root_label: &str| -> BTreeSet<usize> {
            let mut set = BTreeSet::new();
            if let Some(r) = model.bone_index(root_label) {
                set.insert(r);
                for (j, &parent) in parents.iter().enumerate().skip(r + 1) {
                    if let Ok(p) = usize::try_from(parent)
                        && set.contains(&p)
                    {
                        set.insert(j);
                    }
                }
            }
            set
        };
        let arm_r = subtree("upperarm01.R");
        let arm_l = subtree("upperarm01.L");
        let leg_r = subtree("upperleg01.R");
        let leg_l = subtree("upperleg01.L");
        let k = data.header.counts.influences;

        let mut cloud: Vec<[f64; 2]> = (0..mm.len())
            .filter_map(|v| {
                let row = &data.vertex_bone_weights[v * k..(v + 1) * k];
                let best = (0..k).fold(0, |b, j| if row[j] > row[b] { j } else { b });
                let bone = data.vertex_bone_indices[v * k + best] as usize;
                let is_torso_or_right_leg = leg_r.contains(&bone)
                    || (!arm_r.contains(&bone) && !arm_l.contains(&bone) && !leg_l.contains(&bone));
                let p = mm[v];
                (is_torso_or_right_leg && p.x < x0 && p.z <= waist_z && p.z >= crotch_z)
                    .then_some([p.x, p.z])
            })
            .collect();
        cloud.push(top);

        let hull = jarvis_convex_hull(&cloud);
        if hull.len() < 2 {
            return None;
        }
        let start = (0..hull.len()).find(|&i| hull[i] == top)?;
        let widest = (0..hull.len()).min_by(|&i, &k| {
            hull[i][0]
                .total_cmp(&hull[k][0])
                .then(hull[i][1].total_cmp(&hull[k][1]))
        })?;
        let mut arc = vec![hull[start]];
        let mut idx = start;
        while idx != widest {
            idx = (idx + 1) % hull.len();
            arc.push(hull[idx]);
        }
        Some(polyline_len(&arc) + (hull[widest][1] - frames.floor_z))
    })();

    [
        Some(rest_height_m * 1000.0),
        chest,
        waist,
        hip,
        lm("waist"),
        lm("crotch"),
        thigh,
        knee,
        calf,
        ankle,
        upper_arm,
        wrist,
        Some(frames.arm_length),
        lm("knee"),
        back_neck_to_waist,
        neck_and_snp.map(|(g, _)| g),
        head,
        shoulder_and_back.map(|(s, _)| s),
        shoulder_and_back.and_then(|(_, b)| b),
        total_crotch,
        outseam,
    ]
    .map(|v| v.filter(|x| x.is_finite() && *x > 0.0))
}

// ---------------------------------------------------------------------------
// Analytic 5-shell stepped rectangular mannequin builder (exact closed forms)
// ---------------------------------------------------------------------------

struct TierSpec {
    z: f64,
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
    bone: u32,
}

/// Appends a closed, outward-wound stepped rectangular column across `tiers` (`z` strictly
/// non-decreasing; steps occur by repeating `z` with new bounds) and returns the vertex index
/// range of each tier's 4 corners (`CCW` from `(x_min, y_min)`).
fn append_stepped_column(
    tiers: &[TierSpec],
    vertices: &mut Vec<f64>,
    influences: &mut Vec<u32>,
    faces: &mut Vec<u32>,
) -> Vec<[usize; 4]> {
    let mut rings = Vec::with_capacity(tiers.len());
    for t in tiers {
        let base = vertices.len() / 3;
        for (x, y) in [
            (t.x_min, t.y_min),
            (t.x_max, t.y_min),
            (t.x_max, t.y_max),
            (t.x_min, t.y_max),
        ] {
            vertices.extend([x, y, t.z]);
            influences.push(t.bone);
        }
        rings.push([base, base + 1, base + 2, base + 3]);
    }
    // Bottom cap with centre vertex so `crotch_z` sees `(x_mid, y_mid, z0)`.
    let b0 = &tiers[0];
    let bot_c = (vertices.len() / 3) as u32;
    vertices.extend([
        0.5 * (b0.x_min + b0.x_max),
        0.5 * (b0.y_min + b0.y_max),
        b0.z,
    ]);
    influences.push(b0.bone);
    let r0 = rings[0].map(|i| i as u32);
    for k in 0..4 {
        faces.extend([bot_c, r0[(k + 1) % 4], r0[k]]);
    }
    // Side/step quads between consecutive rings.
    for w in rings.windows(2) {
        let a = w[0].map(|i| i as u32);
        let b = w[1].map(|i| i as u32);
        for k in 0..4 {
            let kn = (k + 1) % 4;
            faces.extend([a[k], a[kn], b[kn], a[k], b[kn], b[k]]);
        }
    }
    // Top cap with centre vertex.
    let bt = tiers.last().unwrap();
    let top_c = (vertices.len() / 3) as u32;
    vertices.extend([
        0.5 * (bt.x_min + bt.x_max),
        0.5 * (bt.y_min + bt.y_max),
        bt.z,
    ]);
    influences.push(bt.bone);
    let rt = rings.last().unwrap().map(|i| i as u32);
    for k in 0..4 {
        faces.extend([top_c, rt[k], rt[(k + 1) % 4]]);
    }
    rings
}

fn build_analytic_mannequin() -> AnnyModel {
    let bones = vec![
        "root",
        "upperarm01.R",
        "upperarm02.R",
        "lowerarm01.R",
        "wrist.R",
        "upperarm01.L",
        "upperarm02.L",
        "lowerarm01.L",
        "wrist.L",
        "upperleg01.R",
        "lowerleg01.R",
        "foot.R",
        "upperleg01.L",
        "lowerleg01.L",
        "foot.L",
        "neck01",
        "head",
    ];
    let parents = vec![-1i64, 0, 1, 2, 3, 0, 5, 6, 7, 0, 9, 10, 0, 12, 13, 0, 15];
    // Bone heads and tails (metres). Arms point straight down (-Z) so `-m.col(1)` is `+Z`.
    let joint_specs: [([f64; 3], [f64; 3]); 17] = [
        ([0.0, 0.0, 0.0], [0.0, 0.0, 0.1]),           // 0 root
        ([-0.135, 0.0, 0.820], [-0.135, 0.0, 0.750]), // 1 upperarm01.R
        ([-0.135, 0.0, 0.750], [-0.135, 0.0, 0.620]), // 2 upperarm02.R
        ([-0.135, 0.0, 0.620], [-0.135, 0.0, 0.460]), // 3 lowerarm01.R
        ([-0.135, 0.0, 0.460], [-0.135, 0.0, 0.440]), // 4 wrist.R
        ([0.135, 0.0, 0.820], [0.135, 0.0, 0.750]),   // 5 upperarm01.L
        ([0.135, 0.0, 0.750], [0.135, 0.0, 0.620]),   // 6 upperarm02.L
        ([0.135, 0.0, 0.620], [0.135, 0.0, 0.460]),   // 7 lowerarm01.L
        ([0.135, 0.0, 0.460], [0.135, 0.0, 0.440]),   // 8 wrist.L
        ([-0.055, 0.0, 0.440], [-0.055, 0.0, 0.240]), // 9 upperleg01.R
        ([-0.055, 0.0, 0.240], [-0.055, 0.0, 0.050]), // 10 lowerleg01.R
        ([-0.055, 0.0, 0.050], [-0.055, 0.0, 0.010]), // 11 foot.R
        ([0.055, 0.0, 0.440], [0.055, 0.0, 0.240]),   // 12 upperleg01.L
        ([0.055, 0.0, 0.240], [0.055, 0.0, 0.050]),   // 13 lowerleg01.L
        ([0.055, 0.0, 0.050], [0.055, 0.0, 0.010]),   // 14 foot.L
        ([0.0, 0.0, 0.855], [0.0, 0.0, 0.900]),       // 15 neck01
        ([0.0, 0.0, 0.920], [0.0, 0.0, 0.980]),       // 16 head
    ];
    let heads: Vec<f64> = joint_specs.iter().flat_map(|(h, _)| *h).collect();
    let tails: Vec<f64> = joint_specs.iter().flat_map(|(_, t)| *t).collect();

    let mut vertices = Vec::new();
    let mut influences = Vec::new();
    let mut faces = Vec::new();

    // 1. Stepped Torso + Neck + Head column (`bone = 0`, back wall `y_max = +0.055` continuous
    //    from waist `0.570` through head `1.000`).
    let torso_rings = append_stepped_column(
        &[
            TierSpec {
                z: 0.450,
                x_min: -0.095,
                x_max: 0.095,
                y_min: -0.065,
                y_max: 0.065,
                bone: 0,
            },
            TierSpec {
                z: 0.560,
                x_min: -0.095,
                x_max: 0.095,
                y_min: -0.065,
                y_max: 0.065,
                bone: 0,
            },
            TierSpec {
                z: 0.570,
                x_min: -0.080,
                x_max: 0.080,
                y_min: -0.055,
                y_max: 0.055,
                bone: 0,
            }, // waist ring (index 2)
            TierSpec {
                z: 0.620,
                x_min: -0.080,
                x_max: 0.080,
                y_min: -0.055,
                y_max: 0.055,
                bone: 0,
            },
            TierSpec {
                z: 0.620,
                x_min: -0.110,
                x_max: 0.110,
                y_min: -0.055,
                y_max: 0.055,
                bone: 0,
            }, // chest block
            TierSpec {
                z: 0.780,
                x_min: -0.110,
                x_max: 0.110,
                y_min: -0.055,
                y_max: 0.055,
                bone: 0,
            },
            TierSpec {
                z: 0.780,
                x_min: -0.145,
                x_max: 0.145,
                y_min: -0.055,
                y_max: 0.055,
                bone: 0,
            }, // shoulder shelf
            TierSpec {
                z: 0.850,
                x_min: -0.145,
                x_max: 0.145,
                y_min: -0.055,
                y_max: 0.055,
                bone: 0,
            },
            TierSpec {
                z: 0.850,
                x_min: -0.045,
                x_max: 0.045,
                y_min: -0.035,
                y_max: 0.055,
                bone: 0,
            }, // neck
            TierSpec {
                z: 0.900,
                x_min: -0.045,
                x_max: 0.045,
                y_min: -0.035,
                y_max: 0.055,
                bone: 0,
            },
            TierSpec {
                z: 0.900,
                x_min: -0.075,
                x_max: 0.075,
                y_min: -0.080,
                y_max: 0.055,
                bone: 0,
            }, // head
            TierSpec {
                z: 1.000,
                x_min: -0.075,
                x_max: 0.075,
                y_min: -0.080,
                y_max: 0.055,
                bone: 0,
            },
        ],
        &mut vertices,
        &mut influences,
        &mut faces,
    );
    let waist_loop = torso_rings[2].to_vec();

    // 2 & 3. Right and Left Arm columns (`40 x 40 mm` uniform section, `z = 0.440..0.775`,
    //        upper-arm bottom ring at `z = 0.750` so `armpit_z = 750 mm`).
    for (xc, b_upper, b_lower) in [(-0.135, 1u32, 3u32), (0.135, 5u32, 7u32)] {
        append_stepped_column(
            &[
                TierSpec {
                    z: 0.440,
                    x_min: xc - 0.020,
                    x_max: xc + 0.020,
                    y_min: -0.020,
                    y_max: 0.020,
                    bone: b_lower,
                },
                TierSpec {
                    z: 0.749,
                    x_min: xc - 0.020,
                    x_max: xc + 0.020,
                    y_min: -0.020,
                    y_max: 0.020,
                    bone: b_lower,
                },
                TierSpec {
                    z: 0.750,
                    x_min: xc - 0.020,
                    x_max: xc + 0.020,
                    y_min: -0.020,
                    y_max: 0.020,
                    bone: b_upper,
                },
                TierSpec {
                    z: 0.775,
                    x_min: xc - 0.020,
                    x_max: xc + 0.020,
                    y_min: -0.020,
                    y_max: 0.020,
                    bone: b_upper,
                },
            ],
            &mut vertices,
            &mut influences,
            &mut faces,
        );
    }

    // 4 & 5. Right and Left Leg columns (`z = 0.000..0.445`, centered at `x = ±0.055` so `|x| >= 0.020 > 0.015`
    //        sagittal crotch band, with distinct ankle/calf/knee/thigh tiers).
    for (xc, b_upper, b_lower) in [(-0.055, 9u32, 10u32), (0.055, 12u32, 13u32)] {
        append_stepped_column(
            &[
                TierSpec {
                    z: 0.000,
                    x_min: xc - 0.022,
                    x_max: xc + 0.022,
                    y_min: -0.022,
                    y_max: 0.022,
                    bone: b_lower,
                }, // ankle: 44x44 -> 176 mm
                TierSpec {
                    z: 0.120,
                    x_min: xc - 0.022,
                    x_max: xc + 0.022,
                    y_min: -0.022,
                    y_max: 0.022,
                    bone: b_lower,
                },
                TierSpec {
                    z: 0.120,
                    x_min: xc - 0.030,
                    x_max: xc + 0.030,
                    y_min: -0.030,
                    y_max: 0.030,
                    bone: b_lower,
                }, // calf: 60x60 -> 240 mm
                TierSpec {
                    z: 0.215,
                    x_min: xc - 0.030,
                    x_max: xc + 0.030,
                    y_min: -0.030,
                    y_max: 0.030,
                    bone: b_lower,
                },
                TierSpec {
                    z: 0.215,
                    x_min: xc - 0.028,
                    x_max: xc + 0.028,
                    y_min: -0.028,
                    y_max: 0.028,
                    bone: b_lower,
                }, // knee: 56x56 -> 224 mm
                TierSpec {
                    z: 0.390,
                    x_min: xc - 0.028,
                    x_max: xc + 0.028,
                    y_min: -0.028,
                    y_max: 0.028,
                    bone: b_upper,
                },
                TierSpec {
                    z: 0.390,
                    x_min: xc - 0.035,
                    x_max: xc + 0.035,
                    y_min: -0.035,
                    y_max: 0.035,
                    bone: b_upper,
                }, // thigh: 70x70 -> 280 mm
                TierSpec {
                    z: 0.445,
                    x_min: xc - 0.035,
                    x_max: xc + 0.035,
                    y_min: -0.035,
                    y_max: 0.035,
                    bone: b_upper,
                },
            ],
            &mut vertices,
            &mut influences,
            &mut faces,
        );
    }

    let n = vertices.len() / 3;
    let nfaces = faces.len() / 3;
    let bone_count = bones.len();
    let mut arrays = BTreeMap::new();
    let mut data = Vec::new();
    let mut add = |name: &str, dtype: &str, shape: Vec<usize>, bytes: Vec<u8>| {
        arrays.insert(
            name.to_string(),
            json!({
                "dtype": dtype,
                "shape": shape,
                "offset": data.len(),
                "nbytes": bytes.len(),
                "sha256": sha256_hex(&bytes),
            }),
        );
        data.extend(bytes);
    };
    let f64s = |a: Vec<f64>| a.into_iter().flat_map(f64::to_le_bytes).collect::<Vec<_>>();
    let u32s = |a: Vec<u32>| a.into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>();

    add("template_vertices", "f64", vec![n, 3], f64s(vertices));
    add("faces", "u32", vec![nfaces, 3], u32s(faces));
    add("blendshape_row_ptr", "u32", vec![2], u32s(vec![0, 0]));
    add("blendshape_vertex", "u32", vec![0], Vec::new());
    add("blendshape_delta", "f64", vec![0, 3], Vec::new());
    let mut mask = vec![0u8; 26];
    mask[1] = 1;
    add("phenotype_mask", "u8", vec![1, 26], mask);
    add(
        "template_bone_heads",
        "f64",
        vec![bone_count, 3],
        f64s(heads),
    );
    add(
        "template_bone_tails",
        "f64",
        vec![bone_count, 3],
        f64s(tails),
    );
    add(
        "bone_heads_blendshapes",
        "f64",
        vec![1, bone_count, 3],
        f64s(vec![0.0; 3 * bone_count]),
    );
    add(
        "bone_tails_blendshapes",
        "f64",
        vec![1, bone_count, 3],
        f64s(vec![0.0; 3 * bone_count]),
    );
    add(
        "bone_rolls",
        "f64",
        vec![bone_count, 3, 3],
        f64s([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0].repeat(bone_count)),
    );
    add("vertex_bone_indices", "u32", vec![n, 1], u32s(influences));
    add("vertex_bone_weights", "f64", vec![n, 1], f64s(vec![1.0; n]));
    add(
        "base_mesh_vertex_indices",
        "u32",
        vec![n],
        u32s((0..n as u32).collect()),
    );

    let mut components = vec![json!(["height", "low"]), json!(["height", "high"])];
    components.extend((0..24).map(|i| json!(["race", format!("r{i}")])));
    let header = json!({
        "schema": "ftxbody/1",
        "model": "anny",
        "anny_version": "0.6.1",
        "configuration": "analytic 21-functional mannequin",
        "licence": "synthetic",
        "package_sha256": {},
        "counts": {
            "vertices": n,
            "faces": nfaces,
            "bones": bone_count,
            "blendshapes": 1,
            "phenotype_blendshapes": 1,
            "local_changes": 0,
            "influences": 1
        },
        "phenotype": {
            "features": ["height", "race"],
            "components": components,
            "anchors": {"height": [0, 1]},
            "exposed": ["height"],
            "fixed_value": 0.5
        },
        "y_axis": [0, 1, 0],
        "degenerate_rotation": [[1, 0, 0], [0, 1, 0], [0, 0, 1]],
        "labels": {
            "blendshapes": ["height-high"],
            "bones": bones,
            "bone_parents": parents,
            "local_changes": []
        },
        "waist_loop_final": waist_loop,
        "arrays": arrays
    });
    let mut h = serde_json::to_vec(&header).unwrap();
    while !h.len().is_multiple_of(8) {
        h.push(b' ');
    }
    let mut bytes = b"FTXBODY\0".to_vec();
    bytes.extend(1u32.to_le_bytes());
    bytes.extend(0u32.to_le_bytes());
    bytes.extend((h.len() as u64).to_le_bytes());
    bytes.extend(h);
    bytes.extend(data);

    let anny_data = AnnyData::from_bytes(&bytes, true).unwrap();
    AnnyModel::from_data(anny_data).unwrap()
}

#[test]
fn analytic_articulated_mannequin_recovers_all_21_closed_form_tape_functionals_and_refuses_missing_sections()
 {
    let model = build_analytic_mannequin();
    let params = model.params(Phenotype::default());
    let rest = model.rest(&params).unwrap();
    let posed = model
        .pose(
            &rest,
            &pose_deltas(&model, &rest, &Pose::rest(), 1.0).unwrap(),
        )
        .unwrap();
    let frames = BodyFrames::new(&model, &posed).unwrap();
    let faces: Vec<[usize; 3]> = model
        .data()
        .faces
        .iter()
        .map(|f| f.map(|i| i as usize))
        .collect();
    let rest_height_m = model.anthropometry(&rest).height;

    let candidate = tape_readings(&faces, rest_height_m, &posed, &frames)
        .expect("all 21 tape functionals must be defined on the analytic mannequin");
    let independent = independent_tape_readings_21(&model, &faces, rest_height_m, &posed, &frames);

    // Closed-form elementary expectations for every one of the 21 `TAPE_FIELDS`:
    // Upper-arm corners are at (±20, ±20) from arm axis (-135, 0), so radial distance = 20*sqrt(2),
    // `tip_h = 0.8 * 20 * sqrt(2) = 16 * sqrt(2)`.
    let expected_upper_arm_r = 20.0 * 2.0_f64.sqrt();
    let expected_tip_h = 0.8 * expected_upper_arm_r;
    let expected: [(&str, f64); N_TAPE] = [
        ("stature", 1000.0),
        ("chest", 660.0),
        ("waist", 540.0),
        ("hip", 640.0),
        ("waist_height", 570.0),
        ("crotch_height", 450.0),
        ("thigh", 280.0),
        ("knee", 224.0),
        ("calf", 240.0),
        ("ankle", 176.0),
        ("upper_arm", 160.0),
        ("wrist", 160.0),
        ("arm_length", 360.0 + expected_tip_h),
        ("knee_height", 240.0),
        ("back_neck_to_waist", 290.0),
        ("neck_base", 360.0),
        ("head", 570.0),
        ("shoulder_length", 29.0_f64.sqrt() + 88.0),
        ("across_back", 270.0),
        ("total_crotch_length", 350.0 + 20.0 * 2.0_f64.sqrt()),
        ("outseam", 560.0 + 325.0_f64.sqrt()),
    ];

    for (idx, &(field, analytic_mm)) in expected.iter().enumerate() {
        assert_eq!(TAPE_FIELDS[idx], field);
        let cand_mm = candidate[idx];
        let indep_mm = independent[idx].expect(field);
        assert!(
            (cand_mm - analytic_mm).abs() < 1e-9,
            "{field}: candidate {cand_mm:.12} mm vs closed-form {analytic_mm:.12} mm"
        );
        assert!(
            (indep_mm - analytic_mm).abs() < 1e-9,
            "{field}: independent {indep_mm:.12} mm vs closed-form {analytic_mm:.12} mm"
        );
    }

    // Out-of-domain / missing-section negative control: moving `chest` above the mesh withdraws
    // `chest` to `None` while keeping unrelated functionals intact; `tape_readings` refuses (`None`).
    let mut missing_chest = frames.clone();
    missing_chest.landmarks.insert("chest".into(), 1200.0);
    assert_eq!(
        tape_readings(&faces, rest_height_m, &posed, &missing_chest),
        None
    );
    let partial_cand = partial_tape_readings(&faces, rest_height_m, &posed, &missing_chest);
    let partial_indep =
        independent_tape_readings_21(&model, &faces, rest_height_m, &posed, &missing_chest);
    assert_eq!(partial_cand[1], None);
    assert_eq!(partial_indep[1], None);
    for idx in (0..N_TAPE).filter(|&i| i != 1) {
        assert!(
            partial_cand[idx].is_some(),
            "unexpected withdrawal at {}",
            TAPE_FIELDS[idx]
        );
        assert_eq!(partial_cand[idx].is_some(), partial_indep[idx].is_some());
    }
}

#[test]
#[ignore = "requires local Anny 0.6.1 .ftxbody cache"]
fn admitted_anny_all_21_tape_functionals_match_independent_jarvis_winding_slicer_and_bound_landmark_perturbations()
 {
    let path = default_model_path().expect("default_model_path");
    let data = AnnyData::read(&path, true).expect("verified Anny 0.6.1 .ftxbody");
    let model = AnnyModel::from_data(data).expect("AnnyModel");
    let faces: Vec<[usize; 3]> = model
        .data()
        .faces
        .iter()
        .map(|f| f.map(|i| i as usize))
        .collect();

    let cohorts = [
        ("infant_0_5y", 0.5, 0.5, 0.50, 0.50, 0.50, 0.50),
        ("toddler_2_5y_f", 2.5, 1.0, 0.50, 0.50, 0.50, 0.50),
        ("child_7y_m", 7.0, 0.0, 0.65, 0.45, 0.50, 0.55),
        ("teen_13y_f", 13.0, 1.0, 0.40, 0.60, 0.45, 0.50),
        ("adult_30y_f", 30.0, 1.0, 0.55, 0.50, 0.50, 0.50),
        ("adult_35y_heavy_m", 35.0, 0.0, 0.75, 0.85, 0.75, 0.45),
    ];

    let mut max_abs_delta_mm = 0.0_f64;
    for &(label, years, gender, height, weight, muscle, proportions) in &cohorts {
        let params = model.params(Phenotype {
            gender,
            age: age_parameter_for_years(years),
            height,
            weight,
            muscle,
            proportions,
        });
        let rest = model.rest(&params).unwrap();
        let rest_h = model.anthropometry(&rest).height;
        let posed = model
            .pose(
                &rest,
                &pose_deltas(&model, &rest, &Pose::drape(), 1.0).unwrap(),
            )
            .unwrap();
        let frames = BodyFrames::new(&model, &posed).unwrap();

        let cand = tape_readings(&faces, rest_h, &posed, &frames)
            .unwrap_or_else(|| panic!("{label}: missing tape reading"));
        let indep = independent_tape_readings_21(&model, &faces, rest_h, &posed, &frames);

        for idx in 0..N_TAPE {
            let field = TAPE_FIELDS[idx];
            let a = cand[idx];
            let b = indep[idx].unwrap_or_else(|| panic!("{label}.{field}: independent None"));
            let delta = (a - b).abs();
            max_abs_delta_mm = max_abs_delta_mm.max(delta);
            assert!(
                delta < 1e-6,
                "{label}.{field}: candidate {a:.9} mm vs independent {b:.9} mm (|Δ| = {delta:.3e} mm)"
            );
        }
    }
    eprintln!(
        "[ftx-onn.16] 6-cohort x 21-functional independent verifier max |delta_mm| = {max_abs_delta_mm:.3e} mm"
    );

    // Landmark height sensitivity on the 2.5y toddler (`±5 mm` perturbations on key torso/leg levels).
    let toddler = model.params(Phenotype {
        gender: 1.0,
        age: age_parameter_for_years(2.5),
        ..Phenotype::default()
    });
    let rest = model.rest(&toddler).unwrap();
    let rest_h = model.anthropometry(&rest).height;
    let posed = model
        .pose(
            &rest,
            &pose_deltas(&model, &rest, &Pose::drape(), 1.0).unwrap(),
        )
        .unwrap();
    let base_frames = BodyFrames::new(&model, &posed).unwrap();
    let base = tape_readings(&faces, rest_h, &posed, &base_frames).unwrap();
    for (lm_key, affected_field_idx, max_mm_shift_for_5mm) in [
        ("chest", 1usize, 30.0_f64),
        ("waist", 2usize, 25.0_f64),
        ("knee", 7usize, 20.0_f64),
    ] {
        for shift_mm in [-5.0_f64, 5.0_f64] {
            let mut perturbed = base_frames.clone();
            *perturbed.landmarks.get_mut(lm_key).unwrap() += shift_mm;
            let shifted = tape_readings(&faces, rest_h, &posed, &perturbed).unwrap();
            let diff = (shifted[affected_field_idx] - base[affected_field_idx]).abs();
            assert!(
                diff <= max_mm_shift_for_5mm,
                "5 mm shift on {lm_key} moved {} by {diff:.2} mm (> {max_mm_shift_for_5mm} mm)",
                TAPE_FIELDS[affected_field_idx]
            );
            eprintln!(
                "[ftx-onn.16] toddler 2.5y landmark {lm_key} {shift_mm:+.0} mm -> {} Δ = {:+.2} mm",
                TAPE_FIELDS[affected_field_idx],
                shifted[affected_field_idx] - base[affected_field_idx]
            );
        }
    }
}

#[test]
#[ignore = "requires local Anny 0.6.1 .ftxbody cache"]
fn admitted_anny_posed_colliders_quantify_lbs_volume_loss_and_intersection_status_across_pose_sweeps()
 {
    let path = default_model_path().expect("default_model_path");
    let data = AnnyData::read(&path, true).expect("verified Anny 0.6.1 .ftxbody");
    let model = AnnyModel::from_data(data).expect("AnnyModel");

    let mut standing_50 = Pose::a_pose(50.0);
    standing_50.straight_arms = true;
    let mut arms_up_straight = Pose::arms_up();
    arms_up_straight.straight_arms = true;

    let mut poses = vec![
        ("rest", Pose::rest()),
        ("drape_35", Pose::drape()),
        ("standing_50", standing_50),
        ("arms_up_150", arms_up_straight),
    ];
    for (prefix, legs) in [("seated", Legs::Seated), ("squat", Legs::Squat)] {
        for frac in [0.25, 0.50, 0.75, 1.00] {
            let mut p = Pose::drape();
            p.legs = legs;
            p.leg_fraction = frac;
            poses.push((Box::leak(format!("{prefix}_{frac:.2}").into_boxed_str()), p));
        }
    }

    for (who, years, gender) in [("toddler_2_5y", 2.5, 1.0), ("adult_30y", 30.0, 1.0)] {
        let params = model.params(Phenotype {
            gender,
            age: age_parameter_for_years(years),
            ..Phenotype::default()
        });
        let rest = model.rest(&params).unwrap();
        let rest_full = skin_mesh(&rest.vertices, &model.data().faces).unwrap();
        let rest_collider = clothing_skin(&model, &rest.vertices).unwrap();

        for &(pose_name, ref pose) in &poses {
            let posed = model
                .pose(&rest, &pose_deltas(&model, &rest, pose, 1.0).unwrap())
                .unwrap();
            let full = skin_mesh(&posed.vertices, &model.data().faces).unwrap();
            let collider = clothing_skin(&model, &posed.vertices).unwrap();

            let full_rep = full
                .pose_geometry_against_rest(&rest_full, DEFAULT_POSE_AUDIT_PAIR_BUDGET)
                .unwrap();
            let col_rep = collider
                .pose_geometry_against_rest(&rest_collider, DEFAULT_POSE_AUDIT_PAIR_BUDGET)
                .unwrap();

            eprintln!(
                "[ftx-onn.16] {who} {pose_name:12}: skin ΔV={:+.2}% (adv={}) hits={} | collider ΔV={:+.2}% (adv={}) hits={}",
                full_rep.volume_change_fraction * 100.0,
                full_rep.lbs_volume_loss_advisory,
                full_rep.self_intersections.intersecting_pairs,
                col_rep.volume_change_fraction * 100.0,
                col_rep.lbs_volume_loss_advisory,
                col_rep.self_intersections.intersecting_pairs,
            );

            // Uncapped full Anny skin always exhibits facial (`levator05`) self-intersections,
            // whereas the standing `drape_35` and `standing_50` `clothing_skin` colliders must be
            // strictly intersection-free (`0` intersecting triangle pairs).
            assert!(full_rep.self_intersections.intersecting_pairs > 0);
            if matches!(pose_name, "rest" | "drape_35" | "standing_50") {
                assert_eq!(
                    col_rep.self_intersections.intersecting_pairs, 0,
                    "{who} {pose_name} clothing_skin must have zero self-intersections"
                );
                assert!(
                    !col_rep.lbs_volume_loss_advisory,
                    "{who} {pose_name} standing pose should stay within 3% LBS volume change"
                );
            }
            if matches!(pose_name, "seated_1.00" | "squat_1.00") {
                // Full seated and squat poses drop > 3% enclosed volume under LBS and must trigger
                // `lbs_volume_loss_advisory == true`.
                assert!(
                    full_rep.volume_change_fraction < -0.03 && full_rep.lbs_volume_loss_advisory,
                    "{who} {pose_name} expected >3% LBS volume loss advisory, got {:.2}%",
                    full_rep.volume_change_fraction * 100.0
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Independent `BodyFrames` / landmark / section-radius oracle
// (zero code shared with `ftx_body::landmarks`)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct IndependentBodyFrames {
    torso: Frame,
    arm_r: Frame,
    arm_l: Frame,
    leg_r: Frame,
    leg_l: Frame,
    neck: Frame,
    floor_z: f64,
    tip_h: f64,
    tip_h_l: f64,
    upper_arm_radius: f64,
    upper_arm_radius_l: f64,
    arm_length: f64,
    arm_length_l: f64,
    head_bone: Vec3,
    shoulder_joint_r: Vec3,
    landmarks: BTreeMap<String, f64>,
    leg_landmarks_l: [f64; 3],
    torso_pts: Vec<Vec3>,
    leg_r_pts: Vec<Vec3>,
    leg_l_pts: Vec<Vec3>,
}

fn indep_frame_from_up(origin: Vec3, raw_up: Vec3) -> Frame {
    let up = raw_up * (1.0 / raw_up.norm());
    let neg_y = Vec3::new(0.0, -1.0, 0.0);
    let proj = neg_y - up * neg_y.dot(up);
    let front = proj * (1.0 / proj.norm());
    let lat = Vec3::new(
        up.y * front.z - up.z * front.y,
        up.z * front.x - up.x * front.z,
        up.x * front.y - up.y * front.x,
    );
    Frame {
        origin,
        up,
        front,
        lat,
    }
}

fn indep_line_radial_and_h(p: Vec3, origin: Vec3, unit_dir: Vec3) -> (f64, f64) {
    let rel = p - origin;
    // Cross-product norm `||rel x unit_dir||` avoids `rel - unit_dir * h` projection code sharing.
    let cx = rel.y * unit_dir.z - rel.z * unit_dir.y;
    let cy = rel.z * unit_dir.x - rel.x * unit_dir.z;
    let cz = rel.x * unit_dir.y - rel.y * unit_dir.x;
    ((cx * cx + cy * cy + cz * cz).sqrt(), rel.dot(unit_dir))
}

/// Ancestor-climb membership check (`cur -> parent -> ... == ancestor`), independent of single-pass
/// `descendants` bitset propagation in `ftx_body::landmarks`.
fn has_bone_ancestor(parents: &[i64], mut bone_idx: usize, ancestor: usize) -> bool {
    for _ in 0..=parents.len() {
        if bone_idx == ancestor {
            return true;
        }
        let Ok(next) = usize::try_from(parents[bone_idx]) else {
            return false;
        };
        bone_idx = next;
    }
    false
}

fn independent_body_frames(model: &AnnyModel, posed: &PosedBody) -> IndependentBodyFrames {
    let d = model.data();
    let parents = &d.header.labels.bone_parents;
    let b_idx = |name: &str| -> usize {
        d.header
            .labels
            .bones
            .iter()
            .position(|b| b == name)
            .unwrap_or_else(|| panic!("missing bone {name}"))
    };
    let arm_r_root = b_idx("upperarm01.R");
    let arm_l_root = b_idx("upperarm01.L");
    let leg_r_root = b_idx("upperleg01.R");
    let leg_l_root = b_idx("upperleg01.L");
    let arm_r_upper = [arm_r_root, b_idx("upperarm02.R")];
    let arm_l_upper = [arm_l_root, b_idx("upperarm02.L")];

    let k = d.header.counts.influences;
    let verts_mm: Vec<Vec3> = posed.vertices.iter().map(|v| *v * 1000.0).collect();
    let dom_bone: Vec<usize> = (0..verts_mm.len())
        .map(|v| {
            let w_slice = &d.vertex_bone_weights[v * k..(v + 1) * k];
            let i_slice = &d.vertex_bone_indices[v * k..(v + 1) * k];
            let mut best_slot = 0usize;
            for slot in 1..k {
                if w_slice[slot] > w_slice[best_slot] {
                    best_slot = slot;
                }
            }
            i_slice[best_slot] as usize
        })
        .collect();

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Part {
        Torso,
        ArmR,
        ArmL,
        LegR,
        LegL,
    }

    let parts: Vec<Part> = dom_bone
        .iter()
        .map(|&b| {
            if has_bone_ancestor(parents, b, arm_r_root) {
                Part::ArmR
            } else if has_bone_ancestor(parents, b, arm_l_root) {
                Part::ArmL
            } else if has_bone_ancestor(parents, b, leg_r_root) {
                Part::LegR
            } else if has_bone_ancestor(parents, b, leg_l_root) {
                Part::LegL
            } else {
                Part::Torso
            }
        })
        .collect();

    let floor_z = verts_mm
        .iter()
        .map(|p| p.z)
        .min_by(f64::total_cmp)
        .expect("non-empty mesh");
    let head_top_z = verts_mm
        .iter()
        .map(|p| p.z)
        .max_by(f64::total_cmp)
        .expect("non-empty mesh");

    let waist_pts: Vec<Vec3> = d
        .header
        .waist_loop_final
        .iter()
        .map(|&vi| verts_mm[vi])
        .collect();
    let waist_z = waist_pts.iter().map(|p| p.z).sum::<f64>() / (waist_pts.len() as f64);
    let y_min = waist_pts
        .iter()
        .map(|p| p.y)
        .min_by(f64::total_cmp)
        .unwrap();
    let y_max = waist_pts
        .iter()
        .map(|p| p.y)
        .max_by(f64::total_cmp)
        .unwrap();
    let y_c = 0.5 * (y_min + y_max);

    let crotch_z = verts_mm
        .iter()
        .zip(&parts)
        .filter(|(p, part)| **part == Part::Torso && p.x.abs() < 4.0 && p.z < waist_z)
        .map(|(p, _)| p.z)
        .min_by(f64::total_cmp)
        .expect("crotch vertex");

    let joint_mm = |name: &str| posed.bone_poses[b_idx(name)].t * 1000.0;
    let shoulder_r = joint_mm("upperarm01.R");
    let arm_r = indep_frame_from_up(shoulder_r, -posed.bone_poses[arm_r_root].m.col(1));
    let mut r_vals_r: Vec<f64> = verts_mm
        .iter()
        .zip(&dom_bone)
        .filter(|(_, b)| arm_r_upper.contains(b))
        .map(|(p, _)| indep_line_radial_and_h(*p, arm_r.origin, arm_r.up).0)
        .collect();
    let mid_r = r_vals_r.len() / 2;
    let (_, &mut upper_arm_radius, _) = r_vals_r.select_nth_unstable_by(mid_r, f64::total_cmp);
    let tip_h = 0.8 * upper_arm_radius;

    let armpit_z = verts_mm
        .iter()
        .zip(&dom_bone)
        .filter(|(p, b)| {
            arm_r_upper.contains(b) && p.x.abs() < shoulder_r.x.abs() + upper_arm_radius
        })
        .map(|(p, _)| p.z)
        .min_by(f64::total_cmp)
        .expect("armpit vertex");
    let arm_length = (joint_mm("wrist.R") - shoulder_r).norm() + tip_h;

    let shoulder_l = joint_mm("upperarm01.L");
    let arm_l = indep_frame_from_up(shoulder_l, -posed.bone_poses[arm_l_root].m.col(1));
    let mut r_vals_l: Vec<f64> = verts_mm
        .iter()
        .zip(&dom_bone)
        .filter(|(_, b)| arm_l_upper.contains(b))
        .map(|(p, _)| indep_line_radial_and_h(*p, arm_l.origin, arm_l.up).0)
        .collect();
    let mid_l = r_vals_l.len() / 2;
    let (_, &mut upper_arm_radius_l, _) = r_vals_l.select_nth_unstable_by(mid_l, f64::total_cmp);
    let tip_h_l = 0.8 * upper_arm_radius_l;
    let arm_length_l = (joint_mm("wrist.L") - shoulder_l).norm() + tip_h_l;

    let hip_r = joint_mm("upperleg01.R");
    let ankle_r = joint_mm("foot.R");
    let knee_r = joint_mm("lowerleg01.R");
    let leg_r = indep_frame_from_up(Vec3::new(hip_r.x, y_c, crotch_z + 25.0), hip_r - ankle_r);

    let hip_l = joint_mm("upperleg01.L");
    let ankle_l = joint_mm("foot.L");
    let knee_l = joint_mm("lowerleg01.L");
    let leg_l = indep_frame_from_up(Vec3::new(hip_l.x, y_c, crotch_z + 25.0), hip_l - ankle_l);

    let torso = Frame {
        origin: Vec3::new(0.0, y_c, floor_z),
        up: Vec3::new(0.0, 0.0, 1.0),
        front: Vec3::new(0.0, -1.0, 0.0),
        lat: Vec3::new(1.0, 0.0, 0.0),
    };

    let neck_origin = joint_mm("neck01");
    let head_bone = joint_mm("head");
    let neck = indep_frame_from_up(neck_origin, head_bone - neck_origin);

    let shoulder_z = (shoulder_r + arm_r.up * tip_h).z;
    let landmarks: BTreeMap<String, f64> = [
        ("floor", 0.0),
        ("ankle", ankle_r.z - floor_z),
        ("calf", ankle_r.z + 0.6 * (knee_r.z - ankle_r.z) - floor_z),
        ("knee", knee_r.z - floor_z),
        ("crotch", crotch_z - floor_z),
        ("hip", crotch_z + 0.45 * (waist_z - crotch_z) - floor_z),
        ("waist", waist_z - floor_z),
        ("chest", armpit_z - floor_z),
        ("across_back", 0.5 * (armpit_z + shoulder_z) - floor_z),
        ("shoulder", shoulder_z - floor_z),
        ("snp", neck_origin.z - floor_z),
        ("c7", neck_origin.z + 5.0 - floor_z),
        ("head_top", head_top_z - floor_z),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();

    let collect_part = |want: Part| -> Vec<Vec3> {
        verts_mm
            .iter()
            .zip(&parts)
            .filter(|(_, p)| **p == want)
            .map(|(v, _)| *v)
            .collect()
    };

    IndependentBodyFrames {
        torso,
        arm_r,
        arm_l,
        leg_r,
        leg_l,
        neck,
        floor_z,
        tip_h,
        tip_h_l,
        upper_arm_radius,
        upper_arm_radius_l,
        arm_length,
        arm_length_l,
        head_bone,
        shoulder_joint_r: shoulder_r,
        landmarks,
        leg_landmarks_l: [
            ankle_l.z - floor_z,
            ankle_l.z + 0.6 * (knee_l.z - ankle_l.z) - floor_z,
            knee_l.z - floor_z,
        ],
        torso_pts: collect_part(Part::Torso),
        leg_r_pts: collect_part(Part::LegR),
        leg_l_pts: collect_part(Part::LegL),
    }
}

impl IndependentBodyFrames {
    fn landmark_h(&self, part: &str, name: &str) -> Option<f64> {
        match part {
            "torso" => self.landmarks.get(name).copied(),
            "arm_R" => match name {
                "shoulder_tip" => Some(self.tip_h),
                "elbow" => Some(self.tip_h - 0.55 * self.arm_length),
                "wrist" => Some(self.tip_h - self.arm_length),
                _ => None,
            },
            "arm_L" => match name {
                "shoulder_tip" => Some(self.tip_h_l),
                "elbow" => Some(self.tip_h_l - 0.55 * self.arm_length_l),
                "wrist" => Some(self.tip_h_l - self.arm_length_l),
                _ => None,
            },
            "leg_R" => {
                let z = self.floor_z + *self.landmarks.get(name)?;
                Some(-(self.leg_r.origin.z - z) / self.leg_r.up.z)
            }
            "leg_L" => {
                let h = match name {
                    "ankle" => self.leg_landmarks_l[0],
                    "calf" => self.leg_landmarks_l[1],
                    "knee" => self.leg_landmarks_l[2],
                    _ => *self.landmarks.get(name)?,
                };
                let z = self.floor_z + h;
                Some(-(self.leg_l.origin.z - z) / self.leg_l.up.z)
            }
            _ => None,
        }
    }

    fn section_radius(&self, part: &str, h: f64) -> f64 {
        match part {
            "torso" => {
                let z = self.floor_z + h;
                self.torso_pts
                    .iter()
                    .filter(|p| (p.z - z).abs() <= 10.0)
                    .map(|p| p.x.hypot(p.y - self.torso.origin.y))
                    .fold(0.0, f64::max)
            }
            "arm_R" => 1.15 * self.upper_arm_radius,
            "arm_L" => 1.15 * self.upper_arm_radius_l,
            "leg_R" | "leg_L" => {
                let is_l = part == "leg_L";
                let (fr, pts) = if is_l {
                    (&self.leg_l, &self.leg_l_pts)
                } else {
                    (&self.leg_r, &self.leg_r_pts)
                };
                let mut best = pts
                    .iter()
                    .map(|p| indep_line_radial_and_h(*p, fr.origin, fr.up))
                    .filter(|(_, ph)| (ph - h).abs() <= 10.0)
                    .map(|(r, _)| r)
                    .fold(0.0, f64::max);
                let z = fr.origin.z + h * fr.up.z;
                if z > fr.origin.z - 20.0 {
                    best = self
                        .torso_pts
                        .iter()
                        .filter(|p| {
                            (if is_l { p.x >= 0.0 } else { p.x <= 0.0 }) && (p.z - z).abs() <= 10.0
                        })
                        .map(|p| indep_line_radial_and_h(*p, fr.origin, fr.up).0)
                        .fold(best, f64::max);
                }
                best
            }
            _ => 0.0,
        }
    }
}

fn max_frame_diff(a: Frame, b: Frame) -> f64 {
    (a.origin - b.origin)
        .norm()
        .max((a.up - b.up).norm())
        .max((a.front - b.front).norm())
        .max((a.lat - b.lat).norm())
}

fn assert_body_frames_match_independent(
    cand: &BodyFrames,
    indep: &IndependentBodyFrames,
    ctx: &str,
) -> f64 {
    let mut worst = 0.0_f64;
    for (name, cf, rf) in [
        ("torso", cand.torso, indep.torso),
        ("arm_R", cand.arm_r, indep.arm_r),
        ("arm_L", cand.arm_l, indep.arm_l),
        ("leg_R", cand.leg_r, indep.leg_r),
        ("leg_L", cand.leg_l, indep.leg_l),
        ("neck", cand.neck, indep.neck),
    ] {
        let d = max_frame_diff(cf, rf);
        worst = worst.max(d);
        assert!(d < 1e-11, "{ctx} frame {name} mismatch: {d:.3e}");
        if name != "neck" {
            assert_eq!(cand.frame(name), Some(cf));
        }
    }
    for (label, cv, rv) in [
        ("floor_z", cand.floor_z, indep.floor_z),
        ("tip_h", cand.tip_h, indep.tip_h),
        ("tip_h_l", cand.tip_h_l, indep.tip_h_l),
        (
            "upper_arm_radius",
            cand.upper_arm_radius,
            indep.upper_arm_radius,
        ),
        (
            "upper_arm_radius_l",
            cand.upper_arm_radius_l,
            indep.upper_arm_radius_l,
        ),
        ("arm_length", cand.arm_length, indep.arm_length),
        ("arm_length_l", cand.arm_length_l, indep.arm_length_l),
        ("head_bone", (cand.head_bone - indep.head_bone).norm(), 0.0),
        (
            "shoulder_joint_r",
            (cand.shoulder_joint_r - indep.shoulder_joint_r).norm(),
            0.0,
        ),
    ] {
        let d = (cv - rv).abs();
        worst = worst.max(d);
        assert!(d < 1e-11, "{ctx} scalar {label} mismatch: {d:.3e}");
    }
    assert_eq!(cand.landmarks.len(), indep.landmarks.len());
    for (k, &cv) in &cand.landmarks {
        let rv = indep.landmarks[k];
        let d = (cv - rv).abs();
        worst = worst.max(d);
        assert!(d < 1e-11, "{ctx} landmark {k} mismatch: {d:.3e}");
        assert_eq!(cand.landmark_h("torso", k), Some(cv));
    }
    for part in ["arm_R", "arm_L"] {
        for lm in ["shoulder_tip", "elbow", "wrist"] {
            let cv = cand.landmark_h(part, lm).unwrap();
            let rv = indep.landmark_h(part, lm).unwrap();
            let d = (cv - rv).abs();
            worst = worst.max(d);
            assert!(d < 1e-11, "{ctx} {part}.{lm} mismatch: {d:.3e}");
        }
    }
    for part in ["leg_R", "leg_L"] {
        for lm in ["ankle", "calf", "knee", "crotch", "hip", "waist"] {
            let cv = cand.landmark_h(part, lm).unwrap();
            let rv = indep.landmark_h(part, lm).unwrap();
            let d = (cv - rv).abs();
            worst = worst.max(d);
            assert!(d < 1e-11, "{ctx} {part}.{lm} mismatch: {d:.3e}");
        }
    }
    for (part, probe_levels) in [
        ("torso", [100.0, 350.0, 500.0, 650.0]),
        ("arm_R", [-200.0, -100.0, 0.0, 15.0]),
        ("arm_L", [-200.0, -100.0, 0.0, 15.0]),
        ("leg_R", [-300.0, -150.0, -20.0, 20.0]),
        ("leg_L", [-300.0, -150.0, -20.0, 20.0]),
    ] {
        for h in probe_levels {
            let cr = cand.section_radius(part, h);
            let rr = indep.section_radius(part, h);
            let d = (cr - rr).abs();
            worst = worst.max(d);
            assert!(
                d < 1e-11,
                "{ctx} section_radius({part}, {h}) mismatch: {d:.3e}"
            );
        }
    }
    worst
}

#[test]
fn seeded_functional_landmark_and_collider_errors_fire_all_promised_refusals_and_asymmetric_frames_match()
 {
    let model = build_analytic_mannequin();
    let params = model.params(Phenotype::default());
    let rest = model.rest(&params).unwrap();
    let mut posed = model
        .pose(
            &rest,
            &pose_deltas(&model, &rest, &Pose::rest(), 1.0).unwrap(),
        )
        .unwrap();

    // 1. Asymmetric limb geometry: widen left upper arm (+10 mm radially), lengthen left wrist
    // downward (-25 mm), and lower left knee (-18 mm). Independent `BodyFrames` must track left
    // and right frames, radii, and limb landmarks separately without mirroring right onto left.
    let d = model.data();
    let k = d.header.counts.influences;
    let b_idx = |name: &str| {
        d.header
            .labels
            .bones
            .iter()
            .position(|b| b == name)
            .unwrap()
    };
    let arm_l_b = b_idx("upperarm01.L");
    let wrist_l_b = b_idx("wrist.L");
    let knee_l_b = b_idx("lowerleg01.L");
    posed.bone_poses[wrist_l_b].t.z -= 0.025;
    posed.bone_poses[knee_l_b].t.z -= 0.018;
    let arm_l_joint = posed.bone_poses[arm_l_b].t;
    for (vi, v) in posed.vertices.iter_mut().enumerate() {
        let dom = (0..k).fold(0, |best, s| {
            if d.vertex_bone_weights[vi * k + s] > d.vertex_bone_weights[vi * k + best] {
                s
            } else {
                best
            }
        });
        if d.vertex_bone_indices[vi * k + dom] as usize == arm_l_b {
            v.x = arm_l_joint.x + (v.x - arm_l_joint.x) * 1.5;
            v.y = arm_l_joint.y + (v.y - arm_l_joint.y) * 1.5;
        }
    }
    let asym_cand = BodyFrames::new(&model, &posed).unwrap();
    let asym_indep = independent_body_frames(&model, &posed);
    assert_body_frames_match_independent(&asym_cand, &asym_indep, "analytic_asymmetric");
    assert!(
        (asym_cand.upper_arm_radius_l - 1.5 * asym_cand.upper_arm_radius).abs() < 1e-9,
        "left upper-arm radius must reflect asymmetric left vertices independently"
    );
    assert!(
        asym_cand.arm_length_l > asym_cand.arm_length + 25.0,
        "left arm length must reflect asymmetric left wrist and tip_h_l"
    );
    assert!(
        asym_cand.landmark_h("leg_L", "knee").unwrap()
            < asym_cand.landmark_h("leg_R", "knee").unwrap() - 15.0,
        "left knee landmark must reflect left lowerleg01 bone independently"
    );

    // 2. Seeded `BodyFrames` / landmark error controls:
    // (a) Sagittal gap (`|x| >= 4 mm` for all torso vertices below waist) -> `DegenerateGeometry`.
    let mut no_crotch_posed = posed.clone();
    for v in &mut no_crotch_posed.vertices {
        if v.x.abs() < 0.005 {
            v.x = if v.x >= 0.0 { 0.010 } else { -0.010 };
        }
    }
    let err_no_crotch = BodyFrames::new(&model, &no_crotch_posed).unwrap_err();
    assert_eq!(err_no_crotch.code, RefusalCode::DegenerateGeometry);
    assert!(err_no_crotch.message.contains("crotch"));

    // 3. Seeded functional error controls:
    // (a) Detached exterior distractor box beside torso does NOT corrupt outermost axis-containing
    // torso functionals (`chest`, `waist`, `back_neck_to_waist`, `across_back`, `total_crotch_length`).
    let clean_posed = model
        .pose(
            &rest,
            &pose_deltas(&model, &rest, &Pose::rest(), 1.0).unwrap(),
        )
        .unwrap();
    let clean_frames = BodyFrames::new(&model, &clean_posed).unwrap();
    let base_faces: Vec<[usize; 3]> = d.faces.iter().map(|f| f.map(|i| i as usize)).collect();
    let rest_h = model.anthropometry(&rest).height;
    let clean_tape = tape_readings(&base_faces, rest_h, &clean_posed, &clean_frames).unwrap();

    let mut distractor_posed = clean_posed.clone();
    let mut distractor_faces = base_faces.clone();
    let base_v = distractor_posed.vertices.len();
    for z in [0.40, 0.90] {
        for (x, y) in [(0.35, -0.05), (0.45, -0.05), (0.45, 0.05), (0.35, 0.05)] {
            distractor_posed.vertices.push(Vec3::new(x, y, z));
        }
    }
    for tri in [
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
    ] {
        distractor_faces.push(tri.map(|o| base_v + o));
    }
    let with_distractor =
        tape_readings(&distractor_faces, rest_h, &distractor_posed, &clean_frames).unwrap();
    for idx in [1usize, 2, 3, 14, 18, 19] {
        assert!(
            (with_distractor[idx] - clean_tape[idx]).abs() < 1e-9,
            "detached distractor loop corrupted {}: got {}, want {}",
            TAPE_FIELDS[idx],
            with_distractor[idx],
            clean_tape[idx]
        );
    }

    // (b) Shifting the declared torso axis outside all section loops withdraws axis-containing
    // functionals (`None`) and refuses complete `tape_readings`.
    let mut off_axis_frames = clean_frames.clone();
    off_axis_frames.torso.origin.x = 0.80 * 1000.0;
    assert_eq!(
        tape_readings(&base_faces, rest_h, &clean_posed, &off_axis_frames),
        None
    );
    let off_axis_partial =
        partial_tape_readings(&base_faces, rest_h, &clean_posed, &off_axis_frames);
    for idx in [1usize, 2, 3, 14, 18, 19] {
        assert_eq!(
            off_axis_partial[idx], None,
            "off-axis torso must withdraw {}",
            TAPE_FIELDS[idx]
        );
    }

    // (c) Non-finite landmark (`NaN`) withdraws dependent functionals cleanly without panic.
    let mut nan_frames = clean_frames.clone();
    nan_frames.landmarks.insert("waist".into(), f64::NAN);
    assert_eq!(
        tape_readings(&base_faces, rest_h, &clean_posed, &nan_frames),
        None
    );
    let nan_partial = partial_tape_readings(&base_faces, rest_h, &clean_posed, &nan_frames);
    assert_eq!(nan_partial[2], None); // waist
    assert_eq!(nan_partial[4], None); // waist_height

    // 4. Seeded `fit_stature` and `tape_targets` sigma refusal controls:
    let err_stature_hi = fit_stature(
        &model,
        &Wearer {
            years: 2.5,
            gender: 0.5,
            stature_mm: 2500.0,
            weight_kg: None,
        },
    )
    .unwrap_err();
    assert_eq!(err_stature_hi.code, RefusalCode::OutOfRange);

    let err_stature_neg = fit_stature(
        &model,
        &Wearer {
            years: -1.0,
            gender: 0.5,
            stature_mm: 1000.0,
            weight_kg: None,
        },
    )
    .unwrap_err();
    assert_eq!(err_stature_neg.code, RefusalCode::OutOfRange);

    for bad_sigma in [json!(0.0), json!(-2.5), json!(null), json!("bad")] {
        let bad_doc = json!({
            "fused": {
                "waist": { "value_mm": 540.0, "sigma_mm": bad_sigma }
            }
        });
        let err_sig = tape_targets(&bad_doc).unwrap_err();
        assert_eq!(err_sig.code, RefusalCode::InvalidDocument);
    }

    // 5. Seeded contact-collider error controls on a watertight two-shell torso+arm mesh:
    // Also verify `skin_mesh` catches zero-area step triangles on `build_analytic_mannequin`!
    let err_zero_area = skin_mesh(&clean_posed.vertices, &d.faces).unwrap_err();
    assert_eq!(err_zero_area.code, RefusalCode::DegenerateGeometry);
    assert!(err_zero_area.message.contains("degenerate triangle"));

    let box_shell = |x0: f64, x1: f64, y0: f64, y1: f64, z0: f64, z1: f64, base: u32| {
        let verts = vec![
            Vec3::new(x0, y0, z0),
            Vec3::new(x1, y0, z0),
            Vec3::new(x1, y1, z0),
            Vec3::new(x0, y1, z0),
            Vec3::new(x0, y0, z1),
            Vec3::new(x1, y0, z1),
            Vec3::new(x1, y1, z1),
            Vec3::new(x0, y1, z1),
        ];
        // Outward-wound CCW faces (`volume > 0`).
        let tris: Vec<[u32; 3]> = [
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
        ]
        .into_iter()
        .map(|t| t.map(|i| base + i))
        .collect();
        (verts, tris)
    };
    let (mut col_verts, mut col_faces) = box_shell(-0.12, 0.12, -0.08, 0.08, 0.0, 0.60, 0);
    // Attach a watertight lateral upper-arm tab on the right side (`x = -0.12..-0.22`) by
    // pinching an outward tent on face `(3, 0, 4, 7)` or adding a manifold extruded cap on top:
    // Extrude top face `(4, 5, 6, 7)` into a 2-tier prism (`z = 0.60..0.90`) so we can shear/fold
    // the upper tier across the lower tier while keeping a single closed manifold!
    col_faces.retain(|&t| t != [4, 5, 6] && t != [4, 6, 7]);
    col_verts.extend([
        Vec3::new(-0.10, -0.07, 0.90),
        Vec3::new(0.10, -0.07, 0.90),
        Vec3::new(0.10, 0.07, 0.90),
        Vec3::new(-0.10, 0.07, 0.90),
    ]);
    col_faces.extend([
        [4, 5, 9],
        [4, 9, 8],
        [5, 6, 10],
        [5, 10, 9],
        [6, 7, 11],
        [6, 11, 10],
        [7, 4, 8],
        [7, 8, 11],
        [8, 9, 10],
        [8, 10, 11],
    ]);

    // (a) Open boundary / deleted face -> `DegenerateGeometry` (non-manifold edge).
    let mut open_faces = col_faces.clone();
    open_faces.pop();
    let err_open = skin_mesh(&col_verts, &open_faces).unwrap_err();
    assert_eq!(err_open.code, RefusalCode::DegenerateGeometry);

    // (b) Inverted winding -> `InwardWound` (negative enclosed volume).
    let inv_faces: Vec<[u32; 3]> = col_faces.iter().map(|&[a, b, c]| [a, c, b]).collect();
    let err_inv = skin_mesh(&col_verts, &inv_faces).unwrap_err();
    assert_eq!(err_inv.code, RefusalCode::InwardWound);

    // (c) Self-intersecting posed collider -> `intersecting_pairs > 0` in `audit_self_intersections`.
    let rest_skin = skin_mesh(&col_verts, &col_faces).unwrap();
    let clean_audit = rest_skin
        .audit_self_intersections(DEFAULT_POSE_AUDIT_PAIR_BUDGET)
        .unwrap();
    assert_eq!(clean_audit.intersecting_pairs, 0);

    // Fold the upper tier (`vertices 8..12`) sideways & downward through the lower tier wall while
    // keeping positive total enclosed volume and closed 2-manifold edge incidence!
    let mut crossed_verts = col_verts.clone();
    for v in &mut crossed_verts[8..12] {
        v.x += 0.22;
        v.z = 0.35;
    }
    let crossed_skin = skin_mesh(&crossed_verts, &col_faces).unwrap();
    let crossed_rep = crossed_skin
        .pose_geometry_against_rest(&rest_skin, DEFAULT_POSE_AUDIT_PAIR_BUDGET)
        .unwrap();
    assert!(
        crossed_rep.self_intersections.intersecting_pairs > 0,
        "seeded upper-tier fold through lower wall must be caught by exact triangle-pair intersection audit"
    );
    assert!(
        crossed_rep
            .self_intersections
            .first_intersecting_pair
            .is_some()
    );

    // (d) Pair-budget exhaustion -> `BudgetExceeded`.
    let err_budget = rest_skin.audit_self_intersections(1).unwrap_err();
    assert_eq!(err_budget.code, RefusalCode::BudgetExceeded);

    // (e) >3% enclosed volume loss -> `lbs_volume_loss_advisory == true`.
    let shrunk_verts: Vec<Vec3> = col_verts
        .iter()
        .map(|v| Vec3::new(v.x * 0.98, v.y * 0.98, v.z))
        .collect();
    let shrunk_skin = skin_mesh(&shrunk_verts, &col_faces).unwrap();
    let shrunk_rep = shrunk_skin
        .pose_geometry_against_rest(&rest_skin, DEFAULT_POSE_AUDIT_PAIR_BUDGET)
        .unwrap();
    assert!(shrunk_rep.volume_change_fraction < -0.03);
    assert!(shrunk_rep.lbs_volume_loss_advisory);
}

#[test]
#[ignore = "requires local Anny 0.6.1 .ftxbody cache"]
fn admitted_anny_body_frames_landmarks_and_pose_domain_match_independent_oracle_and_quantify_heuristics()
 {
    let path = default_model_path().expect("default_model_path");
    let data = AnnyData::read(&path, true).expect("verified Anny 0.6.1 .ftxbody");
    let model = AnnyModel::from_data(data).expect("AnnyModel");
    let faces: Vec<[usize; 3]> = model
        .data()
        .faces
        .iter()
        .map(|f| f.map(|i| i as usize))
        .collect();

    let cohorts = [
        ("infant_0_5y", 0.5, 0.5, 0.50, 0.50, 0.50, 0.50),
        ("toddler_2_5y_f", 2.5, 1.0, 0.50, 0.50, 0.50, 0.50),
        ("child_7y_m", 7.0, 0.0, 0.65, 0.45, 0.50, 0.55),
        ("teen_13y_f", 13.0, 1.0, 0.40, 0.60, 0.45, 0.50),
        ("adult_30y_f", 30.0, 1.0, 0.55, 0.50, 0.50, 0.50),
        ("adult_35y_heavy_m", 35.0, 0.0, 0.75, 0.85, 0.75, 0.45),
    ];

    let mut worst_frame_mm = 0.0_f64;
    let mut worst_tape_from_indep_frames_mm = 0.0_f64;

    for &(label, years, gender, height, weight, muscle, proportions) in &cohorts {
        let params = model.params(Phenotype {
            gender,
            age: age_parameter_for_years(years),
            height,
            weight,
            muscle,
            proportions,
        });
        let rest = model.rest(&params).unwrap();
        let rest_h = model.anthropometry(&rest).height;
        let posed = model
            .pose(
                &rest,
                &pose_deltas(&model, &rest, &Pose::drape(), 1.0).unwrap(),
            )
            .unwrap();

        let cand_frames = BodyFrames::new(&model, &posed).unwrap();
        let indep_frames = independent_body_frames(&model, &posed);
        worst_frame_mm = worst_frame_mm.max(assert_body_frames_match_independent(
            &cand_frames,
            &indep_frames,
            label,
        ));

        // End-to-end zero-shared-code chain: reconstruct a `BodyFrames` view from `indep_frames`
        // landmarks/frames and compare `independent_tape_readings_21` to production `tape_readings`.
        let mut bridged = cand_frames.clone();
        bridged.torso = indep_frames.torso;
        bridged.arm_r = indep_frames.arm_r;
        bridged.arm_l = indep_frames.arm_l;
        bridged.leg_r = indep_frames.leg_r;
        bridged.leg_l = indep_frames.leg_l;
        bridged.neck = indep_frames.neck;
        bridged.floor_z = indep_frames.floor_z;
        bridged.tip_h = indep_frames.tip_h;
        bridged.tip_h_l = indep_frames.tip_h_l;
        bridged.upper_arm_radius = indep_frames.upper_arm_radius;
        bridged.upper_arm_radius_l = indep_frames.upper_arm_radius_l;
        bridged.arm_length = indep_frames.arm_length;
        bridged.arm_length_l = indep_frames.arm_length_l;
        bridged.landmarks = indep_frames.landmarks.clone();
        bridged.head_bone = indep_frames.head_bone;
        bridged.shoulder_joint_r = indep_frames.shoulder_joint_r;

        let cand_tape = tape_readings(&faces, rest_h, &posed, &cand_frames).unwrap();
        let indep_tape = independent_tape_readings_21(&model, &faces, rest_h, &posed, &bridged);
        for idx in 0..N_TAPE {
            let d = (cand_tape[idx] - indep_tape[idx].unwrap()).abs();
            worst_tape_from_indep_frames_mm = worst_tape_from_indep_frames_mm.max(d);
            assert!(d < 1e-6, "{label}.{}: {d:.3e} mm", TAPE_FIELDS[idx]);
        }

        // Quantify skeletal-vs-heuristic landmark offsets on the 35° drape pose:
        // - `shoulder` heuristic (`upperarm01.R + 0.8 * r * arm.up`) vs `upperarm01.R` joint head
        //   and `clavicle.R` head;
        // - `chest` armpit heuristic vs `upperarm01.R` joint height;
        // - `crotch` sagittal skin minimum vs `upperleg01.R` hip-joint height.
        let b_z = |bname: &str| {
            let bi = model.bone_index(bname).unwrap();
            posed.bone_poses[bi].t.z * 1000.0 - cand_frames.floor_z
        };
        let sh_joint_z = b_z("upperarm01.R");
        let hip_joint_z = b_z("upperleg01.R");
        let shoulder_tip_dz = cand_frames.landmarks["shoulder"] - sh_joint_z;
        let armpit_below_joint = sh_joint_z - cand_frames.landmarks["chest"];
        let hip_joint_above_crotch = hip_joint_z - cand_frames.landmarks["crotch"];
        eprintln!(
            "[ftx-onn.16] {label:17} landmarks: shoulder_tip-joint={shoulder_tip_dz:+5.1} mm, joint-armpit={armpit_below_joint:5.1} mm, hip_joint-crotch={hip_joint_above_crotch:5.1} mm, r_arm={:4.1} mm",
            cand_frames.upper_arm_radius
        );
        // Physical ordering invariants across every age/shape cohort:
        assert!(cand_frames.landmarks["ankle"] < cand_frames.landmarks["calf"]);
        assert!(cand_frames.landmarks["calf"] < cand_frames.landmarks["knee"]);
        assert!(cand_frames.landmarks["knee"] < cand_frames.landmarks["crotch"]);
        assert!(cand_frames.landmarks["crotch"] < cand_frames.landmarks["hip"]);
        assert!(cand_frames.landmarks["hip"] < cand_frames.landmarks["waist"]);
        assert!(cand_frames.landmarks["waist"] < cand_frames.landmarks["chest"]);
        assert!(cand_frames.landmarks["chest"] < cand_frames.landmarks["across_back"]);
        assert!(cand_frames.landmarks["across_back"] < cand_frames.landmarks["shoulder"]);
        assert!(cand_frames.landmarks["shoulder"] < cand_frames.landmarks["head_top"]);
        assert!(shoulder_tip_dz > 5.0 && shoulder_tip_dz < 90.0);
        assert!(armpit_below_joint > 5.0 && armpit_below_joint < 260.0);
        assert!(hip_joint_above_crotch > 15.0 && hip_joint_above_crotch < 160.0);
    }

    // Pose-domain sweep on 2.5y toddler: quantify how `BodyFrames` landmarks move from `drape_35`
    // across `standing_50`, `arms_up_150`, `seated`, and `squat` (explaining why flat-to-3D wrap
    // uses standing A-pose `35°`/`50°` and reaches `arms_up`/`seated`/`squat` by pose continuation).
    let toddler_params = model.params(Phenotype {
        gender: 1.0,
        age: age_parameter_for_years(2.5),
        ..Phenotype::default()
    });
    let toddler_rest = model.rest(&toddler_params).unwrap();
    let drape_posed = model
        .pose(
            &toddler_rest,
            &pose_deltas(&model, &toddler_rest, &Pose::drape(), 1.0).unwrap(),
        )
        .unwrap();
    let base_bf = BodyFrames::new(&model, &drape_posed).unwrap();

    let mut standing_50 = Pose::a_pose(50.0);
    standing_50.straight_arms = true;
    let mut arms_up = Pose::arms_up();
    arms_up.straight_arms = true;
    let mut seated = Pose::drape();
    seated.legs = Legs::Seated;
    let mut squat = Pose::drape();
    squat.legs = Legs::Squat;

    for (pname, pose) in [
        ("standing_50", standing_50),
        ("arms_up_150", arms_up),
        ("seated_1.00", seated),
        ("squat_1.00", squat),
    ] {
        let p_posed = model
            .pose(
                &toddler_rest,
                &pose_deltas(&model, &toddler_rest, &pose, 1.0).unwrap(),
            )
            .unwrap();
        let bf = BodyFrames::new(&model, &p_posed).unwrap();
        let indep_bf = independent_body_frames(&model, &p_posed);
        assert_body_frames_match_independent(&bf, &indep_bf, pname);
        eprintln!(
            "[ftx-onn.16] toddler_2_5y pose {pname:12} vs drape_35: Δchest={:+6.1} mm, Δshoulder={:+6.1} mm, Δwaist={:+6.1} mm, Δcrotch={:+6.1} mm",
            bf.landmarks["chest"] - base_bf.landmarks["chest"],
            bf.landmarks["shoulder"] - base_bf.landmarks["shoulder"],
            bf.landmarks["waist"] - base_bf.landmarks["waist"],
            bf.landmarks["crotch"] - base_bf.landmarks["crotch"],
        );
        if pname == "standing_50" {
            // Standing 50° A-pose keeps waist/crotch virtually unchanged (< 1 mm), while abducting
            // the arm from 35° to 50° moves lower `upperarm02` vertices outside `|shoulder.x| + r`
            // and shifts the `chest` armpit heuristic upward (+27.0 mm on 2.5y toddler), proving
            // why `tape_readings` / `fit_tape` is pinned to the 35° `Pose::drape()` reference pose.
            assert!((bf.landmarks["waist"] - base_bf.landmarks["waist"]).abs() < 1.0);
            assert!((bf.landmarks["crotch"] - base_bf.landmarks["crotch"]).abs() < 1.0);
            let d_chest = bf.landmarks["chest"] - base_bf.landmarks["chest"];
            assert!(
                d_chest > 15.0 && d_chest < 35.0,
                "standing_50 Δchest = {d_chest:.2} mm"
            );
        }
        if pname == "arms_up_150" {
            // Raising arms to 150° flips `arm.up.z` negative (origin -> elbow points downward when
            // arm points up), dropping `shoulder_tip` below `chest`: direct wrap in 150° is outside
            // the standing A-pose placement domain!
            assert!(
                bf.landmarks["shoulder"] < bf.landmarks["chest"],
                "arms_up_150 inverts shoulder_tip vs armpit height, confirming pose-ramp domain boundary"
            );
        }
    }

    eprintln!(
        "[ftx-onn.16] Independent BodyFrames max |Δ| = {worst_frame_mm:.3e} mm; end-to-end independent frames+tape max |Δ| = {worst_tape_from_indep_frames_mm:.3e} mm"
    );
}

#[test]
#[ignore = "requires local Anny 0.6.1 .ftxbody cache"]
fn admitted_anny_held_out_girth_and_length_fit_tape_cross_validation() {
    let path = default_model_path().expect("default_model_path");
    let data = AnnyData::read(&path, true).expect("verified Anny 0.6.1 .ftxbody");
    let model = AnnyModel::from_data(data).expect("AnnyModel");
    let faces: Vec<[usize; 3]> = model
        .data()
        .faces
        .iter()
        .map(|f| f.map(|i| i as usize))
        .collect();
    let pose = Pose::drape();

    // Three non-midpoint cohorts with authored local-change perturbations so held-out `MODEL_FIT`
    // predictions cannot trivially coast on the `0.5 / 0.0` prior mean.
    type HeldOutCase<'a> = (&'a str, f64, f64, [f64; 4], [(&'a str, f64); 4]);
    let cases: [HeldOutCase<'_>; 3] = [
        (
            "toddler_2_5y_perturbed",
            2.5,
            1.0,
            [0.42, 0.64, 0.55, 0.45],
            [
                ("measure-waist-circ-incr", 0.22),
                ("measure-bust-circ-incr", 0.18),
                ("measure-upperleg-height-incr", -0.15),
                ("measure-upperarm-length-incr", 0.14),
            ],
        ),
        (
            "child_7y_perturbed",
            7.0,
            0.0,
            [0.62, 0.38, 0.48, 0.58],
            [
                ("measure-waist-circ-incr", -0.18),
                ("measure-hips-circ-incr", -0.16),
                ("measure-upperleg-height-incr", 0.18),
                ("measure-lowerleg-height-incr", 0.12),
            ],
        ),
        (
            "adult_30y_perturbed",
            30.0,
            1.0,
            [0.58, 0.66, 0.46, 0.56],
            [
                ("measure-bust-circ-incr", 0.24),
                ("measure-hips-circ-incr", 0.20),
                ("measure-napetowaist-dist-incr", -0.14),
                ("measure-upperarm-circ-incr", 0.16),
            ],
        ),
    ];

    let eval_fit = |fit_params: &ftx_body::BodyParams,
                    truth: &[f64; N_TAPE],
                    fitted_mask: &[bool; N_TAPE]|
     -> (f64, f64, f64, f64) {
        let r_rest = model.rest(fit_params).unwrap();
        let r_h = model.anthropometry(&r_rest).height;
        let r_posed = model
            .pose(&r_rest, &pose_deltas(&model, &r_rest, &pose, 1.0).unwrap())
            .unwrap();
        let r_frames = BodyFrames::new(&model, &r_posed).unwrap();
        let pred = tape_readings(&faces, r_h, &r_posed, &r_frames).unwrap();

        let (mut fit_sq, mut fit_max, mut fit_n) = (0.0, 0.0_f64, 0usize);
        let (mut ho_sq, mut ho_max, mut ho_n) = (0.0, 0.0_f64, 0usize);
        for idx in 0..N_TAPE {
            let err = (pred[idx] - truth[idx]).abs();
            if fitted_mask[idx] {
                fit_sq += err * err;
                fit_max = fit_max.max(err);
                fit_n += 1;
            } else {
                ho_sq += err * err;
                ho_max = ho_max.max(err);
                ho_n += 1;
            }
        }
        (
            (fit_sq / fit_n as f64).sqrt(),
            fit_max,
            (ho_sq / ho_n as f64).sqrt(),
            ho_max,
        )
    };

    for &(label, years, gender, ph, ref locals) in &cases {
        let mut p = model.params(Phenotype {
            gender,
            age: age_parameter_for_years(years),
            height: ph[0],
            weight: ph[1],
            muscle: ph[2],
            proportions: ph[3],
        });
        for &(lname, val) in locals {
            p.local_changes[model.local_change_index(lname).unwrap()] = val;
        }
        let rest = model.rest(&p).unwrap();
        let height_m = model.anthropometry(&rest).height;
        let mass_kg = model.anthropometry(&rest).mass;
        let posed = model
            .pose(&rest, &pose_deltas(&model, &rest, &pose, 1.0).unwrap())
            .unwrap();
        let frames = BodyFrames::new(&model, &posed).unwrap();
        let truth = tape_readings(&faces, height_m, &posed, &frames).unwrap();

        // Regime A: Fit girths + stature (12 targets), hold out all 9 non-stature lengths/heights.
        let mut mask_a = [false; N_TAPE];
        let mut targets_a = Vec::new();
        for (idx, &name) in TAPE_FIELDS.iter().enumerate() {
            if name == "stature" || is_girth(name) {
                mask_a[idx] = true;
                targets_a.push(TapeTarget {
                    name: name.to_string(),
                    value_mm: truth[idx],
                    sigma_mm: None,
                });
            }
        }
        let fit_a = fit_tape(
            &model,
            &Wearer {
                years,
                gender,
                stature_mm: truth[0],
                weight_kg: None,
            },
            &targets_a,
            &pose,
        )
        .unwrap();
        let (a_fit_rmse, a_fit_max, a_ho_rmse, a_ho_max) = eval_fit(&fit_a.params, &truth, &mask_a);
        assert!(
            a_fit_max <= 3.0,
            "{label} Regime A fitted girths+stature max {a_fit_max:.2} mm > 3.0 mm"
        );
        assert!(
            a_ho_max <= 35.0,
            "{label} Regime A held-out length max {a_ho_max:.2} mm exceeded bound"
        );

        // Regime B1 (without weight_kg) & B2 (with weight_kg): Fit lengths/heights + stature (10
        // targets), hold out all 11 girths.
        let mut mask_b = [false; N_TAPE];
        let mut targets_b = Vec::new();
        for (idx, &name) in TAPE_FIELDS.iter().enumerate() {
            if !is_girth(name) {
                mask_b[idx] = true;
                targets_b.push(TapeTarget {
                    name: name.to_string(),
                    value_mm: truth[idx],
                    sigma_mm: None,
                });
            }
        }
        let fit_b_no_wt = fit_tape(
            &model,
            &Wearer {
                years,
                gender,
                stature_mm: truth[0],
                weight_kg: None,
            },
            &targets_b,
            &pose,
        )
        .unwrap();
        let (b1_fit_rmse, b1_fit_max, b1_ho_rmse, b1_ho_max) =
            eval_fit(&fit_b_no_wt.params, &truth, &mask_b);

        let fit_b_wt = fit_tape(
            &model,
            &Wearer {
                years,
                gender,
                stature_mm: truth[0],
                weight_kg: Some(mass_kg),
            },
            &targets_b,
            &pose,
        )
        .unwrap();
        let (b2_fit_rmse, b2_fit_max, b2_ho_rmse, b2_ho_max) =
            eval_fit(&fit_b_wt.params, &truth, &mask_b);
        assert!(b1_fit_max <= 3.5 && b2_fit_max <= 3.5);
        assert!(b1_ho_max <= 35.0 && b2_ho_max <= 30.0);
        if label != "adult_30y_perturbed" {
            // Where global `weight` drives girth variation, adding `weight_kg` cuts held-out girth
            // `MODEL_FIT` RMSE roughly in half; when localized bust/hip/arm blendshapes dominate
            // (`adult_30y_perturbed`), global `weight_kg` alone cannot disambiguate local girths.
            assert!(
                b2_ho_rmse < b1_ho_rmse,
                "{label}: adding weight_kg must improve held-out girth MODEL_FIT RMSE ({b2_ho_rmse:.2} vs {b1_ho_rmse:.2} mm)"
            );
        }

        // Regime C: Fit core trunk (`stature`, `chest`, `waist`, `hip`), hold out all 17 limb/head/length readings.
        let mut mask_c = [false; N_TAPE];
        let mut targets_c = Vec::new();
        for (idx, &name) in TAPE_FIELDS.iter().enumerate() {
            if matches!(name, "stature" | "chest" | "waist" | "hip") {
                mask_c[idx] = true;
                targets_c.push(TapeTarget {
                    name: name.to_string(),
                    value_mm: truth[idx],
                    sigma_mm: None,
                });
            }
        }
        let fit_c = fit_tape(
            &model,
            &Wearer {
                years,
                gender,
                stature_mm: truth[0],
                weight_kg: None,
            },
            &targets_c,
            &pose,
        )
        .unwrap();
        let (c_fit_rmse, c_fit_max, c_ho_rmse, c_ho_max) = eval_fit(&fit_c.params, &truth, &mask_c);
        assert!(c_fit_max <= 3.0);

        eprintln!(
            "[ftx-onn.16] {label:22} held-out CV:\n  A (fit girths -> hold out 9 lengths):  fit RMSE={a_fit_rmse:4.2} max={a_fit_max:4.2} mm | held-out MODEL_FIT RMSE={a_ho_rmse:5.2} max={a_ho_max:5.2} mm\n  B1(fit lengths -> hold out 11 girths): fit RMSE={b1_fit_rmse:4.2} max={b1_fit_max:4.2} mm | held-out MODEL_FIT RMSE={b1_ho_rmse:5.2} max={b1_ho_max:5.2} mm\n  B2(lengths+wt  -> hold out 11 girths): fit RMSE={b2_fit_rmse:4.2} max={b2_fit_max:4.2} mm | held-out MODEL_FIT RMSE={b2_ho_rmse:5.2} max={b2_ho_max:5.2} mm\n  C (core trunk  -> hold out 17 fields): fit RMSE={c_fit_rmse:4.2} max={c_fit_max:4.2} mm | held-out MODEL_FIT RMSE={c_ho_rmse:5.2} max={c_ho_max:5.2} mm"
        );
        let _ = (FREE, N_PHENOTYPE);
    }

    // Independent public-prior 920 mm / 30-month toddler size-chart cross-validation (not drawn
    // from Anny): fit core trunk (`stature=920`, `chest=540`, `waist=510`, `hip=550`) and compare
    // Anny's `MODEL_FIT` predictions against the external chart's held-out lengths/girths.
    let chart_92: [(&str, f64); N_TAPE] = [
        ("stature", 920.0),
        ("chest", 540.0),
        ("waist", 510.0),
        ("hip", 550.0),
        ("waist_height", 530.0),
        ("crotch_height", 372.5),
        ("thigh", 310.0),
        ("knee", 225.0),
        ("calf", 215.0),
        ("ankle", 150.0),
        ("upper_arm", 165.0),
        ("wrist", 120.0),
        ("arm_length", 310.0),
        ("knee_height", 231.0),
        ("back_neck_to_waist", 235.0),
        ("neck_base", 255.0),
        ("head", 495.0),
        ("shoulder_length", 75.0),
        ("across_back", 225.0),
        ("total_crotch_length", 435.0),
        ("outseam", 540.0),
    ];
    let chart_truth: [f64; N_TAPE] = std::array::from_fn(|i| chart_92[i].1);
    let mut chart_mask = [false; N_TAPE];
    let mut chart_targets = Vec::new();
    for (idx, &(name, val)) in chart_92.iter().enumerate() {
        if matches!(name, "stature" | "chest" | "waist" | "hip") {
            chart_mask[idx] = true;
            chart_targets.push(TapeTarget {
                name: name.to_string(),
                value_mm: val,
                sigma_mm: None,
            });
        }
    }
    let chart_fit = fit_tape(
        &model,
        &Wearer {
            years: 2.5,
            gender: 0.5,
            stature_mm: 920.0,
            weight_kg: None,
        },
        &chart_targets,
        &pose,
    )
    .unwrap();
    let (ch_fit_rmse, ch_fit_max, ch_ho_rmse, ch_ho_max) =
        eval_fit(&chart_fit.params, &chart_truth, &chart_mask);
    eprintln!(
        "[ftx-onn.16] chart_92cm_public_prior (fit trunk -> hold out 17 chart fields): fit RMSE={ch_fit_rmse:4.2} max={ch_fit_max:4.2} mm | held-out vs chart RMSE={ch_ho_rmse:5.2} max={ch_ho_max:5.2} mm"
    );
    assert!(ch_fit_max <= 3.0);
    // Held-out `MODEL_FIT` vs external size chart naturally shows tens-of-mm discrepancies (which
    // is precisely why `MODEL_FIT` never satisfies `readiness` and synthetic 21-target recovery
    // must never be conflated with held-out population accuracy!).
    assert!(ch_ho_max > 10.0 && ch_ho_max < 80.0);
}

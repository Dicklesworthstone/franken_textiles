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
    AnnyData, AnnyModel, BodyFrames, DEFAULT_POSE_AUDIT_PAIR_BUDGET, Frame, Legs, N_TAPE,
    Phenotype, Pose, PosedBody, TAPE_FIELDS, age_parameter_for_years, clothing_skin,
    default_model_path, partial_tape_readings, pose_deltas, skin_mesh, tape_readings,
};
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

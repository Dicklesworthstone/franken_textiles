//! Drape metrics and flags (spec 04 §6): what the simulator is the authority for — seam closure, ring
//! strain at girth landmarks, penetration and slip per piece, hems and balance, bands, settledness —
//! with the reference's exact flag texts and its split into gating and advisory flags.
//!
//! Port of the reference's `metrics()` in its arithmetic order (sequential means over frames, pairwise
//! means over vertices, numpy percentiles and medians, Python rounding and float repr in the texts).

use ftx_core::Vec3;
use ftx_core::np::{pairwise_sum, percentile};
use ftx_core::py::{fixed, repr_f64, round};
use ftx_pattern::Pattern;
use serde_json::{Map, Value, json};

use crate::garment::GarmentMesh;
use crate::nearest::NearestTree;
use crate::start::{MM, edge_strain, face_edges};

/// Frames averaged for the final shape (collision jitter).
pub const AVG_FRAMES: usize = 15;

/// Fewest frames the metrics can judge: settledness compares the mean of the last w frames with the
/// mean of the w before the previous w, w ≥ 2.
pub const MIN_FRAMES: usize = 6;

/// What the metrics read besides the mesh.
pub struct MetricsInput<'a> {
    /// The pattern (instances' edges, placements and roles).
    pub pattern: &'a Pattern,
    /// The baked frames (sew then drape), metres.
    pub seq: &'a [Vec<Vec3>],
    /// Frames of the sew phase (`co_sew = seq[sew_end − 1]`).
    pub sew_end: usize,
    /// The wrapped start (the drafted level in 3D).
    pub wrapped: &'a [Vec3],
    /// The true body.
    pub body: &'a NearestTree,
    /// Piece-y of a landmark's level for an instance: `landmark_h(part, landmark) −
    /// landmark_h(part, y0_landmark) + y0` (the reference's `_level_y`).
    pub level_y: &'a dyn Fn(&str, &str) -> Option<f64>,
    /// The fit spec (`points` with `kind`, `instances`, `landmark`, `dy`).
    pub fit_spec: &'a Value,
    /// Mesh spacing (mm).
    pub spacing: f64,
    /// Arm abduction of the pose (degrees, for the ride-up text).
    pub arm_deg: f64,
}

fn norm(d: Vec3) -> f64 {
    (d.x * d.x + d.y * d.y + d.z * d.z).sqrt()
}

/// numpy `mean` of a 1-D array (pairwise sum / n).
fn mean(v: &[f64]) -> f64 {
    pairwise_sum(v) / v.len() as f64
}

/// numpy `median` (the mean of the two middle values for an even count).
fn median(v: &[f64]) -> f64 {
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    let n = s.len();
    if n % 2 == 1 {
        s[n / 2]
    } else {
        (s[n / 2 - 1] + s[n / 2]) / 2.0
    }
}

/// numpy `mean(frames, axis=0)`: frames added in order, then divided.
fn frame_mean(frames: &[Vec<Vec3>]) -> Vec<Vec3> {
    let mut acc = frames[0].clone();
    for f in &frames[1..] {
        for (a, b) in acc.iter_mut().zip(f) {
            *a += *b;
        }
    }
    let n = frames.len() as f64;
    acc.into_iter().map(|p| p / n).collect()
}

fn fraction(flags: impl Iterator<Item = bool>) -> f64 {
    let v: Vec<f64> = flags.map(|b| if b { 1.0 } else { 0.0 }).collect();
    mean(&v)
}

/// The report (`sim_report.json`'s metric fields, flags, gating and tightness gating).
///
/// # Panics
/// When `seq` has fewer than [`MIN_FRAMES`] frames or fewer than `sew_end` (a caller bug).
#[must_use]
#[allow(clippy::too_many_lines)] // one pass, in the reference's order
pub fn metrics(mesh: &GarmentMesh, input: &MetricsInput) -> Value {
    let seq = input.seq;
    let co = frame_mean(&seq[seq.len().saturating_sub(AVG_FRAMES)..]);
    let co_sew = &seq[input.sew_end - 1];
    let rest2d = &mesh.verts2d;
    let edges = face_edges(&mesh.faces);
    let strain = edge_strain(&co, rest2d, &edges);
    let (mut flags, mut gating, mut tightness): (Vec<String>, Vec<String>, Vec<String>) =
        (Vec::new(), Vec::new(), Vec::new());

    // Seams.
    let mut seams = Map::new();
    for s in &mesh.seams {
        let gap = |x: &[Vec3]| -> f64 {
            s.pairs.iter().map(|&(a, b)| norm(x[a] - x[b])).fold(
                if s.pairs.is_empty() {
                    0.0
                } else {
                    f64::NEG_INFINITY
                },
                f64::max,
            )
        };
        let side_e: Vec<(usize, usize)> = s
            .side_a
            .windows(2)
            .chain(s.side_b.windows(2))
            .map(|w| (w[0], w[1]))
            .collect();
        let st = if side_e.is_empty() {
            vec![0.0]
        } else {
            edge_strain(&co, rest2d, &side_e)
        };
        let gap_final = round(gap(&co) / MM, 2);
        let smin = st.iter().copied().fold(f64::INFINITY, f64::min);
        let smax = st.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        if gap_final > 5.0 {
            flags.push(format!(
                "seam {} did not close ({} mm): check its placement and lengths",
                s.id,
                repr_f64(gap_final)
            ));
            gating.push(flags.last().cloned().unwrap_or_default());
        }
        let abs: Vec<f64> = st.iter().map(|v| v.abs()).collect();
        let p90 = percentile(&abs, 90.0);
        let gathered = (s.ratio - 1.0).abs() > 0.15;
        if p90 > 0.08 && !gathered {
            flags.push(format!(
                "seam {} is distorted in the drape (p90 {} %): if its seam walk passes, this is crowding or \
                 pull at that seam (look at the stills), not a length mismatch",
                s.id,
                fixed(p90 * 100.0, 0)
            ));
        }
        seams.insert(
            s.id.clone(),
            json!({
                "gap_after_sew_mm": round(gap(co_sew) / MM, 2),
                "gap_final_mm": gap_final,
                "seam_strain_pct": [round(smin * 100.0, 2), round(smax * 100.0, 2)],
                "seam_strain_p90_pct": round(p90 * 100.0, 2),
                "gathered": gathered,
            }),
        );
    }

    // Landmarks: ring strain at girth points.
    let range_of = |id: &str| -> Option<(usize, usize)> {
        mesh.instances
            .iter()
            .position(|x| x == id)
            .map(|k| mesh.inst_range[k])
    };
    let mut landmarks = Map::new();
    let empty = Vec::new();
    for p in input.fit_spec["points"].as_array().unwrap_or(&empty) {
        if p["kind"].as_str() != Some("girth") {
            continue;
        }
        let mut sel = Vec::new();
        for iid in p["instances"].as_array().unwrap_or(&empty) {
            let Some(iid) = iid.as_str() else { continue };
            let Some((lo, hi)) = range_of(iid) else {
                continue;
            };
            let Some(level) = (input.level_y)(iid, p["landmark"].as_str().unwrap_or("")) else {
                continue;
            };
            let y = level + p.get("dy").and_then(Value::as_f64).unwrap_or(0.0);
            for &(i, j) in &edges {
                if (lo..hi).contains(&i) && (lo..hi).contains(&j) {
                    let d = rest2d[j] - rest2d[i];
                    if ((rest2d[i].y + rest2d[j].y) / 2.0 - y).abs() < input.spacing
                        && d.y.abs() < 0.6 * d.x.abs()
                    {
                        sel.push((i, j));
                    }
                }
            }
        }
        if sel.is_empty() {
            continue;
        }
        let st = edge_strain(&co, rest2d, &sel);
        let mut verts: Vec<usize> = sel.iter().flat_map(|&(a, b)| [a, b]).collect();
        verts.sort_unstable();
        verts.dedup();
        let sd: Vec<f64> = verts
            .iter()
            .map(|&v| input.body.signed_distance(co[v]))
            .collect();
        let z_now: Vec<f64> = verts.iter().map(|&v| co[v].z).collect();
        let z_start: Vec<f64> = verts.iter().map(|&v| seq[0][v].z).collect();
        let mean_pct = round(mean(&st) * 100.0, 2);
        let max_pct = round(
            st.iter().copied().fold(f64::NEG_INFINITY, f64::max) * 100.0,
            2,
        );
        let p90_pct = round(percentile(&st, 90.0) * 100.0, 2);
        let contact = round(fraction(sd.iter().map(|&d| d < 0.003)), 3);
        let reading = if mean_pct > 1.0 || p90_pct > 3.0 {
            "TIGHT"
        } else if contact > 0.6 {
            "SNUG"
        } else {
            "EASY"
        };
        let id = p["id"].as_str().unwrap_or("");
        if reading == "TIGHT" {
            flags.push(format!(
                "{id}: fabric stretched around the body (mean {} %, p90 {} %)",
                repr_f64(mean_pct),
                repr_f64(p90_pct)
            ));
            if mean_pct > 2.0 {
                let f = flags.last().cloned().unwrap_or_default();
                gating.push(f.clone());
                tightness.push(f);
            }
        }
        landmarks.insert(
            id.to_string(),
            json!({
                "ring_strain_mean_pct": mean_pct,
                "ring_strain_max_pct": max_pct,
                "ring_strain_p90_pct": p90_pct,
                "contact_frac": contact,
                "clearance_mean_mm": round(mean(&sd) / MM, 1),
                "ring_shift_mm": round((mean(&z_now) - mean(&z_start)) / MM, 1),
                "reading": reading,
            }),
        );
    }

    // Pieces: contact, penetration against the true body, slip of the corners.
    let raw = input.pattern.raw();
    let piece_def = |id: &str| {
        input
            .pattern
            .instance(id)
            .map_or(&Value::Null, |i| &raw["pieces"][i.piece_index])
    };
    let sd_all: Vec<f64> = co.iter().map(|&p| input.body.signed_distance(p)).collect();
    let mut pieces = Map::new();
    for (k, id) in mesh.instances.iter().enumerate() {
        let (lo, hi) = mesh.inst_range[k];
        let sd = &sd_all[lo..hi];
        let drift: Vec<f64> = input
            .pattern
            .instance(id)
            .map(|inst| {
                inst.verts
                    .iter()
                    .filter_map(|(vid, _)| mesh.corner.get(&(id.clone(), vid.to_string())))
                    .map(|&g| (co[g].z - co_sew[g].z) / MM)
                    .collect()
            })
            .unwrap_or_default();
        let pen = round(
            0.0f64.max(-sd.iter().copied().fold(f64::INFINITY, f64::min)) / MM,
            1,
        );
        let med = if drift.is_empty() {
            0.0
        } else {
            median(&drift)
        };
        if pen > 3.0 {
            flags.push(format!("{id} passes {} mm into the body", repr_f64(pen)));
            gating.push(flags.last().cloned().unwrap_or_default());
        }
        if piece_def(id)["placement"]["part"].as_str() == Some("torso")
            && !drift.is_empty()
            && med < -20.0
        {
            flags.push(format!(
                "{id} slid {} mm down after gravity: nothing is holding it up (too big at the \
                 shoulders/waist?)",
                fixed(med.abs(), 0)
            ));
            gating.push(flags.last().cloned().unwrap_or_default());
        }
        pieces.insert(
            id.clone(),
            json!({
                "contact_frac": round(fraction(sd.iter().map(|&d| d < 0.003)), 3),
                "penetration_max_mm": pen,
                "corner_drop_median_mm": round(med, 1),
            }),
        );
    }

    // Hems: final height against the drafted level.
    let (mut hems, mut rise, mut torso_rise) =
        (Map::new(), Vec::<(String, f64)>::new(), Vec::new());
    for id in &mesh.instances {
        let Some(inst) = input.pattern.instance(id) else {
            continue;
        };
        for e in &inst.edges {
            if e.id.split('.').next() != Some("hem") {
                continue;
            }
            let Some(idx) = mesh.index.get(&(id.clone(), e.id.clone())) else {
                continue;
            };
            let z = mean(&idx.iter().map(|&i| co[i].z).collect::<Vec<_>>()) / MM;
            let w = mean(&idx.iter().map(|&i| input.wrapped[i].z).collect::<Vec<_>>()) / MM;
            let key = format!("{id}/{}", e.id);
            hems.insert(key.clone(), json!(round(z, 1)));
            let r = round(z - w, 1);
            rise.push((key, r));
            if piece_def(id)["placement"]["part"].as_str() == Some("torso") {
                torso_rise.push(r);
            }
        }
    }

    // Bands: elastic casings and cuffs.
    let mut bands = Map::new();
    for (k, id) in mesh.instances.iter().enumerate() {
        let role = piece_def(id)["role"].as_str().unwrap_or("");
        if role != "waistband" && role != "cuff" {
            continue;
        }
        let (lo, hi) = mesh.inst_range[k];
        let now = mean(&co[lo..hi].iter().map(|p| p.z).collect::<Vec<_>>());
        let was = mean(
            &input.wrapped[lo..hi]
                .iter()
                .map(|p| p.z)
                .collect::<Vec<_>>(),
        );
        let shift = (now - was) / MM;
        bands.insert(
            id.clone(),
            json!({"role": role, "shift_mm": round(shift, 1)}),
        );
        if role == "waistband" && shift < -25.0 {
            flags.push(format!(
                "{id} ended {} mm below the waist: something drags it down (compare the crotch and rise rows \
                 of the validation table; look at the stills)",
                fixed(shift.abs(), 0)
            ));
        } else if role == "cuff" && shift.abs() > 25.0 {
            flags.push(format!(
                "{id} sits {} mm {} its drafted level: {}",
                fixed(shift.abs(), 0),
                if shift < 0.0 { "below" } else { "above" },
                if shift < 0.0 {
                    "its ease lets it slide down the leg, so the blousing is lost (snug the cuff or shorten \
                     the leg)"
                } else {
                    "the leg is too short to reach it"
                }
            ));
        }
    }

    let mut report = Map::new();
    let fr: Vec<f64> = rise
        .iter()
        .filter(|(k, _)| k.starts_with("front"))
        .map(|r| r.1)
        .collect();
    let bk: Vec<f64> = rise
        .iter()
        .filter(|(k, _)| k.starts_with("back"))
        .map(|r| r.1)
        .collect();
    if !fr.is_empty() && !bk.is_empty() {
        let balance = round(mean(&fr) - mean(&bk), 1);
        report.insert("hem_balance_mm".into(), json!(balance));
        if balance.abs() > 10.0 {
            let (hi, lo) = if balance > 0.0 {
                ("front", "back")
            } else {
                ("back", "front")
            };
            flags.push(format!(
                "hem out of balance: the {hi} hangs {} mm higher than the {lo} (a protruding belly lifts the \
                 front: add front length)",
                fixed(balance.abs(), 0)
            ));
        }
    }
    let lost = rise.iter().filter(|(_, v)| v.abs() > 150.0).count();
    if lost > 0 {
        report.insert("verdict".into(), json!("LOST"));
        let f = format!(
            "the garment did not stay on the body ({lost} hems moved > 150 mm): usually a girth with negative \
             ease (it is squeezed off, see the validation table) or seams that cannot close; every other sim \
             number is meaningless until this is fixed"
        );
        flags.insert(0, f.clone());
        gating.insert(0, f);
    } else if !torso_rise.is_empty() && mean(&torso_rise) > 25.0 {
        flags.push(format!(
            "the body rides up {} mm with arms at {}° (set-in sleeve pulls the side up; a lower cap or more \
             underarm room frees the arm)",
            fixed(mean(&torso_rise), 0),
            fixed(input.arm_deg, 0)
        ));
    }

    // Settledness: drift of window means, not speed.
    let n = seq.len();
    let w = (n / 6).clamp(2, 10);
    let late = frame_mean(&seq[n - w..]);
    let early = frame_mean(&seq[n - 3 * w..n - 2 * w]);
    let drift: Vec<f64> = late
        .iter()
        .zip(&early)
        .map(|(a, b)| norm(*a - *b) / (2 * w) as f64)
        .collect();
    let steps: Vec<f64> = seq[n - w..]
        .windows(2)
        .flat_map(|f| f[1].iter().zip(&f[0]).map(|(a, b)| norm(*a - *b)))
        .collect();
    let jitter = median(&steps);
    let drift_frac = round(fraction(drift.iter().map(|&d| d > 0.0001)), 3);
    let settled = drift_frac <= 0.02;
    let by_piece: Vec<(String, f64)> = mesh
        .instances
        .iter()
        .zip(&mesh.inst_range)
        .map(|(id, &(lo, hi))| {
            (
                id.clone(),
                round(fraction(drift[lo..hi].iter().map(|&d| d > 0.0001)), 3),
            )
        })
        .collect();
    let p99 = round(percentile(&drift, 99.0) / MM, 3);
    if !settled {
        let (worst, share) = by_piece
            .iter()
            .fold(None::<&(String, f64)>, |best, x| match best {
                Some(b) if b.1 >= x.1 => Some(b),
                _ => Some(x),
            })
            .map_or(("", 0.0), |(k, v)| (k.as_str(), *v));
        flags.push(format!(
            "not settled ({} % of vertices still drifting > 0.1 mm/frame, most in {worst} ({} %), p99 {} \
             mm/frame): drape numbers are provisional; rerun with more drape frames and compare",
            fixed(drift_frac * 100.0, 0),
            fixed(share * 100.0, 0),
            repr_f64(p99)
        ));
    }

    let smin = strain.iter().copied().fold(f64::INFINITY, f64::min);
    let smax = strain.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    report.insert("seams".into(), Value::Object(seams));
    report.insert("landmarks".into(), Value::Object(landmarks));
    report.insert("pieces".into(), Value::Object(pieces));
    report.insert("flags".into(), json!(flags));
    report.insert("gating".into(), json!(gating));
    report.insert("tightness_gating".into(), json!(tightness));
    report.insert("hem_heights_mm".into(), Value::Object(hems));
    report.insert(
        "hem_rise_mm".into(),
        Value::Object(rise.into_iter().map(|(k, v)| (k, json!(v))).collect()),
    );
    report.insert("bands".into(), Value::Object(bands));
    report.insert("drift_frac".into(), json!(drift_frac));
    report.insert("moving_frac".into(), json!(drift_frac));
    report.insert("jitter_mm_per_frame".into(), json!(round(jitter / MM, 3)));
    report.insert("settled".into(), json!(settled));
    report.insert(
        "drift_by_piece".into(),
        Value::Object(by_piece.into_iter().map(|(k, v)| (k, json!(v))).collect()),
    );
    report.insert("drift_p99_mm_per_frame".into(), json!(p99));
    report.insert(
        "global_strain_pct".into(),
        json!({
            "p95": round(percentile(&strain, 95.0) * 100.0, 2),
            "max": round(smax * 100.0, 2),
            "min": round(smin * 100.0, 2),
        }),
    );
    Value::Object(report)
}

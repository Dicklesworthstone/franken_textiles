//! Read completed native drapes and diagnose corner heights, without rerunning the solver.

use std::{fs::File, io::Read, path::Path};

use ftx_core::{Refusal, RefusalCode, py, receipt::sha256_hex};
use ftx_drape::{
    point_cache::{MAX_CACHE_BYTES, PointCache},
    render::{ColouredMesh, read_obj},
};
use ftx_pattern::{
    Pattern,
    draft::form::{Meas, landmarks},
};
use serde_json::{Value, json};

use crate::{Args, bad_input, known_options, required, workspace};

pub(super) fn bytes(path: &Path, limit: usize) -> Result<Vec<u8>, Refusal> {
    let f = File::open(path).map_err(|e| workspace::io_error(path, e))?;
    if !f
        .metadata()
        .map_err(|e| workspace::io_error(path, e))?
        .is_file()
    {
        return Err(bad_input(format!(
            "{}: expected a regular input file",
            path.display()
        )));
    }
    let mut bytes = Vec::new();
    f.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| workspace::io_error(path, e))?;
    if bytes.len() > limit {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            format!("{}: exceeds {limit} byte input budget", path.display()),
        ));
    }
    Ok(bytes)
}

pub(super) fn document(path: &Path) -> Result<(Value, Vec<u8>), Refusal> {
    let raw = bytes(path, 16 * 1024 * 1024)?;
    let value =
        serde_json::from_slice(&raw).map_err(|e| bad_input(format!("{}: {e}", path.display())))?;
    Ok((value, raw))
}

pub(super) fn mesh(path: &Path) -> Result<ColouredMesh, Refusal> {
    let raw = bytes(path, 64 * 1024 * 1024)?;
    let text =
        std::str::from_utf8(&raw).map_err(|e| bad_input(format!("{}: {e}", path.display())))?;
    let m = read_obj(text).map_err(|e| bad_input(format!("{}: {e}", path.display())))?;
    if m.vertices.is_empty()
        || m.triangles.is_empty()
        || m.vertices.iter().any(|p| !p.is_finite())
        || m.colours
            .iter()
            .flatten()
            .any(|x| !x.is_finite() || !(0.0..=1.0).contains(x))
    {
        return Err(bad_input(format!(
            "{}: expected a nonempty finite mesh with valid colours",
            path.display()
        )));
    }
    Ok(m)
}

pub(super) fn floor(body: &ColouredMesh) -> f64 {
    // Ignore unused OBJ vertices; only the displayed skin establishes its floor.
    body.triangles
        .iter()
        .flatten()
        .map(|&i| body.vertices[i].z)
        .fold(f64::INFINITY, f64::min)
}

pub(super) fn check_map(map: &Value, cache: &PointCache<'_>) -> Result<(), Refusal> {
    if map["units"] != "m" || map["vertices"].as_u64() != Some(cache.points() as u64) {
        return Err(bad_input(
            "mesh_map units/count do not match the metric PC2 cache",
        ));
    }
    if let Some(frames) = map.get("frames") {
        let total = ["fold", "zip", "sew", "drape"]
            .iter()
            .try_fold(0_u64, |a, k| {
                frames[*k].as_u64().and_then(|n| a.checked_add(n))
            });
        if total != Some(cache.frames() as u64) {
            return Err(bad_input(
                "mesh_map segment counts do not match PC2 samples",
            ));
        }
    }
    Ok(())
}

pub(super) fn sim_points(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(args, "sim-points", &["pattern", "measurements"])?;
    if args.positional.len() != 2 {
        return Err(bad_input("ftx sim-points needs exactly one SIMDIR"));
    }
    let dir = Path::new(&args.positional[1]);
    let pattern = Pattern::new(document(Path::new(required(args, "pattern")?))?.0);
    let errors = pattern.structure_errors();
    if !errors.is_empty() {
        return Err(bad_input(errors.join("; ")));
    }
    if pattern.raw()["units"] == "px" {
        return Err(Refusal::new(
            RefusalCode::Unscaled,
            "sim-points needs a metric pattern",
        ));
    }
    let (measurements, raw_measurements) = document(Path::new(required(args, "measurements")?))?;
    let levels = landmarks(Meas(&measurements))?;
    let (map, _) = document(&dir.join("mesh_map.json"))?;
    let raw = bytes(&dir.join("fold.pc2"), MAX_CACHE_BYTES)?;
    let cache = PointCache::read(&raw)?;
    check_map(&map, &cache)?;
    let body = mesh(&dir.join("body.obj"))?;
    let floor = floor(&body);
    let count = cache.frames().min(15);
    let positions = cache.mean_tail(count)?;
    let corners = map["corners"]
        .as_object()
        .ok_or_else(|| bad_input("mesh_map has no corner map"))?;
    let mut rows = Vec::new();
    let mut ordered: Vec<_> = corners.iter().collect();
    ordered.sort_by(|a, b| a.0.cmp(b.0));
    for (key, value) in ordered {
        let (id, vertex) = key
            .split_once(':')
            .ok_or_else(|| bad_input("corner keys must be INSTANCE:VERTEX"))?;
        let inst = pattern
            .instance(id)
            .ok_or_else(|| bad_input(format!("unknown corner instance '{id}'")))?;
        let p = inst
            .verts
            .get(vertex)
            .ok_or_else(|| bad_input(format!("unknown corner '{key}'")))?;
        let index = value
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|&i| i < cache.points())
            .ok_or_else(|| bad_input(format!("'{key}': corner index is outside PC2")))?;
        let range = map["inst_range"][id]
            .as_array()
            .filter(|r| r.len() == 2)
            .ok_or_else(|| bad_input(format!("'{id}': missing instance range")))?;
        if !matches!((range[0].as_u64(), range[1].as_u64()), (Some(lo), Some(hi)) if lo <= index as u64 && (index as u64) < hi && hi <= cache.points() as u64)
        {
            return Err(bad_input(format!(
                "'{key}': corner does not belong to its instance range"
            )));
        }
        let placement = &pattern.raw()["pieces"][inst.piece_index]["placement"];
        if placement["part"].as_str().unwrap_or("torso") != "torso" {
            continue;
        }
        let y0 = match placement.get("y0") {
            None => 0.0,
            Some(v) => v
                .as_f64()
                .filter(|x| x.is_finite())
                .ok_or_else(|| bad_input("placement.y0 must be finite"))?,
        };
        let drafted = levels.get(placement["y0_landmark"].as_str().unwrap_or("waist"))? - y0 + p.y;
        let final_z = (positions[index].z - floor) * 1000.0;
        if !drafted.is_finite() || !final_z.is_finite() {
            return Err(bad_input("non-finite corner height"));
        }
        rows.push(json!({"point":key, "drafted_z":py::round(drafted, 1), "final_z":py::round(final_z, 1), "rise":py::round(final_z - drafted, 1)}));
    }
    if rows.is_empty() {
        return Err(bad_input("no torso corners in this fold-up"));
    }
    let mut text = format!(
        "{:<32} {:>11} {:>11} {:>9}\n",
        "point", "drafted z", "final z", "rise"
    );
    for r in &rows {
        text.push_str(&format!(
            "{:<32} {:>11.1} {:>11.1} {:>9.1}\n",
            r["point"].as_str().unwrap(),
            r["drafted_z"].as_f64().unwrap(),
            r["final_z"].as_f64().unwrap(),
            r["rise"].as_f64().unwrap()
        ));
    }
    let no_claim = "Millimetres above the saved body's floor. Drafted levels use the supplied flat pattern and measurements, without sim_scale. Tail means are not a settledness or fit verdict.";
    text.push_str(no_claim);
    Ok((
        json!({"schema":"ftx-sim-points/1", "sim":dir, "frames_averaged":count, "floor_z_m":floor, "measurements_sha256":sha256_hex(&raw_measurements), "rows":rows, "no_claim":no_claim}),
        0,
        text,
    ))
}

fn b64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for k in 0..4 {
            if k <= chunk.len() {
                s.push(A[((n >> (18 - 6 * k)) & 63) as usize] as char);
            } else {
                s.push('=');
            }
        }
    }
    s
}

/// `ftx viewer SIMDIR [--out HTML] [--pattern P]`: rebuilds `fold_viewer.html` from a completed
/// simulation directory (`sim_report.json`, `mesh_map.json`, `fold.pc2`, `body.obj`, `garment.obj`,
/// `garment_strain.obj`) without rerunning the solver (spec 04 §9.7).
pub(super) fn cmd_viewer(args: &Args) -> Result<(Value, i32, String), Refusal> {
    use ftx_core::Vec3;
    use ftx_core::np::linspace;
    use ftx_drape::animation::viewer_html;
    use std::path::PathBuf;

    known_options(args, "viewer", &["out", "pattern"])?;
    if args.positional.len() != 2 {
        return Err(bad_input("ftx viewer needs exactly one SIMDIR"));
    }
    let dir = Path::new(&args.positional[1]);
    let (report, _) = document(&dir.join("sim_report.json"))?;
    let (map, _) = document(&dir.join("mesh_map.json"))?;
    let raw_pc2 = bytes(&dir.join("fold.pc2"), MAX_CACHE_BYTES)?;
    let cache = PointCache::read(&raw_pc2)?;
    check_map(&map, &cache)?;
    let body = mesh(&dir.join("body.obj"))?;
    let garment = mesh(&dir.join("garment.obj"))?;
    if garment.vertices.len() != cache.points() {
        return Err(bad_input(
            "garment.obj vertex count does not match fold.pc2",
        ));
    }
    let strain = match mesh(&dir.join("garment_strain.obj")) {
        Ok(m) if m.vertices.len() == cache.points() => Some(m),
        Ok(_) => {
            return Err(bad_input(
                "garment_strain.obj vertex count does not match fold.pc2",
            ));
        }
        Err(_) => None,
    };

    let pat_doc = if let Some(explicit) = args.options.get("pattern") {
        let (doc, _) = document(Path::new(explicit))?;
        let pat = Pattern::new(doc.clone());
        let errors = pat.structure_errors();
        if !errors.is_empty() {
            return Err(bad_input(errors.join("; ")));
        }
        Some(doc)
    } else {
        let candidate = report["pattern"]
            .as_str()
            .map(PathBuf::from)
            .filter(|p| p.is_file())
            .or_else(|| {
                dir.parent()
                    .map(|p| p.join("pattern.json"))
                    .filter(|p| p.is_file())
            });
        match candidate {
            Some(p) => document(&p).ok().map(|(d, _)| d),
            None => None,
        }
    };

    let title = pat_doc
        .as_ref()
        .and_then(|p| p["meta"]["title"].as_str().or_else(|| p["style"].as_str()))
        .or_else(|| report["style"].as_str())
        .unwrap_or("Garment");

    let n = cache.frames();
    let fold_n = map["frames"]["fold"].as_u64().unwrap_or(30) as usize;
    let zip_n = map["frames"]["zip"].as_u64().unwrap_or(18) as usize;
    let sew_n = map["frames"]["sew"].as_u64().unwrap_or(12) as usize;
    let starts = [0, fold_n, fold_n + zip_n, fold_n + zip_n + sew_n];
    let keep = 60usize;
    let mut kept: Vec<usize> = linspace(0.0, n.saturating_sub(1) as f64, keep.min(n))
        .into_iter()
        .map(|v| v.round_ties_even() as usize)
        .chain(starts.iter().map(|&s| s.min(n.saturating_sub(1))))
        .chain(std::iter::once(n.saturating_sub(1)))
        .collect();
    kept.sort_unstable();
    kept.dedup();

    let mut kept_frames = Vec::with_capacity(kept.len());
    for &k in &kept {
        kept_frames.push(cache.frame(k)?);
    }
    let (mut lo, mut hi) = (
        Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
        Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
    );
    for frame in &kept_frames {
        for p in frame {
            lo = Vec3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
            hi = Vec3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
        }
    }
    let span = [hi.x - lo.x, hi.y - lo.y, hi.z - lo.z].map(|d| d.max(1e-9));
    let (lo3, hi3) = ([lo.x, lo.y, lo.z], [hi.x, hi.y, hi.z]);
    let mut q = Vec::with_capacity(kept.len() * cache.points() * 6);
    for frame in &kept_frames {
        for p in frame {
            for (i, c) in [p.x, p.y, p.z].into_iter().enumerate() {
                let v = ((c - lo3[i]) / span[i] * 65535.0)
                    .round_ties_even()
                    .clamp(0.0, 65535.0);
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                q.extend_from_slice(&(v as u16).to_le_bytes());
            }
        }
    }
    let tris: Vec<u8> = garment
        .triangles
        .iter()
        .flatten()
        .flat_map(|&i| u32::try_from(i).unwrap_or(u32::MAX).to_le_bytes())
        .collect();
    let encode_rgb = |c: &[[f64; 3]]| -> String {
        let bytes: Vec<u8> = c
            .iter()
            .flatten()
            .map(|v| {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let b = (v.clamp(0.0, 1.0) * 255.0).round_ties_even() as u8;
                b
            })
            .collect();
        b64(&bytes)
    };
    let default_seam = vec![[0.55, 0.06, 0.06]; cache.points()];
    let default_strain = vec![[1.0, 1.0, 1.0]; cache.points()];
    let seam_rgb = encode_rgb(if garment.colours.len() == cache.points() {
        &garment.colours
    } else {
        &default_seam
    });
    let strain_rgb = encode_rgb(
        strain
            .as_ref()
            .map(|m| m.colours.as_slice())
            .filter(|c| c.len() == cache.points())
            .unwrap_or(&default_strain),
    );
    let names = [
        "Flat pattern: the pieces leave the board and roll up",
        "Seams pulled together",
        "Sewn (no gravity yet)",
        "Draped under gravity",
    ];
    let phase: Vec<&str> = kept
        .iter()
        .map(|&k| names[starts.iter().rposition(|&s| s <= k).unwrap_or(0)])
        .collect();
    let body_v: Vec<u8> = body
        .vertices
        .iter()
        .flat_map(|p| [p.x, p.y, p.z])
        .flat_map(|c| {
            #[allow(clippy::cast_possible_truncation)]
            let f = c as f32;
            f.to_le_bytes()
        })
        .collect();
    let body_t: Vec<u8> = body
        .triangles
        .iter()
        .flatten()
        .flat_map(|&i| u32::try_from(i).unwrap_or(u32::MAX).to_le_bytes())
        .collect();
    let gating: Vec<String> = report["gating"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    let advisory: Vec<String> = if let Some(arr) = report["advisory"].as_array() {
        arr.iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect()
    } else {
        report["flags"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(String::from))
            .filter(|f| !gating.contains(f))
            .collect()
    };
    let r6 = |v: f64| (v * 1e6).round() / 1e6;
    let payload = json!({
        "schema": "fabric-fold-viewer/1",
        "title": title,
        "vertices": cache.points(),
        "frames": kept.len(),
        "source_frames": n,
        "lo": lo3.map(r6),
        "hi": hi3.map(r6),
        "positions": b64(&q),
        "triangles": b64(&tris),
        "colours": {
            "seam_rgb": seam_rgb,
            "strain_rgb": strain_rgb,
        },
        "phase": phase,
        "body": {"vertices": b64(&body_v), "triangles": b64(&body_t)},
        "report": {
            "gating": gating,
            "advisory": advisory,
            "settled": report["settled"].as_bool().unwrap_or(false),
            "arm_deg": report["arm_deg"].as_f64().unwrap_or(35.0),
        },
    });
    let html = viewer_html(crate::VIEWER_TEMPLATE, &payload).map_err(bad_input)?;
    let out = args
        .options
        .get("out")
        .map_or_else(|| dir.join("fold_viewer.html"), PathBuf::from);
    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        crate::make_dir(parent)?;
    }
    workspace::replace(&out, html.as_bytes())?;
    let text = format!("wrote {}", out.display());
    Ok((
        json!({
            "viewer": out,
            "vertices": cache.points(),
            "frames": kept.len(),
            "source_frames": n,
        }),
        0,
        text,
    ))
}

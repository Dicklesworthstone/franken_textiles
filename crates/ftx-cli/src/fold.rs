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

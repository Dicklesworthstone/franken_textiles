//! Trace a native still pixel through the saved endpoint to an editable source edge.
use std::{io::Read, path::Path};

use ftx_core::{Refusal, Vec3};
use ftx_drape::render::{ColouredMesh, StillHit, pick_still};
use ftx_pattern::Pattern;
use serde_json::{Value, json};

use crate::{Args, bad_input, fold, known_options, required};

fn garment_hit(
    dir: &Path,
    cloth: &ColouredMesh,
    indices: [usize; 3],
    point: Vec3,
) -> Result<Value, Refusal> {
    let (map, _) = fold::document(&dir.join("sim/mesh_map.json"))?;
    if map["vertices"].as_u64() != Some(cloth.vertices.len() as u64) {
        return Err(bad_input("mesh_map vertex count differs from garment.obj"));
    }
    let ranges = map["inst_range"]
        .as_object()
        .ok_or_else(|| bad_input("missing panel ranges"))?;
    let mut owner = None;
    for (name, value) in ranges {
        let [lo, hi]: [usize; 2] =
            serde_json::from_value(value.clone()).map_err(|_| bad_input("invalid panel range"))?;
        if lo >= hi || hi > cloth.vertices.len() {
            return Err(bad_input("invalid panel range"));
        }
        if indices.iter().any(|i| (lo..hi).contains(i)) {
            if owner.is_some() || !indices.iter().all(|i| (lo..hi).contains(i)) {
                return Err(bad_input("triangle has inconsistent panel ownership"));
            }
            owner = Some((name, lo..hi));
        }
    }
    let (id, range) = owner.ok_or_else(|| bad_input("triangle has no panel in mesh_map"))?;
    let (raw, _) = fold::document(&dir.join("pattern.json"))?;
    let pattern = Pattern::new(raw);
    let inst = pattern
        .instance(id)
        .ok_or_else(|| bad_input(format!("pattern has no instance {id}")))?;
    let mut nearest = None;
    for edge in &inst.edges {
        let key = format!("{id}/{}", edge.id);
        let samples: Vec<usize> = serde_json::from_value(map["edges"][&key].clone())
            .map_err(|_| bad_input(format!("missing or invalid sampled edge {key}")))?;
        if samples.len() < 2 || samples.iter().any(|i| !range.contains(i)) {
            return Err(bad_input(format!(
                "sampled edge {key} is outside its panel"
            )));
        }
        for pair in samples.windows(2) {
            let a = cloth.vertices[pair[0]];
            let segment = cloth.vertices[pair[1]] - a;
            let length2 = segment.norm2();
            let t = if length2 == 0.0 {
                0.0
            } else {
                ((point - a).dot(segment) / length2).clamp(0.0, 1.0)
            };
            let distance = (point - (a + segment * t)).norm();
            if nearest.is_none_or(|(best, _)| distance < best) {
                nearest = Some((distance, edge));
            }
        }
    }
    let (distance, edge) = nearest.ok_or_else(|| bad_input("panel has no sampled edges"))?;
    let source = &pattern.raw()["pieces"][inst.piece_index];
    let authored = &source["edges"][edge.source];
    let endpoint = |key: &str| -> Result<Value, Refusal> {
        let vertex = authored[key]
            .as_str()
            .ok_or_else(|| bad_input("missing source edge endpoint"))?;
        Ok(json!({"vertex":vertex, "xy_mm":source["vertices"][vertex]}))
    };
    Ok(
        json!({"surface":"garment","instance":id,"piece":inst.piece,"position_m":[point.x,point.y,point.z],
        "nearest_edge":{"instance_edge":edge.id,"source_edge":authored["id"],
            "posed_distance_mm":distance*1000.0,"from":endpoint("from")?,"to":endpoint("to")?}}),
    )
}

pub(super) fn run(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(args, "inspect-pixel", &["view", "pixel"])?;
    if args.positional.len() != 2 {
        return Err(bad_input("inspect-pixel needs one saved round directory"));
    }
    let dir = Path::new(&args.positional[1]);
    let view = required(args, "view")?;
    if ![
        "front",
        "side",
        "back",
        "three_quarter",
        "strain_front",
        "strain_back",
    ]
    .contains(&view)
    {
        return Err(bad_input("unknown native still view"));
    }
    let (x, y) = required(args, "pixel")?
        .split_once(',')
        .ok_or_else(|| bad_input("give --pixel X,Y"))?;
    let coordinate = |s: &str| {
        s.parse::<usize>()
            .map_err(|_| bad_input("pixel coordinates must be non-negative integers"))
    };
    let pixel = [coordinate(x)?, coordinate(y)?];
    let (round, _) = fold::document(&dir.join("round.json"))?;
    let n = round["round"]
        .as_u64()
        .ok_or_else(|| bad_input("missing round number"))?;
    let file = format!("sim/still_{view}.png");
    let image = format!("rounds/{n:03}/{file}");
    if round["rendering"]["status"] != "PASS"
        || round["sim"].is_null()
        || round["sim"].get("refusal").is_some()
        || !round["images"]
            .as_array()
            .is_some_and(|v| v.iter().any(|v| v == &image))
    {
        return Err(bad_input(
            "this round did not publish the requested still; run a fresh round with --stills",
        ));
    }
    let path = dir.join(&file);
    let mut header = [0; 24];
    std::fs::File::open(&path)
        .and_then(|mut f| f.read_exact(&mut header))
        .map_err(|e| crate::workspace::io_error(&path, e))?;
    if !header.starts_with(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR") {
        return Err(bad_input("invalid native PNG header"));
    }
    let size = [
        u32::from_be_bytes(header[16..20].try_into().unwrap()) as usize,
        u32::from_be_bytes(header[20..24].try_into().unwrap()) as usize,
    ];
    if round["rendering"]["width"] != size[0] || round["rendering"]["height"] != size[1] {
        return Err(bad_input("still dimensions differ from the saved round"));
    }
    if pixel[0] >= size[0] || pixel[1] >= size[1] {
        return Err(bad_input(format!(
            "pixel outside {}x{} still",
            size[0], size[1]
        )));
    }
    let body = fold::mesh(&dir.join("sim/body.obj"))?;
    let cloth = fold::mesh(&dir.join("sim/garment.obj"))?;
    let hit = match pick_still(&body, &cloth, view, size, pixel)
        .ok_or_else(|| bad_input("invalid still pixel"))?
    {
        StillHit::Body => json!({"surface":"body"}),
        StillHit::Background => json!({"surface":"background"}),
        StillHit::Garment {
            vertices,
            position_m,
        } => garment_hit(dir, &cloth, vertices, position_m)?,
    };
    let text = if let Some(edge) = hit.get("nearest_edge") {
        format!(
            "{} on {}, nearest edge {} ({:.1} mm on the posed mesh); source {} / {}",
            view,
            hit["instance"].as_str().unwrap_or(""),
            edge["instance_edge"].as_str().unwrap_or(""),
            edge["posed_distance_mm"].as_f64().unwrap_or_default(),
            hit["piece"].as_str().unwrap_or(""),
            edge["source_edge"].as_str().unwrap_or("")
        )
    } else {
        format!("{view}: {}", hit["surface"].as_str().unwrap_or(""))
    };
    Ok((
        json!({"round":n,"image":image,"size_px":size,"pixel":pixel,"hit":hit,
        "basis":round["basis"], "gating":round["sim"]["gating"],"advisory":round["sim"]["advisory"]}),
        0,
        text,
    ))
}

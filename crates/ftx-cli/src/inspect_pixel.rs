//! Trace a pixel in a native drape or refusal still back to an editable source edge.
use std::{io::Read, path::Path};

use ftx_core::{Refusal, Vec3};
use ftx_drape::render::{ColouredMesh, StillHit, pick_still};
use ftx_pattern::Pattern;
use serde_json::{Value, json};

use crate::{Args, bad_input, fold, known_options, required};

fn garment_hit(
    dir: &Path,
    map: &Value,
    cloth: &ColouredMesh,
    indices: [usize; 3],
    point: Vec3,
) -> Result<Value, Refusal> {
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
    let refused = round["sim"]["refusal"].is_object();
    let file = format!("still_{view}.png");
    crate::round::regular(&dir.join("sim"), true)?;
    let folder = if refused {
        let diagnosis = dir.join("sim/refused");
        crate::round::regular(&diagnosis, true)?;
        let placement = diagnosis.join("placement.json");
        if round["sim"]["refusal"]["code"] == "intersecting_start"
            && crate::round::regular(&placement, false)?
        {
            let (report, _) = fold::document(&placement)?;
            let expected = Path::new("start").join(&file);
            if !report["stills"]["start"].as_array().is_some_and(|names| {
                names
                    .iter()
                    .filter_map(Value::as_str)
                    .any(|name| Path::new(&name.replace('\\', "/")).ends_with(&expected))
            }) {
                return Err(bad_input(
                    "the placement report did not publish this start view",
                ));
            }
            "sim/refused/start"
        } else {
            "sim/refused"
        }
    } else {
        "sim"
    };
    let start = folder == "sim/refused/start";
    let scene = dir.join(folder);
    let image = format!("rounds/{n:03}/{folder}/{file}");
    if !refused
        && (round["rendering"]["status"] != "PASS"
            || !round["sim"].is_object()
            || round["sim"].get("refusal").is_some()
            || !round["images"]
                .as_array()
                .is_some_and(|v| v.iter().any(|v| v == &image)))
    {
        return Err(bad_input(
            "this round did not publish the requested still; run a fresh round with --stills",
        ));
    }
    crate::round::regular(&scene, true)?;
    let path = scene.join(&file);
    crate::round::regular(&path, false)?;
    let map_path = scene.join("mesh_map.json");
    let diagnostic_map = if refused && crate::round::regular(&map_path, false)? {
        let (map, _) = fold::document(&map_path)?;
        let pose = &map["refused_pose"];
        if pose["phase"].as_str().is_none()
            || pose["accepted_steps"].as_u64().is_none()
            || start != (pose["phase"] == "placement")
            || (start && pose["accepted_steps"] != 0)
        {
            return Err(bad_input(
                "the source-edge map does not identify a refused pose",
            ));
        }
        Some(map)
    } else {
        None
    };
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
    let dimensions = if refused {
        diagnostic_map.as_ref().map(|map| &map["refused_pose"])
    } else {
        Some(&round["rendering"])
    };
    if dimensions.is_some_and(|d| d["width"] != size[0] || d["height"] != size[1]) {
        return Err(bad_input("still dimensions differ from the saved round"));
    }
    if pixel[0] >= size[0] || pixel[1] >= size[1] {
        return Err(bad_input(format!(
            "pixel outside {}x{} still",
            size[0], size[1]
        )));
    }
    for name in ["body.obj", "garment.obj"] {
        crate::round::regular(&scene.join(name), false)?;
    }
    let body = fold::mesh(&scene.join("body.obj"))?;
    let cloth = fold::mesh(&scene.join("garment.obj"))?;
    let hit = match pick_still(&body, &cloth, view, size, pixel)
        .ok_or_else(|| bad_input("invalid still pixel"))?
    {
        StillHit::Body => json!({"surface":"body"}),
        StillHit::Background => json!({"surface":"background"}),
        StillHit::Garment {
            vertices,
            position_m,
        } => {
            let normal_map;
            let map = if let Some(map) = &diagnostic_map {
                map
            } else if refused {
                return Err(bad_input(
                    "this refused round has no diagnostic source-edge map; compare its saved views or run a fresh round to trace the garment edge",
                ));
            } else {
                crate::round::regular(&map_path, false)?;
                normal_map = fold::document(&map_path)?.0;
                &normal_map
            };
            garment_hit(dir, map, &cloth, vertices, position_m)?
        }
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
    let mut result = json!({"round":n,"image":image,"size_px":size,"pixel":pixel,"hit":hit,
        "basis":round["basis"], "gating":round["sim"]["gating"],"advisory":round["sim"]["advisory"]});
    if refused {
        if let Some(map) = diagnostic_map {
            result["refused_pose"] = map["refused_pose"].clone();
        } else if start {
            result["refused_pose"] = json!({"phase":"placement","accepted_steps":0});
        }
        result["refusal"] = round["sim"]["refusal"].clone();
        let label = if start {
            "Refused start"
        } else {
            "Refused pose"
        };
        return Ok((
            result,
            0,
            format!(
                "{label} · {text}\n{}",
                round["sim"]["refusal"]["message"].as_str().unwrap_or("")
            ),
        ));
    }
    Ok((result, 0, text))
}

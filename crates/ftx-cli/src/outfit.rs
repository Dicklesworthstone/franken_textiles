//! Whole-outfit proportions from completed native drapes (spec 04 §9.9).
//! Garments are frozen, not re-simulated together. No intersection or fit approval is implied.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use ftx_core::{Refusal, RefusalCode, Vec3, det, receipt::sha256_hex};
use ftx_drape::point_cache::PointCache;
use ftx_drape::render::{Camera, ColouredMesh, png, read_obj, render};
use ftx_export::text::xml_escape;
use serde_json::{Value, json};

use crate::{Args, bad_input, known_options, required, workspace};

const MAX_FILE: u64 = 64 * 1024 * 1024;
const MAX_VERTICES: usize = 250_000;
const MAX_FACES: usize = 500_000;
const NOTE: &str = "Garments were simulated separately: overlaps between them are not resolved. Read proportions only, not combined contact, construction approval or proof of fit.";
const VIEWS: [(&str, f64); 4] = [
    ("front", 0.0),
    ("three_quarter", 35.0),
    ("side", 90.0),
    ("back", 180.0),
];

fn budget(message: &str) -> Refusal {
    Refusal::new(RefusalCode::BudgetExceeded, message)
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, Refusal> {
    let file = File::open(path).map_err(|e| workspace::io_error(path, e))?;
    if !file
        .metadata()
        .map_err(|e| workspace::io_error(path, e))?
        .is_file()
    {
        return Err(bad_input(format!(
            "{} must be a regular input file",
            path.display()
        )));
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| workspace::io_error(path, e))?;
    if bytes.len() as u64 > MAX_FILE {
        return Err(budget("outfit input exceeds the 64 MiB per-file budget"));
    }
    Ok(bytes)
}

fn read_mesh(path: &Path) -> Result<(ColouredMesh, String), Refusal> {
    let bytes = read_bytes(path)?;
    let text = std::str::from_utf8(&bytes).map_err(|e| workspace::io_error(path, e))?;
    let mesh = read_obj(text).map_err(|e| bad_input(format!("{}: {e}", path.display())))?;
    if mesh.vertices.len() > MAX_VERTICES || mesh.triangles.len() > MAX_FACES {
        return Err(budget(
            "outfit mesh exceeds 250000 vertices or 500000 triangles",
        ));
    }
    if mesh.vertices.is_empty()
        || mesh.triangles.is_empty()
        || mesh
            .vertices
            .iter()
            .any(|v| !v.is_finite() || v.x.abs().max(v.y.abs()).max(v.z.abs()) > 1000.0)
        || mesh
            .colours
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        || mesh.triangles.iter().any(|t| {
            (mesh.vertices[t[1]] - mesh.vertices[t[0]])
                .cross(mesh.vertices[t[2]] - mesh.vertices[t[0]])
                .norm2()
                == 0.0
        })
    {
        return Err(bad_input(format!(
            "{}: need nonempty nondegenerate finite geometry within 1000 m and RGB in [0,1]",
            path.display()
        )));
    }
    Ok((mesh, sha256_hex(&bytes)))
}

/// Materialize only the final frame, after the shared reader validates the complete cache.
/// A completed cache plus a stale mesh must not silently render a different simulation state.
fn final_frame(path: &Path, mesh: &mut ColouredMesh, report: &Value) -> Result<Value, Refusal> {
    let bytes = read_bytes(path)?;
    let cache = PointCache::read(&bytes)?;
    let (vertices, frames) = (cache.points() as u64, cache.frames() as u64);
    if vertices != mesh.vertices.len() as u64
        || report["vertices"].as_u64() != Some(vertices)
        || report["faces"].as_u64() != Some(mesh.triangles.len() as u64)
        || report["animation"]["frames"].as_u64() != Some(frames)
    {
        return Err(bad_input(format!(
            "{} is not a complete matching native point cache",
            path.display()
        ))
        .repair("use an intact completed ftx drape output directory"));
    }
    let last = cache.frame(cache.frames() - 1)?;
    for (v, cached) in mesh.vertices.iter_mut().zip(last) {
        if cached
            != Vec3::new(
                f64::from(v.x as f32),
                f64::from(v.y as f32),
                f64::from(v.z as f32),
            )
        {
            return Err(bad_input(
                "garment.obj differs from the final PC2 frame; refuse a stale or mixed run",
            ));
        }
        *v = cached;
    }
    if mesh.triangles.iter().any(|t| {
        (mesh.vertices[t[1]] - mesh.vertices[t[0]])
            .cross(mesh.vertices[t[2]] - mesh.vertices[t[0]])
            .norm2()
            == 0.0
    }) {
        return Err(bad_input("a garment triangle collapses at PC2 precision"));
    }
    Ok(json!({"frames":frames,"final_frame":frames-1,"pc2_sha256":sha256_hex(&bytes)}))
}

struct Garment {
    mesh: ColouredMesh,
    record: Value,
}

struct Outfit {
    body: ColouredMesh,
    garments: Vec<Garment>,
    measurements_sha256: String,
    arm_deg: f64,
    flagged: bool,
}

fn text_list(report: &Value, key: &str) -> bool {
    report[key]
        .as_array()
        .is_some_and(|a| a.iter().all(Value::is_string))
}

fn assemble(paths: &[String], measurements_sha256: String, seams: bool) -> Result<Outfit, Refusal> {
    if paths.is_empty() || paths.len() > 16 {
        return Err(bad_input(
            "outfit needs 1..16 completed native drapes, each supplied with --sim DIR",
        ));
    }
    let mut seen = BTreeSet::new();
    let mut garments = Vec::new();
    let mut body: Option<ColouredMesh> = None;
    let mut arm_deg = 0.0;
    let mut flagged = false;
    let (mut vertices, mut faces) = (0, 0);
    for name in paths {
        let dir = Path::new(name);
        let canonical = fs::canonicalize(dir).map_err(|e| workspace::io_error(dir, e))?;
        if !seen.insert(canonical) {
            return Err(bad_input(
                "the same drape directory was supplied more than once",
            ));
        }
        let bytes = read_bytes(&dir.join("sim_report.json"))?;
        let report: Value =
            serde_json::from_slice(&bytes).map_err(|e| workspace::io_error(dir, e))?;
        let angle = report["arm_deg"]
            .as_f64()
            .filter(|v| v.is_finite() && (0.0..=180.0).contains(v));
        if report["measurements_sha256"].as_str() != Some(&measurements_sha256) {
            return Err(bad_input(format!("{name}: missing or different measurement fingerprint; cannot place this garment on the requested body"))
                .repair("pass the exact measurements file used by every drape, or drape again for this wearer"));
        }
        if angle.is_none()
            || !report["settled"].is_boolean()
            || !text_list(&report, "gating")
            || !text_list(&report, "flags")
            || !report["style"].is_string()
        {
            return Err(bad_input(format!(
                "{name}: incomplete native simulation report"
            )));
        }
        let (next_body, body_hash) = read_mesh(&dir.join("body.obj"))?;
        if let Some(first) = &body {
            if angle != Some(arm_deg)
                || first.triangles != next_body.triangles
                || first.vertices.len() != next_body.vertices.len()
                || first
                    .vertices
                    .iter()
                    .zip(&next_body.vertices)
                    .any(|(a, b)| (*a - *b).norm() > 1e-9)
            {
                return Err(bad_input("outfit drapes use different body geometry or poses; no garment was moved or refitted")
                    .repair("drape all garments with the same measurements, model and --arm-deg"));
            }
        } else {
            arm_deg = angle.unwrap();
            vertices += next_body.vertices.len();
            faces += next_body.triangles.len();
            body = Some(next_body);
        }
        let (mut mesh, mesh_hash) = read_mesh(&dir.join("garment.obj"))?;
        let cache = final_frame(&dir.join("fold.pc2"), &mut mesh, &report)?;
        vertices += mesh.vertices.len();
        faces += mesh.triangles.len();
        if vertices > MAX_VERTICES || faces > MAX_FACES {
            return Err(budget("combined outfit exceeds the mesh budget"));
        }
        let style = report["style"].as_str().unwrap();
        if seams && mesh.colours.is_empty() {
            return Err(bad_input(
                "--seams requires per-vertex colours in every garment.obj",
            ));
        }
        if !seams {
            let colour = if style.to_ascii_uppercase().contains("BOLERO") {
                [0.05, 0.07, 0.22]
            } else {
                [0.55, 0.05, 0.05]
            };
            mesh.colours = vec![colour; mesh.vertices.len()];
        }
        flagged |= report["settled"] != true || !report["gating"].as_array().unwrap().is_empty();
        let record = json!({"sim":name,"style":style,"same_body":true,"gating":report["gating"],
            "flags":report["flags"],"settled":report["settled"],"bands":report["bands"],
            "hem_rise_mm":report["hem_rise_mm"],"self_contact":report["self_contact"],
            "source":{"report_sha256":sha256_hex(&bytes),"body_obj_sha256":body_hash,"garment_obj_sha256":mesh_hash,"cache":cache}});
        garments.push(Garment { mesh, record });
    }
    Ok(Outfit {
        body: body.unwrap(),
        garments,
        measurements_sha256,
        arm_deg,
        flagged,
    })
}

fn shots(outfit: &Outfit, width: usize, height: usize) -> Vec<(String, Vec<u8>)> {
    let meshes: Vec<_> = std::iter::once(&outfit.body)
        .chain(outfit.garments.iter().map(|g| &g.mesh))
        .collect();
    let mut lo = Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let mut hi = -lo;
    for v in meshes.iter().flat_map(|m| &m.vertices) {
        lo = Vec3::new(lo.x.min(v.x), lo.y.min(v.y), lo.z.min(v.z));
        hi = Vec3::new(hi.x.max(v.x), hi.y.max(v.y), hi.z.max(v.z));
    }
    let center = (lo + hi) * 0.5;
    let radius = (hi - lo).norm() * 0.5;
    // Fit a bounding sphere in BOTH image axes, including a portrait or very wide --res.
    let focal = 50.0 / 36.0;
    let tan_half = (0.5 / focal) * (height as f64 / width as f64).min(1.0);
    let distance = 1.08 * radius * (1.0 + tan_half * tan_half).sqrt() / tan_half;
    VIEWS
        .into_iter()
        .map(|(name, angle)| {
            let (s, c) = det::sin_cos(angle.to_radians());
            let dir = Vec3::new(s, -c, 0.1).normalized();
            let cam = Camera {
                eye: center + dir * distance,
                target: center,
                focal_over_sensor: focal,
            };
            let rgb = render(&meshes, cam, width, height);
            (format!("outfit_{name}.png"), png(width, height, &rgb))
        })
        .collect()
}

fn review(report: &Value) -> String {
    use std::fmt::Write;
    let mut html = String::from(
        r#"<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src 'self'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'"><title>Outfit proportions</title><style>body{font:16px/1.6 system-ui,sans-serif;max-width:1100px;margin:2em auto;padding:0 1em;background:#f6f4ef;color:#25231e}aside{background:#fff0ce;padding:1em;border-left:4px solid #956921}section{display:grid;grid-template-columns:repeat(auto-fit,minmax(260px,1fr));gap:1em}figure{margin:0}img{width:100%;border:1px solid #cec8bc}pre{white-space:pre-wrap;overflow-wrap:anywhere}article{border-top:1px solid #cec8bc;margin-top:2em}</style><h1>Whole-outfit proportions</h1>"#,
    );
    let _ = write!(
        html,
        "<aside>{}</aside><p>Status: <strong>{}</strong>. Arms: {} degrees. No garment positions were changed.</p><section>",
        xml_escape(NOTE),
        xml_escape(report["status"].as_str().unwrap()),
        report["pose"]["arm_deg"]
    );
    for (name, _) in VIEWS {
        let _ = write!(
            html,
            "<figure><img src=\"outfit_{name}.png\" alt=\"Outfit {name}\"><figcaption>{name}</figcaption></figure>"
        );
    }
    html.push_str("</section><p><a href=\"outfit.json\">Source checks and diagnostics</a></p>");
    for garment in report["garments"].as_array().unwrap() {
        let _ = write!(
            html,
            "<article><h2>{}</h2><p>Source: {}<br>Settled: {}. Gating flags: {}.</p><pre>{}</pre></article>",
            xml_escape(garment["style"].as_str().unwrap()),
            xml_escape(garment["sim"].as_str().unwrap()),
            garment["settled"],
            garment["gating"].as_array().unwrap().len(),
            xml_escape(&serde_json::to_string_pretty(garment).unwrap())
        );
    }
    html.push_str("</html>\n");
    html
}

pub(super) fn cmd_outfit(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(
        args,
        "outfit",
        &["measurements", "sim", "out", "seams", "res"],
    )?;
    if args.positional.len() != 1 {
        return Err(bad_input(
            "outfit takes --sim DIR for each garment, not positional paths",
        ));
    }
    let (width, height) = crate::render::resolution(Some(
        args.options.get("res").map_or("960x960", String::as_str),
    ))?;
    let output = PathBuf::from(required(args, "out")?);
    match fs::symlink_metadata(&output) {
        Ok(_) => {
            return Err(bad_input(
                "outfit output must be a new directory; previous runs are never overwritten",
            ));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(workspace::io_error(&output, e)),
    }
    let evidence = read_bytes(Path::new(required(args, "measurements")?))?;
    let raw: Value =
        serde_json::from_slice(&evidence).map_err(|e| bad_input(format!("measurements: {e}")))?;
    crate::round::measurements(&raw)?;
    let outfit = assemble(
        &args.values,
        sha256_hex(&evidence),
        args.options.contains_key("seams"),
    )?;
    let images = shots(&outfit, width, height);
    let mut report = json!({"schema":"ftx-outfit/1","status":if outfit.flagged {"VISUALIZATION_WITH_FLAGS"}else{"VISUALIZATION_ONLY"},
        "note":NOTE,"measurements_sha256":outfit.measurements_sha256,"pose":{"arm_deg":outfit.arm_deg,"body_source":"first native drape; all body meshes checked"},
        "colour_mode":if args.options.contains_key("seams"){"seams"}else{"plain"},"garments":outfit.garments.iter().map(|g| &g.record).collect::<Vec<_>>(),
        "width":width,"height":height,"stills":{},"report":"report.html","hat":null,"turntable":null,"inter_garment_contact":"not_simulated"});
    for (name, _) in VIEWS {
        report["stills"][name] = json!(format!("outfit_{name}.png"));
    }
    let html = review(&report);
    let bytes = workspace::json_bytes(&report)?;
    if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(|e| workspace::io_error(parent, e))?;
    }
    fs::create_dir(&output).map_err(|e| workspace::io_error(&output, e))?;
    for (name, bytes) in images {
        workspace::write_new(&output.join(name), &bytes)?;
    }
    workspace::write_new(&output.join("report.html"), html.as_bytes())?;
    // Publish the completion record last; a failed write never advertises a complete outfit.
    workspace::write_new(&output.join("outfit.json"), &bytes)?;
    let code = if outfit.flagged { 2 } else { 0 };
    let text = format!(
        "OUTFIT: {} ({} garments)\n{NOTE}\n{}",
        report["status"].as_str().unwrap(),
        outfit.garments.len(),
        output.join("report.html").display()
    );
    Ok((report, code, text))
}

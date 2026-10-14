//! Saved sewn-drape -> geometry-derived coverage; physical layer names and interfaces stay explicit.
use std::{collections::BTreeMap, fs::File, io::Read};

use ftx_core::{Refusal, Vec3, receipt::sha256_hex};
use ftx_drape::{
    render::{ColouredMesh, read_obj},
    thermal::surface::{
        Surface,
        coverage::Layout,
        coverage::motion::Frame,
        projection::{self, Projection, Request, Settings},
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{invalid, io, read};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Input {
    schema: String,
    /// Every retained drape instance, exactly once. Several sewn pieces may form one physical layer.
    instance_layers: BTreeMap<String, String>,
    settings: Settings,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Receipt {
    request: Input,
    /// Identity of the imported OBJ bytes, not a global body-validity certificate.
    body_obj_sha256: String,
    /// Includes polygons, allocations, geometry/material identity and numerical-resolution reports.
    result: Value,
}

fn project(
    source: &Surface,
    cloth: &[Vec3],
    report: &Value,
    body: &[Vec3],
    faces: &[[usize; 3]],
    input: &Input,
) -> Result<Projection, Refusal> {
    projection::project(source, cloth, body, faces, &request(report, input)?)
}

fn request(report: &Value, input: &Input) -> Result<Request, Refusal> {
    if input.schema != "ftx-drape-thermal-projection/1" {
        return Err(invalid("unknown drape thermal projection schema"));
    }
    let instances: Vec<String> =
        serde_json::from_value(report["thermal_surface"]["instances"].clone())
            .map_err(|e| invalid(format!("thermal instance identities: {e}")))?;
    let mut names = std::collections::BTreeSet::new();
    for name in &instances {
        if name.trim().is_empty()
            || !names.insert(name)
            || input
                .instance_layers
                .get(name)
                .is_none_or(|s| s.trim().is_empty())
        {
            return Err(invalid(
                "projection needs one physical-layer identity per unique drape instance",
            ));
        }
    }
    if input.instance_layers.len() != names.len() {
        return Err(invalid(
            "projection names an instance absent from the saved drape",
        ));
    }
    let owners: Vec<usize> =
        serde_json::from_value(report["thermal_surface"]["triangle_instance"].clone())
            .map_err(|e| invalid(e.to_string()))?;
    let mut layers = Vec::with_capacity(owners.len());
    for owner in owners {
        let name = instances
            .get(owner)
            .ok_or_else(|| invalid("invalid drape instance owner"))?;
        layers.push(input.instance_layers[name].clone());
    }
    Ok(Request {
        schema: "ftx-thermal-projection/1".into(),
        cloth_layers: layers,
        settings: input.settings.clone(),
    })
}

// The renderer also accepts polygon fans and homogeneous vertices. Do not silently reinterpret
// those for a physical area calculation: this adapter accepts the triangular OBJ that ftx exports.
fn body_mesh(text: &str) -> Result<ColouredMesh, Refusal> {
    let mut vertices = 0;
    let mut faces = 0;
    for (i, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("");
        let mut words = line.split_whitespace();
        match words.next() {
            None => (),
            Some("v") => {
                if !matches!(words.count(), 3 | 6) {
                    return Err(invalid(format!(
                        "body OBJ line {} requires xyz, optionally RGB",
                        i + 1
                    )));
                }
                vertices += 1;
            }
            Some("f") => {
                if words.count() != 3 {
                    return Err(invalid(format!(
                        "body OBJ line {} must be a triangle; triangulate explicitly",
                        i + 1
                    )));
                }
                faces += 1;
            }
            Some("o" | "g" | "s" | "vn" | "vt" | "usemtl" | "mtllib") => (),
            Some(_) => {
                return Err(invalid(format!(
                    "unsupported body OBJ geometry on line {}",
                    i + 1
                )));
            }
        }
        if vertices > 196_608 || faces > 65_536 {
            return Err(invalid("body OBJ exceeds thermal geometry budget"));
        }
    }
    // Remove inline comments for the shared parser; no material files or external URLs are read.
    let geometry = text
        .lines()
        .map(|line| line.split('#').next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");
    read_obj(&geometry).map_err(|e| invalid(format!("body OBJ: {e}")))
}

pub(super) fn run(report: &str, body: &str, settings: &str) -> Result<Value, Refusal> {
    let report: Value = read(report)?;
    let input: Input = read(settings)?;
    let (source, cloth) = Surface::from_drape_report(&report)?;
    let mut bytes = Vec::new();
    File::open(body)
        .map_err(|e| io(body, e))?
        .take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| io(body, e))?;
    if bytes.len() > 64 * 1024 * 1024 {
        return Err(invalid("body OBJ exceeds 64 MiB"));
    }
    let text = std::str::from_utf8(&bytes).map_err(|e| invalid(format!("body OBJ UTF-8: {e}")))?;
    let mesh = body_mesh(text)?;
    let result = project(
        &source,
        &cloth,
        &report,
        &mesh.vertices,
        &mesh.triangles,
        &input,
    )?;
    let out = json!({"schema":"ftx-drape-thermal-coverage/1",
        "drape_thermal_frame_sha256":report["thermal_surface"]["sha256"],
        "body_positions_m":mesh.vertices,"layout":result.layout,
        "projection":Receipt { request: input, body_obj_sha256: sha256_hex(&bytes),
            result: serde_json::to_value(&result).map_err(|e| invalid(e.to_string()))? }});
    // Match the pretty-printed command output, including its final newline, not a smaller compact
    // encoding that could produce a successful file which the downstream 64 MiB reader refuses.
    if serde_json::to_vec_pretty(&out)
        .map_err(|e| invalid(e.to_string()))?
        .len()
        >= 64 * 1024 * 1024
    {
        return Err(invalid(
            "projected coverage exceeds 64 MiB replay limit; reduce the supplied mesh",
        ));
    }
    Ok(out)
}

/// Recompute correspondence, not merely its digest or an authored 'verified' flag. Changes to
/// the saved layout, body positions, layer semantics or retained polygons must agree with the law.
pub(super) fn verify(
    source: &Surface,
    cloth: &[Vec3],
    report: &Value,
    body: &[Vec3],
    layout: &Layout,
    receipt: &Receipt,
) -> Result<Value, Refusal> {
    let result = project(
        source,
        cloth,
        report,
        body,
        &layout.body_triangles,
        &receipt.request,
    )?;
    verify_result(&result, layout, receipt)
}

/// A moving state requires the actual polygons and material orientation, not a manual area budget.
pub(super) fn motion_frame(
    source: Surface,
    cloth: Vec<Vec3>,
    report: &Value,
    body: Vec<Vec3>,
    layout: &Layout,
    receipt: &Receipt,
) -> Result<Frame, Refusal> {
    let frame = Frame::new(
        source,
        cloth,
        body,
        &layout.body_triangles,
        request(report, &receipt.request)?,
    )?;
    verify_result(frame.projection(), layout, receipt)?;
    Ok(frame)
}

fn verify_result(
    result: &Projection,
    layout: &Layout,
    receipt: &Receipt,
) -> Result<Value, Refusal> {
    if receipt.body_obj_sha256.len() != 64
        || !receipt
            .body_obj_sha256
            .bytes()
            .all(|b| b.is_ascii_hexdigit())
    {
        return Err(invalid("invalid imported body OBJ identity"));
    }
    if serde_json::to_value(result).map_err(|e| invalid(e.to_string()))? != receipt.result
        || serde_json::to_value(&result.layout).map_err(|e| invalid(e.to_string()))?
            != serde_json::to_value(layout).map_err(|e| invalid(e.to_string()))?
    {
        return Err(invalid(
            "geometric coverage receipt differs from recomputed projection; regenerate thermal-map",
        ));
    }
    serde_json::to_value(receipt).map_err(|e| invalid(e.to_string()))
}

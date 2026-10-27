//! Source-qualified dimensional treatment in the ordinary pattern representation.
//!
//! The affine map acts on seam lines and material marks in the declared grain frame. The resulting
//! pattern is the sole rest-geometry authority for drape, fit, cutting and sewing consumers.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode, Vec2, curve, receipt::sha256_hex};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{Pattern, doc::Piece, material_binding::Changes, textile_import, validate};

/// Version of a source-piece treatment or compensation request.
pub const SCHEMA: &str = "ftx-pattern-treatment/1";

/// Which physical state the returned seam-line geometry describes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Apply measured dimensional change and bind each selected piece to the AFTER material.
    Treat,
    /// Invert the measured dimensional change for cutting in the BEFORE material.
    Compensate,
}

/// Atomic operation on named source pieces, never on an implicitly guessed material family.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    /// Must be [`SCHEMA`].
    pub schema: String,
    /// Treat the authored cut geometry, or compensate the desired finished seam-line geometry.
    pub mode: Mode,
    /// Exact, fully frozen material transitions indexed by caller-chosen local names.
    pub transitions: BTreeMap<String, super::Input>,
    /// Source-piece ID to transition name. Omitted pieces retain geometry and material.
    pub pieces: BTreeMap<String, String>,
    /// Explicit replacement for source-locked material-pair friction observations. Omission retains
    /// the source table; downstream contact must refuse any stale pair rather than invent one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interfaces: Option<Vec<Value>>,
    /// Default seam-walk tolerance in millimetres; an authored per-seam tolerance still takes precedence.
    #[serde(default = "default_tolerance")]
    pub seam_tolerance_mm: f64,
}

fn default_tolerance() -> f64 {
    3.0
}

fn bad(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

fn finite_point(p: Vec2) -> Result<Vec2, Refusal> {
    if p.x.is_finite() && p.y.is_finite() {
        Ok(p)
    } else {
        Err(bad("treatment geometry needs finite coordinates"))
    }
}

fn point(value: &Value) -> Result<Vec2, Refusal> {
    let p: Vec2 = serde_json::from_value(value.clone()).map_err(|e| bad(e.to_string()))?;
    finite_point(p)
}

struct Map {
    anchor: Vec2,
    warp: Vec2,
    scale: [f64; 2],
}

impl Map {
    fn point(&self, p: Vec2) -> Result<Vec2, Refusal> {
        finite_point(p)?;
        let relative = p - self.anchor;
        let weft = self.warp.perp();
        finite_point(
            self.anchor
                + self.warp * (relative.dot(self.warp) * self.scale[0])
                + weft * (relative.dot(weft) * self.scale[1]),
        )
    }

    fn value(&self, v: &mut Value) -> Result<(), Refusal> {
        *v = serde_json::to_value(self.point(point(v)?)?).map_err(|e| bad(e.to_string()))?;
        Ok(())
    }
}

fn material_map(piece: &Piece, raw: &Value, scale: [f64; 2]) -> Result<Map, Refusal> {
    let [a, b] = piece
        .grain
        .as_ref()
        .and_then(|g| g.points())
        .ok_or_else(|| {
            bad(format!(
                "{}: treatment requires an explicit grainline",
                piece.id
            ))
        })?;
    let delta = finite_point(b)? - finite_point(a)?;
    let length = delta.dot(delta).sqrt();
    if !length.is_finite() || length <= 0.0 {
        return Err(bad(format!(
            "{}: treatment grainline is degenerate",
            piece.id
        )));
    }
    let warp = delta / length;
    let coordinate = |key: &str| match raw["placement"].get(key) {
        None => Ok(0.0),
        Some(value) => value
            .as_f64()
            .filter(|v| v.is_finite())
            .ok_or_else(|| bad(format!("{}: invalid placement {key}", piece.id))),
    };
    let map = Map {
        anchor: Vec2::new(coordinate("x0")?, coordinate("y0")?),
        warp,
        scale,
    };
    // The schema unfolds a half pattern by Euclidean reflection. Reflection and anisotropic
    // deformation commute only when the fold is a principal material direction.
    if scale[0] != scale[1]
        && let Some(fold) = piece.on_fold()
    {
        let edge = piece.edges.iter().find(|e| e.id == fold).unwrap();
        let d = piece.vertices.get(&edge.to).unwrap() - piece.vertices.get(&edge.from).unwrap();
        let length = d.dot(d).sqrt();
        let parallel = d.dot(warp).abs();
        let perpendicular = d.dot(warp.perp()).abs();
        if !length.is_finite()
            || length <= 0.0
            || (parallel > 1e-10 * length && perpendicular > 1e-10 * length)
        {
            return Err(bad(format!(
                "{}: anisotropic treatment of an on-fold piece requires a warp- or weft-aligned fold; supply a full explicit piece otherwise",
                piece.id
            )));
        }
    }
    if let Some([sx, sy]) = piece.sim_scale {
        let cross = warp.x * warp.y * (scale[0] - scale[1]);
        if !sx.is_finite()
            || !sy.is_finite()
            || sx <= 0.0
            || sy <= 0.0
            || (cross * (sx - sy)).abs() > 1e-12
        {
            return Err(bad(format!(
                "{}: treatment cannot commute with the authored sim_scale; resolve the finished rest geometry explicitly",
                piece.id
            )));
        }
    }
    Ok(map)
}

fn cumulative(points: &[Vec2]) -> Result<Vec<f64>, Refusal> {
    let mut lengths = Vec::with_capacity(points.len());
    lengths.push(0.0);
    for pair in points.windows(2) {
        let d = pair[1] - pair[0];
        let length = lengths.last().unwrap() + d.dot(d).sqrt();
        if !length.is_finite() {
            return Err(bad("treatment edge arclength overflow"));
        }
        lengths.push(length);
    }
    if lengths.last().is_none_or(|v| *v <= 0.0) {
        return Err(bad("treatment requires nondegenerate edges"));
    }
    Ok(lengths)
}

fn notch_fraction(before: &[f64], after: &[f64], fraction: f64) -> Result<f64, Refusal> {
    if !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) {
        return Err(bad("treatment notch fraction must lie in [0,1]"));
    }
    let target = fraction * before.last().unwrap();
    for i in 1..before.len() {
        if before[i] >= target && before[i] > before[i - 1] {
            let t = (target - before[i - 1]) / (before[i] - before[i - 1]);
            return Ok((after[i - 1] + t * (after[i] - after[i - 1])) / after.last().unwrap());
        }
    }
    Ok(1.0)
}

fn transform_piece(raw: &mut Value, piece: &Piece, map: &Map) -> Result<(), Refusal> {
    let mut arclengths = BTreeMap::new();
    for (i, edge) in piece.edges.iter().enumerate() {
        let a = piece.vertices.get(&edge.from).unwrap();
        let b = piece.vertices.get(&edge.to).unwrap();
        let sampled = curve::curve_points(a, b, edge.curve.as_ref(), curve::EDGE_DENSE)
            .map_err(|e| bad(e.to_string()))?;
        let transformed: Vec<_> = sampled
            .iter()
            .map(|&p| map.point(p))
            .collect::<Result<_, _>>()?;
        arclengths.insert(
            edge.id.as_str(),
            (cumulative(&sampled)?, cumulative(&transformed)?),
        );
        if let Some(shape) = edge.curve.as_ref().filter(|c| !c.is_line()) {
            let controls: Vec<_> = shape
                .controls()
                .iter()
                .map(|&c| {
                    let q = map.point(curve::rel_to_abs(a, b, c))?;
                    curve::abs_to_rel(map.point(a)?, map.point(b)?, q)
                        .map_err(|e| bad(e.to_string()))
                })
                .collect::<Result<_, Refusal>>()?;
            if controls.iter().flatten().any(|x| !x.is_finite()) {
                return Err(bad("treatment curve controls overflow"));
            }
            raw["edges"][i]["curve"]["c"] = json!(controls);
        }
    }
    for value in raw["vertices"].as_object_mut().unwrap().values_mut() {
        map.value(value)?;
    }
    let grain = &mut raw["grain"];
    if grain.is_object() {
        map.value(&mut grain["p0"])?;
        map.value(&mut grain["p1"])?;
    } else {
        map.value(&mut grain[0])?;
        map.value(&mut grain[1])?;
    }
    for key in ["internal", "decor", "trace_marks"] {
        if let Some(marks) = raw.get_mut(key).and_then(Value::as_array_mut) {
            for mark in marks {
                if let Some(points) = mark.get_mut("pts").and_then(Value::as_array_mut) {
                    for value in points {
                        map.value(value)?;
                    }
                }
            }
        }
    }
    if let Some(drills) = raw.get_mut("drills").and_then(Value::as_array_mut) {
        for drill in drills {
            map.value(&mut drill["p"])?;
        }
    }
    if let Some(notches) = raw.get_mut("notches").and_then(Value::as_array_mut) {
        for notch in notches {
            let edge = notch["edge"]
                .as_str()
                .ok_or_else(|| bad("notch needs an edge"))?;
            let (before, after) = arclengths
                .get(edge)
                .ok_or_else(|| bad("unknown notch edge"))?;
            let fraction = notch["s"]
                .as_f64()
                .ok_or_else(|| bad("notch needs a fraction"))?;
            notch["s"] = json!(notch_fraction(before, after, fraction)?);
        }
    }
    Ok(())
}

impl Input {
    /// Derive one ordinary pattern and recompute its seam compatibility. Source data, material
    /// assignments and the original input stay unchanged on every failure. Compensation keeps the
    /// BEFORE material; applying the same transition in Treat mode then recovers the target geometry.
    ///
    /// # Errors
    /// Invalid source locks/qualifiers, missing or ambiguous pieces/grain, repeat application,
    /// unsupported fold/simulation transforms, malformed geometry, or exceeded work limits.
    pub fn apply(&self, pattern: &Pattern) -> Result<Value, Refusal> {
        if self.schema != SCHEMA
            || self.transitions.is_empty()
            || self.transitions.len() > 64
            || self.pieces.is_empty()
            || self.pieces.len() > 4096
            || !self.seam_tolerance_mm.is_finite()
            || self.seam_tolerance_mm < 0.0
        {
            return Err(bad(
                "invalid treatment schema, transition/piece budget or seam tolerance",
            ));
        }
        let errors = pattern.structure_errors();
        if !errors.is_empty() {
            return Err(bad(errors.join("; ")));
        }
        let doc = pattern.doc.as_ref().unwrap();
        let mut reports = BTreeMap::new();
        for (name, transition) in &self.transitions {
            if name.trim().is_empty() || name.len() > 256 {
                return Err(bad(
                    "treatment transition names must be bounded nonempty text",
                ));
            }
            reports.insert(name.clone(), super::evaluate(transition)?);
        }
        let used: BTreeSet<_> = self.pieces.values().collect();
        if used.len() != reports.len() || used.iter().any(|name| !reports.contains_key(*name)) {
            return Err(bad(
                "every treatment transition must be referenced by a source piece",
            ));
        }
        let mut changes = Changes {
            schema: "ftx-material-bindings/1".into(),
            interfaces: self.interfaces.clone(),
            ..Changes::default()
        };
        let mut geometry = pattern.raw().clone();
        let mode = serde_json::to_value(self.mode).map_err(|e| bad(e.to_string()))?;
        let history = geometry["meta"].get("material_treatment_history");
        if history.is_some_and(|h| !h.is_array()) {
            return Err(bad("meta.material_treatment_history must be a list"));
        }
        let mut assignments = Vec::new();
        for (id, transition_name) in &self.pieces {
            let index = doc
                .pieces
                .iter()
                .position(|p| &p.id == id)
                .ok_or_else(|| bad(format!("unknown treatment source piece {id}")))?;
            let piece = &doc.pieces[index];
            let report = &reports[transition_name];
            let selected =
                textile_import::resolve_fabric(textile_import::piece_fabric(pattern, index)?)?
                    .ok_or_else(|| {
                        bad(format!(
                            "{id}: treatment needs a source-bearing BEFORE selection"
                        ))
                    })?;
            if selected.sha256() != report.before_sha256 {
                return Err(bad(format!(
                    "{id}: assigned material does not match the transition BEFORE snapshot"
                )));
            }
            let transition_hash = sha256_hex(
                &serde_json::to_vec(&self.transitions[transition_name])
                    .map_err(|e| bad(e.to_string()))?,
            );
            if self.mode == Mode::Compensate && pattern.raw()["meta"]["material_treatment_history"]
                .as_array()
                .is_some_and(|history| {
                    history.iter().rev().find(|entry| entry["pieces"].as_array()
                        .is_some_and(|pieces| pieces.iter().any(|p| p["piece_id"] == *id)))
                        .is_some_and(|entry| entry["mode"] == "compensate")
                })
            {
                return Err(bad(format!(
                    "{id}: compensation was already applied; derive revised compensation from the original target pattern"
                )));
            }
            let scale = match self.mode {
                Mode::Treat => report.stretch,
                Mode::Compensate => report.compensation,
            };
            let map = material_map(piece, &geometry["pieces"][index], scale)?;
            transform_piece(&mut geometry["pieces"][index], piece, &map)?;
            changes.pieces.insert(
                id.clone(),
                match self.mode {
                    Mode::Treat => self.transitions[transition_name].after.clone(),
                    Mode::Compensate => self.transitions[transition_name].before.clone(),
                },
            );
            assignments.push(json!({"piece_id":id,"transition":transition_name,
                "transition_sha256":transition_hash,"scale_warp_weft":scale,
                "anchor_mm":map.anchor,"before_material_sha256":selected.sha256(),
                "after_material_sha256":report.after_sha256}));
        }
        let source_pattern_json =
            serde_json::to_string(pattern.raw()).map_err(|e| bad(e.to_string()))?;
        let source_pattern_sha256 = sha256_hex(source_pattern_json.as_bytes());
        let request_json = serde_json::to_string(self).map_err(|e| bad(e.to_string()))?;
        let request_sha256 = sha256_hex(request_json.as_bytes());
        let meta = geometry
            .as_object_mut()
            .unwrap()
            .entry("meta")
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .ok_or_else(|| bad("pattern meta must be an object"))?;
        meta.entry("material_treatment_history")
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .unwrap()
            .push(json!({"mode":mode,"request_sha256":request_sha256,
                "source_pattern_sha256":source_pattern_sha256,"pieces":assignments}));
        let derived = Pattern::new(geometry);
        let binding = changes.apply(&derived)?;
        let result = Pattern::new(binding["pattern"].clone());
        let errors = result.structure_errors();
        if !errors.is_empty() {
            return Err(bad(errors.join("; ")));
        }
        Ok(
            json!({"schema":"ftx-pattern-treatment-report/1","mode":mode,
            "source_pattern_sha256":source_pattern_sha256,"source_pattern_json":source_pattern_json,
            "request_sha256":request_sha256,"frozen_request":self,"frozen_request_json":request_json,
            "transitions":reports,"pieces":assignments,
            "pattern":binding["pattern"],"pattern_json":binding["pattern_json"],
            "pattern_sha256":binding["pattern_sha256"],
            "seams_before":validate::seam_walk(pattern,self.seam_tolerance_mm),
            "seams_after":validate::seam_walk(&result,self.seam_tolerance_mm),
            "scope":"Homogeneous, source-qualified directional dimensional change of seam lines and material marks. Seam allowance and tool widths remain authored manufacturing choices. Missing AFTER properties and stale pair qualifications remain consumer refusals. No treatment kinetics, constrained-seam shrinkage, empirical garment fit or hardware qualification is inferred."}),
        )
    }
}

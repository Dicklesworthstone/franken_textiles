//! Validation of a pattern against a fit spec and a body (`docs/spec/01` §9): the seam walk, truing,
//! points of measure, the report and its status, the text table, and the role-based default fit spec.
//!
//! The report's field names are an on-disk format (`validation.json`). Numbers are rounded as the
//! reference rounds them (Python `round`, half-even on the binary value); messages use its templates.

use std::collections::BTreeSet;

use ftx_core::polyline::horizontal_width;
use ftx_core::{det, py};
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::instance::{Instance, PyError};
use crate::pattern::{Pattern, py_repr_value};

/// What validation needs from a body (spec 01 §12): measurement values, their uncertainty, and
/// landmark heights in a part frame. Implemented for measurement files by [`Measurements`]; a fitted
/// body model can implement it too.
pub trait BodyMeasure {
    /// A measurement value (mm, or degrees for angles).
    ///
    /// # Errors
    /// `Key(name)` when the measurement is missing.
    fn value(&self, name: &str) -> Result<f64, PyError>;

    /// The fused σ of a measurement rounded to 0.1, or `None` when it is not known.
    fn sigma(&self, name: &str) -> Option<f64>;

    /// Height of landmark `name` in part frame `part_key` (`torso`: mm above the floor; `arm_R`: along
    /// the arm from its joint; `leg_R`: along the leg axis).
    ///
    /// # Errors
    /// `Key` for an unknown part, landmark or a missing measurement it needs.
    fn landmark_h(&self, part_key: &str, name: &str) -> Result<f64, PyError>;
}

/// A measurement file (`fabric-measurements/1` with `fused`) or a flat `{name: value}` object, with the
/// reference's analytic landmark heights (its dress-form formulas).
#[derive(Clone, Debug)]
pub struct Measurements(pub Value);

const TWO_PI: f64 = 2.0 * std::f64::consts::PI;
const DEG: f64 = std::f64::consts::PI / 180.0;
const LEG_SPREAD_DEG: f64 = 4.0;

impl Measurements {
    fn landmarks(&self) -> Result<Vec<(&'static str, f64)>, PyError> {
        let m = |n: &str| self.value(n);
        let z_waist = m("waist_height")?;
        let z_c7 = z_waist + m("back_neck_to_waist")?;
        let slope = m("shoulder_slope_deg")? * DEG;
        let neck_r = m("neck_base")? / TWO_PI;
        let shoulder_half = neck_r + m("shoulder_length")? * det::cos(slope);
        let z_snp = z_c7 - 5.0; // the stable default form; ANATOMICAL would put it 12 mm above C7
        let z_shoulder = z_snp - m("shoulder_length")? * det::sin(slope);
        let z_chest = z_c7 - m("armscye_depth")?;
        let z_crotch = m("crotch_height")?;
        let z_hip = z_crotch + 0.45 * (z_waist - z_crotch);
        let head_r = m("head")? / TWO_PI;
        let calf = 55.0 + 0.6 * (m("knee_height")? - 55.0);
        let knee = m("knee_height")?;
        let stature = m("stature")?;
        Ok(vec![
            ("floor", 0.0),
            ("ankle", 55.0),
            ("calf", calf),
            ("knee", knee),
            ("crotch", z_crotch),
            ("hip", z_hip),
            ("waist", z_waist),
            ("chest", z_chest),
            ("across_back", 0.5 * (z_chest + z_shoulder)),
            ("shoulder", z_shoulder),
            ("snp", z_snp),
            ("c7", z_c7),
            ("head_top", stature),
            ("head_centre", stature - head_r),
            ("neck_r", neck_r),
            ("head_r", head_r),
            ("shoulder_half_width", shoulder_half),
            ("hip_joint_x", m("hip_width")? / 4.0),
            ("arm_length", m("arm_length")?),
        ])
    }
}

fn lookup(l: &[(&str, f64)], name: &str) -> Result<f64, PyError> {
    l.iter()
        .find(|(k, _)| *k == name)
        .map(|(_, v)| *v)
        .ok_or_else(|| PyError::Key(name.to_string()))
}

impl BodyMeasure for Measurements {
    fn value(&self, name: &str) -> Result<f64, PyError> {
        let missing = || PyError::Key(name.to_string());
        let v = match self.0.get("fused") {
            Some(fused) => {
                let e = fused.get(name).ok_or_else(missing)?;
                e.get("value_mm")
                    .or_else(|| e.get("value_deg"))
                    .ok_or_else(missing)?
            }
            None => self.0.get(name).ok_or_else(missing)?,
        };
        v.as_f64().ok_or_else(missing)
    }

    fn sigma(&self, name: &str) -> Option<f64> {
        let e = self.0.get("fused")?.get(name)?;
        let s = match e.get("sigma_mm").or_else(|| e.get("sigma_deg")) {
            None => 0.0,
            Some(v) => v.as_f64()?,
        };
        Some(py::round(s, 1))
    }

    fn landmark_h(&self, part_key: &str, name: &str) -> Result<f64, PyError> {
        let l = self.landmarks()?;
        if part_key == "torso" {
            return lookup(&l, name);
        }
        if !matches!(part_key, "arm_R" | "arm_L" | "leg_R" | "leg_L") {
            return Err(PyError::Key(part_key.to_string()));
        }
        // The reference builds every frame first, which needs these two.
        let up_arm = self.value("upper_arm")? / TWO_PI;
        self.value("chest_width")?;
        if part_key.starts_with("arm") {
            let tip = 0.8 * up_arm;
            let arm = lookup(&l, "arm_length")?;
            return match name {
                "shoulder_tip" => Ok(tip),
                "elbow" => Ok(tip - 0.55 * arm),
                "wrist" => Ok(tip - arm),
                _ => Err(PyError::Key(name.to_string())),
            };
        }
        let z = lookup(&l, name)?;
        let origin_z = lookup(&l, "crotch")? + 25.0;
        Ok(-(origin_z - z) / det::cos(LEG_SPREAD_DEG * DEG))
    }
}

/// A seam-walk row (`ERROR` rows carry only `seam`, `status` and `message`).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SeamRow {
    /// Seam id (`null` when the seam has none).
    pub seam: Value,
    /// Side a ref.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub a: Option<Value>,
    /// Side b ref.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub b: Option<Value>,
    /// Side a length, 0.1 mm.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub len_a_mm: Option<f64>,
    /// Side b length, 0.1 mm.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub len_b_mm: Option<f64>,
    /// a/b, 4 decimals.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ratio: Option<f64>,
    /// `"1.000"` or `"1.000–1.080"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_ratio: Option<String>,
    /// Length error against the target, 0.1 mm.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub residual_mm: Option<f64>,
    /// PASS, FAIL, WARN or ERROR.
    pub status: String,
    /// How the notches paired.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notch_match: Option<String>,
    /// Ease error per notch interval, 0.1 mm.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notch_segments_mm: Option<Vec<f64>>,
    /// Why it failed or warned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// A truing row.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TruingRow {
    /// Seam id.
    pub seam: Value,
    /// `start` or `end` (in side a's direction).
    pub end: String,
    /// `"{inst_a}:{va} + {inst_b}:{vb}"`.
    pub at: String,
    /// `"{edge_a} / {edge_b}"`, the line crossing the seam.
    pub line: String,
    /// Sum of the two corner angles, 0.1°.
    pub angle_sum_deg: f64,
    /// Sum − 180°, 0.1°.
    pub deviation_deg: f64,
    /// PASS or FAIL.
    pub status: String,
    /// Empty, or how to true the corner.
    pub message: String,
}

/// A point-of-measure row.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PointRow {
    /// Point id.
    pub id: Value,
    /// Display label.
    pub label: Value,
    /// Always `ANALYTIC` (the flat pattern decides).
    pub authority: String,
    /// Finished value, 0.1 mm.
    pub finished_mm: Option<f64>,
    /// Body value (scaled and offset), 0.1 mm.
    pub body_mm: Option<f64>,
    /// σ of the body value; absent on `UNMEASURABLE` rows, `null` when unknown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_sigma_mm: Option<Option<f64>>,
    /// Finished − body, 0.1 mm.
    pub ease_mm: Option<f64>,
    /// `[min, nominal, max]` as given.
    pub target_mm: Value,
    /// Distance outside the band, 0.1 mm.
    pub residual_mm: Option<f64>,
    /// IN_BAND, LOW, HIGH or UNMEASURABLE.
    pub status: String,
    /// σ warning.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Why it could not be measured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Failure counts.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Summary {
    /// Seams FAIL or ERROR.
    pub seams_failing: usize,
    /// Truing FAIL.
    pub truing_failing: usize,
    /// Points not IN_BAND.
    pub points_out_of_band: usize,
}

/// The validation report (`validation.json`).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Report {
    /// The pattern's structure is broken; nothing else was checked.
    Blocked {
        /// `BLOCKED`.
        status: String,
        /// The structure errors.
        structure_errors: Vec<String>,
    },
    /// A full report.
    Full {
        /// CONVERGED, NOT_CONVERGED or BLOCKED (missing measurements).
        status: String,
        /// Body measurements the spec needs and the body lacks.
        missing_measurements: Vec<String>,
        /// Missing grainlines and cut instructions.
        completeness: Vec<String>,
        /// Seam walk.
        seams: Vec<SeamRow>,
        /// Truing.
        truing: Vec<TruingRow>,
        /// Points of measure.
        points: Vec<PointRow>,
        /// Counts.
        summary: Summary,
        /// Up to six worst failures, as text.
        worst: Vec<String>,
    },
}

impl Report {
    /// The status.
    #[must_use]
    pub fn status(&self) -> &str {
        match self {
            Self::Blocked { status, .. } | Self::Full { status, .. } => status,
        }
    }

    /// CLI exit code: 0 CONVERGED, 2 NOT_CONVERGED, 3 BLOCKED.
    #[must_use]
    pub fn exit_code(&self) -> i32 {
        match self.status() {
            "CONVERGED" => 0,
            "NOT_CONVERGED" => 2,
            _ => 3,
        }
    }
}

fn get_f64(o: &Value, k: &str) -> Option<f64> {
    o.get(k).and_then(Value::as_f64)
}

impl SeamRow {
    fn error(seam: Value, message: String) -> Self {
        Self {
            seam,
            a: None,
            b: None,
            len_a_mm: None,
            len_b_mm: None,
            ratio: None,
            target_ratio: None,
            residual_mm: None,
            status: "ERROR".into(),
            notch_match: None,
            notch_segments_mm: None,
            message: Some(message),
        }
    }
}

/// The seam walk (spec 01 §9.1).
#[must_use]
pub fn seam_walk(pat: &Pattern, tol_mm: f64) -> Vec<SeamRow> {
    let mut rows = Vec::new();
    for s in pat.seams() {
        let o = Value::Object(s.0.clone());
        let id = o.get("id").cloned().unwrap_or(Value::Null);
        let pr = match pat.seam_pairs(s, 5.0) {
            Ok(pr) => pr,
            Err(e) => {
                rows.push(SeamRow::error(id, e.to_string()));
                continue;
            }
        };
        let (la, lb) = (pr.len_a, pr.len_b);
        let ratio = if lb != 0.0 { la / lb } else { f64::INFINITY };
        // `ratio_band` when the key exists (even null), else the legacy `band`; only a truthy band counts.
        let band_v = if s.0.contains_key("ratio_band") {
            s.0.get("ratio_band")
        } else {
            s.0.get("band")
        };
        let band = match band_v.filter(|b| crate::doc::truthy(b)) {
            None => None,
            Some(b) => match b
                .as_array()
                .map(|a| a.iter().map(Value::as_f64).collect::<Vec<_>>())
                .as_deref()
            {
                Some([Some(lo), Some(hi)]) => Some((*lo, *hi)),
                _ => {
                    rows.push(SeamRow::error(
                        id,
                        format!("seam {}: ratio_band must be [lo, hi]", s.id_text()),
                    ));
                    continue;
                }
            },
        };
        // `ratio` if present (a null ratio with no band reads as 1.0), else 1.0 without a band.
        let nominal = match s.0.get("ratio") {
            Some(v) => v.as_f64().or(if band.is_none() { Some(1.0) } else { None }),
            None => band.is_none().then_some(1.0),
        };
        let (resid, target) = match band {
            Some((lo, hi)) => {
                let resid = if lo <= ratio && ratio <= hi {
                    0.0
                } else {
                    la - (if ratio < lo { lo } else { hi }) * lb
                };
                (resid, format!("{}–{}", py::fixed(lo, 3), py::fixed(hi, 3)))
            }
            None => {
                let n = nominal.unwrap_or(1.0);
                (la - n * lb, py::fixed(n, 3))
            }
        };
        let tol = get_f64(&o, "tol_mm").unwrap_or(tol_mm);
        let mut row = SeamRow {
            seam: id,
            a: o.get("a").cloned(),
            b: o.get("b").cloned(),
            len_a_mm: Some(py::round(la, 1)),
            len_b_mm: Some(py::round(lb, 1)),
            ratio: Some(py::round(ratio, 4)),
            target_ratio: Some(target.clone()),
            residual_mm: Some(py::round(resid, 1)),
            status: if resid.abs() <= tol { "PASS" } else { "FAIL" }.into(),
            notch_match: Some(pr.notch_match.clone()),
            notch_segments_mm: None,
            message: None,
        };
        if pr.notch_match == "piecewise" {
            let ka = [&[0.0][..], &pr.notches_a, &[1.0][..]].concat();
            let kb = [&[0.0][..], &pr.notches_b, &[1.0][..]].concat();
            let r = nominal.filter(|&n| n != 0.0).unwrap_or(ratio);
            let segs: Vec<f64> = (0..ka.len() - 1)
                .map(|i| py::round((ka[i + 1] - ka[i]) * la - r * (kb[i + 1] - kb[i]) * lb, 1))
                .collect();
            let limit = tol.max(0.08 * la / segs.len() as f64);
            if segs.iter().any(|x| x.abs() > limit) {
                row.status = "FAIL".into();
                row.message = Some("ease is not where the notches say it is".into());
            }
            row.notch_segments_mm = Some(segs);
        } else if pr.notch_match.starts_with("count-mismatch") {
            if row.status == "PASS" {
                row.status = "WARN".into();
            }
            row.message = Some(format!(
                "notches don't pair ({}); ease distribution is unchecked",
                pr.notch_match
            ));
        }
        if row.status == "FAIL" && row.message.is_none() {
            row.message = Some(if band.is_none() {
                let longer = if resid > 0.0 { "a" } else { "b" };
                let side = o.get(longer).map_or_else(String::new, |v| {
                    v.as_str().map_or_else(|| v.to_string(), str::to_string)
                });
                format!(
                    "{side} is {} mm too long for the declared ratio {target}",
                    py::fixed(resid.abs(), 1)
                )
            } else {
                format!(
                    "ratio {} is outside {target} by {} mm",
                    py::fixed(ratio, 3),
                    py::fixed(resid.abs(), 1)
                )
            });
        }
        rows.push(row);
    }
    rows
}

/// The first loop edge not in `path` that touches `vid`.
fn continuation<'a>(inst: &'a Instance, vid: &str, path: &[String]) -> Option<&'a str> {
    inst.edges
        .iter()
        .find(|e| !path.contains(&e.id) && (e.to == vid || e.from == vid))
        .map(|e| e.id.as_str())
}

/// Truing: corner angles that must sum to ~180° where a garment line crosses a seam (spec 01 §9.2).
/// Ends whose continuation edges are both free are checked automatically; sewn ones only when the
/// spec's `truing` list names them.
#[must_use]
pub fn truing(pat: &Pattern, spec: &Value) -> Vec<TruingRow> {
    let tol = get_f64(spec, "truing_tol_deg").unwrap_or(8.0);
    let mut sewn: BTreeSet<(String, String)> = BTreeSet::new();
    for s in pat.seams() {
        for k in ["a", "b"] {
            if let Some(Ok((inst, eids))) = s.side_ref(k).map(|r| pat.side(r)) {
                sewn.extend(eids.into_iter().map(|e| (inst.id.clone(), e)));
            }
        }
    }
    let explicit: BTreeSet<(String, String)> = spec
        .get("truing")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|t| {
                    Some((
                        t.get("seam")?.as_str()?.to_string(),
                        t.get("end")?.as_str()?.to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    let mut rows = Vec::new();
    for s in pat.seams() {
        let (Some(ra), Some(rb)) = (s.side_ref("a"), s.side_ref("b")) else {
            continue;
        };
        let (Ok((ia, ea)), Ok((ib, eb))) = (pat.side(ra), pat.side(rb)) else {
            continue;
        };
        let rev = s.reverse();
        let id = s.0.get("id").cloned().unwrap_or(Value::Null);
        for end in ["start", "end"] {
            let vertex = |inst: &Instance, path: &[String], at: &str| -> Option<String> {
                let e = if at == "start" {
                    inst.edge(&path[0]).ok()?.from.clone()
                } else {
                    inst.edge(path.last()?).ok()?.to.clone()
                };
                Some(e)
            };
            let b_end = if rev {
                if end == "start" { "end" } else { "start" }
            } else {
                end
            };
            let (Some(va), Some(vb)) = (vertex(ia, &ea, end), vertex(ib, &eb, b_end)) else {
                continue;
            };
            let (Some(ca), Some(cb)) = (continuation(ia, &va, &ea), continuation(ib, &vb, &eb))
            else {
                continue;
            };
            let free = !sewn.contains(&(ia.id.clone(), ca.to_string()))
                && !sewn.contains(&(ib.id.clone(), cb.to_string()));
            if !free && !explicit.contains(&(s.id_text(), end.to_string())) {
                continue;
            }
            let (Ok(a1), Ok(a2)) = (ia.corner_angle(&va), ib.corner_angle(&vb)) else {
                continue;
            };
            let ang = a1 + a2;
            let dev = ang - 180.0;
            let ok = dev.abs() <= tol;
            rows.push(TruingRow {
                seam: id.clone(),
                end: end.into(),
                at: format!("{}:{va} + {}:{vb}", ia.id, ib.id),
                line: format!("{ca} / {cb}"),
                angle_sum_deg: py::round(ang, 1),
                deviation_deg: py::round(dev, 1),
                status: if ok { "PASS" } else { "FAIL" }.into(),
                message: if ok {
                    String::new()
                } else {
                    format!(
                        "the {ca}/{cb} line {} by {}° at this seam; true the corner so the joined line runs straight through",
                        if dev > 0.0 { "dips" } else { "peaks" },
                        py::fixed(dev.abs(), 0)
                    )
                },
            });
        }
    }
    rows
}

fn level_y(
    pat: &Pattern,
    inst_id: &str,
    body: &dyn BodyMeasure,
    landmark: &str,
) -> Result<f64, PyError> {
    let inst = pat
        .instance(inst_id)
        .ok_or_else(|| PyError::Key(inst_id.to_string()))?;
    let pl = pat
        .doc
        .as_ref()
        .and_then(|d| d.pieces.get(inst.piece_index))
        .and_then(|p| p.placement.clone())
        .unwrap_or_default();
    let part = pl.part.as_deref().unwrap_or("torso");
    let part_key = if part == "torso" {
        "torso".to_string()
    } else {
        format!("{part}_R")
    };
    let h = body.landmark_h(&part_key, landmark)?;
    let h0 = body.landmark_h(&part_key, pl.y0_landmark.as_deref().unwrap_or("waist"))?;
    Ok(h - h0 + pl.y0.unwrap_or(0.0))
}

fn str_of<'a>(p: &'a Value, k: &str) -> Result<&'a str, PyError> {
    p.get(k)
        .and_then(Value::as_str)
        .ok_or_else(|| PyError::Key(k.to_string()))
}

fn instance_of<'a>(pat: &'a Pattern, id: &str) -> Result<&'a Instance, PyError> {
    pat.instance(id).ok_or_else(|| PyError::Key(id.to_string()))
}

fn finished_value(pat: &Pattern, p: &Value, body: &dyn BodyMeasure) -> Result<f64, PyError> {
    let kind = p.get("kind").ok_or_else(|| PyError::Key("kind".into()))?;
    let dy = get_f64(p, "dy").unwrap_or(0.0);
    match kind.as_str() {
        Some("girth") => {
            let mut val = 0.0;
            for iid in p
                .get("instances")
                .and_then(Value::as_array)
                .ok_or_else(|| PyError::Key("instances".into()))?
            {
                let iid = iid.as_str().ok_or_else(|| PyError::Key(iid.to_string()))?;
                let y = level_y(pat, iid, body, str_of(p, "landmark")?)? + dy;
                val += horizontal_width(
                    &instance_of(pat, iid)?.outline(ftx_core::curve::MEASURE_DENSE)?,
                    y,
                );
            }
            Ok(val)
        }
        Some("width") => {
            let iid = str_of(p, "instance")?;
            let inst = instance_of(pat, iid)?;
            let y = match p.get("at_vertex") {
                Some(v) => {
                    let v = v.as_str().ok_or_else(|| PyError::Key(v.to_string()))?;
                    inst.verts
                        .get(v)
                        .ok_or_else(|| PyError::Key(v.to_string()))?
                        .y
                }
                None => level_y(pat, iid, body, str_of(p, "landmark")?)?,
            };
            Ok(horizontal_width(
                &inst.outline(ftx_core::curve::MEASURE_DENSE)?,
                y + dy,
            ))
        }
        Some("vertical") => {
            let inst = instance_of(pat, str_of(p, "instance")?)?;
            let (f, t) = (str_of(p, "from")?, str_of(p, "to")?);
            let yf = inst
                .verts
                .get(f)
                .ok_or_else(|| PyError::Key(f.to_string()))?
                .y;
            let yt = inst
                .verts
                .get(t)
                .ok_or_else(|| PyError::Key(t.to_string()))?
                .y;
            Ok((yf - yt).abs())
        }
        Some("path") => {
            let mut total = 0.0;
            for r in p
                .get("refs")
                .and_then(Value::as_array)
                .ok_or_else(|| PyError::Key("refs".into()))?
            {
                let r = r.as_str().ok_or_else(|| PyError::Key(r.to_string()))?;
                total += pat.side_length(r)?;
            }
            Ok(total * get_f64(p, "scale").unwrap_or(1.0))
        }
        _ => Err(PyError::Value(format!(
            "unknown point kind {}",
            py_repr_value(kind)
        ))),
    }
}

/// One point of measure (spec 01 §9.3). Never fails: anything that cannot be measured is an
/// `UNMEASURABLE` row with the reason.
#[must_use]
pub fn measure_point(pat: &Pattern, p: &Value, body: &dyn BodyMeasure) -> PointRow {
    let id = p.get("id").cloned().unwrap_or(Value::Null);
    let label = p.get("label").cloned().unwrap_or_else(|| id.clone());
    let target = p.get("target_mm").cloned().unwrap_or(Value::Null);
    let measured = (|| -> Result<PointRow, PyError> {
        let val = finished_value(pat, p, body)?;
        let body_name = str_of(p, "body")?;
        let bval = body.value(body_name)? * get_f64(p, "body_scale").unwrap_or(1.0)
            + get_f64(p, "body_offset_mm").unwrap_or(0.0);
        let ease = val - bval;
        let t: Vec<f64> = target
            .as_array()
            .map(|a| a.iter().filter_map(Value::as_f64).collect())
            .unwrap_or_default();
        let arity = target.as_array().map_or(0, Vec::len);
        let (lo, hi) = match (arity, t.as_slice()) {
            (3, [lo, _, hi]) => (*lo, *hi),
            _ => {
                return Err(PyError::Value(format!(
                    "expected 3 target values, got {arity}"
                )));
            }
        };
        let status = if lo <= ease && ease <= hi {
            "IN_BAND"
        } else if ease < lo {
            "LOW"
        } else {
            "HIGH"
        };
        let resid = match status {
            "IN_BAND" => 0.0,
            "LOW" => ease - lo,
            _ => ease - hi,
        };
        let sigma = body.sigma(body_name);
        let note = sigma.filter(|&s| s != 0.0 && s > (hi - lo) / 2.0).map(|s| {
            format!(
                "body {body_name} is only known to ±{} mm, wider than half the target band; a tape measurement would decide this point",
                py::fixed(s, 0)
            )
        });
        Ok(PointRow {
            id: id.clone(),
            label: label.clone(),
            authority: "ANALYTIC".into(),
            finished_mm: Some(py::round(val, 1)),
            body_mm: Some(py::round(bval, 1)),
            body_sigma_mm: Some(sigma),
            ease_mm: Some(py::round(ease, 1)),
            target_mm: target.clone(),
            residual_mm: Some(py::round(resid, 1)),
            status: status.into(),
            note,
            message: None,
        })
    })();
    measured.unwrap_or_else(|e| {
        let name = match &label {
            Value::String(s) => s.clone(),
            other => py_repr_value(other),
        };
        PointRow {
            id,
            label,
            authority: "ANALYTIC".into(),
            finished_mm: None,
            body_mm: None,
            body_sigma_mm: None,
            ease_mm: None,
            target_mm: target,
            residual_mm: None,
            status: "UNMEASURABLE".into(),
            note: None,
            message: Some(format!(
                "{name}: cannot measure on this pattern ({e}); rename the vertex/edge or adapt the fit spec"
            )),
        }
    })
}

/// Validates a pattern (spec 01 §9.4).
#[must_use]
pub fn validate(pat: &Pattern, spec: &Value, body: &dyn BodyMeasure) -> Report {
    let structure = pat.structure_errors();
    if !structure.is_empty() {
        return Report::Blocked {
            status: "BLOCKED".into(),
            structure_errors: structure,
        };
    }
    let seams = seam_walk(pat, get_f64(spec, "seam_tol_mm").unwrap_or(3.0));
    let tru = truing(pat, spec);
    let point_specs = spec
        .get("points")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let points: Vec<PointRow> = point_specs
        .iter()
        .map(|p| measure_point(pat, p, body))
        .collect();
    let needed: BTreeSet<&str> = point_specs
        .iter()
        .filter_map(|p| p.get("body").and_then(Value::as_str))
        .collect();
    let missing: Vec<String> = needed
        .into_iter()
        .filter(|n| body.value(n).is_err())
        .map(str::to_string)
        .collect();
    let mut completeness = Vec::new();
    let mut seen = Vec::new();
    for p in pat
        .raw()
        .get("pieces")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice)
    {
        let Some(pid) = p.get("id").and_then(Value::as_str) else {
            continue;
        };
        if seen.contains(&pid) {
            continue;
        }
        seen.push(pid);
        if !p.get("grain").is_some_and(crate::doc::truthy) {
            completeness.push(format!("{pid}: no grainline"));
        }
        if !p.get("cut").is_some_and(crate::doc::truthy) {
            completeness.push(format!("{pid}: no cut instruction"));
        }
    }
    // Failures in report order, each with its sort key and text.
    let mut fails: Vec<(f64, String)> = Vec::new();
    let label_text = |v: &Value| v.as_str().map_or_else(|| py_repr_value(v), str::to_string);
    for r in seams
        .iter()
        .filter(|r| r.status == "FAIL" || r.status == "ERROR")
    {
        let key = r.residual_mm.unwrap_or(0.0);
        let text = r
            .message
            .clone()
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| {
                format!(
                    "{}: {} (residual {})",
                    label_text(&r.seam),
                    r.status,
                    r.residual_mm.map_or("None".into(), py::repr_f64)
                )
            });
        fails.push((key, text));
    }
    for r in tru.iter().filter(|r| r.status == "FAIL") {
        let text = if r.message.is_empty() {
            format!(
                "{}: {} (residual {})",
                label_text(&r.seam),
                r.status,
                py::repr_f64(r.deviation_deg)
            )
        } else {
            r.message.clone()
        };
        fails.push((r.deviation_deg, text));
    }
    for r in points.iter().filter(|r| r.status != "IN_BAND") {
        let text = r
            .message
            .clone()
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| {
                format!(
                    "{}: {} (residual {})",
                    label_text(&r.label),
                    r.status,
                    r.residual_mm.map_or("None".into(), py::repr_f64)
                )
            });
        fails.push((r.residual_mm.unwrap_or(0.0), text));
    }
    let status = if !missing.is_empty() {
        "BLOCKED"
    } else if fails.is_empty() && completeness.is_empty() {
        "CONVERGED"
    } else {
        "NOT_CONVERGED"
    };
    let summary = Summary {
        seams_failing: seams
            .iter()
            .filter(|r| r.status == "FAIL" || r.status == "ERROR")
            .count(),
        truing_failing: tru.iter().filter(|r| r.status == "FAIL").count(),
        points_out_of_band: points.iter().filter(|r| r.status != "IN_BAND").count(),
    };
    fails.sort_by(|a, b| b.0.abs().total_cmp(&a.0.abs())); // stable: ties keep report order
    let worst = fails.into_iter().take(6).map(|(_, t)| t).collect();
    Report::Full {
        status: status.into(),
        missing_measurements: missing,
        completeness,
        seams,
        truing: tru,
        points,
        summary,
        worst,
    }
}

fn pad(s: &str, w: usize) -> String {
    let n = s.chars().count();
    if n >= w {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(w - n))
    }
}

fn rpad(s: &str, w: usize) -> String {
    let n = s.chars().count();
    if n >= w {
        s.to_string()
    } else {
        format!("{}{s}", " ".repeat(w - n))
    }
}

fn head(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// The text table (spec 01 §9.6): lines joined by `\n`, no trailing newline.
#[must_use]
pub fn format_table(rep: &Report) -> String {
    let mut lines = vec![format!("STATUS: {}", rep.status())];
    let Report::Full {
        seams,
        truing,
        points,
        completeness,
        worst,
        ..
    } = rep
    else {
        if let Report::Blocked {
            structure_errors, ..
        } = rep
        {
            lines.extend(structure_errors.iter().map(|e| format!("  structure: {e}")));
        }
        return lines.join("\n");
    };
    lines.push("Finished measurements (authority: flat pattern)".into());
    lines.push(format!(
        "  {} {} {} {}  target(min/nom/max)  status",
        pad("point", 28),
        rpad("body", 8),
        rpad("finished", 9),
        rpad("ease", 7)
    ));
    let f =
        |v: Option<f64>, w: usize| rpad(&v.map_or_else(|| "—".to_string(), |x| py::fixed(x, 1)), w);
    for r in points {
        let label = r
            .label
            .as_str()
            .map_or_else(|| py_repr_value(&r.label), str::to_string);
        let targets: Vec<String> = r
            .target_mm
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|x| format!("{}", x.as_f64().unwrap_or(0.0).trunc() as i64))
                    .collect()
            })
            .unwrap_or_default();
        lines.push(format!(
            "  {} {} {} {}  {}  {}",
            pad(&head(&label, 28), 28),
            f(r.body_mm, 8),
            f(r.finished_mm, 9),
            f(r.ease_mm, 7),
            pad(&targets.join("/"), 19),
            r.status
        ));
    }
    lines.push("Seam walk".into());
    let seam_text = |v: &Value| v.as_str().map_or_else(|| py_repr_value(v), str::to_string);
    for r in seams {
        if r.status == "ERROR" {
            lines.push(format!(
                "  {} ERROR {}",
                pad(&seam_text(&r.seam), 22),
                r.message.as_deref().unwrap_or("")
            ));
            continue;
        }
        lines.push(format!(
            "  {} a={} b={} ratio={} target={} resid={}  {}",
            pad(&seam_text(&r.seam), 22),
            rpad(&py::fixed(r.len_a_mm.unwrap_or(0.0), 1), 7),
            rpad(&py::fixed(r.len_b_mm.unwrap_or(0.0), 1), 7),
            py::fixed(r.ratio.unwrap_or(0.0), 3),
            pad(r.target_ratio.as_deref().unwrap_or(""), 11),
            rpad(&py::fixed(r.residual_mm.unwrap_or(0.0), 1), 6),
            r.status
        ));
    }
    if !truing.is_empty() {
        lines.push("Truing (angle sums across seams)".into());
        for r in truing {
            lines.push(format!(
                "  {} {} {} {}°  {}",
                pad(&seam_text(&r.seam), 22),
                pad(&r.end, 5),
                pad(&head(&r.line, 30), 30),
                rpad(&py::fixed(r.angle_sum_deg, 1), 6),
                r.status
            ));
        }
    }
    lines.extend(completeness.iter().map(|c| format!("  completeness: {c}")));
    if !worst.is_empty() {
        lines.push("Worst:".into());
        lines.extend(worst.iter().map(|w| format!("  - {w}")));
    }
    lines.join("\n")
}

/// Role-based points of measure for patterns without a drafting program (spec 01 §9.5).
#[must_use]
pub fn default_fit_spec(pat: &Pattern, garment: &str) -> Value {
    let role_of = |inst: &Instance| -> String {
        pat.doc
            .as_ref()
            .and_then(|d| d.pieces.get(inst.piece_index))
            .map(|p| p.role().to_string())
            .unwrap_or_default()
    };
    let with_role =
        |r: &str| -> Vec<&Instance> { pat.instances.iter().filter(|i| role_of(i) == r).collect() };
    let ids =
        |v: &[&Instance]| -> Vec<Value> { v.iter().map(|i| Value::String(i.id.clone())).collect() };
    let mut pts: Vec<Value> = Vec::new();
    let backs = with_role("bodice_back");
    let bod: Vec<&Instance> = with_role("bodice_front")
        .into_iter()
        .chain(backs.iter().copied())
        .collect();
    if !bod.is_empty() {
        pts.push(json!({"id": "chest", "kind": "girth", "instances": ids(&bod), "landmark": "chest", "dy": -24.0,
                        "body": "chest", "target_mm": [70, 100, 130], "label": "Chest ease (pieces edge to edge)"}));
        if !backs.is_empty() {
            pts.push(json!({"id": "across_back", "kind": "girth", "instances": ids(&backs), "landmark": "across_back",
                            "body": "across_back", "target_mm": [10, 25, 45], "label": "Across-back ease"}));
        }
        let necks: Vec<String> = bod
            .iter()
            .flat_map(|i| {
                i.edges
                    .iter()
                    .filter(|e| e.id.split('.').next() == Some("neck"))
                    .map(move |e| format!("{}/{}", i.id, e.id))
            })
            .collect();
        if !necks.is_empty() {
            pts.push(json!({"id": "neckline", "kind": "path", "refs": necks, "body": "neck_base", "target_mm": [20, 50, 90],
                            "label": "Neckline ease"}));
        }
    }
    if let Some(sl) = with_role("sleeve").first() {
        if sl.edges.iter().any(|e| e.id == "hem") {
            pts.push(json!({"id": "cuff_pass", "kind": "path", "refs": [format!("{}/hem", sl.id)], "body": "hand_pass",
                            "target_mm": [10, 25, 60], "label": "Sleeve hem passes the hand"}));
        }
        if let Some(cap) = sl.edges.iter().rev().find(|e| e.id == "cap_f") {
            pts.push(json!({"id": "bicep", "kind": "width", "instance": sl.id, "at_vertex": cap.from, "body": "upper_arm",
                            "target_mm": [35, 50, 70], "label": "Bicep ease"}));
        }
    }
    let legs: Vec<&Instance> = with_role("pant_front")
        .into_iter()
        .chain(with_role("pant_back"))
        .collect();
    if !legs.is_empty() {
        pts.push(json!({"id": "seat", "kind": "girth", "instances": ids(&legs), "landmark": "hip", "body": "hip",
                        "target_mm": [60, 160, 480], "label": "Seat ease"}));
    }
    let mut out = Map::new();
    out.insert("schema".into(), "fabric-fit/1".into());
    out.insert("garment".into(), garment.into());
    out.insert(
        "generated".into(),
        "validate.default_fit_spec (role-based)".into(),
    );
    out.insert("seam_tol_mm".into(), json!(3.0));
    out.insert("truing_tol_deg".into(), json!(8.0));
    out.insert("points".into(), Value::Array(pts));
    Value::Object(out)
}

//! The round-0 audit (spec 04 §10.12–10.13): scale-free checks of a traced (or drafted) pattern, every
//! finding with its class (T topology, S style, M metric), severity, refs, one-line message and route, and
//! the consensus of the sheet's dimension labels.

use std::collections::BTreeSet;

use ftx_core::polyline::cumulative;
use ftx_core::{Vec2, det, np, py};
use ftx_pattern::doc::{Grain, PatternDoc};
use ftx_pattern::{Pattern, PyError};
use serde_json::{Map, Value, json};

use crate::geos::hausdorff;
use crate::length::parse_length;
use crate::refs::{measure_ref, outline, piece_instance};

/// The report's schema.
pub const AUDIT_SCHEMA: &str = "fabric-round0-audit/1";

fn route(class: &str) -> &'static str {
    match class {
        "T" => {
            "topology: regenerate the sheet (regeneration hygiene) or pick a program option; never invent topology silently"
        }
        "S" => {
            "style: transfer the curve character onto the measured block; regenerate only if the user wants another style"
        }
        _ => {
            "metric: fix with vector edits or the measured block; never send numbers back to the image model"
        }
    }
}

/// The audit's tolerances (the reference's keyword defaults).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AuditOptions {
    /// Millimetre-equivalent tolerance of every metric check.
    pub tol_mm: f64,
    /// The median seam side's nominal length when nothing else gives the scale.
    pub median_seam_mm: f64,
    /// Relative agreement of two labels' implied scales.
    pub label_tol: f64,
    /// Allowed departure from square where an edge meets the fold.
    pub fold_tol_deg: f64,
}

impl Default for AuditOptions {
    fn default() -> Self {
        Self {
            tol_mm: 3.0,
            median_seam_mm: 250.0,
            label_tol: 0.03,
            fold_tol_deg: 5.0,
        }
    }
}

fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|x| x != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(m)) => !m.is_empty(),
    }
}

/// Python `str(value)` of a JSON value in a message.
fn py_str(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => "None".into(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(b)) => if *b { "True" } else { "False" }.into(),
        Some(Value::Number(n)) => n.as_i64().map_or_else(
            || py::repr_f64(n.as_f64().unwrap_or(f64::NAN)),
            |i| i.to_string(),
        ),
        Some(v) => v.to_string(),
    }
}

/// Python `re.search(r'(\bin\b|inch|"|”|″)', text)`.
fn says_inches(text: &str) -> bool {
    if text.contains("inch") || text.contains(['"', '\u{201d}', '\u{2033}']) {
        return true;
    }
    let c: Vec<char> = text.chars().collect();
    let word = |i: Option<usize>| {
        i.and_then(|i| c.get(i))
            .is_some_and(|c| c.is_alphanumeric() || *c == '_')
    };
    (0..c.len().saturating_sub(1))
        .any(|i| c[i] == 'i' && c[i + 1] == 'n' && !word(i.checked_sub(1)) && !word(Some(i + 2)))
}

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

/// The consensus of the sheet's dimension labels (spec 04 §10.12): every label proposes a scale (exhaustive
/// RANSAC), the largest agreeing group wins (ties by the smallest log spread), a disjoint group as large
/// makes it weak, and the scale is the geometric median of the winners.
#[must_use]
pub fn label_consensus(pattern: &Pattern, labels: &[Value], tol: f64) -> Value {
    let mut rows: Vec<Map<String, Value>> = Vec::new();
    let mut unparseable: Vec<Value> = Vec::new();
    let mut implied: Vec<f64> = Vec::new();
    for lab in labels {
        let text = match lab.get("text") {
            None => String::new(),
            v => py_str(v),
        };
        let flagged = truthy(lab.get("unparseable"));
        let mut v = lab
            .get("value_mm")
            .filter(|x| !x.is_null())
            .and_then(Value::as_f64);
        if v.is_none() && !flagged {
            v = parse_length(&text, None);
        }
        let r = lab.get("ref").filter(|r| truthy(Some(r)));
        let (Some(v), false, Some(r)) = (v, flagged, r) else {
            let why = if v.is_none() {
                "no value"
            } else {
                "a value but no single-piece ref"
            };
            unparseable.push(json!({"text": text, "ref": lab.get("ref").cloned().unwrap_or(Value::Null), "why": why}));
            continue;
        };
        let measured = match measure_ref(pattern, r.as_str().unwrap_or_default()) {
            Ok(l) => l,
            Err(e) => {
                unparseable.push(json!({"text": text, "ref": r, "why": e.to_string()}));
                continue;
            }
        };
        if measured <= 0.0 {
            continue;
        }
        let unit = if says_inches(&text) { "in" } else { "mm" };
        implied.push(v / measured);
        let row = json!({"text": text, "ref": r, "value_mm": v, "measured_units": py::round(measured, 4),
                         "implied_mm_per_unit": v / measured, "unit": unit});
        rows.push(row.as_object().cloned().unwrap_or_default());
    }
    let mut out = Map::new();
    out.insert("scale_mm_per_unit".into(), Value::Null);
    out.insert("unparseable".into(), Value::Array(unparseable));
    out.insert("inliers".into(), json!([]));
    out.insert("outliers".into(), json!([]));
    let finish = |mut out: Map<String, Value>, rows: Vec<Map<String, Value>>| {
        out.insert(
            "per_label".into(),
            Value::Array(rows.into_iter().map(Value::Object).collect()),
        );
        Value::Object(out)
    };
    if rows.is_empty() {
        return finish(out, rows);
    }
    let s = &implied;
    let n = s.len();
    let mut cands: Vec<((usize, f64), Vec<bool>)> = s
        .iter()
        .map(|&si| {
            let inl: Vec<bool> = s.iter().map(|&sj| (sj / si - 1.0).abs() <= tol).collect();
            let logs: Vec<f64> = s
                .iter()
                .zip(&inl)
                .filter(|(_, ok)| **ok)
                .map(|(&sj, _)| det::ln(sj / si).abs())
                .collect();
            (
                (inl.iter().filter(|x| **x).count(), -np::pairwise_sum(&logs)),
                inl,
            )
        })
        .collect();
    cands.sort_by(|a, b| {
        b.0.0.cmp(&a.0.0).then(
            b.0.1
                .partial_cmp(&a.0.1)
                .unwrap_or(std::cmp::Ordering::Equal),
        )
    });
    let (best, inl) = (cands[0].0, cands[0].1.clone());
    let rival = cands[1..]
        .iter()
        .find(|c| c.0.0 == best.0 && !c.1.iter().zip(&inl).any(|(a, b)| *a && *b));
    let agree = best.0;
    out.insert("n_labels".into(), json!(n));
    out.insert("n_agree".into(), json!(agree));
    let need = 2usize.max(n.div_ceil(2));
    out.insert("weak".into(), json!(rival.is_some() || agree < need));
    if let Some(rv) = rival {
        let group: Vec<Value> = rows
            .iter()
            .zip(&rv.1)
            .filter(|(_, ok)| **ok)
            .map(|(r, _)| r["text"].clone())
            .collect();
        out.insert("rival_group".into(), Value::Array(group));
    }
    if agree < 2 && n >= 2 {
        out.insert("no_consensus".into(), json!(true));
        for r in &mut rows {
            r.insert("inlier".into(), json!(false));
        }
        out.insert(
            "outliers".into(),
            Value::Array(rows.iter().map(|r| r["text"].clone()).collect()),
        );
        return finish(out, rows);
    }
    let logs: Vec<f64> = s
        .iter()
        .zip(&inl)
        .filter(|(_, ok)| **ok)
        .map(|(&x, _)| det::ln(x))
        .collect();
    let sc = det::exp(median(&logs));
    out.insert("scale_mm_per_unit".into(), json!(sc));
    for (r, ok) in rows.iter_mut().zip(&inl) {
        let mu = r["measured_units"].as_f64().unwrap_or(0.0);
        let value = r["value_mm"].as_f64().unwrap_or(0.0);
        r.insert("inlier".into(), json!(*ok));
        r.insert("fitted_mm".into(), json!(py::round(mu * sc, 2)));
        r.insert(
            "dev_pct".into(),
            json!(py::round((mu * sc / value - 1.0) * 100.0, 2)),
        );
    }
    let texts = |want: bool| -> Value {
        Value::Array(
            rows.iter()
                .filter(|r| r["inlier"].as_bool() == Some(want))
                .map(|r| r["text"].clone())
                .collect(),
        )
    };
    out.insert("inliers".into(), texts(true));
    out.insert("outliers".into(), texts(false));
    finish(out, rows)
}

/// Unit tangent (direction of travel) at the start or end of a polyline: the chord angles over `dist`
/// and `dist / 2` Richardson-extrapolated (`2·a(s/2) − a(s)`).
fn end_tangent(pts: &[Vec2], at_start: bool, dist: f64) -> Vec2 {
    let s = cumulative(pts);
    let total = s[s.len() - 1];
    if total <= 0.0 {
        return Vec2::new(1.0, 0.0);
    }
    let dist = dist.min(0.5 * total);
    let (xs, ys): (Vec<f64>, Vec<f64>) = pts.iter().map(|p| (p.x, p.y)).unzip();
    let chord = |d: f64| {
        if at_start {
            Vec2::new(np::interp(d, &s, &xs), np::interp(d, &s, &ys)) - pts[0]
        } else {
            pts[pts.len() - 1]
                - Vec2::new(
                    np::interp(total - d, &s, &xs),
                    np::interp(total - d, &s, &ys),
                )
        }
    };
    let (full, half) = (chord(dist), chord(dist / 2.0));
    let a_full = det::atan2(full.y, full.x);
    let a_half = a_full
        + det::atan2(
            full.x * half.y - full.y * half.x,
            full.x * half.x + full.y * half.y,
        );
    let a = 2.0 * a_half - a_full;
    Vec2::new(det::cos(a), det::sin(a))
}

/// Python `math.degrees`.
fn degrees(x: f64) -> f64 {
    x * (180.0 / std::f64::consts::PI)
}

struct Findings(Vec<Value>);

impl Findings {
    fn add(
        &mut self,
        class: &str,
        kind: &str,
        msg: String,
        refs: Vec<Value>,
        severity: &str,
        extra: Vec<(&str, Value)>,
    ) {
        let mut f = Map::new();
        f.insert("id".into(), json!(format!("F{}", self.0.len() + 1)));
        f.insert("class".into(), json!(class));
        f.insert("kind".into(), json!(kind));
        f.insert("severity".into(), json!(severity));
        f.insert("refs".into(), Value::Array(refs));
        f.insert("message".into(), json!(msg));
        f.insert("route".into(), json!(route(class)));
        for (k, v) in extra {
            f.insert(k.into(), v);
        }
        self.0.push(Value::Object(f));
    }
}

fn fmt_len(mm: f64, unit: &str) -> String {
    if unit == "in" {
        format!("{} in", py::fixed(mm / 25.4, 1))
    } else {
        format!("{} mm", py::fixed(mm, 0))
    }
}

/// The scale-free round-0 audit (spec 04 §10.13). Tolerances are mm-equivalent: millimetres per unit come
/// from a FITTED pattern (1), else a non-weak label consensus, else a nominal median seam side.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn audit(doc: &PatternDoc, labels: Option<&[Value]>, o: &AuditOptions) -> Value {
    let pattern = Pattern::from_doc(doc.clone());
    let meta = doc.meta.clone().unwrap_or_default();
    let mut f = Findings(Vec::new());
    let errs = pattern.structure_errors();
    for e in &errs {
        f.add(
            "T",
            "structure",
            format!("Pattern structure problem: {e}"),
            vec![],
            "error",
            vec![],
        );
    }
    let lab = labels
        .filter(|l| !l.is_empty())
        .map(|l| label_consensus(&pattern, l, o.label_tol));
    let seams: Vec<&Map<String, Value>> = pattern
        .seams()
        .iter()
        .map(|s| &s.0)
        .filter(|s| !truthy(s.get("twin_of")))
        .collect();
    let key = |s: &Map<String, Value>| s.get("id").map(Value::to_string);
    let mut lens: Vec<((String, &str), f64)> = Vec::new();
    for s in &seams {
        for k in ["a", "b"] {
            let Some(r) = s.get(k).and_then(Value::as_str) else {
                continue;
            };
            let Ok(l) = pattern.side_length(r) else {
                continue;
            };
            let Some(id) = key(s) else { continue };
            match lens.iter_mut().find(|(x, _)| x.0 == id && x.1 == k) {
                Some(slot) => slot.1 = l,
                None => lens.push(((id, k), l)),
            }
        }
    }
    let len_of = |s: &Map<String, Value>, k: &str| -> Option<f64> {
        let id = key(s)?;
        lens.iter()
            .find(|(x, _)| x.0 == id && x.1 == k)
            .map(|(_, l)| *l)
    };
    let unscaled = meta.get("scale_status").and_then(Value::as_str) == Some("UNSCALED");
    let lab_scale = lab
        .as_ref()
        .and_then(|l| l["scale_mm_per_unit"].as_f64())
        .filter(|x| *x != 0.0);
    let lab_weak = lab.as_ref().is_some_and(|l| truthy(l.get("weak")));
    let (mpu, basis) = if !unscaled {
        (1.0, json!({"kind": "FITTED", "mm_per_unit": 1.0}))
    } else if let (Some(sc), false) = (lab_scale, lab_weak) {
        let n = lab
            .as_ref()
            .and_then(|l| l["inliers"].as_array())
            .map_or(0, Vec::len);
        (
            sc,
            json!({"kind": "LABELS", "mm_per_unit": sc, "n_labels": n}),
        )
    } else {
        let mut ref_len: Vec<f64> = lens.iter().map(|(_, l)| *l).collect();
        if ref_len.is_empty() {
            for inst in &pattern.instances {
                for e in &inst.edges {
                    if let Ok(l) = inst.edge_length(&e.id) {
                        ref_len.push(l);
                    }
                }
            }
        }
        let med = if ref_len.is_empty() {
            1.0
        } else {
            median(&ref_len)
        };
        let mpu = o.median_seam_mm / med;
        let note = format!(
            "median {} ({} units) taken as {} mm",
            if lens.is_empty() { "edge" } else { "seam side" },
            py::fixed(med, 1),
            py::fixed(o.median_seam_mm, 0)
        );
        (
            mpu,
            json!({"kind": "NOMINAL", "mm_per_unit": mpu, "note": note}),
        )
    };
    let eq = if unscaled { "-equiv" } else { "" };

    let mut seam_rows: Vec<Value> = Vec::new();
    for s in &seams {
        let (Some(la), Some(lb)) = (len_of(s, "a"), len_of(s, "b")) else {
            continue;
        };
        let (sa, sb) = (
            s.get("a").cloned().unwrap_or(Value::Null),
            s.get("b").cloned().unwrap_or(Value::Null),
        );
        let (ta, tb) = (py_str(s.get("a")), py_str(s.get("b")));
        let id = py_str(s.get("id"));
        let ratio = if lb == 0.0 { f64::INFINITY } else { la / lb };
        let band = if s.contains_key("ratio_band") {
            s.get("ratio_band")
        } else {
            s.get("band")
        };
        let r = s
            .get("ratio")
            .or_else(|| s.get("ease_ratio"))
            .and_then(Value::as_f64)
            .unwrap_or(1.0);
        let gather = matches!(
            s.get("kind").and_then(Value::as_str),
            Some("gather" | "gathered")
        );
        if band.is_none_or(Value::is_null) && gather && !s.contains_key("ratio") {
            seam_rows.push(json!({"id": s.get("id"), "a": sa, "b": sb, "len_a": la, "len_b": lb, "ratio": ratio,
                                  "expected": "gathered (design ratio, not checked)", "ok": true}));
            continue;
        }
        let band = band.filter(|b| truthy(Some(b))).and_then(Value::as_array);
        let (target, dev_units, expected, lo_hi) = match band {
            Some(b) => {
                let lo = b.first().and_then(Value::as_f64).unwrap_or(f64::NAN);
                let hi = b.get(1).and_then(Value::as_f64).unwrap_or(f64::NAN);
                let target = ratio.max(lo).min(hi);
                (
                    target,
                    la - target * lb,
                    json!({"band": [lo, hi]}),
                    Some((lo, hi)),
                )
            }
            None => (r, la - r * lb, json!({"ratio": r}), None),
        };
        let dev_mm = dev_units * mpu;
        let dev_pct = if target == 0.0 {
            0.0
        } else {
            (ratio / target - 1.0) * 100.0
        };
        let ok = dev_mm.abs() <= o.tol_mm;
        seam_rows.push(json!({"id": s.get("id"), "a": sa, "b": sb, "len_a": py::round(la, 3),
            "len_b": py::round(lb, 3), "len_a_mm": py::round(la * mpu, 1), "len_b_mm": py::round(lb * mpu, 1),
            "ratio": py::round(ratio, 4), "expected": expected, "dev_mm": py::round(dev_mm, 2),
            "dev_pct": py::round(dev_pct, 2), "ok": ok}));
        if ok {
            continue;
        }
        let word = if dev_mm > 0.0 { "longer" } else { "shorter" };
        let msg = if let Some((lo, hi)) = lo_hi {
            format!(
                "Seam {id}: {ta} is {} x {tb}, outside the declared {}-{} band ({} mm{eq} too {} ease). Re-true the curve in the vectors.",
                py::fixed(ratio, 3),
                py::fixed(lo, 2),
                py::fixed(hi, 2),
                py::fixed(dev_mm.abs(), 0),
                if dev_mm > 0.0 { "much" } else { "little" }
            )
        } else if (r - 1.0).abs() < 1e-9 {
            format!(
                "Seam {id} does not walk: {ta} is {} mm{eq} {word} than {tb} ({} %). True the seam lengths in the vector pattern, not by regenerating the image.",
                py::fixed(dev_mm.abs(), 0),
                py::signed_fixed(dev_pct, 1)
            )
        } else {
            format!(
                "Seam {id}: {ta}/{tb} = {} but {} was declared ({} mm{eq} off). Adjust the vector edges.",
                py::fixed(ratio, 3),
                py::fixed(r, 3),
                py::fixed(dev_mm.abs(), 0)
            )
        };
        f.add(
            "M",
            "seam_mismatch",
            msg,
            vec![sa, sb],
            "error",
            vec![
                ("dev_mm", json!(py::round(dev_mm, 2))),
                ("dev_pct", json!(py::round(dev_pct, 2))),
            ],
        );
    }

    let checks: Vec<Value> = doc
        .extra
        .get("checks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for c in &checks {
        let sum = |k: &str| -> Result<(f64, Vec<String>), PyError> {
            let list = c.get(k).ok_or_else(|| PyError::Key(k.to_string()))?;
            let refs: Vec<String> = list
                .as_array()
                .map_or_else(Vec::new, |a| a.iter().map(|r| py_str(Some(r))).collect());
            let mut total = 0.0;
            for r in &refs {
                total += measure_ref(&pattern, r)?;
            }
            Ok((total, refs))
        };
        let ((la, ra), (lb, rb)) = match sum("a").and_then(|a| sum("b").map(|b| (a, b))) {
            Ok(x) => x,
            Err(e) => {
                f.add(
                    "T",
                    "check_ref",
                    format!("Check {}: {e}", py_str(c.get("id"))),
                    vec![],
                    "error",
                    vec![],
                );
                continue;
            }
        };
        let ratio = if lb == 0.0 { f64::INFINITY } else { la / lb };
        let (lo, hi, target) = match c
            .get("band")
            .filter(|b| truthy(Some(b)))
            .and_then(Value::as_array)
        {
            Some(b) => {
                let lo = b.first().and_then(Value::as_f64).unwrap_or(f64::NAN);
                let hi = b.get(1).and_then(Value::as_f64).unwrap_or(f64::NAN);
                (lo, hi, ratio.max(lo).min(hi))
            }
            None => {
                let r = c.get("ratio").and_then(Value::as_f64).unwrap_or(1.0);
                (r, r, r)
            }
        };
        let dev_mm = (la - target * lb) * mpu;
        seam_rows.push(
            json!({"id": c.get("id"), "a": c.get("a"), "b": c.get("b"), "len_a": py::round(la, 3),
            "len_b": py::round(lb, 3), "ratio": py::round(ratio, 4), "expected": {"band": [lo, hi]},
            "dev_mm": py::round(dev_mm, 2), "ok": dev_mm.abs() <= o.tol_mm, "check": true}),
        );
        if dev_mm.abs() > o.tol_mm {
            let what = c
                .get("note")
                .filter(|n| truthy(Some(n)))
                .map_or_else(|| "ratio check".to_string(), |n| py_str(Some(n)));
            let refs: Vec<Value> = ra.iter().chain(&rb).map(|r| json!(r)).collect();
            f.add(
                "M",
                "ratio_check",
                format!(
                    "{} ({what}): {} is {} x {}, outside {}-{} ({} mm{eq}). Fix in the vectors.",
                    py_str(c.get("id")),
                    ra.join(" + "),
                    py::fixed(ratio, 3),
                    rb.join(" + "),
                    py::fixed(lo, 2),
                    py::fixed(hi, 2),
                    py::fixed(dev_mm.abs(), 0)
                ),
                refs,
                "error",
                vec![("dev_mm", json!(py::round(dev_mm, 2)))],
            );
        }
    }

    // on-fold pieces: the edges that meet the fold must be square to it
    let ppu = meta
        .get("px_per_unit")
        .and_then(Value::as_f64)
        .filter(|x| *x != 0.0)
        .unwrap_or(1.0);
    for p in &doc.pieces {
        let Some(fold) = p.on_fold() else { continue };
        if !errs.is_empty() {
            continue;
        }
        let inst = piece_instance(p);
        let Some(i) = p.edges.iter().position(|e| e.id == fold) else {
            continue;
        };
        let n = p.edges.len();
        let (prev_e, next_e) = (&p.edges[(i + n - 1) % n], &p.edges[(i + 1) % n]);
        let Ok(f_pts) = inst.edge_dense(fold, 200) else {
            continue;
        };
        let span = (15.0 / mpu).max(10.0 / ppu);
        for (other, at_start) in [(prev_e, true), (next_e, false)] {
            let Ok(o_pts) = inst.edge_dense(&other.id, 200) else {
                continue;
            };
            let (t_in, t_out) = if at_start {
                (
                    end_tangent(&o_pts, false, span),
                    end_tangent(&f_pts, true, span),
                )
            } else {
                (
                    end_tangent(&f_pts, false, span),
                    end_tangent(&o_pts, true, span),
                )
            };
            let turn = degrees(det::atan2(
                t_in.x * t_out.y - t_in.y * t_out.x,
                t_in.x * t_out.x + t_in.y * t_out.y,
            ));
            let interior = 180.0 - turn;
            if (interior - 90.0).abs() > o.fold_tol_deg {
                let shape = if interior < 90.0 {
                    "a point"
                } else {
                    "a V-shaped dip"
                };
                f.add("M", "fold_not_square",
                    format!("{}: edge {} meets the fold ({fold}) at {} deg, not 90: unfolded, the piece gets {shape} at the fold. Square the corner (~2 cm) unless that shape is intended.",
                        p.id, other.id, py::fixed(interior, 0)),
                    vec![json!(format!("{}/{}", p.id, other.id))], "error",
                    vec![("angle_deg", json!(py::round(interior, 1)))]);
            }
        }
    }

    // declared-symmetric pieces: the halves must mirror
    for p in &doc.pieces {
        if !truthy(p.extra.get("symmetric")) {
            continue;
        }
        let Ok(o80) = outline(p, 80) else { continue };
        let ang = match p.grain.as_ref().and_then(Grain::points) {
            Some([g0, g1]) => det::atan2(g1.y - g0.y, g1.x - g0.x),
            None => 90.0 * (std::f64::consts::PI / 180.0),
        };
        let u = Vec2::new(det::cos(ang), det::sin(ang));
        let nrm = Vec2::new(-u.y, u.x);
        let mut sum = Vec2::new(0.0, 0.0);
        for q in &o80 {
            sum = Vec2::new(sum.x + q.x, sum.y + q.y);
        }
        let c0 = Vec2::new(sum.x / o80.len() as f64, sum.y / o80.len() as f64);
        let proj: Vec<f64> = o80
            .iter()
            .map(|q| (q.x - c0.x) * nrm.x + (q.y - c0.y) * nrm.y)
            .collect();
        let width = proj.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            - proj.iter().copied().fold(f64::INFINITY, f64::min);
        let mut best: Option<f64> = None;
        for t in np::linspace(-0.1, 0.1, 41) {
            let off = t * width;
            let c = Vec2::new(c0.x + off * nrm.x, c0.y + off * nrm.y);
            let m: Vec<Vec2> = o80
                .iter()
                .map(|q| {
                    let d = (q.x - c.x) * nrm.x + (q.y - c.y) * nrm.y;
                    Vec2::new(q.x - 2.0 * (d * nrm.x), q.y - 2.0 * (d * nrm.y))
                })
                .collect();
            let h = hausdorff(&o80, &m);
            if best.is_none_or(|b| h < b) {
                best = Some(h);
            }
        }
        let hmm = best.unwrap_or(0.0) * mpu;
        if hmm > o.tol_mm {
            f.add("M", "asymmetric",
                format!("{} should mirror about its {} but its halves differ by up to {} mm{eq}. Make the halves match (draft one half, mirror it).",
                    p.id, if p.grain.is_some() { "grain line" } else { "vertical axis" }, py::fixed(hmm, 0)),
                vec![json!(format!("{}:width", p.id))], "error",
                vec![("hausdorff_mm", json!(py::round(hmm, 2)))]);
        }
    }

    // inventory and topology
    let mut roles: Vec<(String, Vec<String>)> = Vec::new();
    for p in &doc.pieces {
        if let Some(role) = p.role.as_deref().filter(|r| !r.is_empty()) {
            match roles.iter_mut().find(|(r, _)| r == role) {
                Some((_, ids)) => ids.push(p.id.clone()),
                None => roles.push((role.to_string(), vec![p.id.clone()])),
            }
        }
        let tr = p.extra.get("trace");
        if truthy(tr.and_then(|t| t.get("touches_border"))) {
            let erased = truthy(tr.and_then(|t| t.get("touches_erased")));
            let place = if erased {
                "an erased (UI-covered) region"
            } else {
                "the image/crop border"
            };
            f.add("T", "cut_off",
                format!("{} touches {place}, so part of its outline is missing or invented. Re-crop, or regenerate with every piece fully inside the frame.", p.id),
                vec![json!(format!("{}:width", p.id))], "error", vec![]);
        }
    }
    let role = |r: &str| -> Vec<String> {
        roles
            .iter()
            .find(|(k, _)| k == r)
            .map_or_else(Vec::new, |(_, v)| v.clone())
    };
    let stem = |r: &str| -> String {
        let s = r
            .split('/')
            .next()
            .unwrap_or("")
            .split(':')
            .next()
            .unwrap_or("");
        let s = s.strip_suffix(".R").unwrap_or(s);
        s.strip_suffix(".L").unwrap_or(s).to_string()
    };
    let mut joined: Vec<(String, BTreeSet<String>)> = Vec::new();
    let join = |a: String, b: &[String], joined: &mut Vec<(String, BTreeSet<String>)>| match joined
        .iter_mut()
        .find(|(k, _)| *k == a)
    {
        Some((_, set)) => set.extend(b.iter().cloned()),
        None => joined.push((a, b.iter().cloned().collect())),
    };
    for p in &doc.pieces {
        if !joined.iter().any(|(k, _)| *k == p.id) {
            joined.push((p.id.clone(), BTreeSet::new()));
        }
    }
    for s in pattern.seams() {
        let (a, b) = (stem(&py_str(s.0.get("a"))), stem(&py_str(s.0.get("b"))));
        join(a.clone(), std::slice::from_ref(&b), &mut joined);
        join(b, std::slice::from_ref(&a), &mut joined);
    }
    for c in &checks {
        let set = |k: &str| -> Vec<String> {
            let mut v: Vec<String> = c
                .get(k)
                .and_then(Value::as_array)
                .map_or_else(Vec::new, |a| {
                    a.iter().map(|r| stem(&py_str(Some(r)))).collect()
                });
            v.sort();
            v.dedup();
            v
        };
        let (sa, sb) = (set("a"), set("b"));
        for a in &sa {
            join(a.clone(), &sb, &mut joined);
        }
        for b in &sb {
            join(b.clone(), &sa, &mut joined);
        }
    }
    let bodice: BTreeSet<String> = role("bodice_front")
        .into_iter()
        .chain(role("bodice_back"))
        .collect();
    for sl in role("sleeve") {
        if bodice.is_empty() {
            f.add("T", "inventory",
                format!("The sleeve '{sl}' has no bodice to sew into: the sheet has no front/back bodice piece."),
                vec![json!(format!("{sl}:width"))], "error", vec![]);
        } else if joined
            .iter()
            .find(|(k, _)| *k == sl)
            .is_none_or(|(_, set)| set.is_disjoint(&bodice))
        {
            f.add("T", "inventory",
                format!("The sleeve '{sl}' has no armhole partner: no seam or check joins its cap to a bodice armhole. Declare the cap/armhole seam (and a cap-ease check) or the sleeve does not belong."),
                vec![json!(format!("{sl}:width"))], "error", vec![]);
        }
    }
    for (a, b, what) in [
        ("bodice_front", "bodice_back", "bodice"),
        ("pant_front", "pant_back", "trouser"),
        ("skirt_front", "skirt_back", "skirt"),
    ] {
        let (ra, rb) = (role(a), role(b));
        if ra.is_empty() != rb.is_empty() {
            let (have, miss, ids) = if ra.is_empty() {
                (b, a, rb)
            } else {
                (a, b, ra)
            };
            f.add("T", "inventory",
                format!("There is a {} ({}) but no {}: a {what} piece seems to be missing from the sheet.",
                    have.replace('_', " "), ids.join(", "), miss.replace('_', " ")),
                vec![], "error", vec![]);
        }
    }
    if !role("waistband").is_empty()
        && role("pant_front").is_empty()
        && role("skirt_front").is_empty()
        && role("pant_back").is_empty()
    {
        f.add(
            "T",
            "inventory",
            "There is a waistband but no trouser or skirt piece for it to finish.".into(),
            vec![],
            "error",
            vec![],
        );
    }
    if pattern.seams().is_empty() {
        f.add("T", "no_seams",
            "No seams declared yet: view overlay.png, then declare seam pairs in names.json and re-run annotate so the seam-walk ratios can be checked.".into(),
            vec![], "info", vec![]);
    } else {
        for (pid, partners) in &joined {
            if doc.pieces.iter().any(|p| p.id == *pid) && partners.is_empty() {
                f.add("T", "unsewn",
                    format!("{pid} is not sewn to anything in the declared seams (missing seam, or not a piece?)."),
                    vec![json!(format!("{pid}:width"))], "warn", vec![]);
            }
        }
    }

    if let Some(lab) = &lab {
        let sc = lab["scale_mm_per_unit"].as_f64().filter(|x| *x != 0.0);
        let rows: Vec<&Value> = lab["per_label"]
            .as_array()
            .map_or_else(Vec::new, |a| a.iter().collect());
        let inl: Vec<&Value> = rows
            .iter()
            .copied()
            .filter(|r| truthy(r.get("inlier")))
            .collect();
        let weak = truthy(lab.get("weak"));
        if weak && sc.is_some() {
            let rival = match lab
                .get("rival_group")
                .and_then(Value::as_array)
                .filter(|g| !g.is_empty())
            {
                Some(g) => format!(
                    "; an equally large group disagrees ({})",
                    g.iter()
                        .map(|t| py::repr_str(&py_str(Some(t))))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                None => String::new(),
            };
            f.add("M", "labels_inconsistent",
                format!("Only {} of {} dimension labels agree on any one scale{rival}. The sheet's numbers are mutually inconsistent, so none of them can size the pattern; size it from the body.",
                    py_str(lab.get("n_agree")), py_str(lab.get("n_labels"))),
                inl.iter().map(|r| r["ref"].clone()).collect(), "error", vec![]);
        }
        for r in &rows {
            if truthy(r.get("inlier")) {
                continue;
            }
            let (text, rf) = (py_str(r.get("text")), py_str(r.get("ref")));
            if let Some(sc) = sc {
                let others: Vec<String> = inl
                    .iter()
                    .take(3)
                    .map(|x| format!("'{}'", py_str(x.get("text"))))
                    .collect();
                let unit = py_str(r.get("unit"));
                let mu = r["measured_units"].as_f64().unwrap_or(0.0);
                let value = r["value_mm"].as_f64().unwrap_or(0.0);
                let dev = r["dev_pct"].as_f64().unwrap_or(0.0);
                let mut refs = vec![r["ref"].clone()];
                refs.extend(inl.iter().take(3).map(|x| x["ref"].clone()));
                f.add("M", "label_contradiction",
                    format!("Label '{text}' ({rf}) contradicts {} ({}): at their common scale that feature measures {}, not {} ({} %). These labels cannot all hold for the drawn geometry; dimensions come from the measured block.",
                        if weak { "the largest agreeing group" } else { "the other labels" }, others.join(", "),
                        fmt_len(mu * sc, &unit), fmt_len(value, &unit), py::signed_fixed(dev, 0)),
                    refs, "error", vec![("dev_pct", r["dev_pct"].clone())]);
            } else {
                f.add("M", "label_contradiction",
                    format!("Label '{text}' ({rf}) agrees with no other label: the sheet's dimension labels contradict each other, so none of them can be trusted."),
                    vec![r["ref"].clone()], "error", vec![]);
            }
        }
        for u in lab["unparseable"]
            .as_array()
            .map_or_else(Vec::new, Clone::clone)
        {
            let text = py_str(u.get("text"));
            let class = if text.to_lowercase().contains("cut") {
                "T"
            } else {
                "M"
            };
            let tail = if class == "T" {
                "Cut quantities come from the pattern program, not the image."
            } else {
                ""
            };
            let msg = format!(
                "Label '{text}' carries no usable dimension ({}): metadata or garble, not geometry. {tail}",
                py_str(u.get("why"))
            );
            let refs = if truthy(u.get("ref")) {
                vec![u["ref"].clone()]
            } else {
                vec![]
            };
            f.add(
                class,
                "label_unparseable",
                msg.trim().to_string(),
                refs,
                "warn",
                vec![],
            );
        }
    }

    let count = |c: &str| {
        f.0.iter()
            .filter(|x| x["class"] == c && x["severity"] != "info")
            .count()
    };
    let ok = !f.0.iter().any(|x| x["severity"] == "error");
    let summary = json!({"T": count("T"), "S": count("S"), "M": count("M"), "ok": ok});
    let mut report = json!({
        "schema": AUDIT_SCHEMA,
        "pattern": doc.extra.get("name").cloned().unwrap_or(Value::Null),
        "scale_status": meta.get("scale_status").cloned().unwrap_or_else(|| json!("FITTED")),
        "scale_basis": basis,
        "tol_mm": o.tol_mm,
        "seams": seam_rows,
        "labels": lab.clone().unwrap_or(Value::Null),
        "findings": f.0,
        "summary": summary,
    });
    if let (Some(sc), true, false) = (lab_scale, unscaled, lab_weak) {
        report["suggested_scale"] = json!({"mm_per_unit": sc, "note": "label consensus; prefer known body/seam targets for scale-fit"});
    }
    report
}

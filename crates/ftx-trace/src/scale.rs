//! Scale-fit (spec 04 §10.14): the uniform scale that best turns measured references into known lengths.

use ftx_core::{Vec2, np, py};
use ftx_pattern::doc::{Grain, PatternDoc};
use ftx_pattern::{Pattern, PyError};
use serde_json::{Map, Value, json};

use crate::refs::measure_ref;

/// Scales a pattern by `k = Σaᵢ / Σaᵢ²` with `aᵢ = Lᵢ / Tᵢ` (each target counts equally in percent):
/// vertices, grain and internal/trace-mark points (r4), mark offsets (r3) and the trace's `to_px`; relative
/// curves and notch fractions stay. `px_per_unit` and `scale_status` (`FITTED`) are updated and the report
/// is stored as `meta.scale_fit`. Returns the scaled document and the report.
///
/// # Errors
/// No targets, a non-positive length, or a reference that does not measure.
pub fn fit_scale(
    doc: &PatternDoc,
    targets: &[(String, f64)],
) -> Result<(PatternDoc, Value), PyError> {
    if targets.is_empty() {
        return Err(PyError::Value(
            "scale-fit needs at least one --target REF=MM".into(),
        ));
    }
    let mut data = doc.clone();
    let pattern = Pattern::from_doc(doc.clone());
    let mut rows = Vec::new();
    for (r, mm) in targets {
        if *mm <= 0.0 {
            return Err(PyError::Value(format!(
                "target {}: length must be positive",
                py::repr_str(r)
            )));
        }
        rows.push((r.clone(), *mm, measure_ref(&pattern, r)?));
    }
    let a: Vec<f64> = rows.iter().map(|(_, t, l)| l / t).collect();
    let sq: Vec<f64> = a.iter().map(|x| x * x).collect();
    let k = np::pairwise_sum(&a) / np::pairwise_sum(&sq);
    let r4 = |p: Vec2| Vec2::new(py::round(p.x * k, 4), py::round(p.y * k, 4));
    for p in &mut data.pieces {
        for (_, v) in &mut p.vertices.0 {
            *v = r4(*v);
        }
        if let Some([g0, g1]) = p.grain.as_ref().and_then(Grain::points) {
            p.grain = Some(Grain::Points(vec![r4(g0), r4(g1)]));
        }
        for it in p
            .internal
            .iter_mut()
            .flatten()
            .chain(p.trace_marks.iter_mut().flatten())
        {
            for q in &mut it.pts {
                *q = r4(*q);
            }
            if let Some(off) = it.extra.get("offset").and_then(Value::as_f64) {
                it.extra
                    .insert("offset".into(), json!(py::round(off * k, 3)));
            }
        }
        if let Some(tr) = p.extra.get_mut("trace").and_then(Value::as_object_mut)
            && let Some(rows) = tr.get_mut("to_px").and_then(Value::as_array_mut)
        {
            for row in rows.iter_mut().filter_map(Value::as_array_mut) {
                for c in row.iter_mut().take(2) {
                    if let Some(x) = c.as_f64() {
                        *c = json!(x / k);
                    }
                }
            }
        }
    }
    let meta = data.meta.get_or_insert_with(Map::new);
    let ppu_before = meta
        .get("px_per_unit")
        .and_then(Value::as_f64)
        .unwrap_or(1.0);
    meta.insert("px_per_unit".into(), json!(ppu_before / k));
    meta.insert("scale_status".into(), json!("FITTED"));
    let res: Vec<(String, f64, f64, f64, f64)> = rows
        .iter()
        .map(|(r, t, l)| {
            (
                r.clone(),
                *t,
                py::round(*l, 4),
                py::round(k * l, 3),
                py::round((k * l - t) / t * 100.0, 3),
            )
        })
        .collect();
    let squares: Vec<f64> = res.iter().map(|x| x.4 * x.4).collect();
    let rms = (np::pairwise_sum(&squares) / squares.len() as f64).sqrt();
    let warning = res.iter().any(|x| x.4.abs() > 2.0).then_some(
        "targets disagree by more than 2 %: at least one known length (or its ref) is wrong",
    );
    let report = json!({
        "k": k,
        "mm_per_px": if ppu_before == 0.0 { Value::Null } else { json!(k / ppu_before) },
        "targets": res.iter().map(|(r, t, l, f, p)| json!({
            "ref": r, "target_mm": t, "measured_before": l, "fitted_mm": f, "residual_pct": p
        })).collect::<Vec<_>>(),
        "rms_pct": py::round(rms, 3),
        "warning": warning,
    });
    meta.insert("scale_fit".into(), report.clone());
    Ok((data, report))
}

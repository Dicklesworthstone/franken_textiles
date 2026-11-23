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
        if !mm.is_finite() || *mm <= 0.0 {
            return Err(PyError::Value(format!(
                "target {}: length must be positive",
                py::repr_str(r)
            )));
        }
        let measured = measure_ref(&pattern, r)?;
        if !measured.is_finite() || measured <= 0.0 {
            return Err(PyError::Value(format!(
                "target {}: reference length must be positive",
                py::repr_str(r)
            )));
        }
        rows.push((r.clone(), *mm, measured));
    }
    let a: Vec<f64> = rows.iter().map(|(_, t, l)| l / t).collect();
    let sq: Vec<f64> = a.iter().map(|x| x * x).collect();
    let k = np::pairwise_sum(&a) / np::pairwise_sum(&sq);
    if !k.is_finite() || k <= 0.0 {
        return Err(PyError::Value(
            "scale-fit produced a non-positive or non-finite scale".into(),
        ));
    }
    let r4 = |p: Vec2| -> Result<Vec2, PyError> {
        let out = Vec2::new(py::round(p.x * k, 4), py::round(p.y * k, 4));
        if !out.is_finite() {
            return Err(PyError::Value(
                "scale-fit produced non-finite coordinates".into(),
            ));
        }
        Ok(out)
    };
    for p in &mut data.pieces {
        for (_, v) in &mut p.vertices.0 {
            *v = r4(*v)?;
        }
        if let Some([g0, g1]) = p.grain.as_ref().and_then(Grain::points) {
            p.grain = Some(Grain::Points(vec![r4(g0)?, r4(g1)?]));
        }
        for it in p
            .internal
            .iter_mut()
            .flatten()
            .chain(p.trace_marks.iter_mut().flatten())
        {
            for q in &mut it.pts {
                *q = r4(*q)?;
            }
            if let Some(off) = it.extra.get("offset").and_then(Value::as_f64) {
                let scaled = py::round(off * k, 3);
                if !scaled.is_finite() {
                    return Err(PyError::Value(
                        "scale-fit produced a non-finite mark offset".into(),
                    ));
                }
                it.extra.insert("offset".into(), json!(scaled));
            }
        }
        if let Some(tr) = p.extra.get_mut("trace").and_then(Value::as_object_mut)
            && let Some(rows) = tr.get_mut("to_px").and_then(Value::as_array_mut)
        {
            // np.asarray(to_px, float) with A[:, :2] /= k: every entry comes back a float
            for row in rows.iter_mut().filter_map(Value::as_array_mut) {
                for (i, c) in row.iter_mut().enumerate() {
                    if let Some(x) = c.as_f64() {
                        let v = if i < 2 { x / k } else { x };
                        if !v.is_finite() {
                            return Err(PyError::Value(
                                "scale-fit produced a non-finite to_px entry".into(),
                            ));
                        }
                        *c = json!(v);
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
    let ppu_after = ppu_before / k;
    if !ppu_after.is_finite() {
        return Err(PyError::Value(
            "scale-fit produced a non-finite px_per_unit".into(),
        ));
    }
    let mm_per_px = if ppu_before == 0.0 {
        Value::Null
    } else {
        let v = k / ppu_before;
        if !v.is_finite() {
            return Err(PyError::Value(
                "scale-fit produced a non-finite mm_per_px".into(),
            ));
        }
        json!(v)
    };
    meta.insert("px_per_unit".into(), json!(ppu_after));
    meta.insert("scale_status".into(), json!("FITTED"));
    let mut res: Vec<(String, f64, f64, f64, f64)> = Vec::with_capacity(rows.len());
    for (r, t, l) in &rows {
        let meas_r = py::round(*l, 4);
        let fitted = py::round(k * l, 3);
        let pct = py::round((k * l - t) / t * 100.0, 3);
        if !meas_r.is_finite() || !fitted.is_finite() || !pct.is_finite() {
            return Err(PyError::Value(
                "scale-fit produced a non-finite residual".into(),
            ));
        }
        res.push((r.clone(), *t, meas_r, fitted, pct));
    }
    let squares: Vec<f64> = res.iter().map(|x| x.4 * x.4).collect();
    let rms = py::round(
        (np::pairwise_sum(&squares) / squares.len() as f64).sqrt(),
        3,
    );
    if !rms.is_finite() {
        return Err(PyError::Value(
            "scale-fit produced a non-finite rms residual".into(),
        ));
    }
    let warning = res.iter().any(|x| x.4.abs() > 2.0).then_some(
        "targets disagree by more than 2 %: at least one known length (or its ref) is wrong",
    );
    let report = json!({
        "k": k,
        "mm_per_px": mm_per_px,
        "targets": res.iter().map(|(r, t, l, f, p)| json!({
            "ref": r, "target_mm": t, "measured_before": l, "fitted_mm": f, "residual_pct": p
        })).collect::<Vec<_>>(),
        "rms_pct": rms,
        "warning": warning,
    });
    meta.insert("scale_fit".into(), report.clone());
    Ok((data, report))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect_doc() -> PatternDoc {
        serde_json::from_value(json!({
            "schema": "fabric-pattern/1",
            "units": "mm",
            "meta": {"scale_status": "UNSCALED", "px_per_unit": 1.0},
            "pieces": [{
                "id": "p1",
                "vertices": {
                    "c1": [0.0, 0.0],
                    "c2": [100.0, 0.0],
                    "c3": [100.0, 50.0],
                    "c4": [0.0, 50.0]
                },
                "edges": [
                    {"id": "e1", "from": "c1", "to": "c2"},
                    {"id": "e2", "from": "c2", "to": "c3"},
                    {"id": "e3", "from": "c3", "to": "c4"},
                    {"id": "e4", "from": "c4", "to": "c1"}
                ]
            }]
        }))
        .unwrap()
    }

    #[test]
    fn fit_scale_refuses_non_finite_targets_and_zero_length_references() {
        let doc = rect_doc();
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -10.0] {
            let err = fit_scale(&doc, &[("p1/e1".into(), bad)])
                .unwrap_err()
                .to_string();
            assert!(err.contains("positive"), "bad={bad}: {err}");
        }
        // Horizontal edge projected onto y measures 0.0; previously gave k = 0/0 = NaN.
        let err = fit_scale(&doc, &[("p1:c1-c2:y".into(), 100.0)])
            .unwrap_err()
            .to_string();
        assert!(err.contains("reference length must be positive"), "{err}");

        // Extreme target causing k underflow/overflow must also refuse cleanly.
        let err = fit_scale(&doc, &[("p1/e1".into(), 1e308)])
            .unwrap_err()
            .to_string();
        assert!(err.contains("non-positive or non-finite scale"), "{err}");

        // Finite k that overflows scaled coordinates or ppu metadata must also refuse cleanly.
        let mut huge_ppu = doc.clone();
        huge_ppu
            .meta
            .as_mut()
            .unwrap()
            .insert("px_per_unit".into(), json!(1e-200));
        let err = fit_scale(&huge_ppu, &[("p1/e1".into(), 1e150)])
            .unwrap_err()
            .to_string();
        assert!(err.contains("non-finite"), "{err}");

        let mut huge_vert = doc.clone();
        huge_vert.pieces[0].vertices.0[2].1 = Vec2::new(1e160, 50.0);
        let err = fit_scale(&huge_vert, &[("p1/e1".into(), 1e152)])
            .unwrap_err()
            .to_string();
        assert!(err.contains("non-finite"), "{err}");

        // Valid target still scales cleanly.
        let (scaled, report) = fit_scale(&doc, &[("p1/e1".into(), 200.0)]).unwrap();
        assert!((report["k"].as_f64().unwrap() - 2.0).abs() < 1e-12);
        assert_eq!(scaled.pieces[0].vertices.0[1].1, Vec2::new(200.0, 0.0));
    }
}

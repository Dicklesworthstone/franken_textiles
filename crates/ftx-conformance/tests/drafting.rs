//! The analytic body and the drafting programs against `tests/fixtures/oracle/draft/` (captured by
//! `tools/oracle/capture_drafting.py`; spec 02 Parts B–C, D.3–D.4): landmarks, sections, frames, section radii
//! and landmark heights for three reference wearers and a flat dict; every draft (three programs at three
//! statures, style variants, flat dicts) equal in structure with every number within 1e-9 (vertices, curve
//! controls and braid points bit-identical); the refusals with the reference's messages; the soutache borders
//! (shapely's offset curve and clip) with the same points to 1e-9 mm.

use ftx_conformance::fixture;
use ftx_core::Vec2;
use ftx_pattern::draft::{self, form, geos};
use serde_json::Value;

/// Structural equality; numbers within `tol(path)`. Collects (path, got, want).
fn diff(
    a: &Value,
    b: &Value,
    path: &str,
    tol: &dyn Fn(&str) -> f64,
    out: &mut Vec<(String, String, String)>,
) {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            let (x, y) = (x.as_f64().unwrap(), y.as_f64().unwrap());
            if (x - y).abs() > tol(path) {
                out.push((path.into(), x.to_string(), y.to_string()));
            }
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
            for (i, (p, q)) in x.iter().zip(y).enumerate() {
                diff(p, q, &format!("{path}[{i}]"), tol, out);
            }
        }
        (Value::Object(x), Value::Object(y)) => {
            for k in x.keys().chain(y.keys()) {
                match (x.get(k), y.get(k)) {
                    (Some(p), Some(q)) => {
                        if x.contains_key(k)
                            && y.contains_key(k)
                            && !out.iter().any(|o| o.0 == format!("{path}.{k}"))
                        {
                            diff(p, q, &format!("{path}.{k}"), tol, out);
                        }
                    }
                    (p, q) => out.push((format!("{path}.{k}"), format!("{p:?}"), format!("{q:?}"))),
                }
            }
        }
        _ if a == b => {}
        _ => out.push((path.into(), short(a), short(b))),
    }
}

fn numbers(v: &Value) -> usize {
    match v {
        Value::Number(_) => 1,
        Value::Array(a) => a.iter().map(numbers).sum(),
        Value::Object(o) => o.values().map(numbers).sum(),
        _ => 0,
    }
}

fn short(v: &Value) -> String {
    let s = v.to_string();
    if s.len() > 120 {
        format!("{}… ({} chars)", &s[..120], s.len())
    } else {
        s
    }
}

/// The unit a stored number was rounded to, from where it sits (one unit of slack for a rounding flip).
/// Every stored number, rounded or not, within 1e-9 (in practice all but a few unrounded trigonometric ones
/// are bit-identical; those differ by ~1e-13).
fn draft_tol(_path: &str) -> f64 {
    1e-9
}

#[test]
fn the_body_matches_the_reference() {
    let fx = fixture("draft", "body");
    let mut failures = Vec::new();
    let mut n = 0;
    for case in fx["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let m = form::Meas(&case["input"]["measurements"]);
        let out = &case["output"];
        let mut check = |what: String, got: f64, want: f64| {
            n += 1;
            if (got - want).abs() > 1e-9 * want.abs().max(1.0) {
                failures.push(format!("{id} {what}: {got} vs {want}"));
            }
        };
        let l = form::landmarks(m).unwrap();
        for (k, v) in out["landmarks"].as_object().unwrap() {
            check(
                format!("landmark {k}"),
                l.get(k).unwrap(),
                v.as_f64().unwrap(),
            );
        }
        let secs = form::torso_sections(m).unwrap();
        for (i, (s, w)) in secs
            .iter()
            .zip(out["sections"].as_array().unwrap())
            .enumerate()
        {
            for (j, g) in [s.0, s.1, s.2, s.3].into_iter().enumerate() {
                check(format!("section {i}.{j}"), g, w[j].as_f64().unwrap());
            }
        }
        for (part, f) in out["frames"].as_object().unwrap() {
            let fr = form::frame(m, part).unwrap();
            for (key, v3) in [
                ("origin", fr.origin),
                ("up", fr.up),
                ("front", fr.front),
                ("lat", fr.lat),
            ] {
                for (j, g) in [v3.x, v3.y, v3.z].into_iter().enumerate() {
                    check(
                        format!("frame {part}.{key}[{j}]"),
                        g,
                        f[key][j].as_f64().unwrap(),
                    );
                }
            }
            for (key, g) in [
                ("tip_h", fr.tip_h),
                ("length", fr.length),
                ("knee_d", fr.knee_d),
            ] {
                if let Some(w) = f.get(key) {
                    check(
                        format!("frame {part}.{key}"),
                        g.unwrap_or(f64::NAN),
                        w.as_f64().unwrap(),
                    );
                }
            }
        }
        for (part, rows) in out["section_radius"].as_object().unwrap() {
            for r in rows.as_array().unwrap() {
                let h = r[0].as_f64().unwrap();
                check(
                    format!("radius {part}@{h}"),
                    form::section_radius(m, part, h).unwrap(),
                    r[1].as_f64().unwrap(),
                );
            }
        }
        for (part, names) in out["landmark_h"].as_object().unwrap() {
            for (name, v) in names.as_object().unwrap() {
                check(
                    format!("landmark_h {part}.{name}"),
                    form::landmark_h(m, part, name).unwrap(),
                    v.as_f64().unwrap(),
                );
            }
        }
        check(
            "crotch path".into(),
            form::crotch_path_mm(m).unwrap(),
            out["crotch_path_mm"].as_f64().unwrap(),
        );
    }
    println!("body: {n} numbers");
    assert!(
        failures.is_empty(),
        "{} of {n} differ:\n{}",
        failures.len(),
        failures
            .iter()
            .take(30)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn drafts_match_the_reference() {
    let fx = fixture("draft", "programs");
    let mut failures = Vec::new();
    let mut summary = Vec::new();
    for case in fx["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let inp = &case["input"];
        let style = (!inp["style"].is_null()).then_some(&inp["style"]);
        let got = draft::draft(
            inp["program"].as_str().unwrap(),
            &inp["measurements"],
            style,
        );
        let out = &case["output"];
        match (got, out.get("error")) {
            (Err(r), Some(kind)) => {
                let msg = out["message"].as_str().unwrap();
                let ok = if kind == "KeyError" {
                    r.message.contains(msg)
                } else {
                    r.message == msg
                };
                if !ok {
                    failures.push(format!(
                        "{id}: refusal {:?}\n  want {kind}: {msg}",
                        r.message
                    ));
                }
            }
            (Err(r), None) => failures.push(format!("{id}: refused {r}")),
            (Ok(_), Some(kind)) => {
                failures.push(format!("{id}: drafted, the reference raised {kind}"))
            }
            (Ok((pat, spec)), None) => {
                let mut d = Vec::new();
                diff(&pat, &out["pattern"], "pattern", &draft_tol, &mut d);
                diff(&spec, &out["fit_spec"], "fit_spec", &draft_tol, &mut d);
                // how many numbers are not bit-identical, and by how much
                let mut inexact = Vec::new();
                diff(&pat, &out["pattern"], "pattern", &|_| 0.0, &mut inexact);
                diff(&spec, &out["fit_spec"], "fit_spec", &|_| 0.0, &mut inexact);
                let max = inexact
                    .iter()
                    .filter_map(|(_, g, w)| {
                        Some((g.parse::<f64>().ok()? - w.parse::<f64>().ok()?).abs())
                    })
                    .fold(0.0, f64::max);
                summary.push(format!(
                    "{id}: {} of {} numbers not identical (max {max:e}), {} beyond rounding",
                    inexact.len(),
                    numbers(&out["pattern"]) + numbers(&out["fit_spec"]),
                    d.len()
                ));
                for (p, g, w) in d.iter().take(12) {
                    failures.push(format!("{id} {p}: {g} vs {w}"));
                }
            }
        }
    }
    println!("{}", summary.join("\n"));
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures
            .iter()
            .take(60)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

fn pts(v: &Value) -> Vec<Vec2> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|p| Vec2::new(p[0].as_f64().unwrap(), p[1].as_f64().unwrap()))
        .collect()
}

#[test]
fn soutache_borders_match_shapely() {
    let fx = fixture("draft", "programs");
    let mut failures = Vec::new();
    let mut worst: f64 = 0.0;
    for case in fx["cases"].as_array().unwrap() {
        let Some(trace) = case["output"].get("soutache_trace") else {
            continue;
        };
        let id = case["id"].as_str().unwrap();
        for (pid, t) in trace.as_object().unwrap() {
            let ext = pts(&t["ext"]);
            let inset = t["inset"].as_f64().unwrap();
            let secs = geos::offset_curve(&ext, inset, 3.0);
            let want: Vec<Vec<Vec2>> = t["offset"].as_array().unwrap().iter().map(pts).collect();
            if secs.len() != want.len() {
                failures.push(format!(
                    "{id}/{pid}: {} sections vs {}",
                    secs.len(),
                    want.len()
                ));
                continue;
            }
            for (g, w) in secs.iter().zip(&want) {
                if g.len() != w.len() {
                    failures.push(format!(
                        "{id}/{pid}: offset {} points vs {}",
                        g.len(),
                        w.len()
                    ));
                    continue;
                }
                let d = g.iter().zip(w).map(|(a, b)| a.dist(*b)).fold(0.0, f64::max);
                worst = worst.max(d);
                if d > 1e-9 {
                    failures.push(format!("{id}/{pid}: offset points up to {d:e} apart"));
                }
            }
            let outline = pts(&t["outline"]);
            let parts: Vec<Vec<Vec2>> = secs
                .iter()
                .flat_map(|s| geos::clip_inside(s, &outline))
                .collect();
            let want: Vec<Vec<Vec2>> = t["parts"].as_array().unwrap().iter().map(pts).collect();
            if parts.len() != want.len() || parts.iter().zip(&want).any(|(g, w)| g.len() != w.len())
            {
                failures.push(format!(
                    "{id}/{pid}: parts {:?} vs {:?}",
                    parts.iter().map(Vec::len).collect::<Vec<_>>(),
                    want.iter().map(Vec::len).collect::<Vec<_>>()
                ));
                continue;
            }
            for (g, w) in parts.iter().zip(&want) {
                let d = g.iter().zip(w).map(|(a, b)| a.dist(*b)).fold(0.0, f64::max);
                worst = worst.max(d);
                if d > 1e-9 {
                    failures.push(format!("{id}/{pid}: clipped points up to {d:e} apart"));
                }
            }
        }
    }
    println!("soutache borders: worst point {worst:e} mm from shapely's");
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

//! `tests/fixtures/oracle/pattern/geometry.json` (captured by `tools/oracle/capture_pattern_geometry.py`)
//! against `ftx-core`: chord-relative encoding, curve sampling at every sample count, mirroring, and the
//! polyline/polygon helpers, on the spec's fixture inputs (01 §14.2 F1–F6) and seeded random ones.

use ftx_conformance::{Compare, fixture};
use ftx_core::curve::{abs_to_rel, curve_points, mirror_curve, rel_to_abs, reverse_curve};
use ftx_core::polyline::{
    at_fractions, cumulative, horizontal_width, point_in_polygon, polyline_length, signed_area,
    tangent_at,
};
use ftx_core::{Curve, End, Vec2};
use serde_json::{Value, json};

fn p(v: &Value) -> Vec2 {
    Vec2::new(v[0].as_f64().unwrap(), v[1].as_f64().unwrap())
}

fn pts(v: &Value) -> Vec<Vec2> {
    v.as_array().unwrap().iter().map(p).collect()
}

fn flat(points: &[Vec2]) -> Vec<f64> {
    points.iter().flat_map(|q| [q.x, q.y]).collect()
}

fn curve(v: &Value) -> Option<Curve> {
    if v.is_null() {
        None
    } else {
        Some(serde_json::from_value(v.clone()).unwrap())
    }
}

fn flat_json(v: &Value) -> Value {
    Value::Array(
        v.as_array()
            .unwrap()
            .iter()
            .flat_map(|q| q.as_array().unwrap().clone())
            .collect(),
    )
}

#[test]
fn geometry_matches_the_reference() {
    let fx = fixture("pattern", "geometry");
    let mut cmp = Compare::new(1e-9);
    let mut seen = std::collections::BTreeMap::<String, usize>::new();
    for case in fx["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let f = case["fn"].as_str().unwrap();
        *seen.entry(f.to_string()).or_default() += 1;
        let inp = &case["input"];
        let out = &case["output"];
        match f {
            "abs_to_rel" => match abs_to_rel(p(&inp["p0"]), p(&inp["p1"]), p(&inp["q"])) {
                Ok(th) => cmp.nums(id, &th, &out["value"]),
                Err(e) => cmp.json(id, &json!({"error": e.to_string()}), out),
            },
            "rel_to_abs" => {
                let th = [
                    inp["th"][0].as_f64().unwrap(),
                    inp["th"][1].as_f64().unwrap(),
                ];
                let q = rel_to_abs(p(&inp["p0"]), p(&inp["p1"]), th);
                cmp.nums(id, &[q.x, q.y], &out["value"]);
            }
            "curve_points" => {
                let dense = inp["dense"].as_u64().unwrap() as usize;
                match curve_points(
                    p(&inp["p0"]),
                    p(&inp["p1"]),
                    curve(&inp["curve"]).as_ref(),
                    dense,
                ) {
                    Ok(q) => cmp.nums(id, &flat(&q), &flat_json(&out["value"])),
                    Err(e) => cmp.json(id, &json!({"error": e.to_string()}), out),
                }
            }
            "mirror_curve" | "reverse_curve" => {
                let c = curve(&inp["curve"]);
                let got = if f == "mirror_curve" {
                    mirror_curve(c.as_ref())
                } else {
                    reverse_curve(c.as_ref())
                };
                cmp.json(id, &serde_json::to_value(got).unwrap(), &out["value"]);
            }
            "polyline_length" => cmp.num(
                id,
                polyline_length(&pts(&inp["pts"])),
                out["value"].as_f64().unwrap(),
            ),
            "cumulative" => cmp.nums(id, &cumulative(&pts(&inp["pts"])), &out["value"]),
            "at_fractions" => {
                let fr: Vec<f64> = inp["fractions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_f64().unwrap())
                    .collect();
                cmp.nums(
                    id,
                    &flat(&at_fractions(&pts(&inp["pts"]), &fr)),
                    &flat_json(&out["value"]),
                );
            }
            "tangent_at" => {
                let end = if inp["end"] == "start" {
                    End::Start
                } else {
                    End::End
                };
                let t = tangent_at(&pts(&inp["pts"]), end);
                cmp.nums(id, &[t.x, t.y], &out["value"]);
            }
            "signed_area" => cmp.num(
                id,
                signed_area(&pts(&inp["poly"])),
                out["value"].as_f64().unwrap(),
            ),
            "horizontal_width" => {
                let poly = pts(&inp["poly"]);
                let got: Vec<f64> = inp["ys"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|y| horizontal_width(&poly, y.as_f64().unwrap()))
                    .collect();
                cmp.nums(id, &got, &out["value"]);
            }
            "point_in_polygon" => {
                let poly = pts(&inp["poly"]);
                let got: Vec<Value> = pts(&inp["pts"])
                    .iter()
                    .map(|&q| Value::Bool(point_in_polygon(q, &poly)))
                    .collect();
                cmp.json(id, &Value::Array(got), &out["value"]);
            }
            other => panic!("{id}: no comparison for fn '{other}'"),
        }
    }
    println!("cases per function: {seen:?}");
    cmp.finish("pattern geometry");
}

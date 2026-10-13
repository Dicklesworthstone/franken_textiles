//! Drape metrics and flags against `tests/fixtures/oracle/drape/metrics.json` (captured in Blender by
//! `tools/oracle/capture_metrics.py`; spec 04 §6): the reference's `metrics()` on synthetic frame
//! sequences built from the exact start state of `start_state.json` by a recipe of additions and
//! multiplications, rebuilt here bit for bit. Every report field must agree (numbers to 1e-12 relative)
//! and every flag text exactly.

use std::collections::BTreeMap;

use ftx_conformance::{Compare, fixture};
use ftx_core::Vec3;
use ftx_drape::metrics::{MetricsInput, metrics};
use ftx_drape::{NearestTree, build_garment};
use ftx_pattern::Pattern;
use serde_json::Value;

fn points(v: &Value) -> Vec<Vec3> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|p| {
            Vec3::new(
                p[0].as_f64().unwrap(),
                p[1].as_f64().unwrap(),
                p[2].as_f64().unwrap(),
            )
        })
        .collect()
}

/// The capture's sequence recipe, in its arithmetic order.
fn build_seq(
    relaxed: &[Vec3],
    ranges: &BTreeMap<String, (usize, usize)>,
    recipe: &Value,
    n: usize,
    y_c: f64,
) -> Vec<Vec<Vec3>> {
    let mut shift = vec![Vec3::new(0.0, 0.0, recipe["sag"].as_f64().unwrap()); relaxed.len()];
    if let Some(m) = recipe["move"].as_array() {
        let (lo, hi) = ranges[m[0].as_str().unwrap()];
        for s in &mut shift[lo..hi] {
            s.x += m[1].as_f64().unwrap();
        }
    }
    if let Some(m) = recipe["push"].as_array() {
        let (lo, hi) = ranges[m[0].as_str().unwrap()];
        for s in &mut shift[lo..hi] {
            s.y += m[1].as_f64().unwrap();
        }
    }
    let (stretch, jitter) = (
        recipe["stretch"].as_f64().unwrap(),
        recipe["jitter"].as_f64().unwrap(),
    );
    let w = (n / 6).clamp(2, 10);
    (0..n)
        .map(|k| {
            let t = k as f64 / (n - 1) as f64;
            let s = 1.0 + t * stretch;
            relaxed
                .iter()
                .zip(&shift)
                .map(|(r, d)| {
                    let x = *r + *d * t;
                    let mut z = x.z;
                    if k >= n - w && jitter != 0.0 {
                        z += if k % 2 == 1 { 1.0 } else { -1.0 } * jitter;
                    }
                    Vec3::new(x.x * s, y_c + (x.y - y_c) * s, z)
                })
                .collect()
        })
        .collect()
}

#[test]
fn metrics_match_the_reference() {
    let fx = fixture("drape", "metrics");
    let drafts = fixture("pattern", "drafts");
    let starts = fixture("drape", "start_state");
    let mut cmp = Compare::new(1e-12);
    let mut flag_failures = Vec::new();
    for case in fx["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let inp = &case["input"];
        let draft = inp["draft"].as_str().unwrap();
        let d = drafts["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == draft)
            .unwrap();
        let st = &starts["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"].as_str().unwrap().starts_with(draft))
            .unwrap()["output"];
        let pattern = Pattern::new(d["input"]["pattern"].clone());
        let mesh = build_garment(&pattern, 12.0).unwrap();
        let ranges: BTreeMap<String, (usize, usize)> = mesh
            .instances
            .iter()
            .cloned()
            .zip(mesh.inst_range.iter().copied())
            .collect();
        let relaxed = points(&st["exact"]["relaxed"]);
        let wrapped = points(&st["wrapped"]);
        let body = NearestTree::new(
            points(&st["form"]["vertices"]),
            st["form"]["triangles"]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| std::array::from_fn(|j| t[j].as_u64().unwrap() as usize))
                .collect(),
        )
        .unwrap();
        let n = inp["frames"].as_u64().unwrap() as usize;
        let seq = build_seq(
            &relaxed,
            &ranges,
            &inp["recipe"],
            n,
            inp["y_c"].as_f64().unwrap(),
        );
        // The reference's level heights, keyed by (instance, landmark).
        let spec = &d["input"]["fit_spec"];
        let mut levels: BTreeMap<(String, String), f64> = BTreeMap::new();
        for p in spec["points"].as_array().unwrap() {
            if let Some(per) = inp["levels"][p["id"].as_str().unwrap()].as_object() {
                for (inst, y) in per {
                    levels.insert(
                        (inst.clone(), p["landmark"].as_str().unwrap().to_string()),
                        y.as_f64().unwrap(),
                    );
                }
            }
        }
        let level_y =
            |inst: &str, lm: &str| levels.get(&(inst.to_string(), lm.to_string())).copied();
        let rep = metrics(
            &mesh,
            &MetricsInput {
                pattern: &pattern,
                seq: &seq,
                sew_end: inp["sew_end"].as_u64().unwrap() as usize,
                wrapped: &wrapped,
                body: &body,
                level_y: &level_y,
                fit_spec: spec,
                spacing: inp["spacing"].as_f64().unwrap(),
                arm_deg: inp["arm_deg"].as_f64().unwrap(),
            },
        );
        let want = &case["output"]["report"];
        for key in ["flags", "gating", "tightness_gating"] {
            if rep[key] != want[key] {
                flag_failures.push(format!(
                    "{id} {key}:\n  got  {}\n  want {}",
                    rep[key], want[key]
                ));
            }
        }
        cmp.json(id, &rep, want);
        println!(
            "{id}: {} flags ({} gating), settled {}",
            rep["flags"].as_array().unwrap().len(),
            rep["gating"].as_array().unwrap().len(),
            rep["settled"]
        );
    }
    assert!(flag_failures.is_empty(), "{}", flag_failures.join("\n"));
    cmp.finish("metrics");
}

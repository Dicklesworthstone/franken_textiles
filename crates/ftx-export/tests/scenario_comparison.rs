//! Synthetic callback outcomes test comparison admission, not cloth physics or measured fit.
//! Actual production-solver isolation is exercised separately in ftx-wasm/material_scenario.
use ftx_core::{Refusal, receipt::sha256_hex};
use ftx_export::material_study::Request;
use ftx_pattern::{Pattern, material_screen::Direction};
use serde_json::{Value, json};

const PATTERN: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-scenario-pattern.json");
fn descriptor() -> Value {
    json!({"scenario_sha256":"1".repeat(64), "body_geometry_sha256":"2".repeat(64)})
}
fn request() -> Request {
    let p: Value = serde_json::from_str(PATTERN).unwrap();
    let material = p["fabric"]["material"].clone();
    serde_json::from_value(json!({"schema":"ftx-material-study/1", "variants":[
        {"key":"a","changes":{"schema":"ftx-material-bindings/1","default":material}},
        {"key":"b","changes":{"schema":"ftx-material-bindings/1","default":material}}],
        "cutting":{"copies":1,"gap_mm":1.,"edge_margin_mm":0.,"end_margin_mm":0.},
        "comparison":{"basis":"nominal","objectives":[
            {"metric":"independent_surface_power_w","direction":"minimize"},
            {"metric":"simulated_cloth_mass_kg","direction":"minimize"}],
            "constraints":[{"metric":"max_absolute_edge_strain_pct","maximum":12.}],
            "required_consumers":["coverage"]}}))
    .unwrap()
}
fn synthetic(pattern: &Pattern, power: f64, mass: f64) -> Value {
    json!({"schema":"ftx-drape-scenario-result/1",
        "pattern_sha256":sha256_hex(&serde_json::to_vec(pattern.raw()).unwrap()),
        "scenario_sha256":descriptor()["scenario_sha256"],
        "body_geometry_sha256":descriptor()["body_geometry_sha256"],
        "metrics":{"simulation_gating_clear":true},
        "drape":{"settled":true,"gating":[],"mass_kg":mass,"global_strain_pct":{"min":-12.,"max":4.}},
        "surface":{"status":"available","result":{"steady":{"power_w":power}}},
        "coverage":{"status":"available","result":{"steady":{"power_w":power * 0.5}}}})
}
fn run(r: &Request, f: impl FnMut(&str, &Pattern) -> Result<Value, Refusal>) -> Value {
    serde_json::to_value(
        r.evaluate_with(&Pattern::from_json(PATTERN).unwrap(), &descriptor(), f)
            .unwrap(),
    )
    .unwrap()
}
#[test]
fn signed_heat_and_mass_tradeoffs_are_not_absolute_values_or_a_weighted_score() {
    let r = request();
    assert!(r.evaluate(&Pattern::from_json(PATTERN).unwrap()).is_err());
    let report = run(&r, |key, p| {
        Ok(if key == "a" {
            synthetic(p, -8., 0.2)
        } else {
            synthetic(p, -4., 0.1)
        })
    });
    assert_eq!(report["comparison"]["frontier"], json!(["a", "b"]));
    let a = &report["comparison"]["alternatives"][0];
    assert_eq!(a["readings"]["max_absolute_edge_strain_pct"]["value"], 12.);
    assert_eq!(a["readings"]["independent_surface_power_w"]["value"], -8.);
    assert_eq!(a["readings"]["independent_surface_power_w"]["unit"], "W");
    assert_eq!(a["eligible"], true);
    assert!(report["manufacturing"].get("comparison").is_none());
    let mut changed = r.clone();
    changed.comparison.as_mut().unwrap().objectives[0].direction = Direction::Maximize;
    let opposite = run(&changed, |key, p| {
        Ok(if key == "a" {
            synthetic(p, -8., 0.2)
        } else {
            synthetic(p, -4., 0.1)
        })
    });
    assert_eq!(opposite["comparison"]["frontier"], json!(["b"]));
    assert_ne!(
        report["comparison"]["context_sha256"],
        opposite["comparison"]["context_sha256"]
    );
    assert_eq!(
        report["manufacturing"]["alternatives"],
        opposite["manufacturing"]["alternatives"]
    );
    assert_eq!(report["simulations"], opposite["simulations"]);
}

#[test]
fn stale_missing_and_unsettled_outputs_never_enter_the_frontier_even_with_protocol_override() {
    let mut r = request();
    r.comparison.as_mut().unwrap().allow_mixed_protocols = true;
    for fault in [
        "pattern",
        "body",
        "scenario",
        "schema",
        "settled",
        "gating",
        "gating_flag",
        "missing_identity",
        "strain",
    ] {
        let report = run(&r, |key, p| {
            let mut v = synthetic(p, -8., 0.1);
            if key == "a" {
                match fault {
                    "pattern" => v["pattern_sha256"] = json!("wrong"),
                    "body" => v["body_geometry_sha256"] = json!("wrong"),
                    "scenario" => v["scenario_sha256"] = json!("wrong"),
                    "schema" => v["schema"] = json!("unrelated-result"),
                    "settled" => v["drape"]["settled"] = json!(false),
                    "gating" => v["drape"]["gating"] = json!(["penetration"]),
                    "gating_flag" => v["metrics"]["simulation_gating_clear"] = json!(false),
                    "missing_identity" => v["scenario_sha256"] = Value::Null,
                    "strain" => v["drape"]["global_strain_pct"]["min"] = json!(-13.),
                    _ => unreachable!(),
                }
            }
            Ok(v)
        });
        assert_eq!(report["comparison"]["frontier"], json!(["b"]), "{fault}");
        assert_eq!(report["comparison"]["alternatives"][0]["eligible"], false);
        assert!(
            !report["comparison"]["alternatives"][0]["exclusions"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            report["manufacturing"]["alternatives"][0]["cutting"]["status"],
            "available"
        );
    }
}

#[test]
fn required_heat_domains_are_independent_and_missing_power_is_not_zero() {
    let mut r = request();
    for fault in ["absent", "refused", "no_power", "no_coverage"] {
        let report = run(&r, |key, p| {
            let mut v = synthetic(p, 0., 0.1);
            if key == "a" {
                match fault {
                    "absent" => v["surface"] = json!({"status":"not_requested"}),
                    "refused" => {
                        v["surface"] = json!({"status":"refused","refusal":{"message":"outside qualified range"}})
                    }
                    "no_power" => v["surface"]["result"]["steady"]["power_w"] = Value::Null,
                    "no_coverage" => {
                        v["coverage"] = json!({"status":"refused","refusal":{"message":"unavailable layer geometry"}})
                    }
                    _ => unreachable!(),
                }
            }
            Ok(v)
        });
        assert_eq!(report["comparison"]["frontier"], json!(["b"]), "{fault}");
        assert_eq!(
            report["comparison"]["alternatives"][1]["readings"]["independent_surface_power_w"]["value"],
            0.
        );
    }
    // A failed unrequested coverage consumer must not block a surface-only objective.
    r.comparison.as_mut().unwrap().required_consumers.clear();
    let report = run(&r, |_, p| {
        let mut v = synthetic(p, -8., 0.1);
        v["coverage"] = json!({"status":"refused"});
        Ok(v)
    });
    assert_eq!(report["comparison"]["frontier"], json!(["a", "b"]));
    let mut manufacture_only = r;
    manufacture_only.comparison = Some(serde_json::from_value(json!({"basis":"nominal","objectives":[{"metric":"stock_mass_kg","direction":"minimize"}]})).unwrap());
    let report = run(&manufacture_only, |_, _| {
        Err(ftx_core::Refusal::new(
            ftx_core::RefusalCode::NotConverged,
            "synthetic failure",
        ))
    });
    assert_eq!(report["comparison"]["frontier"], json!(["a", "b"]));
}

#[test]
fn source_protocol_changes_and_missing_descriptors_are_not_silently_comparable() {
    let mut r = request();
    r.variants[0]
        .changes
        .default
        .as_mut()
        .unwrap()
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("thermal_conductivity")
        .unwrap()
        .conditions
        .insert("conditioning".into(), "different".into());
    let report = run(&r, |key, p| {
        Ok(synthetic(p, if key == "a" { -8. } else { -4. }, 0.1))
    });
    assert_eq!(report["comparison"]["frontier"], json!(["a", "b"]));
    assert_eq!(
        report["comparison"]["incomparable"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    r.comparison.as_mut().unwrap().allow_mixed_protocols = true;
    let report = run(&r, |key, p| {
        Ok(synthetic(p, if key == "a" { -8. } else { -4. }, 0.1))
    });
    assert_eq!(report["comparison"]["frontier"], json!(["a"]));
    let p = Pattern::from_json(PATTERN).unwrap();
    let report = r
        .evaluate_with(&p, &json!({}), |_, p| Ok(synthetic(p, -8., 0.1)))
        .unwrap();
    assert!(report.comparison.unwrap().frontier.is_empty());
}

fn digest(value: &Value) -> String {
    sha256_hex(&serde_json::to_vec(value).unwrap())
}

fn motion_descriptor() -> Value {
    let mut d = descriptor();
    d["motion_path"] = json!({"sha256":"3".repeat(64)});
    d["scenario"] = json!({"motion":{"schema":"ftx-drape-motion/1","max_trials":4,
        "keyframes":[{"time_s":0.5,"arm_abduction_deg":40.},
            {"time_s":1.,"arm_abduction_deg":45.}]}});
    d
}

fn motion_request() -> Request {
    let mut r = request();
    r.comparison = Some(
        serde_json::from_value(json!({"basis":"nominal", "objectives":[
        {"metric":"motion_max_absolute_edge_strain_pct","direction":"minimize"}],
        "constraints":[{"metric":"motion_max_absolute_edge_strain_pct","maximum":10.}]}))
        .unwrap(),
    );
    r
}

// Explicit synthetic callback: only admission/selection is tested here. Production continuation
// and native/WASM equality have separate solver-backed tests, not relabeled measured evidence.
fn synthetic_motion(p: &Pattern, strain: f64) -> Value {
    let mut v = synthetic(p, -4., 0.1);
    let x = json!([[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]]);
    let triangles = json!([[0, 1, 2]]);
    let d = motion_descriptor();
    let samples: Vec<_> = d["scenario"]["motion"]["keyframes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            json!({"time_s":f["time_s"],"positions_m":x,"body_positions_m":x,
            "solve":{"converged":true}})
        })
        .collect();
    let endpoints: Vec<_> = d["scenario"]["motion"]["keyframes"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(i, f)| {
            json!({"time_s":f["time_s"],"arm_abduction_deg":f["arm_abduction_deg"],
            "sample_index":i,"positions_sha256":digest(&x),
            "body_collision_sha256":digest(&json!([x,triangles]))})
        })
        .collect();
    v["positions_sha256"] = json!(digest(&x));
    v["motion"] = json!({"status":"available","result":{
        "schema":"ftx-drape-motion-result/1","path_sha256":d["motion_path"]["sha256"],
        "initial_positions_sha256":digest(&x),"initial_drape_settled":true,
        "initial_drape_gating":[],"continuous_contact_checked":true,"duration_s":1.,
        "trials":2,"samples":samples,"keyframes":endpoints,"body_triangles":triangles,
        "metrics":{"max_absolute_edge_strain_pct":strain}}});
    v
}

fn run_motion(r: &Request, f: impl FnMut(&str, &Pattern) -> Result<Value, Refusal>) -> Value {
    serde_json::to_value(
        r.evaluate_with(
            &Pattern::from_json(PATTERN).unwrap(),
            &motion_descriptor(),
            f,
        )
        .unwrap(),
    )
    .unwrap()
}

#[test]
fn motion_objective_is_not_static_strain_and_can_qualify_manufacturing_frontiers() {
    let mut r = motion_request();
    let report = run_motion(&r, |key, p| {
        Ok(synthetic_motion(p, if key == "a" { 5. } else { 10. }))
    });
    assert_eq!(report["comparison"]["motion_required"], true);
    assert_eq!(report["comparison"]["frontier"], json!(["a"]));
    assert_eq!(
        report["comparison"]["alternatives"][1]["readings"]["motion_max_absolute_edge_strain_pct"]
            ["value"],
        10.
    );
    assert_eq!(
        report["comparison"]["alternatives"][0]["readings"]["motion_max_absolute_edge_strain_pct"]
            ["unit"],
        "%"
    );
    assert_eq!(
        report["simulations"][0]["evaluation"]["result"]["drape"]["global_strain_pct"]["min"],
        -12.
    );
    r.comparison = Some(
        serde_json::from_value(json!({"basis":"nominal","objectives":[
        {"metric":"stock_area_m2","direction":"minimize"}],"required_consumers":["motion"]}))
        .unwrap(),
    );
    let failed = run_motion(&r, |key, p| {
        let mut s = synthetic_motion(p, 5.);
        if key == "a" {
            s["motion"] = json!({"status":"refused"});
        }
        Ok(s)
    });
    assert_eq!(failed["comparison"]["frontier"], json!(["b"]));
    assert_eq!(
        failed["manufacturing"]["alternatives"][0]["cutting"]["status"],
        "available"
    );
    r.comparison.as_mut().unwrap().required_consumers.clear();
    let static_only = run_motion(&r, |_, p| {
        let mut s = synthetic_motion(p, 5.);
        s["motion"] = json!({"status":"refused"});
        Ok(s)
    });
    assert_eq!(static_only["comparison"]["motion_required"], false);
    assert_eq!(static_only["comparison"]["frontier"], json!(["a", "b"]));
}

#[test]
fn partial_stale_and_unchecked_motion_cannot_win_even_with_protocol_override() {
    let mut r = motion_request();
    r.comparison.as_mut().unwrap().allow_mixed_protocols = true;
    for fault in [
        "missing",
        "schema",
        "path",
        "initial",
        "settled",
        "gating",
        "contact",
        "duration",
        "last_sample",
        "time_order",
        "convergence",
        "endpoint_count",
        "index",
        "pose",
        "cloth",
        "body",
        "trials",
        "strain",
    ] {
        let report = run_motion(&r, |key, p| {
            let mut s = synthetic_motion(p, 0.);
            if key == "a" {
                let m = &mut s["motion"]["result"];
                match fault {
                    "missing" => *m = Value::Null,
                    "schema" => m["schema"] = json!("wrong"),
                    "path" => m["path_sha256"] = json!("old-path"),
                    "initial" => m["initial_positions_sha256"] = json!("old-drape"),
                    "settled" => m["initial_drape_settled"] = json!(false),
                    "gating" => m["initial_drape_gating"] = json!(["penetration"]),
                    "contact" => m["continuous_contact_checked"] = json!(false),
                    "duration" => m["duration_s"] = json!(0.5),
                    "last_sample" => {
                        m["samples"].as_array_mut().unwrap().pop();
                    }
                    "time_order" => m["samples"][0]["time_s"] = json!(1.),
                    "convergence" => m["samples"][0]["solve"]["converged"] = json!(false),
                    "endpoint_count" => {
                        m["keyframes"].as_array_mut().unwrap().pop();
                    }
                    "index" => m["keyframes"][1]["sample_index"] = json!(0),
                    "pose" => m["keyframes"][1]["arm_abduction_deg"] = json!(44.),
                    "cloth" => m["keyframes"][1]["positions_sha256"] = json!("stale"),
                    "body" => m["keyframes"][1]["body_collision_sha256"] = json!("stale"),
                    "trials" => m["trials"] = json!(0),
                    "strain" => m["metrics"]["max_absolute_edge_strain_pct"] = Value::Null,
                    _ => unreachable!(),
                }
            }
            Ok(s)
        });
        assert_eq!(report["comparison"]["frontier"], json!(["b"]), "{fault}");
        assert!(
            !report["comparison"]["alternatives"][0]["exclusions"]
                .as_array()
                .unwrap()
                .is_empty(),
            "{fault}"
        );
        assert_eq!(
            report["manufacturing"]["alternatives"][0]["cutting"]["status"],
            "available"
        );
    }
}

#[test]
fn unrequested_motion_or_failed_path_never_qualifies_a_scenario() {
    let r = motion_request();
    let p = Pattern::from_json(PATTERN).unwrap();
    assert!(r.evaluate(&p).is_err());
    let unrequested = r
        .evaluate_with(&p, &descriptor(), |_, p| Ok(synthetic_motion(p, 5.)))
        .unwrap();
    assert!(unrequested.comparison.unwrap().frontier.is_empty());
    // A requested motion whose path preparation failed still returns useful manufacturing.
    let mut d = motion_descriptor();
    d["motion_path"] = json!({"refusal":{"message":"bad topology"}});
    let result = r
        .evaluate_with(&p, &d, |_, p| Ok(synthetic_motion(p, 5.)))
        .unwrap();
    assert!(result.comparison.unwrap().frontier.is_empty());
    assert!(result.manufacturing.alternatives.iter().all(|r| matches!(
        r.cutting,
        ftx_export::material_study::Outcome::Available { .. }
    )));
}

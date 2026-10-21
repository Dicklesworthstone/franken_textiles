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

fn heat_descriptor() -> Value {
    let mut d = motion_descriptor();
    let film = json!({"inside_c":-5.,"outside_c":-15.,
        "inside_resistance_m2_k_w":0.1,"outside_resistance_m2_k_w":0.2});
    for (name, schema) in [
        ("transient_surface", "ftx-motion-surface-heat/1"),
        ("transient_coverage", "ftx-motion-coverage-heat/1"),
    ] {
        d["scenario"]["motion"][name] = json!({"schema":schema,
            "initial_temperature_c":-12.,"boundaries":[film,film],
            "max_step_s":0.25,"max_cell_steps":100});
    }
    d["scenario"]["motion"]["transient_surface"]["cells_per_layer"] = json!(2);
    d["scenario"]["motion"]["transient_coverage"]["exposed_boundaries"] = json!([null, null]);
    d
}

fn heat_request() -> Request {
    let mut r = motion_request();
    r.comparison = Some(
        serde_json::from_value(json!({"basis":"nominal","objectives":[
        {"metric":"transient_surface_inside_energy_j","direction":"minimize"},
        {"metric":"transient_coverage_inside_energy_j","direction":"minimize"}],
        "constraints":[
            {"metric":"transient_surface_min_sample_temperature_c","minimum":-12.},
            {"metric":"transient_surface_max_sample_temperature_c","maximum":-2.},
            {"metric":"transient_coverage_min_sample_temperature_c","minimum":-12.},
            {"metric":"transient_coverage_max_sample_temperature_c","maximum":-2.}]}))
        .unwrap(),
    );
    // Qualifiers are synthetic, equal across alternatives, and present independently of values.
    for v in &mut r.variants {
        v.changes
            .default
            .as_mut()
            .unwrap()
            .record
            .as_mut()
            .unwrap()
            .properties
            .insert(
                "specific_heat".into(),
                serde_json::from_value(json!({"value":1000.,"unit":"J/(kg*K)",
                "evidence":"estimate","source":"test","method":"synthetic heat comparison"}))
                .unwrap(),
            );
    }
    r
}

fn synthetic_heat(p: &Pattern, inside_j: f64, coverage_j: f64) -> Value {
    let mut s = synthetic_motion(p, 5.);
    let d = heat_descriptor();
    for (name, schema, energy) in [
        (
            "transient_surface",
            "ftx-motion-surface-heat-result/1",
            inside_j,
        ),
        (
            "transient_coverage",
            "ftx-motion-coverage-heat-result/1",
            coverage_j,
        ),
    ] {
        let motion = &s["motion"]["result"];
        let trace: Vec<_> = motion["samples"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(i, sample)| {
                json!({"sample_index":i,"interval_index":i,"time_s":sample["time_s"],
                "thermal_substeps":2,"temperature_range_c":[-8.,if i==0 {-4.} else {-2.}],
                "positions_sha256":digest(&sample["positions_m"]),
                "body_positions_sha256":digest(&sample["body_positions_m"])})
            })
            .collect();
        let h = json!({"schema":schema,"request":d["scenario"]["motion"][name],
            "path_sha256":motion["path_sha256"],"initial_positions_sha256":motion["initial_positions_sha256"],
            "duration_s":motion["duration_s"],"cell_steps":8,"thermal_steps":4,
            "inside_energy_j":energy,"outside_energy_j":energy,"net_boundary_energy_j":0.,
            "stored_change_j":0.,"physical_stored_change_j":0.,"balance_error_j":0.,
            "absolute_step_balance_error_j":0.,"bare_energy_j":3.,
            "signed_remap_residual_j":1e-9,"absolute_remap_residual_j":1e-9,"trace":trace});
        s["motion"]["result"][name] = json!({"status":"available","result":h});
    }
    s
}

fn run_heat(r: &Request, f: impl FnMut(&str, &Pattern) -> Result<Value, Refusal>) -> Value {
    serde_json::to_value(
        r.evaluate_with(&Pattern::from_json(PATTERN).unwrap(), &heat_descriptor(), f)
            .unwrap(),
    )
    .unwrap()
}

#[test]
fn transient_objectives_preserve_energy_signs_domains_and_initial_inclusive_temperature_extrema() {
    let mut r = heat_request();
    let report = run_heat(&r, |key, p| {
        Ok(if key == "a" {
            synthetic_heat(p, -8., 6.)
        } else {
            synthetic_heat(p, -4., 2.)
        })
    });
    assert_eq!(report["comparison"]["motion_required"], true);
    assert_eq!(report["comparison"]["frontier"], json!(["a", "b"]));
    let readings = &report["comparison"]["alternatives"][0]["readings"];
    assert_eq!(readings["transient_surface_inside_energy_j"]["value"], -8.);
    assert_eq!(readings["transient_surface_inside_energy_j"]["unit"], "J");
    assert_eq!(readings["transient_coverage_inside_energy_j"]["value"], 6.);
    assert_eq!(
        readings["transient_surface_min_sample_temperature_c"]["value"],
        -12.
    );
    assert_eq!(
        readings["transient_surface_max_sample_temperature_c"]["value"],
        -2.
    );
    assert_eq!(
        readings["transient_coverage_min_sample_temperature_c"]["unit"],
        "C"
    );
    assert_eq!(
        readings["transient_coverage_max_sample_temperature_c"]["value"],
        -2.
    );
    r.comparison.as_mut().unwrap().objectives[0].direction = Direction::Maximize;
    let changed = run_heat(&r, |key, p| {
        Ok(if key == "a" {
            synthetic_heat(p, -8., 6.)
        } else {
            synthetic_heat(p, -4., 2.)
        })
    });
    assert_eq!(changed["comparison"]["frontier"], json!(["b"]));
    assert_eq!(changed["simulations"], report["simulations"]);
    assert_eq!(
        changed["manufacturing"]["alternatives"],
        report["manufacturing"]["alternatives"]
    );
    r.comparison.as_mut().unwrap().constraints[1].maximum = Some(-3.);
    let constrained = run_heat(&r, |_, p| Ok(synthetic_heat(p, -8., 6.)));
    assert_eq!(constrained["comparison"]["frontier"], json!([]));
    // Initial temperature is included even when it exceeds every retained endpoint temperature.
    let mut d = heat_descriptor();
    d["scenario"]["motion"]["transient_surface"]["initial_temperature_c"] = json!(10.);
    r.comparison.as_mut().unwrap().constraints.clear();
    r.comparison.as_mut().unwrap().objectives = serde_json::from_value(json!([
        {"metric":"transient_surface_max_sample_temperature_c","direction":"minimize"}]))
    .unwrap();
    let result = r
        .evaluate_with(&Pattern::from_json(PATTERN).unwrap(), &d, |_, p| {
            let mut s = synthetic_heat(p, 0., 0.);
            s["motion"]["result"]["transient_surface"]["result"]["request"] =
                d["scenario"]["motion"]["transient_surface"].clone();
            Ok(s)
        })
        .unwrap();
    assert_eq!(
        serde_json::to_value(result).unwrap()["comparison"]["alternatives"][0]["readings"]["transient_surface_max_sample_temperature_c"]
            ["value"],
        10.
    );
}

#[test]
fn partial_stale_or_invalid_thermal_histories_cannot_win_or_erase_independent_results() {
    let mut r = heat_request();
    r.comparison.as_mut().unwrap().allow_mixed_protocols = true;
    for name in ["transient_surface", "transient_coverage"] {
        for fault in [
            "status",
            "policy",
            "path",
            "initial",
            "duration",
            "trace",
            "index",
            "interval",
            "time",
            "geometry",
            "range",
            "reversed_range",
            "absolute_zero",
            "steps",
            "cells",
            "energy",
        ] {
            let result = run_heat(&r, |key, p| {
                let mut s = synthetic_heat(p, -100., -100.);
                if key == "a" {
                    let h = &mut s["motion"]["result"][name]["result"];
                    match fault {
                        "status" => *h = Value::Null,
                        "policy" => h["request"]["initial_temperature_c"] = json!(99.),
                        "path" => h["path_sha256"] = json!("old"),
                        "initial" => h["initial_positions_sha256"] = json!("old"),
                        "duration" => h["duration_s"] = json!(0.5),
                        "trace" => {
                            h["trace"].as_array_mut().unwrap().pop();
                        }
                        "index" => h["trace"][1]["sample_index"] = json!(0),
                        "interval" => h["trace"][1]["interval_index"] = json!(0),
                        "time" => h["trace"][1]["time_s"] = json!(0.7),
                        "geometry" => {
                            h["trace"][1][if name == "transient_coverage" {
                                "body_positions_sha256"
                            } else {
                                "positions_sha256"
                            }] = json!("old")
                        }
                        "range" => h["trace"][1]["temperature_range_c"] = Value::Null,
                        "reversed_range" => h["trace"][1]["temperature_range_c"] = json!([10., 5.]),
                        "absolute_zero" => {
                            h["trace"][1]["temperature_range_c"] = json!([-300., 5.])
                        }
                        "steps" => h["thermal_steps"] = json!(3),
                        "cells" => h["cell_steps"] = json!(101),
                        "energy" => h["inside_energy_j"] = Value::Null,
                        _ => unreachable!(),
                    }
                }
                Ok(s)
            });
            assert_eq!(
                result["comparison"]["frontier"],
                json!(["b"]),
                "{name}/{fault}"
            );
            let other = if name == "transient_surface" {
                "transient_coverage"
            } else {
                "transient_surface"
            };
            assert!(
                result["comparison"]["alternatives"][0]["readings"]
                    [format!("{other}_inside_energy_j")]
                .is_object()
            );
            assert_eq!(
                result["simulations"][0]["evaluation"]["result"]["motion"]["status"],
                "available"
            );
            assert_eq!(
                result["manufacturing"]["alternatives"][0]["cutting"]["status"],
                "available"
            );
        }
    }
    r.comparison.as_mut().unwrap().constraints.clear();
    r.comparison.as_mut().unwrap().objectives.truncate(1);
    let surface_only = run_heat(&r, |_, p| {
        let mut s = synthetic_heat(p, 0., 0.);
        s["motion"]["result"]["transient_coverage"] = json!({"status":"refused"});
        Ok(s)
    });
    assert_eq!(surface_only["comparison"]["frontier"], json!(["a", "b"]));
    assert_eq!(
        surface_only["comparison"]["alternatives"][0]["readings"]["transient_surface_inside_energy_j"]
            ["value"],
        0.
    );
    r.comparison.as_mut().unwrap().required_consumers =
        serde_json::from_value(json!(["transient_coverage"])).unwrap();
    let required = run_heat(&r, |_, p| {
        let mut s = synthetic_heat(p, 0., 0.);
        s["motion"]["result"]["transient_coverage"] = json!({"status":"refused"});
        Ok(s)
    });
    assert_eq!(required["comparison"]["frontier"], json!([]));
}

#[test]
fn heat_capacity_protocols_are_compared_and_missing_thermal_policy_cannot_qualify() {
    let mut r = heat_request();
    r.variants[0]
        .changes
        .default
        .as_mut()
        .unwrap()
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("specific_heat")
        .unwrap()
        .method = "Different characterization".into();
    let result = run_heat(&r, |key, p| {
        Ok(synthetic_heat(p, if key == "a" { -8. } else { -4. }, 0.))
    });
    assert_eq!(result["comparison"]["frontier"], json!(["a", "b"]));
    assert_eq!(
        result["comparison"]["incomparable"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    r.comparison.as_mut().unwrap().allow_mixed_protocols = true;
    let result = run_heat(&r, |key, p| {
        Ok(synthetic_heat(p, if key == "a" { -8. } else { -4. }, 0.))
    });
    assert_eq!(result["comparison"]["frontier"], json!(["a"]));
    let mut d = heat_descriptor();
    d["scenario"]["motion"]["transient_surface"] = Value::Null;
    let result = r
        .evaluate_with(&Pattern::from_json(PATTERN).unwrap(), &d, |_, p| {
            Ok(synthetic_heat(p, 0., 0.))
        })
        .unwrap();
    assert!(result.comparison.unwrap().frontier.is_empty());
}

#[test]
fn net_boundary_energy_keeps_signed_domains_without_substituting_incoming_heat_or_remap_error() {
    let mut r = heat_request();
    r.comparison.as_mut().unwrap().objectives = serde_json::from_value(json!([
        {"metric":"transient_surface_net_boundary_energy_j","direction":"minimize"},
        {"metric":"transient_coverage_net_boundary_energy_j","direction":"minimize"}]))
    .unwrap();
    let evaluate = |key: &str, p: &Pattern| {
        let mut s = synthetic_heat(p, -100., 100.);
        for (name, stored, net) in [
            (
                "transient_surface",
                "stored_change_j",
                if key == "a" { -3. } else { -1. },
            ),
            (
                "transient_coverage",
                "physical_stored_change_j",
                if key == "a" { 7. } else { 2. },
            ),
        ] {
            let h = &mut s["motion"]["result"][name]["result"];
            h["net_boundary_energy_j"] = json!(net);
            h[stored] = json!(net);
            h["outside_energy_j"] = json!(h["inside_energy_j"].as_f64().unwrap() - net);
        }
        Ok(s)
    };
    let report = run_heat(&r, evaluate);
    assert_eq!(report["comparison"]["frontier"], json!(["a", "b"]));
    let readings = &report["comparison"]["alternatives"][0]["readings"];
    assert_eq!(
        readings["transient_surface_net_boundary_energy_j"]["value"],
        -3.
    );
    assert_eq!(
        readings["transient_coverage_net_boundary_energy_j"]["value"],
        7.
    );
    assert_eq!(
        readings["transient_surface_net_boundary_energy_j"]["unit"],
        "J"
    );
    r.comparison.as_mut().unwrap().objectives[0].direction = Direction::Maximize;
    assert_eq!(
        run_heat(&r, evaluate)["comparison"]["frontier"],
        json!(["b"])
    );
    for name in ["transient_surface", "transient_coverage"] {
        let report = run_heat(&r, |key, p| {
            let mut s = evaluate(key, p)?;
            if key == "a" {
                s["motion"]["result"][name]["result"]["net_boundary_energy_j"] = Value::Null;
            }
            Ok(s)
        });
        assert_eq!(report["comparison"]["frontier"], json!(["b"]));
        assert!(
            report["comparison"]["alternatives"][0]["readings"]
                [format!("{name}_net_boundary_energy_j")]
            .is_null()
        );
        let other = if name == "transient_surface" {
            "transient_coverage"
        } else {
            "transient_surface"
        };
        assert!(
            report["comparison"]["alternatives"][0]["readings"]
                [format!("{other}_net_boundary_energy_j")]
            .is_object()
        );
    }
}

#[test]
fn final_temperature_objectives_use_the_last_sample_without_changing_initial_inclusive_extrema() {
    let mut r = heat_request();
    r.comparison.as_mut().unwrap().objectives = serde_json::from_value(json!([
        {"metric":"transient_surface_final_min_temperature_c","direction":"maximize"},
        {"metric":"transient_surface_final_max_temperature_c","direction":"minimize"},
        {"metric":"transient_coverage_final_min_temperature_c","direction":"maximize"},
        {"metric":"transient_coverage_final_max_temperature_c","direction":"minimize"}]))
    .unwrap();
    let evaluate = |key: &str, p: &Pattern| {
        let mut s = synthetic_heat(p, 0., 0.);
        for name in ["transient_surface", "transient_coverage"] {
            s["motion"]["result"][name]["result"]["trace"][1]["temperature_range_c"] = if key == "a"
            {
                json!([-6., -3.])
            } else {
                json!([-7., -5.])
            };
        }
        Ok(s)
    };
    let report = run_heat(&r, evaluate);
    assert_eq!(report["comparison"]["frontier"], json!(["a", "b"]));
    for name in ["transient_surface", "transient_coverage"] {
        let a = &report["comparison"]["alternatives"][0]["readings"];
        let b = &report["comparison"]["alternatives"][1]["readings"];
        assert_eq!(a[format!("{name}_final_min_temperature_c")]["value"], -6.);
        assert_eq!(a[format!("{name}_final_max_temperature_c")]["value"], -3.);
        assert_eq!(b[format!("{name}_final_min_temperature_c")]["value"], -7.);
        assert_eq!(b[format!("{name}_final_max_temperature_c")]["value"], -5.);
        assert_eq!(a[format!("{name}_final_min_temperature_c")]["unit"], "C");
        assert_eq!(a[format!("{name}_min_sample_temperature_c")]["value"], -12.);
        assert_eq!(b[format!("{name}_max_sample_temperature_c")]["value"], -4.);
    }
    r.comparison.as_mut().unwrap().constraints.push(
        serde_json::from_value(
            json!({"metric":"transient_coverage_final_max_temperature_c","maximum":-4.}),
        )
        .unwrap(),
    );
    assert_eq!(
        run_heat(&r, evaluate)["comparison"]["frontier"],
        json!(["b"])
    );
}

fn plastic_heat_request() -> Request {
    let mut r = heat_request();
    r.comparison.as_mut().unwrap().constraints.clear();
    r.comparison.as_mut().unwrap().objectives = serde_json::from_value(json!([
        {"metric":"transient_surface_final_max_temperature_c","direction":"minimize"},
        {"metric":"transient_coverage_inside_energy_j","direction":"minimize"}]))
    .unwrap();
    for (i, variant) in r.variants.iter_mut().enumerate() {
        let mut selection: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/materials/synthetic-membrane-plasticity.json"
        ))
        .unwrap();
        selection["record"]["properties"]["membrane_plastic_heat_fraction"] = json!({
            "value":if i == 0 {0.} else {0.8},"unit":"1","source":"test","evidence":"estimate",
            "method":"Synthetic irreversible-yield conversion, not measured textile data",
            "conditions":{"specimen_id":"artificial-stretch-A","conditioning":"synthetic constant dry state",
                "work_basis":"irreversible_membrane_plastic_dissipation",
                "temperature_min_c":"-20","temperature_max_c":"80"}});
        variant.changes.default = Some(serde_json::from_value(selection).unwrap());
    }
    r
}

fn plastic_heat_descriptor() -> Value {
    let mut d = heat_descriptor();
    d["scenario"]["motion"]["transient_surface"]["plastic_dissipation_heat"] =
        json!("uniform_reference_thickness");
    d["scenario"]["motion"]["transient_surface"]["max_cell_steps"] = json!(5_000_000);
    d
}

// Source-qualified synthetic callback accounting. This deliberately does not claim a solver result;
// the real body-model/native/WASM scenario exercises the actual conversion independently.
fn synthetic_plastic_heat(p: &Pattern, d: &Value) -> Value {
    synthetic_plastic_heat_for(p, d, "transient_surface")
}

fn synthetic_plastic_heat_for(p: &Pattern, d: &Value, name: &str) -> Value {
    use ftx_pattern::materials::{CATALOG_SCHEMA, Catalog};
    let selection = serde_json::from_value(p.raw()["fabric"]["material"].clone()).unwrap();
    let material = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: vec![],
    }
    .resolve(&selection)
    .unwrap();
    let fraction = material.membrane_plastic_heat_fraction().unwrap().unwrap();
    let law = material
        .membrane_plasticity()
        .unwrap()
        .unwrap()
        .fingerprint();
    let mut s = synthetic_heat(p, -8., 0.);
    let m = &mut s["motion"]["result"];
    m["triangles"] = json!([[0, 1, 2]]);
    m["initial_sample"] = json!({"time_s":0.,"membrane_plastic_dissipation_j":[2.]});
    m["initial_membrane_plasticity_history"] = json!({"plastic_dissipation_j":2.,
        "elements":[{"triangle":0,"vertices":[0,1,2],"law_sha256":law,"plastic_dissipation_j":2.}]});
    m["membrane_plasticity_history"] = json!({"plastic_dissipation_j":5.});
    let snapshot = |work: f64| {
        json!({"distribution":"uniform_reference_thickness",
        "temporal_distribution":"uniform_over_accepted_interval","plastic_dissipation_j":work,
        "supplied_heat_j":fraction.fraction*work,"unconverted_dissipation_j":(1.-fraction.fraction)*work,
        "initial_plastic_dissipation_j":2.})
    };
    let stored = if name == "transient_coverage" {
        "physical_stored_change_j"
    } else {
        "stored_change_j"
    };
    for (i, work) in [1., 3.].into_iter().enumerate() {
        m["samples"][i]["membrane_plastic_dissipation_j"] = json!([2. + work]);
        let h = &mut m[name]["result"]["trace"][i];
        h["plastic_dissipation_heat"] = snapshot(work);
        h["inside_energy_j"] = json!(-4. * (i + 1) as f64);
        h["outside_energy_j"] = h["inside_energy_j"].clone();
        h["net_boundary_energy_j"] = json!(0.);
        h[stored] = json!(fraction.fraction * work);
        h["balance_error_j"] = json!(0.);
        h["temperature_range_c"] = json!([-8., -2. + fraction.fraction * work]);
    }
    let h = &mut m[name]["result"];
    h["request"] = d["scenario"]["motion"][name].clone();
    h["cell_steps"] = json!(1032);
    h["inside_energy_j"] = json!(-8.);
    h["outside_energy_j"] = json!(-8.);
    h[stored] = json!(fraction.fraction * 3.);
    h["plastic_dissipation_heat"] = snapshot(3.);
    h["plastic_dissipation_heat"]["material_fractions"] = json!([{
        "material_sha256":material.sha256(),"law_sha256":law,"fraction":fraction.fraction,
        "temperature_range_c":fraction.temperature_range_c,
        "observation":material.authored().properties["membrane_plastic_heat_fraction"]}]);
    s
}

#[test]
fn plastic_heat_protocols_compare_fractions_only_under_the_consumed_policy() {
    let mut r = plastic_heat_request();
    let d = plastic_heat_descriptor();
    let evaluate = |r: &Request| {
        serde_json::to_value(
            r.evaluate_with(&Pattern::from_json(PATTERN).unwrap(), &d, |_, p| {
                Ok(synthetic_plastic_heat(p, &d))
            })
            .unwrap(),
        )
        .unwrap()
    };
    let result = evaluate(&r);
    assert_eq!(result["comparison"]["frontier"], json!(["a"]));
    assert_eq!(result["comparison"]["incomparable"], json!([]));
    for qualifier in ["method", "conditions"] {
        let mut different = plastic_heat_request();
        let property = different.variants[1]
            .changes
            .default
            .as_mut()
            .unwrap()
            .record
            .as_mut()
            .unwrap()
            .properties
            .get_mut("membrane_plastic_heat_fraction")
            .unwrap();
        if qualifier == "method" {
            property.method = "A different characterization procedure".into();
        } else {
            property
                .conditions
                .insert("temperature_max_c".into(), "85".into());
        }
        let result = evaluate(&different);
        assert_eq!(
            result["comparison"]["frontier"],
            json!(["a", "b"]),
            "{qualifier}"
        );
        assert_eq!(
            result["comparison"]["incomparable"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        different.comparison.as_mut().unwrap().allow_mixed_protocols = true;
        assert_eq!(evaluate(&different)["comparison"]["frontier"], json!(["a"]));
        r = different;
        r.comparison.as_mut().unwrap().allow_mixed_protocols = false;
    }
    // An unconsumed fraction observation must not change ordinary uncoupled thermal ordering.
    r.comparison.as_mut().unwrap().objectives = serde_json::from_value(json!([
        {"metric":"transient_surface_inside_energy_j","direction":"minimize"}]))
    .unwrap();
    let uncoupled = run_heat(&r, |key, p| {
        Ok(synthetic_heat(p, if key == "a" { -8. } else { -4. }, 0.))
    });
    assert_eq!(uncoupled["comparison"]["frontier"], json!(["a"]));
    assert_eq!(uncoupled["comparison"]["incomparable"], json!([]));
    // The scenario may request heat while the actual comparison consumes only motion.
    r.comparison = motion_request().comparison;
    let motion_only = serde_json::to_value(
        r.evaluate_with(&Pattern::from_json(PATTERN).unwrap(), &d, |key, p| {
            Ok(synthetic_motion(p, if key == "a" { 3. } else { 6. }))
        })
        .unwrap(),
    )
    .unwrap();
    assert_eq!(motion_only["comparison"]["frontier"], json!(["a"]));
}

#[test]
fn inconsistent_plastic_heat_cannot_enter_a_frontier_or_erase_independent_coverage() {
    let mut r = plastic_heat_request();
    r.comparison.as_mut().unwrap().allow_mixed_protocols = true;
    let d = plastic_heat_descriptor();
    for fault in [
        "receipt",
        "material",
        "law",
        "fraction",
        "observation",
        "duplicate",
        "initial_time",
        "initial_total",
        "initial_element",
        "sample",
        "regression",
        "shape",
        "trace",
        "trace_work",
        "trace_heat",
        "distribution",
        "summary",
        "stored",
        "boundary",
        "final_total",
    ] {
        let result = serde_json::to_value(r.evaluate_with(&Pattern::from_json(PATTERN).unwrap(), &d,
            |key,p| {
                let mut s = synthetic_plastic_heat(p, &d);
                if key == "a" {
                    let m = &mut s["motion"]["result"];
                    match fault {
                        "initial_time" => m["initial_sample"]["time_s"] = json!(0.5),
                        "initial_total" => m["initial_membrane_plasticity_history"]["plastic_dissipation_j"] = json!(1.),
                        "initial_element" => m["initial_membrane_plasticity_history"]["elements"][0]["law_sha256"] = json!("stale"),
                        "sample" => m["samples"][0]["membrane_plastic_dissipation_j"] = Value::Null,
                        "regression" => m["samples"][1]["membrane_plastic_dissipation_j"] = json!([2.5]),
                        "shape" => m["samples"][0]["membrane_plastic_dissipation_j"] = json!([3.,0.]),
                        "final_total" => m["membrane_plasticity_history"]["plastic_dissipation_j"] = json!(6.),
                        _ => {
                            let h = &mut m["transient_surface"]["result"];
                            match fault {
                                "receipt" => h["plastic_dissipation_heat"]["material_fractions"] = Value::Null,
                                "material" => h["plastic_dissipation_heat"]["material_fractions"][0]["material_sha256"] = json!("unbound"),
                                "law" => h["plastic_dissipation_heat"]["material_fractions"][0]["law_sha256"] = json!("stale"),
                                "fraction" => h["plastic_dissipation_heat"]["material_fractions"][0]["fraction"] = json!(0.5),
                                "observation" => h["plastic_dissipation_heat"]["material_fractions"][0]["observation"]["method"] = json!("unrelated source"),
                                "duplicate" => { let receipt = h["plastic_dissipation_heat"]["material_fractions"][0].clone();
                                    h["plastic_dissipation_heat"]["material_fractions"].as_array_mut().unwrap().push(receipt); }
                                "trace" => h["trace"][0]["plastic_dissipation_heat"] = Value::Null,
                                "trace_work" => h["trace"][0]["plastic_dissipation_heat"]["plastic_dissipation_j"] = json!(3.),
                                "trace_heat" => h["trace"][0]["plastic_dissipation_heat"]["supplied_heat_j"] = json!(0.5),
                                "distribution" => h["plastic_dissipation_heat"]["temporal_distribution"] = json!("instantaneous"),
                                "summary" => h["plastic_dissipation_heat"]["initial_plastic_dissipation_j"] = json!(0.),
                                "stored" => h["stored_change_j"] = json!(1.),
                                "boundary" => h["trace"][0]["outside_energy_j"] = json!(0.),
                                _ => unreachable!(),
                            }
                        }
                    }
                }
                Ok(s)
            }).unwrap()).unwrap();
        assert_eq!(result["comparison"]["frontier"], json!(["b"]), "{fault}");
        assert!(result["comparison"]["alternatives"][0]["readings"]
            ["transient_surface_final_max_temperature_c"].is_null(), "{fault}");
        assert!(result["comparison"]["alternatives"][0]["readings"]
            ["transient_coverage_inside_energy_j"].is_object(), "{fault}");
        assert_eq!(
            result["simulations"][0]["evaluation"]["result"]["motion"]["status"],
            "available"
        );
        assert_eq!(
            result["manufacturing"]["alternatives"][0]["cutting"]["status"],
            "available"
        );
    }
}

#[test]
fn plastic_heat_requires_an_explicit_supported_policy_in_the_consumed_domain() {
    let r = plastic_heat_request();
    for policy in ["unknown", "unrequested", "coverage"] {
        let mut d = plastic_heat_descriptor();
        let affected = if policy == "coverage" {
            "transient_coverage"
        } else {
            "transient_surface"
        };
        d["scenario"]["motion"][affected]["plastic_dissipation_heat"] = match policy {
            "unknown" => json!("unsupported_distribution"),
            "unrequested" => Value::Null,
            _ => json!("uniform_reference_thickness"),
        };
        let result = serde_json::to_value(
            r.evaluate_with(&Pattern::from_json(PATTERN).unwrap(), &d, |_, p| {
                let mut s = synthetic_plastic_heat(p, &d);
                s["motion"]["result"][affected]["result"]["request"] =
                    d["scenario"]["motion"][affected].clone();
                Ok(s)
            })
            .unwrap(),
        )
        .unwrap();
        assert_eq!(result["comparison"]["frontier"], json!([]), "{policy}");
        let independent = if policy == "coverage" {
            "transient_surface_final_max_temperature_c"
        } else {
            "transient_coverage_inside_energy_j"
        };
        assert!(
            result["comparison"]["alternatives"][0]["readings"][independent].is_object(),
            "{policy}"
        );
    }
}

fn coverage_plastic_heat_inputs() -> (Request, Value) {
    let mut r = plastic_heat_request();
    r.comparison.as_mut().unwrap().objectives = serde_json::from_value(json!([
        {"metric":"transient_coverage_final_max_temperature_c","direction":"minimize"},
        {"metric":"transient_surface_inside_energy_j","direction":"minimize"}]))
    .unwrap();
    let mut d = heat_descriptor();
    d["scenario"]["motion"]["transient_coverage"]["plastic_dissipation_heat"] =
        json!("uniform_reference_thickness");
    d["scenario"]["motion"]["transient_coverage"]["max_cell_steps"] = json!(5_000_000);
    (r, d)
}

#[test]
fn coverage_plastic_heat_admits_only_complete_source_energy_and_physical_storage() {
    let (r, d) = coverage_plastic_heat_inputs();
    for fault in [
        "none",
        "missing_receipt",
        "stale_material",
        "missing_trace",
        "trace_heat",
        "physical_storage",
        "remap_as_heat",
        "boundary",
        "missing_work",
    ] {
        let report = serde_json::to_value(
            r.evaluate_with(&Pattern::from_json(PATTERN).unwrap(), &d, |key, p| {
                let mut s = synthetic_plastic_heat_for(p, &d, "transient_coverage");
                let motion = &mut s["motion"]["result"];
                let h = &mut motion["transient_coverage"]["result"];
                // Neither an unrelated sheet-storage field nor remap residual supplies energy.
                h["stored_change_j"] = json!(1234.);
                h["signed_remap_residual_j"] = json!(0.25);
                h["absolute_remap_residual_j"] = json!(0.25);
                if key == "a" {
                    match fault {
                        "none" => {}
                        "missing_receipt" => {
                            h["plastic_dissipation_heat"]["material_fractions"] = Value::Null;
                        }
                        "stale_material" => {
                            h["plastic_dissipation_heat"]["material_fractions"][0]
                                ["material_sha256"] = json!("unbound");
                        }
                        "missing_trace" => {
                            h["trace"][0]["plastic_dissipation_heat"] = Value::Null;
                        }
                        "trace_heat" => {
                            h["trace"][1]["plastic_dissipation_heat"]["supplied_heat_j"] = json!(1.);
                        }
                        "physical_storage" => h["physical_stored_change_j"] = json!(1.),
                        "remap_as_heat" => h["balance_error_j"] = json!(-0.25),
                        "boundary" => h["trace"][0]["outside_energy_j"] = json!(0.),
                        "missing_work" => {
                            motion["samples"][0]["membrane_plastic_dissipation_j"] = Value::Null;
                        }
                        _ => unreachable!(),
                    }
                }
                Ok(s)
            })
            .unwrap(),
        )
        .unwrap();
        let expected = if fault == "none" { "a" } else { "b" };
        assert_eq!(
            report["comparison"]["frontier"],
            json!([expected]),
            "{fault}"
        );
        let readings = &report["comparison"]["alternatives"][0]["readings"];
        assert_eq!(
            readings["transient_coverage_final_max_temperature_c"].is_object(),
            fault == "none",
            "{fault}"
        );
        assert!(
            readings["transient_surface_inside_energy_j"].is_object(),
            "{fault}"
        );
        assert_eq!(
            report["manufacturing"]["alternatives"][0]["cutting"]["status"], "available",
            "{fault}"
        );
    }
}

#[test]
fn coverage_conversion_protocol_participates_only_when_that_heat_domain_is_compared() {
    let (mut r, d) = coverage_plastic_heat_inputs();
    r.variants[1]
        .changes
        .default
        .as_mut()
        .unwrap()
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("membrane_plastic_heat_fraction")
        .unwrap()
        .method = "A separately characterized conversion protocol".into();
    let evaluate = |r: &Request| {
        serde_json::to_value(
            r.evaluate_with(&Pattern::from_json(PATTERN).unwrap(), &d, |_, p| {
                Ok(synthetic_plastic_heat_for(p, &d, "transient_coverage"))
            })
            .unwrap(),
        )
        .unwrap()
    };
    let mixed = evaluate(&r);
    assert_eq!(mixed["comparison"]["frontier"], json!(["a", "b"]));
    assert_eq!(
        mixed["comparison"]["incomparable"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    r.comparison.as_mut().unwrap().allow_mixed_protocols = true;
    assert_eq!(evaluate(&r)["comparison"]["frontier"], json!(["a"]));
    r.comparison.as_mut().unwrap().allow_mixed_protocols = false;
    r.comparison.as_mut().unwrap().objectives = serde_json::from_value(json!([
        {"metric":"transient_surface_inside_energy_j","direction":"minimize"}]))
    .unwrap();
    let sheet_only = evaluate(&r);
    assert_eq!(sheet_only["comparison"]["frontier"], json!(["a", "b"]));
    assert_eq!(sheet_only["comparison"]["incomparable"], json!([]));
}

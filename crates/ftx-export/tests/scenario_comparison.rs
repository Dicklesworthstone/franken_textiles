//! Synthetic callback outcomes test comparison admission, not cloth physics or measured fit.
//! Actual production-solver isolation is exercised separately in ftx-wasm/material_scenario.
use ftx_core::{Refusal, receipt::sha256_hex};
use ftx_export::material_study::Request;
use ftx_pattern::{
    Pattern,
    material_catalog::{ResolvedMaterial, Selection},
    material_screen::Direction,
    materials::{CATALOG_SCHEMA, Catalog},
};
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

fn vapor_coverage_inputs() -> (Request, Value) {
    let mut r = request();
    for (index, variant) in r.variants.iter_mut().enumerate() {
        variant
            .changes
            .default
            .as_mut()
            .unwrap()
            .record
            .as_mut()
            .unwrap()
            .properties
            .insert(
                "vapor_resistance".into(),
                serde_json::from_value(json!({
                    "value":if index == 0 { 10. } else { 20. },
                    "unit":"m2*Pa/W", "source":"test", "evidence":"estimate",
                    "method":"synthetic comparison protocol"
                }))
                .unwrap(),
            );
    }
    r.comparison = Some(
        serde_json::from_value(json!({"basis":"nominal", "objectives":[
        {"metric":"covered_vapor_potential_heat_flow_w","direction":"maximize"}],
        "required_consumers":["vapor_coverage"]}))
        .unwrap(),
    );
    let mut d = descriptor();
    let body = json!({"positions_m":[[0.,0.,0.],[1.,0.,0.],[0.,1.,0.]],"triangles":[[0,1,2]]});
    d["vapor_coverage_body_mesh_sha256"] = json!(digest(&body));
    d["scenario"]["vapor_coverage"] = json!({"body_triangle_ids":[0],
        "instance_layers":{"band":"shell"},"settings":{"max_distance_m":0.1,
            "min_normal_cosine":0.5,"separation_tolerance_m":0.0001,"interfaces":[]},
        "conditions":{"boundaries":[{"inside_vapor_pressure_pa":2000.,"outside_vapor_pressure_pa":1000.,
            "inside_film_resistance_m2_pa_w":1.,"outside_film_resistance_m2_pa_w":4.,
            "latent_heat_j_kg":2500000.}],"required_conditions":{},"required_method":null,
            "allow_mixed_protocols":false}});
    (r, d)
}

fn frozen_vapor_material(value: &Value) -> ResolvedMaterial {
    let selection: Selection = serde_json::from_value(value.clone()).unwrap();
    Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: vec![],
    }
    .resolve(&selection)
    .unwrap()
}

fn vapor_test_bounds<'a>(
    terms: impl IntoIterator<Item = (&'a Value, f64)>,
    input_names: [&str; 2],
    output_names: [&str; 2],
) -> Value {
    let mut total = [[0.; 2]; 2];
    for (bounds, weight) in terms {
        if bounds.is_null() {
            return Value::Null;
        }
        for (range, name) in total.iter_mut().zip(input_names) {
            let incoming: [f64; 2] = serde_json::from_value(bounds[name].clone()).unwrap();
            for (sum, endpoint) in range.iter_mut().zip(incoming) {
                *sum += endpoint * weight;
            }
        }
    }
    json!({(output_names[0]):total[0],(output_names[1]):total[1]})
}

fn bound_vapor_variants(request: &mut Request, bounds: [Option<[f64; 2]>; 2]) {
    for (variant, bounds) in request.variants.iter_mut().zip(bounds) {
        let selection = variant.changes.default.as_mut().unwrap();
        selection.expected_sha256 = None;
        selection
            .record
            .as_mut()
            .unwrap()
            .properties
            .get_mut("vapor_resistance")
            .unwrap()
            .bounds = bounds;
    }
}

// Known equal-area single-sheet regions use the production material authority. The deliberately
// synthetic projection tests comparison admission independently of the drape projection kernel.
fn canonical_vapor_column(conditions: &Value, selection: &Value, area: f64) -> Value {
    let mut request = conditions["boundaries"][0].clone();
    request["schema"] = json!("ftx-vapor-stack/1");
    request["area_m2"] = json!(area);
    request["layers"] = json!([{"key":"shell","selection":selection,"area_m2":area}]);
    for key in [
        "required_conditions",
        "required_method",
        "allow_mixed_protocols",
    ] {
        request[key] = conditions[key].clone();
    }
    let request: ftx_pattern::material_vapor::Request = serde_json::from_value(request).unwrap();
    json!(
        request
            .evaluate(&Catalog {
                schema: CATALOG_SCHEMA.into(),
                materials: vec![],
            })
            .unwrap()
    )
}

fn synthetic_vapor_coverage(pattern: &Pattern, d: &Value) -> Value {
    let mut simulation = synthetic(pattern, 0., 0.2);
    simulation["positions_m"] = json!([[0., 0., 0.01], [1., 0., 0.01], [0., 1., 0.01]]);
    simulation["triangles"] = json!([[0, 1, 2]]);
    let material = frozen_vapor_material(&pattern.raw()["fabric"]["material"]);
    simulation["drape"]["material"] = json!({"basis":"characterized_record",
        "record_sha256":material.sha256(), "record":material});
    let mut conditions = d["scenario"]["vapor_coverage"]["conditions"].clone();
    conditions["body_mesh_sha256"] = d["vapor_coverage_body_mesh_sha256"].clone();
    let source = json!({"schema":"ftx-vapor-coverage-source/1", "positions_m":simulation["positions_m"],
        "patches":[{"vertices":[0,1,2],"reference_area_m2":0.5,"column":0}],
        "instances":["band"], "triangle_instance":[0], "materials":[material.frozen_selection()],
        "material_report_sha256":digest(&simulation["drape"]["material"])});
    let request = json!({"schema":"ftx-vapor-coverage/1","source":source,
        "map":{"body_positions_m":[[0.,0.,0.],[1.,0.,0.],[0.,1.,0.]],"body_triangles":[[0,1,2]],
            "instance_layers":d["scenario"]["vapor_coverage"]["instance_layers"],
            "settings":d["scenario"]["vapor_coverage"]["settings"]},"conditions":conditions});
    let vapor = canonical_vapor_column(&conditions, &source["materials"][0], 0.5);
    let bounds = vapor_test_bounds(
        [(&vapor["reported_bounds"], 1.)],
        ["potential_heat_flow_w", "potential_mass_flow_kg_s"],
        [
            "covered_potential_heat_flow_w",
            "covered_potential_mass_flow_kg_s",
        ],
    );
    simulation["vapor_coverage"] = json!({"status":"available","result":{
        "schema":"ftx-vapor-coverage-report/1","request":request,
        "request_sha256":digest(&request),"source_sha256":digest(&source),
        "replay_request_json":serde_json::to_string(&request).unwrap(),"mixed_protocols":false,
        "body_mesh_sha256":d["vapor_coverage_body_mesh_sha256"],
        "projection":{"regions":[{"id":"body-0-region-0", "body_triangle":0, "body_fraction":1.,
            "layers":[{"layer":"shell", "cloth_triangle":0, "fraction":1.}]}]},
        "coverage":{"body_area_m2":0.5,"covered_area_m2":0.5,"uncovered_area_m2":0.,
            "body":[{"area_m2":0.5,"covered_area_m2":0.5,"uncovered_area_m2":0.}],
            "covered_potential_heat_flow_w":vapor["potential_heat_flow_w"],
            "covered_potential_mass_flow_kg_s":vapor["potential_mass_flow_kg_s"],
            "reported_bounds":bounds,
            "regions":[{"id":"body-0-region-0", "body_triangle":0,
                "body_area_m2":0.5,"layer_area_m2":[0.5],"vapor":vapor}]}}});
    simulation
}

#[test]
fn covered_vapor_objectives_require_complete_current_geometry_and_independent_admission() {
    let (r, d) = vapor_coverage_inputs();
    for fault in [
        "none",
        "partial",
        "geometry",
        "material",
        "boundary",
        "missing_power",
        "zero_totals",
        "regional_transport",
        "regional_source",
        "regional_protocol",
        "source_selection",
        "canonical_source_substitution",
    ] {
        let report = serde_json::to_value(r.evaluate_with(
            &Pattern::from_json(PATTERN).unwrap(), &d, |key, p| {
                let mut s = synthetic_vapor_coverage(p, &d);
                // An unrelated dry heat refusal must not hide a supported vapor consumer.
                s["coverage"] = json!({"status":"refused"});
                s["surface"] = json!({"status":"refused"});
                if key == "a" {
                    let result = &mut s["vapor_coverage"]["result"];
                    match fault {
                        "partial" => {
                            result["coverage"]["uncovered_area_m2"] = json!(0.1);
                            result["coverage"]["covered_area_m2"] = json!(0.4);
                        }
                        "geometry" => result["request"]["source"]["positions_m"][0][2] = json!(0.02),
                        "material" => result["request"]["source"]["material_report_sha256"] = json!("9".repeat(64)),
                        "boundary" => result["request"]["conditions"]["boundaries"][0]["inside_vapor_pressure_pa"] = json!(2100.),
                        "missing_power" => result["coverage"]["covered_potential_heat_flow_w"] = Value::Null,
                        "zero_totals" => {
                            result["coverage"]["covered_potential_heat_flow_w"] = json!(0.);
                            result["coverage"]["covered_potential_mass_flow_kg_s"] = json!(0.);
                        }
                        "regional_transport" => result["coverage"]["regions"][0]["vapor"]["potential_heat_flow_w"] = json!(0.),
                        "regional_protocol" => result["coverage"]["regions"][0]["vapor"]["layers"][0]["observation"]["method"] = json!("unconsumed protocol"),
                        "regional_source" => {
                            let vapor = &mut result["coverage"]["regions"][0]["vapor"];
                            let selected = &mut vapor["request"]["layers"][0]["selection"];
                            selected["record"]["properties"]["vapor_resistance"]["value"] = json!(1.);
                            selected["expected_sha256"] = Value::Null;
                            *selected = json!(frozen_vapor_material(selected).frozen_selection());
                            let network: ftx_pattern::material_vapor::Request =
                                serde_json::from_value(vapor["request"].clone()).unwrap();
                            *vapor = json!(network.evaluate(&Catalog {
                                schema: CATALOG_SCHEMA.into(), materials: vec![] }).unwrap());
                            let power = vapor["potential_heat_flow_w"].clone();
                            let flow = vapor["potential_mass_flow_kg_s"].clone();
                            result["coverage"]["covered_potential_heat_flow_w"] = power;
                            result["coverage"]["covered_potential_mass_flow_kg_s"] = flow;
                        }
                        "source_selection" | "canonical_source_substitution" => {
                            let source_material = &mut result["request"]["source"]["materials"][0];
                            source_material["record"]["properties"]["vapor_resistance"]["value"] = json!(1.);
                            if fault == "canonical_source_substitution" {
                                source_material["expected_sha256"] = Value::Null;
                                *source_material = json!(frozen_vapor_material(source_material).frozen_selection());
                            }
                        }
                        _ => {}
                    }
                    result["source_sha256"] = json!(digest(&result["request"]["source"]));
                    result["request_sha256"] = json!(digest(&result["request"]));
                }
                Ok(s)
            }).unwrap()).unwrap();
        assert_eq!(
            report["comparison"]["frontier"],
            if fault == "none" {
                json!(["a"])
            } else {
                json!(["b"])
            },
            "{fault}"
        );
        assert_eq!(
            report["comparison"]["alternatives"][0]["eligible"],
            fault == "none",
            "{fault}: invalid transport must be refused, not merely rank lower"
        );
        assert_eq!(
            report["manufacturing"]["alternatives"][0]["cutting"]["status"],
            "available"
        );
        let reading = &report["comparison"]["alternatives"][1]["readings"]["covered_vapor_potential_heat_flow_w"];
        // Half a square metre at 1000 Pa, Ret=20 plus explicit films 1+4, gives 20 W.
        assert_eq!(reading["value"], 20.);
        assert_eq!(reading["unit"], "W");
        assert_eq!(reading["consumer"], "vapor_coverage");
    }
}

#[test]
fn covered_vapor_protocols_follow_consumed_layers_and_require_separate_comparison_consent() {
    let (mut r, d) = vapor_coverage_inputs();
    r.variants[0]
        .changes
        .default
        .as_mut()
        .unwrap()
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("vapor_resistance")
        .unwrap()
        .method = "different acquired protocol".into();
    let evaluate = |r: &Request| {
        serde_json::to_value(
            r.evaluate_with(&Pattern::from_json(PATTERN).unwrap(), &d, |_, p| {
                Ok(synthetic_vapor_coverage(p, &d))
            })
            .unwrap(),
        )
        .unwrap()
    };
    let report = evaluate(&r);
    assert_eq!(report["comparison"]["frontier"], json!(["a", "b"]));
    assert_eq!(
        report["comparison"]["incomparable"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    r.comparison.as_mut().unwrap().allow_mixed_protocols = true;
    assert_eq!(evaluate(&r)["comparison"]["frontier"], json!(["a"]));
    r.comparison.as_mut().unwrap().basis =
        ftx_export::material_study::comparison::Basis::ReportedBounds;
    assert!(r.comparison.as_ref().unwrap().validate(&r, true).is_ok());
    // A canonical nominal network cannot acquire absent source bounds by selecting this policy.
    assert_eq!(evaluate(&r)["comparison"]["frontier"], json!([]));
}

fn moving_vapor_inputs() -> (Request, Value) {
    let (mut request, coverage) = vapor_coverage_inputs();
    let mut descriptor = motion_descriptor();
    descriptor["scenario"]["vapor_coverage"] = coverage["scenario"]["vapor_coverage"].clone();
    descriptor["vapor_coverage_body_mesh_sha256"] =
        coverage["vapor_coverage_body_mesh_sha256"].clone();
    descriptor["scenario"]["motion"]["vapor_coverage_history"] = json!({
        "schema":"ftx-motion-vapor-coverage/1","integration":"right_endpoint",
        "max_geometry_operations":10000});
    request.variants[1]
        .changes
        .default
        .as_mut()
        .unwrap()
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("vapor_resistance")
        .unwrap()
        .value = 20.;
    request.comparison = Some(
        serde_json::from_value(json!({"basis":"nominal", "objectives":[
        {"metric":"motion_covered_vapor_potential_energy_j","direction":"maximize"},
        {"metric":"motion_covered_vapor_potential_mass_kg","direction":"maximize"}],
        "required_consumers":["motion_vapor_coverage"]}))
        .unwrap(),
    );
    (request, descriptor)
}

// Synthetic geometry with independently specified area changes; each regional network is evaluated
// by the production material authority. These callbacks test comparison admission, not projection.
fn moving_vapor_frame(pattern: &Pattern, d: &Value, width: f64) -> Value {
    let mut frame = synthetic_vapor_coverage(pattern, d);
    frame["positions_m"][1][0] = json!(width);
    let positions = frame["positions_m"].clone();
    let result = &mut frame["vapor_coverage"]["result"];
    result["request"]["source"]["positions_m"] = positions;
    result["request"]["map"]["body_positions_m"][1][0] = json!(width);
    let body_hash = digest(
        &json!({"positions_m":result["request"]["map"]["body_positions_m"],
        "triangles":result["request"]["map"]["body_triangles"]}),
    );
    result["request"]["conditions"]["body_mesh_sha256"] = json!(body_hash);
    result["body_mesh_sha256"] = json!(body_hash);
    let boundary = &result["request"]["conditions"]["boundaries"][0];
    let network: ftx_pattern::material_vapor::Request = serde_json::from_value(json!({
        "schema":"ftx-vapor-stack/1","layers":[{"key":"shell",
            "selection":result["request"]["source"]["materials"][0],"area_m2":width * 0.5}],
        "area_m2":width * 0.5,
        "inside_vapor_pressure_pa":boundary["inside_vapor_pressure_pa"],
        "outside_vapor_pressure_pa":boundary["outside_vapor_pressure_pa"],
        "inside_film_resistance_m2_pa_w":boundary["inside_film_resistance_m2_pa_w"],
        "outside_film_resistance_m2_pa_w":boundary["outside_film_resistance_m2_pa_w"],
        "latent_heat_j_kg":boundary["latent_heat_j_kg"],"required_conditions":{},
        "required_method":null,"allow_mixed_protocols":false}))
    .unwrap();
    let vapor = network
        .evaluate(&Catalog {
            schema: CATALOG_SCHEMA.into(),
            materials: vec![],
        })
        .unwrap();
    result["coverage"]["body_area_m2"] = json!(width * 0.5);
    result["coverage"]["body"] = json!([{"area_m2":width * 0.5,
        "covered_area_m2":width * 0.5,"uncovered_area_m2":0.}]);
    result["mixed_protocols"] = json!(false);
    result["coverage"]["covered_area_m2"] = json!(width * 0.5);
    result["coverage"]["regions"][0]["body_area_m2"] = json!(width * 0.5);
    result["coverage"]["regions"][0]["layer_area_m2"] = json!([width * 0.5]);
    result["coverage"]["covered_potential_heat_flow_w"] = json!(vapor.potential_heat_flow_w);
    result["coverage"]["covered_potential_mass_flow_kg_s"] = json!(vapor.potential_mass_flow_kg_s);
    result["coverage"]["reported_bounds"] = vapor
        .reported_bounds
        .map(|b| {
            json!({"covered_potential_heat_flow_w":b.potential_heat_flow_w,
            "covered_potential_mass_flow_kg_s":b.potential_mass_flow_kg_s})
        })
        .unwrap_or(Value::Null);
    result["coverage"]["regions"][0]["vapor"] = json!(vapor);
    result["projection"]["geometry_operations"] = json!(11);
    result["source_sha256"] = json!(digest(&result["request"]["source"]));
    result["request_sha256"] = json!(digest(&result["request"]));
    result["replay_request_json"] = json!(serde_json::to_string(&result["request"]).unwrap());
    frame
}

fn synthetic_moving_vapor(pattern: &Pattern, d: &Value) -> Value {
    synthetic_moving_vapor_with_widths(pattern, d, [1.5, 2.])
}

fn synthetic_moving_vapor_with_widths(pattern: &Pattern, d: &Value, widths: [f64; 2]) -> Value {
    let mut simulation = moving_vapor_frame(pattern, d, 1.);
    simulation["positions_sha256"] = json!(digest(&simulation["positions_m"]));
    let mut motion = synthetic_motion(pattern, 3.)["motion"]["result"].clone();
    motion["initial_positions_sha256"] = simulation["positions_sha256"].clone();
    motion["triangles"] = simulation["triangles"].clone();
    let initial_result = simulation["vapor_coverage"]["result"].clone();
    let initial_request = &initial_result["request"];
    motion["initial_sample"] = json!({"time_s":0.,"positions_m":simulation["positions_m"],
        "body_positions_m":initial_request["map"]["body_positions_m"]});
    let initial = json!({"time_s":0.,"positions_sha256":simulation["positions_sha256"],
        "body_positions_sha256":digest(&initial_request["map"]["body_positions_m"]),
        "result":initial_result});
    let mut rows = Vec::new();
    let mut retained = Vec::new();
    let mut energy = 0.;
    let mut mass = 0.;
    let integration = d["scenario"]["motion"]["vapor_coverage_history"]["integration"]
        .as_str()
        .unwrap();
    let mut previous_power = initial_result["coverage"]["covered_potential_heat_flow_w"]
        .as_f64()
        .unwrap();
    let mut previous_flow = initial_result["coverage"]["covered_potential_mass_flow_kg_s"]
        .as_f64()
        .unwrap();
    let mut previous_bounds = initial_result["coverage"]["reported_bounds"].clone();
    for (i, width) in widths.into_iter().enumerate() {
        let frame = moving_vapor_frame(pattern, d, width);
        let result = &frame["vapor_coverage"]["result"];
        let sample = json!({"time_s":0.5 * (i + 1) as f64,"positions_m":frame["positions_m"],
            "body_positions_m":result["request"]["map"]["body_positions_m"]});
        motion["samples"][i] = sample.clone();
        motion["samples"][i]["solve"] = json!({"converged":true});
        motion["keyframes"][i]["positions_sha256"] = json!(digest(&sample["positions_m"]));
        motion["keyframes"][i]["body_collision_sha256"] = json!(digest(&json!([
            sample["body_positions_m"],
            motion["body_triangles"]
        ])));
        let power = result["coverage"]["covered_potential_heat_flow_w"]
            .as_f64()
            .unwrap();
        let flow = result["coverage"]["covered_potential_mass_flow_kg_s"]
            .as_f64()
            .unwrap();
        let (e, m) = match integration {
            "right_endpoint" => (power * 0.5, flow * 0.5),
            "composite_trapezoid" => (
                previous_power * 0.25 + power * 0.25,
                previous_flow * 0.25 + flow * 0.25,
            ),
            _ => panic!("unsupported synthetic quadrature"),
        };
        let terms = match integration {
            "right_endpoint" => vec![(&result["coverage"]["reported_bounds"], 0.5)],
            "composite_trapezoid" => vec![
                (&previous_bounds, 0.25),
                (&result["coverage"]["reported_bounds"], 0.25),
            ],
            _ => unreachable!(),
        };
        let bounds = vapor_test_bounds(
            terms,
            [
                "covered_potential_heat_flow_w",
                "covered_potential_mass_flow_kg_s",
            ],
            ["potential_energy_j", "potential_mass_kg"],
        );
        previous_power = power;
        previous_flow = flow;
        previous_bounds = result["coverage"]["reported_bounds"].clone();
        energy += e;
        mass += m;
        rows.push(
            json!({"sample_index":i,"time_s":sample["time_s"],"dt_s":0.5,
            "positions_sha256":digest(&sample["positions_m"]),
            "body_positions_sha256":digest(&sample["body_positions_m"]),
            "result":result,"potential_energy_j":e,"potential_mass_kg":m,
            "reported_bounds":bounds}),
        );
        retained.push(sample);
    }
    let request = json!({"schema":"ftx-vapor-coverage-history/1",
        "policy":d["scenario"]["motion"]["vapor_coverage_history"],
        "initial":initial_request,"samples":retained,"path_sha256":motion["path_sha256"]});
    let bounds = vapor_test_bounds(
        rows.iter().map(|row| (&row["reported_bounds"], 1.)),
        ["potential_energy_j", "potential_mass_kg"],
        ["potential_energy_j", "potential_mass_kg"],
    );
    motion["vapor_coverage_history"] = json!({"status":"available","result":{
        "schema":"ftx-vapor-coverage-history-report/1","request":request,
        "request_sha256":digest(&request),"replay_request_json":serde_json::to_string(&request).unwrap(),
        "path_sha256":motion["path_sha256"],"initial_positions_sha256":simulation["positions_sha256"],
        "duration_s":1.,"integration":integration,"geometry_operations":33,
        "initial":initial,"samples":rows,"totals":{"potential_energy_j":energy,"potential_mass_kg":mass,
            "reported_bounds":bounds}}});
    simulation["motion"] = json!({"status":"available","result":motion});
    simulation["surface"] = json!({"status":"refused"});
    simulation["coverage"] = json!({"status":"refused"});
    simulation
}

#[test]
fn moving_vapor_objectives_integrate_every_current_area_and_remain_independent_of_dry_heat() {
    let (r, d) = moving_vapor_inputs();
    let report = serde_json::to_value(
        r.evaluate_with(&Pattern::from_json(PATTERN).unwrap(), &d, |_, p| {
            Ok(synthetic_moving_vapor(p, &d))
        })
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        report["comparison"]["frontier"],
        json!(["a"]),
        "{}",
        report["comparison"]
    );
    assert_eq!(report["comparison"]["motion_required"], true);
    let readings = &report["comparison"]["alternatives"][0]["readings"];
    // Ret=10, films=5, pressure difference=1000 Pa. Accepted body areas are 0.75 and 1 m².
    let expected = (0.75 + 1.) * 0.5 * 1000. / 15.;
    assert!(
        (readings["motion_covered_vapor_potential_energy_j"]["value"]
            .as_f64()
            .unwrap()
            - expected)
            .abs()
            < 1e-12
    );
    assert!(
        (readings["motion_covered_vapor_potential_mass_kg"]["value"]
            .as_f64()
            .unwrap()
            - expected / 2500000.)
            .abs()
            < 1e-18
    );
    assert_eq!(
        readings["motion_covered_vapor_potential_energy_j"]["unit"],
        "J"
    );
    assert_eq!(
        readings["motion_covered_vapor_potential_mass_kg"]["unit"],
        "kg"
    );
    assert_eq!(
        readings["motion_covered_vapor_potential_energy_j"]["consumer"],
        "motion_vapor_coverage"
    );
    assert_eq!(
        report["manufacturing"]["alternatives"][0]["cutting"]["status"],
        "available"
    );
}

#[test]
fn reported_bounds_admit_canonical_covered_networks_and_reject_forged_ranges() {
    let (mut request, descriptor) = moving_vapor_inputs();
    request.comparison = Some(
        serde_json::from_value(json!({"basis":"reported_bounds",
        "objectives":[{"metric":"covered_vapor_potential_heat_flow_w","direction":"maximize"},
            {"metric":"covered_vapor_potential_mass_flow_kg_s","direction":"maximize"}],
        "required_consumers":["vapor_coverage"]}))
        .unwrap(),
    );
    for fault in ["none", "missing_source", "forged_total", "forged_network"] {
        let mut input = request.clone();
        bound_vapor_variants(
            &mut input,
            [
                if fault == "missing_source" {
                    None
                } else {
                    Some([8., 12.])
                },
                Some([18., 22.]),
            ],
        );
        let report =
            serde_json::to_value(
                input
                    .evaluate_with(
                        &Pattern::from_json(PATTERN).unwrap(),
                        &descriptor,
                        |key, pattern| {
                            let mut simulation = moving_vapor_frame(pattern, &descriptor, 1.);
                            if key == "a" && fault.starts_with("forged") {
                                let result = &mut simulation["vapor_coverage"]["result"];
                                result["coverage"]["reported_bounds"] = json!({
                        "covered_potential_heat_flow_w":[30.,40.],
                        "covered_potential_mass_flow_kg_s":[30./2500000.,40./2500000.]});
                                if fault == "forged_network" {
                                    result["coverage"]["regions"][0]["vapor"]["reported_bounds"]
                                        ["potential_heat_flow_w"] = json!([30., 40.]);
                                    result["coverage"]["regions"][0]["vapor"]["reported_bounds"]
                                        ["potential_mass_flow_kg_s"] =
                                        json!([30. / 2500000., 40. / 2500000.]);
                                }
                            }
                            Ok(simulation)
                        },
                    )
                    .unwrap(),
            )
            .unwrap();
        assert_eq!(
            report["comparison"]["frontier"],
            if fault == "none" {
                json!(["a"])
            } else {
                json!(["b"])
            },
            "{fault}: {}",
            report["comparison"]
        );
        assert_eq!(
            report["simulations"][0]["evaluation"]["result"]["vapor_coverage"]["status"],
            "available"
        );
        let reading = &report["comparison"]["alternatives"][0]["readings"]["covered_vapor_potential_heat_flow_w"];
        if fault == "forged_network" {
            assert!(reading.is_null());
        } else {
            assert_eq!(reading["value"], 500. / 15.);
        }
        if fault == "none" {
            assert_eq!(reading["reported_bounds"], json!([500. / 17., 500. / 13.]));
        } else {
            assert_eq!(report["comparison"]["alternatives"][0]["eligible"], false);
        }
    }
}

#[test]
fn reported_motion_vapor_ranges_require_every_interval_and_preserve_nominal_outputs() {
    let (mut request, descriptor) = moving_vapor_inputs();
    request.comparison.as_mut().unwrap().basis =
        ftx_export::material_study::comparison::Basis::ReportedBounds;
    for fault in ["none", "missing_source", "late_range", "forged_aggregate"] {
        let mut input = request.clone();
        bound_vapor_variants(
            &mut input,
            [
                if fault == "missing_source" {
                    None
                } else {
                    Some([8., 12.])
                },
                Some([18., 22.]),
            ],
        );
        let report = serde_json::to_value(input.evaluate_with(
            &Pattern::from_json(PATTERN).unwrap(), &descriptor, |key, pattern| {
                let mut simulation = synthetic_moving_vapor(pattern, &descriptor);
                if key == "a" {
                    let history = &mut simulation["motion"]["result"]["vapor_coverage_history"]["result"];
                    match fault {
                        "late_range" => history["samples"][1]["reported_bounds"] = Value::Null,
                        "forged_aggregate" => history["totals"]["reported_bounds"] = json!({
                            "potential_energy_j":[55.,60.],
                            "potential_mass_kg":[55./2500000.,60./2500000.]}),
                        _ => {}
                    }
                }
                Ok(simulation)
            }).unwrap()).unwrap();
        assert_eq!(
            report["comparison"]["frontier"],
            if fault == "none" {
                json!(["a"])
            } else {
                json!(["b"])
            },
            "{fault}: {}",
            report["comparison"]
        );
        let reading = &report["comparison"]["alternatives"][0]["readings"]["motion_covered_vapor_potential_energy_j"];
        assert!((reading["value"].as_f64().unwrap() - 875. / 15.).abs() < 1e-12);
        if fault == "none" {
            for (endpoint, resistance) in [17., 13.].into_iter().enumerate() {
                assert!(
                    (reading["reported_bounds"][endpoint].as_f64().unwrap() - 875. / resistance)
                        .abs()
                        < 1e-12
                );
            }
        }
        assert_eq!(
            report["simulations"][0]["evaluation"]["result"]["motion"]["result"]["vapor_coverage_history"]
                ["status"],
            "available"
        );
        assert_eq!(
            report["manufacturing"]["alternatives"][0]["cutting"]["status"],
            "available"
        );
    }
}

#[test]
fn reported_motion_ranges_follow_the_rule_and_require_each_weighted_endpoint() {
    for integration in ["right_endpoint", "composite_trapezoid"] {
        let (mut request, mut descriptor) = moving_vapor_inputs();
        descriptor["scenario"]["motion"]["vapor_coverage_history"]["integration"] =
            json!(integration);
        request.comparison.as_mut().unwrap().basis =
            ftx_export::material_study::comparison::Basis::ReportedBounds;
        bound_vapor_variants(&mut request, [Some([8., 12.]), Some([18., 22.])]);
        for fault in [
            "none",
            "missing_initial_range",
            "right_endpoint_ranges",
            "missing_late_range",
        ] {
            if integration == "right_endpoint"
                && matches!(fault, "right_endpoint_ranges" | "missing_late_range")
            {
                continue;
            }
            let report = serde_json::to_value(request.evaluate_with(
                &Pattern::from_json(PATTERN).unwrap(), &descriptor, |key, pattern| {
                    let mut simulation = synthetic_moving_vapor_with_widths(pattern, &descriptor, [2.5, 2.]);
                    if key == "a" {
                        let history = &mut simulation["motion"]["result"]["vapor_coverage_history"]["result"];
                        match fault {
                            "missing_initial_range" => {
                                history["initial"]["result"]["coverage"]["reported_bounds"] = Value::Null;
                                if integration == "composite_trapezoid" {
                                    history["samples"][0]["reported_bounds"] = Value::Null;
                                    history["totals"]["reported_bounds"] = Value::Null;
                                }
                            }
                            "right_endpoint_ranges" => {
                                // Keep nominal trapezoid integrals intact and make every forged
                                // range internally consistent with the wrong endpoint weights.
                                for sample in history["samples"].as_array_mut().unwrap() {
                                    sample["reported_bounds"] = vapor_test_bounds(
                                        [(&sample["result"]["coverage"]["reported_bounds"], 0.5)],
                                        ["covered_potential_heat_flow_w", "covered_potential_mass_flow_kg_s"],
                                        ["potential_energy_j", "potential_mass_kg"],
                                    );
                                }
                                history["totals"]["reported_bounds"] = vapor_test_bounds(
                                    history["samples"].as_array().unwrap().iter()
                                        .map(|sample| (&sample["reported_bounds"], 1.)),
                                    ["potential_energy_j", "potential_mass_kg"],
                                    ["potential_energy_j", "potential_mass_kg"],
                                );
                            }
                            "missing_late_range" => {
                                history["samples"][1]["reported_bounds"] = Value::Null;
                                history["totals"]["reported_bounds"] = Value::Null;
                            }
                            _ => {}
                        }
                    }
                    Ok(simulation)
                }).unwrap()).unwrap();
            let eligible = fault == "none"
                || (integration == "right_endpoint" && fault == "missing_initial_range");
            assert_eq!(
                report["comparison"]["frontier"],
                if eligible { json!(["a"]) } else { json!(["b"]) },
                "{integration}/{fault}: {}",
                report["comparison"]
            );
            let candidate = &report["comparison"]["alternatives"][0];
            assert_eq!(candidate["eligible"], eligible);
            let coefficient = if integration == "right_endpoint" {
                1125.
            } else {
                1000.
            };
            for (metric, conversion) in [
                ("motion_covered_vapor_potential_energy_j", 1.),
                ("motion_covered_vapor_potential_mass_kg", 2500000.),
            ] {
                let reading = &candidate["readings"][metric];
                assert!(
                    (reading["value"].as_f64().unwrap() - coefficient / 15. / conversion).abs()
                        < 1e-12 / conversion
                );
                if eligible {
                    for (endpoint, resistance) in [17., 13.].into_iter().enumerate() {
                        assert!(
                            (reading["reported_bounds"][endpoint].as_f64().unwrap()
                                - coefficient / resistance / conversion)
                                .abs()
                                < 1e-12 / conversion
                        );
                    }
                } else {
                    assert!(reading["reported_bounds"].is_null());
                }
            }
            assert_eq!(
                report["simulations"][0]["evaluation"]["result"]["motion"]["result"]["vapor_coverage_history"]
                    ["status"],
                "available"
            );
        }
    }
}

#[test]
fn moving_vapor_trapezoid_uses_initial_and_every_sample_and_refuses_relabelled_integrals() {
    let (mut request, mut descriptor) = moving_vapor_inputs();
    descriptor["scenario"]["motion"]["vapor_coverage_history"]["integration"] =
        json!("composite_trapezoid");
    request.comparison.as_mut().unwrap().allow_mixed_protocols = true;
    for fault in [
        "none",
        "right_endpoint",
        "missing_initial",
        "stale_previous",
        "missing_sample",
    ] {
        let report = serde_json::to_value(request.evaluate_with(
            &Pattern::from_json(PATTERN).unwrap(), &descriptor, |key, pattern| {
                let mut produced = descriptor.clone();
                if key == "a" && fault == "right_endpoint" {
                    produced["scenario"]["motion"]["vapor_coverage_history"]["integration"] =
                        json!("right_endpoint");
                }
                let mut simulation = synthetic_moving_vapor_with_widths(pattern, &produced, [2.5, 2.]);
                if key == "a" && fault != "none" {
                    let history = &mut simulation["motion"]["result"]["vapor_coverage_history"]["result"];
                    if fault == "missing_initial" || fault == "stale_previous" {
                        let index = if fault == "missing_initial" { 0 } else { 1 };
                        for (rate, integral) in [("covered_potential_heat_flow_w", "potential_energy_j"),
                            ("covered_potential_mass_flow_kg_s", "potential_mass_kg")] {
                            let initial = if fault == "missing_initial" { 0. } else {
                                history["initial"]["result"]["coverage"][rate].as_f64().unwrap()
                            };
                            let current = history["samples"][index]["result"]["coverage"][rate]
                                .as_f64().unwrap();
                            history["samples"][index][integral] = json!(initial * 0.25 + current * 0.25);
                        }
                    } else if fault == "missing_sample" {
                        history["samples"].as_array_mut().unwrap().remove(0);
                        history["request"]["samples"].as_array_mut().unwrap().remove(0);
                    }
                    // Keep aggregate totals and exact replay identities consistent, so each
                    // rejection depends on the declared rule and complete canonical sample data.
                    for integral in ["potential_energy_j", "potential_mass_kg"] {
                        history["totals"][integral] = json!(history["samples"].as_array().unwrap()
                            .iter().map(|row| row[integral].as_f64().unwrap()).sum::<f64>());
                    }
                    history["integration"] = json!("composite_trapezoid");
                    history["request"]["policy"] = descriptor["scenario"]["motion"]["vapor_coverage_history"].clone();
                    history["request_sha256"] = json!(digest(&history["request"]));
                    history["replay_request_json"] = json!(serde_json::to_string(&history["request"]).unwrap());
                }
                Ok(simulation)
            }).unwrap()).unwrap();
        assert_eq!(
            report["comparison"]["frontier"],
            if fault == "none" {
                json!(["a"])
            } else {
                json!(["b"])
            },
            "{fault}: {}",
            report["comparison"]
        );
        assert_eq!(
            report["comparison"]["alternatives"][0]["eligible"],
            fault == "none",
            "{fault}"
        );
        if fault == "none" {
            // Body areas are 0.5, 1.25 and 1 m² at 0, 0.5 and 1 s. Integrating both
            // intervals gives (0.5 + 2*1.25 + 1)*0.25*1000/(10+1+4), or 66 2/3 J.
            let readings = &report["comparison"]["alternatives"][0]["readings"];
            let expected = 1000. / 15.;
            assert!(
                (readings["motion_covered_vapor_potential_energy_j"]["value"]
                    .as_f64()
                    .unwrap()
                    - expected)
                    .abs()
                    < 1e-12
            );
            assert!(
                (readings["motion_covered_vapor_potential_mass_kg"]["value"]
                    .as_f64()
                    .unwrap()
                    - expected / 2500000.)
                    .abs()
                    < 1e-18
            );
            assert_eq!(
                readings["motion_covered_vapor_potential_energy_j"]["unit"],
                "J"
            );
            assert_eq!(
                readings["motion_covered_vapor_potential_mass_kg"]["unit"],
                "kg"
            );
        }
    }
}

#[test]
fn moving_vapor_refuses_partial_stale_and_modified_histories_even_with_protocol_consent() {
    let (mut r, d) = moving_vapor_inputs();
    r.comparison.as_mut().unwrap().allow_mixed_protocols = true;
    for fault in [
        "missing",
        "partial",
        "initial_partial",
        "path",
        "body",
        "geometry",
        "time",
        "integral",
        "total",
        "rule",
        "canonical_transport",
        "source",
        "bound_material",
        "area_accounting",
        "frame_replay",
        "boundary",
        "replay",
        "work",
    ] {
        let report = serde_json::to_value(r.evaluate_with(&Pattern::from_json(PATTERN).unwrap(), &d,
            |key, p| {
                let mut simulation = synthetic_moving_vapor(p, &d);
                    if key == "a" && fault == "bound_material" {
                        let mut other = p.raw().clone();
                        other["fabric"]["material"]["expected_sha256"] = Value::Null;
                        other["fabric"]["material"]["record"]["properties"]["vapor_resistance"]["value"] = json!(1.);
                        simulation = synthetic_moving_vapor(&Pattern::new(other), &d);
                        simulation["pattern_sha256"] = json!(digest(p.raw()));
                    }
                if key == "a" {
                    let history = &mut simulation["motion"]["result"]["vapor_coverage_history"]["result"];
                    match fault {
                        "missing" => *history = Value::Null,
                        "partial" => { history["samples"].as_array_mut().unwrap().pop(); },
                        "initial_partial" => history["initial"]["result"]["coverage"]["uncovered_area_m2"] = json!(0.1),
                        "path" => history["path_sha256"] = json!("4".repeat(64)),
                        "body" => history["samples"][1]["result"]["request"]["map"]["body_positions_m"][0][2] = json!(0.02),
                        "geometry" => history["samples"][1]["result"]["request"]["source"]["positions_m"][0][2] = json!(0.04),
                        "time" => history["samples"][1]["dt_s"] = json!(0.25),
                        "integral" => history["samples"][1]["potential_energy_j"] = json!(500.),
                        "total" => history["totals"]["potential_mass_kg"] = json!(100.),
                        "rule" => history["integration"] = json!("left_endpoint"),
                        "canonical_transport" => history["samples"][1]["result"]["coverage"]["regions"][0]["vapor"]["potential_heat_flow_w"] = json!(1000.),
                        "source" => history["samples"][1]["result"]["request"]["source"]["materials"][0]["expected_sha256"] = json!("5".repeat(64)),
                        "boundary" => history["samples"][1]["result"]["request"]["conditions"]["boundaries"][0]["inside_vapor_pressure_pa"] = json!(10000.),
                        "replay" => history["replay_request_json"] = json!("{}"),
                        "frame_replay" => history["samples"][1]["result"]["replay_request_json"] = json!("{}"),
                        "work" => history["geometry_operations"] = json!(10001),
                        "bound_material" => {},
                        "area_accounting" => {
                            let frame = &mut history["samples"][1]["result"];
                            frame["projection"]["regions"][0]["body_fraction"] = json!(0.5);
                            frame["projection"]["regions"][0]["layers"][0]["fraction"] = json!(0.5);
                            let region = &mut frame["coverage"]["regions"][0];
                            region["body_area_m2"] = json!(0.5);
                            region["layer_area_m2"] = json!([0.5]);
                            let mut request: ftx_pattern::material_vapor::Request =
                                serde_json::from_value(region["vapor"]["request"].clone()).unwrap();
                            request.area_m2 = 0.5;
                            request.layers[0].area_m2 = Some(0.5);
                            let canonical = request.evaluate(&Catalog {
                                schema: CATALOG_SCHEMA.into(), materials:vec![] }).unwrap();
                            let power = canonical.potential_heat_flow_w;
                            let flow = canonical.potential_mass_flow_kg_s;
                            region["vapor"] = json!(canonical);
                            frame["coverage"]["covered_potential_heat_flow_w"] = json!(power);
                            frame["coverage"]["covered_potential_mass_flow_kg_s"] = json!(flow);
                            history["samples"][1]["potential_energy_j"] = json!(power * 0.5);
                            history["samples"][1]["potential_mass_kg"] = json!(flow * 0.5);
                            history["totals"]["potential_energy_j"] = json!(
                                history["samples"][0]["potential_energy_j"].as_f64().unwrap() + power * 0.5);
                            history["totals"]["potential_mass_kg"] = json!(
                                history["samples"][0]["potential_mass_kg"].as_f64().unwrap() + flow * 0.5);
                        },
                        _ => unreachable!(),
                    }
                }
                Ok(simulation)
            }).unwrap()).unwrap();
        assert_eq!(
            report["comparison"]["frontier"],
            json!(["b"]),
            "{fault}: {}",
            report["comparison"]
        );
        assert_eq!(
            report["manufacturing"]["alternatives"][0]["cutting"]["status"],
            "available"
        );
        assert_eq!(
            report["simulations"][0]["evaluation"]["result"]["motion"]["status"],
            "available"
        );
    }
}

#[test]
fn moving_vapor_requires_history_only_when_consumed_and_preserves_source_protocol_comparison() {
    let (mut r, d) = moving_vapor_inputs();
    r.variants[0]
        .changes
        .default
        .as_mut()
        .unwrap()
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("vapor_resistance")
        .unwrap()
        .method = "different acquired protocol".into();
    let evaluate = |request: &Request, missing: bool| {
        serde_json::to_value(
            request
                .evaluate_with(&Pattern::from_json(PATTERN).unwrap(), &d, |_, p| {
                    let mut simulation = synthetic_moving_vapor(p, &d);
                    if missing {
                        simulation["motion"]["result"]["vapor_coverage_history"] =
                            json!({"status":"refused"});
                    }
                    Ok(simulation)
                })
                .unwrap(),
        )
        .unwrap()
    };
    let report = evaluate(&r, false);
    assert_eq!(report["comparison"]["frontier"], json!(["a", "b"]));
    assert_eq!(
        report["comparison"]["incomparable"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    r.comparison.as_mut().unwrap().allow_mixed_protocols = true;
    assert_eq!(evaluate(&r, false)["comparison"]["frontier"], json!(["a"]));
    r.comparison = Some(serde_json::from_value(json!({"basis":"nominal","objectives":[
        {"metric":"stock_mass_kg","direction":"minimize"}],"required_consumers":["motion_vapor_coverage"]})).unwrap());
    assert!(
        evaluate(&r, true)["comparison"]["frontier"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    r.comparison.as_mut().unwrap().required_consumers.clear();
    let static_only = evaluate(&r, true);
    assert_eq!(static_only["comparison"]["frontier"], json!(["a", "b"]));
    assert_eq!(static_only["comparison"]["motion_required"], false);
}

fn vapor_piece_inputs() -> (Pattern, Request, Value) {
    let (mut request, mut descriptor) = vapor_coverage_inputs();
    let mut raw: Value = serde_json::from_str(PATTERN).unwrap();
    let mut back = raw["pieces"][0].clone();
    back["id"] = json!("back");
    raw["pieces"].as_array_mut().unwrap().push(back);
    descriptor["scenario"]["vapor_coverage"]["instance_layers"] =
        json!({"band":"shell", "back":"shell"});
    descriptor["scenario"]["vapor_coverage"]["conditions"]["allow_mixed_protocols"] = json!(true);
    for variant in &mut request.variants {
        for (piece, method) in [
            ("band", "synthetic method A"),
            ("back", "synthetic method B"),
        ] {
            let mut selection = variant.changes.default.clone().unwrap();
            let record = selection.record.as_mut().unwrap();
            selection.id = format!("synthetic-{piece}");
            record.id = selection.id.clone();
            record
                .properties
                .get_mut("vapor_resistance")
                .unwrap()
                .method = method.into();
            variant.changes.pieces.insert(piece.into(), selection);
        }
    }
    (Pattern::new(raw), request, descriptor)
}

// Two original pieces share the physical layer "shell". The refined variant splits one piece's
// triangle without changing its instance identity, selected material, or test protocol.
fn synthetic_piece_vapor(pattern: &Pattern, d: &Value, refined: bool) -> Value {
    let mut simulation = synthetic_vapor_coverage(pattern, d);
    let instances = ["band", "back"];
    let materials: Vec<_> = instances
        .iter()
        .map(|id| {
            let piece = pattern.raw()["pieces"]
                .as_array()
                .unwrap()
                .iter()
                .find(|piece| piece["id"] == *id)
                .unwrap();
            frozen_vapor_material(&piece["fabric"]["material"])
        })
        .collect();
    let by_hash: std::collections::BTreeMap<_, _> = materials
        .iter()
        .map(|material| (material.sha256(), material))
        .collect();
    let hashes: Vec<_> = by_hash.keys().copied().collect();
    let columns: Vec<_> = materials
        .iter()
        .map(|material| {
            hashes
                .iter()
                .position(|hash| *hash == material.sha256())
                .unwrap()
        })
        .collect();
    let (triangles, owners, areas) = if refined {
        (
            vec![[0, 4, 2], [4, 1, 2], [1, 3, 2]],
            vec![0, 0, 1],
            vec![0.125, 0.125, 0.25],
        )
    } else {
        (vec![[0, 1, 2], [1, 3, 2]], vec![0, 1], vec![0.25, 0.25])
    };
    simulation["positions_m"] = json!([
        [0., 0., 0.01],
        [0.5, 0., 0.01],
        [0., 1., 0.01],
        [1., 0., 0.01],
        [0.25, 0., 0.01]
    ]);
    simulation["triangles"] = json!(triangles);
    let receipts: Vec<_> = instances
        .iter()
        .zip(&materials)
        .enumerate()
        .map(|(i, (instance, material))| {
            json!({"instance":instance, "piece_index":i,
            "record_sha256":material.sha256(), "rest_area_m2":0.25, "mass_kg":0.05})
        })
        .collect();
    let records: serde_json::Map<_, _> = by_hash
        .iter()
        .map(|(hash, material)| (hash.to_string(), json!({"record":material})))
        .collect();
    simulation["drape"]["material"] = json!({"basis":"per_piece_characterization",
        "instances":receipts, "triangle_instance":owners, "records":records});
    let patches: Vec<_> = triangles
        .iter()
        .zip(&owners)
        .zip(&areas)
        .map(|((vertices, &owner), area)| {
            json!({"vertices":vertices,
            "reference_area_m2":area, "column":columns[owner]})
        })
        .collect();
    let source = json!({"schema":"ftx-vapor-coverage-source/1", "positions_m":simulation["positions_m"],
        "patches":patches, "instances":instances, "triangle_instance":owners,
        "materials":by_hash.values().map(|m| m.frozen_selection()).collect::<Vec<_>>(),
        "material_report_sha256":digest(&simulation["drape"]["material"])});
    let result = &mut simulation["vapor_coverage"]["result"];
    result["request"]["source"] = source;
    result["source_sha256"] = json!(digest(&result["request"]["source"]));
    result["request_sha256"] = json!(digest(&result["request"]));
    result["projection"]["regions"] = json!(owners.iter().zip(&areas).enumerate()
        .map(|(i, (_, area))| json!({"id":format!("body-0-region-{i}"), "body_triangle":0,
            "body_fraction":area/0.5, "layers":[{"layer":"shell", "cloth_triangle":i, "fraction":1.}]}))
        .collect::<Vec<_>>());
    let mut power = 0.;
    let mut flow = 0.;
    let regions: Vec<_> = owners
        .iter()
        .zip(&areas)
        .enumerate()
        .map(|(i, (&owner, &area))| {
            let vapor = canonical_vapor_column(
                &result["request"]["conditions"],
                &result["request"]["source"]["materials"][columns[owner]],
                area,
            );
            power += vapor["potential_heat_flow_w"].as_f64().unwrap();
            flow += vapor["potential_mass_flow_kg_s"].as_f64().unwrap();
            json!({"id":format!("body-0-region-{i}"), "body_triangle":0,
            "body_area_m2":area,"layer_area_m2":[area],"vapor":vapor})
        })
        .collect();
    result["coverage"]["reported_bounds"] = vapor_test_bounds(
        regions
            .iter()
            .map(|region| (&region["vapor"]["reported_bounds"], 1.)),
        ["potential_heat_flow_w", "potential_mass_flow_kg_s"],
        [
            "covered_potential_heat_flow_w",
            "covered_potential_mass_flow_kg_s",
        ],
    );
    result["coverage"]["regions"] = json!(regions);
    result["coverage"]["covered_potential_heat_flow_w"] = json!(power);
    result["coverage"]["covered_potential_mass_flow_kg_s"] = json!(flow);
    result["mixed_protocols"] = json!(true);
    simulation
}

#[test]
fn covered_vapor_patch_materials_and_owners_must_match_retained_instance_receipts() {
    let (pattern, request, descriptor) = vapor_piece_inputs();
    for fault in [
        "none",
        "patch_column",
        "owner",
        "instance_order",
        "canonical_receipt",
    ] {
        let report = serde_json::to_value(
            request
                .evaluate_with(&pattern, &descriptor, |key, p| {
                    let mut simulation = synthetic_piece_vapor(p, &descriptor, false);
                    if key == "a" {
                        let result = &mut simulation["vapor_coverage"]["result"];
                        let source = &mut result["request"]["source"];
                        match fault {
                            "patch_column" => {
                                source["patches"][0]["column"] =
                                    source["patches"][1]["column"].clone()
                            }
                            "owner" => source["triangle_instance"] = json!([1, 0]),
                            "instance_order" => source["instances"] = json!(["back", "band"]),
                            "canonical_receipt" => {
                                let selected = &mut source["materials"][0];
                                selected["record"]["properties"]["vapor_resistance"]["value"] =
                                    json!(1.);
                                selected["expected_sha256"] = Value::Null;
                                *selected =
                                    json!(frozen_vapor_material(selected).frozen_selection());
                            }
                            _ => {}
                        }
                        result["source_sha256"] = json!(digest(&result["request"]["source"]));
                        result["request_sha256"] = json!(digest(&result["request"]));
                    }
                    Ok(simulation)
                })
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            report["comparison"]["frontier"],
            if fault == "none" {
                json!(["a"])
            } else {
                json!(["b"])
            },
            "{fault}"
        );
        assert_eq!(
            report["manufacturing"]["alternatives"][0]["cutting"]["status"],
            "available"
        );
    }
}

#[test]
fn covered_vapor_protocols_preserve_instances_without_binding_to_triangulation() {
    let (pattern, mut request, descriptor) = vapor_piece_inputs();
    let evaluate = |request: &Request| {
        serde_json::to_value(
            request
                .evaluate_with(&pattern, &descriptor, |key, p| {
                    Ok(synthetic_piece_vapor(p, &descriptor, key == "b"))
                })
                .unwrap(),
        )
        .unwrap()
    };
    let same = evaluate(&request);
    assert_eq!(same["comparison"]["frontier"], json!(["a"]));
    assert_eq!(same["comparison"]["incomparable"], json!([]));
    let pieces = &mut request.variants[1].changes.pieces;
    let band = pieces["band"].clone();
    let back = pieces["back"].clone();
    pieces.insert("band".into(), back);
    pieces.insert("back".into(), band);
    let swapped = evaluate(&request);
    assert_eq!(swapped["comparison"]["frontier"], json!(["a", "b"]));
    assert_eq!(
        swapped["comparison"]["incomparable"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    for alternative in swapped["comparison"]["alternatives"].as_array().unwrap() {
        assert_eq!(alternative["eligible"], true);
    }
    request.comparison.as_mut().unwrap().allow_mixed_protocols = true;
    assert_eq!(evaluate(&request)["comparison"]["frontier"], json!(["a"]));
}

fn vapor_inputs() -> (Request, Value) {
    let mut r = request();
    for (variant, ret) in r.variants.iter_mut().zip([2.0, 7.0]) {
        let record = variant
            .changes
            .default
            .as_mut()
            .unwrap()
            .record
            .as_mut()
            .unwrap();
        record.properties.remove("thermal_conductivity");
        record.properties.insert(
            "vapor_resistance".into(),
            serde_json::from_value(json!({
            "value":ret,"unit":"m2*Pa/W","source":"test","evidence":"estimate",
            "method":"Synthetic Ret protocol","conditions":{"temperature_c":"35"}}))
            .unwrap(),
        );
    }
    r.comparison = Some(
        serde_json::from_value(json!({"basis":"nominal",
        "objectives":[{"metric":"independent_vapor_potential_heat_flow_w","direction":"maximize"}],
        "constraints":[{"metric":"independent_vapor_potential_mass_flow_kg_s","minimum":0.0}],
        "required_consumers":["vapor_surface"]}))
        .unwrap(),
    );
    let mut d = descriptor();
    d["scenario"] = json!({"vapor_surface":{"schema":"ftx-vapor-surface-conditions/1",
        "boundaries":[{"inside_vapor_pressure_pa":2000.0,"outside_vapor_pressure_pa":1000.0,
            "inside_film_resistance_m2_pa_w":1.0,"outside_film_resistance_m2_pa_w":4.0,
            "latent_heat_j_kg":2000000.0}],
        "required_conditions":{},"required_method":null,"allow_mixed_protocols":true}});
    (r, d)
}

// Explicit callback fixture: canonical source/transport columns on synthetic triangles exercise
// admission and ownership. The production sewn-geometry regression lives in ftx-wasm.
fn synthetic_vapor(p: &Pattern, d: &Value, selections: Vec<Value>, assigned: &[usize]) -> Value {
    use ftx_pattern::{
        material_catalog::Selection,
        materials::{CATALOG_SCHEMA, Catalog},
    };
    let catalog = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: vec![],
    };
    let records: Vec<_> = selections
        .into_iter()
        .map(|s| {
            catalog
                .resolve(&serde_json::from_value::<Selection>(s).unwrap())
                .unwrap()
        })
        .collect();
    let selections: Vec<_> = records.iter().map(|r| r.frozen_selection()).collect();
    let conditions = &d["scenario"]["vapor_surface"];
    let columns: Vec<Value> = selections
        .iter()
        .map(|selection| {
            let mut request = conditions["boundaries"][0].clone();
            request.as_object_mut().unwrap().extend(
                json!({"schema":"ftx-vapor-stack/1",
            "layers":[{"key":"sheet","selection":selection}],"area_m2":1.0,
            "required_conditions":conditions["required_conditions"],
            "required_method":conditions["required_method"],
            "allow_mixed_protocols":conditions["allow_mixed_protocols"]})
                .as_object()
                .unwrap()
                .clone(),
            );
            let request: ftx_pattern::material_vapor::Request =
                serde_json::from_value(request).unwrap();
            serde_json::to_value(request.evaluate(&catalog).unwrap()).unwrap()
        })
        .collect();
    let positions: Vec<_> = assigned
        .iter()
        .enumerate()
        .flat_map(|(i, _)| {
            let x = 2.0 * i as f64;
            [[x, 0.0, 0.0], [x + 1.0, 0.0, 0.0], [x, 1.0, 0.0]]
        })
        .collect();
    let triangles: Vec<_> = assigned
        .iter()
        .enumerate()
        .map(|(i, _)| [3 * i, 3 * i + 1, 3 * i + 2])
        .collect();
    let topology: Vec<_> = triangles.iter().zip(assigned).map(|(vertices, column)|
        json!({"vertices":vertices,"column":column,"reference_area_m2":0.5})).collect();
    let owners: Vec<_> = (0..assigned.len()).collect();
    let instances: Vec<_> = owners.iter().map(|i| format!("band-{i}")).collect();
    let mut material = json!({"basis":"per_piece_characterization","records":{},"instances":[]});
    for (instance, &column) in instances.iter().zip(assigned) {
        let record = &records[column];
        material["records"][record.sha256()] = json!({"record":record});
        material["instances"]
            .as_array_mut()
            .unwrap()
            .push(json!({"instance":instance,
            "record_sha256":record.sha256()}));
    }
    let mut model = json!({"schema":"ftx-vapor-surface/1","columns":selections,
        "vertex_count":positions.len(),"patches":topology,
        "geometry_policy":"fixed source Ret per current cloth area; independent sheets"});
    model["sha256"] = json!(digest(&model));
    let mut frame = json!({"schema":"ftx-drape-vapor-surface/1","available":true,
        "model":model,"positions_m":positions,"triangle_instance":owners,"instances":instances,
        "material_report_sha256":digest(&material),
        "scope":"retained independent sheet geometry; transport requires explicit vapor conditions; no inferred body coverage"});
    frame["sha256"] = json!(digest(&frame));
    let mut frozen = conditions.clone();
    frozen["model_sha256"] = model["sha256"].clone();
    let request = json!({"report":{"material":material,"vapor_surface":frame},"conditions":frozen});
    let mut heat = 0.0;
    let mut mass = 0.0;
    let patches: Vec<_> = assigned
        .iter()
        .enumerate()
        .map(|(instance, &column)| {
            let q = columns[column]["potential_heat_flux_w_m2"]
                .as_f64()
                .unwrap();
            let m = columns[column]["potential_mass_flux_kg_m2_s"]
                .as_f64()
                .unwrap();
            heat += q * 0.5;
            mass += m * 0.5;
            json!({"column":column,"instance":instance,"area_m2":0.5,
            "potential_heat_flux_w_m2":q,"potential_mass_flux_kg_m2_s":m,
            "potential_heat_flow_w":q*0.5,"potential_mass_flow_kg_s":m*0.5})
        })
        .collect();
    let mut v = synthetic(p, 0.0, 0.1);
    let bounds = vapor_test_bounds(
        assigned
            .iter()
            .map(|&column| (&columns[column]["reported_bounds"], 0.5)),
        ["potential_heat_flux_w_m2", "potential_mass_flux_kg_m2_s"],
        ["potential_heat_flow_w", "potential_mass_flow_kg_s"],
    );
    v["surface"] = json!({"status":"refused","refusal":{"message":"No dry heat data"}});
    v["drape"]["material"] = material;
    v["drape"]["vapor_surface"] = frame.clone();
    v["positions_m"] = json!(positions);
    v["positions_sha256"] = json!(digest(&v["positions_m"]));
    v["triangles"] = json!(triangles);
    v["vapor_surface"] = json!({"status":"available","result":{
        "schema":"ftx-vapor-surface-result/1","request_sha256":digest(&request),"request":request,
        "drape_vapor_frame_sha256":frame["sha256"],"model_sha256":model["sha256"],
        "positions_m":positions,"patch_topology":topology,"triangle_instance":owners,"instances":instances,
        "columns":columns,"patches":patches,"area_m2":assigned.len() as f64 * 0.5,
        "potential_heat_flow_w":heat,"potential_mass_flow_kg_s":mass,"reported_bounds":bounds}});
    v
}

fn run_vapor(r: &Request, d: &Value, mut change: impl FnMut(&str, &mut Value)) -> Value {
    serde_json::to_value(
        r.evaluate_with(&Pattern::from_json(PATTERN).unwrap(), d, |key, p| {
            let mut v = synthetic_vapor(p, d, vec![p.raw()["fabric"]["material"].clone()], &[0]);
            change(key, &mut v);
            Ok(v)
        })
        .unwrap(),
    )
    .unwrap()
}

#[test]
fn draped_vapor_metrics_use_qualified_sheet_totals_independently_from_dry_heat() {
    let (mut r, d) = vapor_inputs();
    assert!(r.evaluate(&Pattern::from_json(PATTERN).unwrap()).is_err());
    let report = run_vapor(&r, &d, |_, _| {});
    assert_eq!(report["comparison"]["frontier"], json!(["a"]));
    let readings = &report["comparison"]["alternatives"][0]["readings"];
    let heat = &readings["independent_vapor_potential_heat_flow_w"];
    assert_eq!(heat["value"], 500.0 / 7.0);
    assert_eq!(heat["unit"], "W");
    assert_eq!(
        readings["independent_vapor_potential_mass_flow_kg_s"]["unit"],
        "kg/s"
    );
    assert_eq!(report["comparison"]["alternatives"][0]["eligible"], true);
    r.comparison.as_mut().unwrap().constraints[0].minimum = Some(0.00003);
    assert_eq!(
        run_vapor(&r, &d, |_, _| {})["comparison"]["alternatives"][1]["eligible"],
        false
    );
    r.comparison.as_mut().unwrap().basis =
        ftx_export::material_study::comparison::Basis::ReportedBounds;
    let unbounded = run_vapor(&r, &d, |_, _| {});
    assert_eq!(unbounded["comparison"]["frontier"], json!([]));
    assert_eq!(
        unbounded["simulations"][0]["evaluation"]["result"]["vapor_surface"]["status"],
        "available"
    );
}

#[test]
fn reported_sheet_ranges_drive_inclusive_constraints_and_leave_overlap_unresolved() {
    let (mut input, descriptor) = vapor_inputs();
    input.comparison.as_mut().unwrap().basis =
        ftx_export::material_study::comparison::Basis::ReportedBounds;
    bound_vapor_variants(&mut input, [Some([1., 4.]), Some([5., 9.])]);
    let result = run_vapor(&input, &descriptor, |_, _| {});
    assert_eq!(
        result["comparison"]["frontier"],
        json!(["a"]),
        "{}",
        result["comparison"]
    );
    let reading = &result["comparison"]["alternatives"][0]["readings"]["independent_vapor_potential_heat_flow_w"];
    assert_eq!(reading["reported_bounds"], json!([500. / 9., 500. / 6.]));
    input.comparison.as_mut().unwrap().constraints.push(
        serde_json::from_value(json!({"metric":"independent_vapor_potential_heat_flow_w",
            "minimum":500./9.,"maximum":500./6.}))
        .unwrap(),
    );
    let inclusive = run_vapor(&input, &descriptor, |_, _| {});
    assert_eq!(inclusive["comparison"]["frontier"], json!(["a"]));
    // A nominally passing threshold still excludes an interval extending below that threshold.
    input.comparison.as_mut().unwrap().constraints[1].minimum = Some(500. / 7.);
    assert_eq!(
        run_vapor(&input, &descriptor, |_, _| {})["comparison"]["frontier"],
        json!([])
    );
    input.comparison.as_mut().unwrap().constraints.truncate(1);
    bound_vapor_variants(&mut input, [Some([1., 8.]), Some([5., 9.])]);
    let overlap = run_vapor(&input, &descriptor, |_, _| {});
    assert_eq!(overlap["comparison"]["frontier"], json!(["a", "b"]));
    assert_eq!(
        overlap["comparison"]["incomparable"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    bound_vapor_variants(&mut input, [None, Some([5., 9.])]);
    let missing = run_vapor(&input, &descriptor, |_, _| {});
    assert_eq!(missing["comparison"]["frontier"], json!(["b"]));
    assert_eq!(
        missing["comparison"]["alternatives"][0]["readings"]["independent_vapor_potential_heat_flow_w"]
            ["value"],
        500. / 7.
    );
    assert_eq!(
        missing["simulations"][0]["evaluation"]["result"]["vapor_surface"]["status"],
        "available"
    );

    bound_vapor_variants(&mut input, [Some([1., 4.]), Some([5., 9.])]);
    let forged = run_vapor(&input, &descriptor, |key, simulation| {
        if key == "a" {
            simulation["vapor_surface"]["result"]["reported_bounds"] = json!({
                "potential_heat_flow_w":[60.,80.],
                "potential_mass_flow_kg_s":[60./2000000.,80./2000000.]});
        }
    });
    assert_eq!(forged["comparison"]["frontier"], json!(["b"]));
}

#[test]
fn draped_vapor_missing_stale_or_malformed_results_never_rank_as_zero() {
    let (mut r, d) = vapor_inputs();
    r.comparison.as_mut().unwrap().allow_mixed_protocols = true;
    for (path, value) in [
        ("/vapor_surface/status", json!("refused")),
        ("/vapor_surface/result/potential_heat_flow_w", Value::Null),
        (
            "/vapor_surface/result/potential_mass_flow_kg_s",
            json!(-1.0),
        ),
        ("/vapor_surface/result/model_sha256", json!("stale")),
        (
            "/vapor_surface/result/request/conditions/boundaries/0/latent_heat_j_kg",
            json!(1.0),
        ),
        (
            "/vapor_surface/result/columns/0/layers/0/observation/method",
            json!("stale"),
        ),
        (
            "/vapor_surface/result/columns/0/layers/0/source/reference",
            json!("test:stale"),
        ),
        ("/vapor_surface/result/patches/0/area_m2", json!(1.0)),
        ("/vapor_surface/result/patches/0/instance", json!(1)),
        ("/positions_m/1/0", json!(2.0)),
        ("/drape/material", Value::Null),
        ("/drape/settled", json!(false)),
        ("/metrics/simulation_gating_clear", json!(false)),
    ] {
        let report = run_vapor(&r, &d, |key, v| {
            if key == "a" {
                *v.pointer_mut(path).unwrap() = value.clone();
            }
        });
        assert_eq!(report["comparison"]["frontier"], json!(["b"]), "{path}");
        assert_eq!(
            report["comparison"]["alternatives"][0]["readings"],
            json!({}),
            "{path}"
        );
        assert_eq!(
            report["manufacturing"]["alternatives"][0]["cutting"]["status"],
            "available"
        );
    }
    // Requiring this domain alone still validates its whole result and declared scenario request.
    r.comparison.as_mut().unwrap().objectives = serde_json::from_value(json!([
        {"metric":"simulated_cloth_mass_kg","direction":"minimize"}]))
    .unwrap();
    r.comparison.as_mut().unwrap().constraints.clear();
    let mut absent = d.clone();
    absent["scenario"]["vapor_surface"] = Value::Null;
    let report = serde_json::to_value(
        r.evaluate_with(&Pattern::from_json(PATTERN).unwrap(), &absent, |_, p| {
            Ok(synthetic_vapor(
                p,
                &d,
                vec![p.raw()["fabric"]["material"].clone()],
                &[0],
            ))
        })
        .unwrap(),
    )
    .unwrap();
    assert_eq!(report["comparison"]["frontier"], json!([]));
    r.comparison.as_mut().unwrap().required_consumers.clear();
    assert_eq!(
        run_vapor(&r, &d, |_, v| v["vapor_surface"] =
            json!({"status":"refused"}))["comparison"]["frontier"],
        json!(["a", "b"])
    );
}

#[test]
fn draped_vapor_protocols_follow_instance_ownership_not_column_order_or_unused_records() {
    let (mut r, d) = vapor_inputs();
    let evaluate = |r: &Request, case: &str| {
        serde_json::to_value(
            r.evaluate_with(&Pattern::from_json(PATTERN).unwrap(), &d, |key, p| {
                let first = p.raw()["fabric"]["material"].clone();
                let mut second = first.clone();
                second["expected_sha256"] = Value::Null;
                second["record"]["properties"]["vapor_resistance"]["method"] =
                    json!(if case == "unused" && key == "b" {
                        "Unused different protocol"
                    } else {
                        "Second synthetic protocol"
                    });
                Ok(match (case, key) {
                    ("reordered", "b") => synthetic_vapor(p, &d, vec![second, first], &[1, 0]),
                    ("swapped_owners", "b") => synthetic_vapor(p, &d, vec![first, second], &[1, 0]),
                    ("unused", _) => synthetic_vapor(p, &d, vec![first, second], &[0]),
                    _ => synthetic_vapor(p, &d, vec![first, second], &[0, 1]),
                })
            })
            .unwrap(),
        )
        .unwrap()
    };
    for case in ["unused", "reordered"] {
        let report = evaluate(&r, case);
        assert_eq!(report["comparison"]["frontier"], json!(["a"]), "{case}");
        assert_eq!(report["comparison"]["incomparable"], json!([]), "{case}");
    }
    // Within-result consent is already true. It must not authorize cross-alternative ordering.
    let strict = evaluate(&r, "swapped_owners");
    assert_eq!(strict["comparison"]["frontier"], json!(["a", "b"]));
    assert_eq!(
        strict["comparison"]["incomparable"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    r.comparison.as_mut().unwrap().allow_mixed_protocols = true;
    assert_eq!(
        evaluate(&r, "swapped_owners")["comparison"]["frontier"],
        json!(["a"])
    );
}

#[test]
fn independent_vapor_objectives_read_admitted_sheet_outputs_with_fixed_units() {
    let (mut r, d) = vapor_inputs();
    r.comparison = Some(
        serde_json::from_value(json!({"basis":"nominal","objectives":[
        {"metric":"independent_vapor_potential_heat_flow_w","direction":"maximize"},
        {"metric":"independent_vapor_potential_mass_flow_kg_s","direction":"maximize"}],
        "required_consumers":["vapor_surface"]}))
        .unwrap(),
    );
    // Canonical Ret values give exactly 3 W and 1 W under the declared 0.5 m2 geometry.
    for (variant, ret) in r.variants.iter_mut().zip([1000.0 / 6.0 - 5.0, 495.0]) {
        variant
            .changes
            .default
            .as_mut()
            .unwrap()
            .record
            .as_mut()
            .unwrap()
            .properties
            .get_mut("vapor_resistance")
            .unwrap()
            .value = ret;
    }
    assert!(r.evaluate(&Pattern::from_json(PATTERN).unwrap()).is_err());
    let report = run_vapor(&r, &d, |_, v| {
        // Convenient summaries cannot substitute for the actual requested outcome.
        v["metrics"]["independent_vapor_potential_heat_flow_w"] = json!(999.0);
    });
    assert_eq!(report["comparison"]["frontier"], json!(["a"]));
    let a = &report["comparison"]["alternatives"][0];
    let heat = &a["readings"]["independent_vapor_potential_heat_flow_w"];
    let mass = &a["readings"]["independent_vapor_potential_mass_flow_kg_s"];
    assert_eq!(heat["value"], 3.0);
    assert_eq!(heat["unit"], "W");
    assert_eq!(heat["consumer"], "vapor_surface");
    assert_eq!(mass["value"], 3.0 / 2e6);
    assert_eq!(mass["unit"], "kg/s");
    assert_eq!(report["comparison"]["physical_validation_claim"], false);
    r.comparison.as_mut().unwrap().basis =
        ftx_export::material_study::comparison::Basis::ReportedBounds;
    let unbounded = run_vapor(&r, &d, |_, _| {});
    assert_eq!(unbounded["comparison"]["frontier"], json!([]));
    for (candidate, nominal_heat) in unbounded["comparison"]["alternatives"]
        .as_array()
        .unwrap()
        .iter()
        .zip([3., 1.])
    {
        assert_eq!(candidate["eligible"], false);
        for (metric, value) in [
            ("independent_vapor_potential_heat_flow_w", nominal_heat),
            (
                "independent_vapor_potential_mass_flow_kg_s",
                nominal_heat / 2e6,
            ),
        ] {
            assert_eq!(candidate["readings"][metric]["value"], value);
            assert!(candidate["readings"][metric]["reported_bounds"].is_null());
        }
    }
    assert_eq!(
        unbounded["simulations"][0]["evaluation"]["result"]["vapor_surface"]["status"],
        "available"
    );
}

#[test]
fn independent_vapor_requires_fresh_complete_outputs_even_with_protocol_override() {
    let (mut r, mut d) = vapor_inputs();
    r.comparison.as_mut().unwrap().allow_mixed_protocols = true;
    // Equal prescribed reservoirs produce a real zero. Missing outcomes cannot stand in for it.
    d["scenario"]["vapor_surface"]["boundaries"][0]["inside_vapor_pressure_pa"] = json!(1000.0);
    for fault in [
        "absent",
        "refused",
        "heat",
        "mass",
        "negative",
        "settled",
        "gating",
        "identity",
        "columns",
        "observation",
        "unit",
        "evidence",
        "source",
    ] {
        let report = run_vapor(&r, &d, |key, v| {
            if key == "a" {
                match fault {
                    "absent" => {
                        v.as_object_mut().unwrap().remove("vapor_surface");
                    }
                    "refused" => v["vapor_surface"] = json!({"status":"refused"}),
                    "heat" => v["vapor_surface"]["result"]["potential_heat_flow_w"] = Value::Null,
                    "mass" => {
                        v["vapor_surface"]["result"]["potential_mass_flow_kg_s"] = Value::Null
                    }
                    "negative" => {
                        v["vapor_surface"]["result"]["potential_heat_flow_w"] = json!(-1.0)
                    }
                    "settled" => v["drape"]["settled"] = json!(false),
                    "gating" => v["metrics"]["simulation_gating_clear"] = json!(false),
                    "identity" => v["pattern_sha256"] = json!("stale"),
                    "columns" => v["vapor_surface"]["result"]["columns"] = json!([]),
                    "observation" => {
                        v["vapor_surface"]["result"]["columns"][0]["layers"][0]["observation"] =
                            Value::Null
                    }
                    "unit" => {
                        v["vapor_surface"]["result"]["columns"][0]["layers"][0]["observation"]["unit"] =
                            json!("unqualified")
                    }
                    "evidence" => {
                        v["vapor_surface"]["result"]["columns"][0]["layers"][0]["observation"]
                            .as_object_mut()
                            .unwrap()
                            .remove("evidence");
                    }
                    "source" => {
                        v["vapor_surface"]["result"]["columns"][0]["layers"][0]["observation"]["source"] =
                            json!(" ")
                    }
                    _ => unreachable!(),
                }
            }
        });
        assert_eq!(report["comparison"]["frontier"], json!(["b"]), "{fault}");
        assert_eq!(
            report["comparison"]["alternatives"][0]["eligible"], false,
            "{fault}"
        );
        assert_eq!(
            report["comparison"]["alternatives"][1]["readings"]["independent_vapor_potential_heat_flow_w"]
                ["value"],
            0.0
        );
        assert_eq!(
            report["manufacturing"]["alternatives"][0]["cutting"]["status"],
            "available"
        );
    }
    // A declared vapor requirement also gates a manufacturing-only objective.
    r.comparison = Some(
        serde_json::from_value(json!({"basis":"nominal",
        "objectives":[{"metric":"stock_mass_kg","direction":"minimize"}],
        "required_consumers":["vapor_surface"]}))
        .unwrap(),
    );
    let report = run_vapor(&r, &d, |key, v| {
        if key == "a" {
            v["vapor_surface"] = json!({"status":"not_requested"});
        }
    });
    assert_eq!(report["comparison"]["frontier"], json!(["b"]));
}

#[test]
fn independent_vapor_protocols_follow_consumed_qualifications_not_column_deduplication() {
    let (mut r, d) = vapor_inputs();
    // Dry heat is not required, so its unrelated protocol cannot block vapor ranking.
    let p: Value = serde_json::from_str(PATTERN).unwrap();
    let mut dry = p["fabric"]["material"]["record"]["properties"]["thermal_conductivity"].clone();
    dry["method"] = json!("Unused dry protocol");
    r.variants[0]
        .changes
        .default
        .as_mut()
        .unwrap()
        .record
        .as_mut()
        .unwrap()
        .properties
        .insert(
            "thermal_conductivity".into(),
            serde_json::from_value(dry).unwrap(),
        );
    let report = serde_json::to_value(
        r.evaluate_with(&Pattern::from_json(PATTERN).unwrap(), &d, |key, p| {
            let first = p.raw()["fabric"]["material"].clone();
            Ok(if key == "a" {
                let mut duplicate = first.clone();
                duplicate["expected_sha256"] = Value::Null;
                duplicate["record"]["properties"]["vapor_resistance"]["value"] = json!(10.0);
                duplicate["record"]["properties"]["vapor_resistance"]["source"] =
                    json!("different-source-id");
                duplicate["record"]["sources"]["different-source-id"] =
                    duplicate["record"]["sources"]["test"].clone();
                synthetic_vapor(p, &d, vec![first, duplicate], &[0, 1])
            } else {
                synthetic_vapor(p, &d, vec![first], &[0, 0])
            })
        })
        .unwrap(),
    )
    .unwrap();
    assert_eq!(report["comparison"]["frontier"], json!(["a"]));
    assert_eq!(report["comparison"]["incomparable"], json!([]));
    for field in ["method", "conditions", "evidence"] {
        let mut changed = r.clone();
        let observation = changed.variants[0]
            .changes
            .default
            .as_mut()
            .unwrap()
            .record
            .as_mut()
            .unwrap()
            .properties
            .get_mut("vapor_resistance")
            .unwrap();
        match field {
            "method" => observation.method = "Different consumed method".into(),
            "conditions" => {
                observation
                    .conditions
                    .insert("conditioning".into(), "different consumed state".into());
            }
            _ => observation.evidence = serde_json::from_value(json!("published")).unwrap(),
        }
        let report = run_vapor(&changed, &d, |_, _| {});
        assert_eq!(
            report["comparison"]["frontier"],
            json!(["a", "b"]),
            "{field}"
        );
        assert_eq!(
            report["comparison"]["incomparable"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        changed.comparison.as_mut().unwrap().allow_mixed_protocols = true;
        assert_eq!(
            run_vapor(&changed, &d, |_, _| {})["comparison"]["frontier"],
            json!(["a"]),
            "{field}"
        );
    }
}

#[test]
fn draped_vapor_material_ownership_and_within_result_protocol_consent_are_independent_gates() {
    let (r, d) = vapor_inputs();
    for fault in [
        "rehashed_owner_material_swap",
        "mixed_columns_without_consent",
    ] {
        let mut d = d.clone();
        if fault == "mixed_columns_without_consent" {
            d["scenario"]["vapor_surface"]["allow_mixed_protocols"] = json!(false);
        }
        let report = serde_json::to_value(
            r.evaluate_with(&Pattern::from_json(PATTERN).unwrap(), &d, |key, p| {
                let first = p.raw()["fabric"]["material"].clone();
                let mut second = first.clone();
                second["expected_sha256"] = Value::Null;
                second["record"]["properties"]["vapor_resistance"]["value"] = json!(10.0);
                if fault == "mixed_columns_without_consent" && key == "a" {
                    second["record"]["properties"]["vapor_resistance"]["method"] =
                        json!("Different column protocol");
                }
                let mut v = synthetic_vapor(p, &d, vec![first, second], &[0, 1]);
                if fault == "rehashed_owner_material_swap" && key == "a" {
                    let a = v["drape"]["material"]["instances"][0]["record_sha256"].clone();
                    let b = v["drape"]["material"]["instances"][1]["record_sha256"].clone();
                    v["drape"]["material"]["instances"][0]["record_sha256"] = b;
                    v["drape"]["material"]["instances"][1]["record_sha256"] = a;
                    let mut frame = v["drape"]["vapor_surface"].clone();
                    frame["material_report_sha256"] = json!(digest(&v["drape"]["material"]));
                    frame.as_object_mut().unwrap().remove("sha256");
                    frame["sha256"] = json!(digest(&frame));
                    v["drape"]["vapor_surface"] = frame.clone();
                    v["vapor_surface"]["result"]["drape_vapor_frame_sha256"] =
                        frame["sha256"].clone();
                    v["vapor_surface"]["result"]["request"]["report"] =
                        json!({"material":v["drape"]["material"],"vapor_surface":frame});
                    v["vapor_surface"]["result"]["request_sha256"] =
                        json!(digest(&v["vapor_surface"]["result"]["request"]));
                }
                Ok(v)
            })
            .unwrap(),
        )
        .unwrap();
        assert_eq!(report["comparison"]["frontier"], json!(["b"]), "{fault}");
        assert_eq!(
            report["comparison"]["alternatives"][0]["readings"],
            json!({}),
            "{fault}"
        );
    }
}

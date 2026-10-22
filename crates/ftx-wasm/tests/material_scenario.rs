//! Whole-study production equality, frozen replay and independent failures on a synthetic cylinder.
use ftx_core::{Vec3, det};
use ftx_drape::{
    Body, PartFrame,
    run::DrapeBody,
    scene::PartFrames,
    study::{Scenario, Session},
};
use ftx_export::material_study::Request;
use ftx_pattern::Pattern;
use ftx_wasm::materials::scenario::on_body;
use serde_json::{Value, json};

const PATTERN: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-scenario-pattern.json");
const SCENARIO: &str = include_str!("../../../tests/fixtures/materials/synthetic-scenario.json");
struct Form;
impl PartFrames for Form {
    fn frame(&self, _: &str) -> Option<PartFrame> {
        Some(PartFrame {
            origin: Vec3::ZERO,
            up: Vec3::new(0., 0., 1.),
            front: Vec3::new(0., -1., 0.),
            lat: Vec3::new(1., 0., 0.),
        })
    }
    fn landmark_h(&self, _: &str, _: &str) -> Option<f64> {
        Some(20.)
    }
    fn section_radius(&self, _: &str, _: f64) -> f64 {
        30.
    }
}
fn form() -> Body {
    let mut vertices = Vec::new();
    for z in [-0.02, 0.12] {
        for i in 0..16 {
            let (s, c) = det::sin_cos(i as f64 * std::f64::consts::TAU / 16.);
            vertices.push(Vec3::new(0.03 * c, 0.03 * s, z));
        }
    }
    let mut triangles = Vec::new();
    for i in 0..16 {
        let j = (i + 1) % 16;
        triangles.extend([[i, j, j + 16], [i, j + 16, i + 16]]);
    }
    Body::new(vertices, triangles, 0.01).unwrap()
}
fn request() -> Request {
    let p: Value = serde_json::from_str(PATTERN).unwrap();
    let base = p["fabric"]["material"].clone();
    let mut thermal = base.clone();
    thermal["record"]["properties"]["thermal_conductivity"]["value"] = json!(0.1);
    let mut unavailable = base.clone();
    unavailable["record"]["properties"]
        .as_object_mut()
        .unwrap()
        .remove("warp_stiffness");
    serde_json::from_value(json!({"schema":"ftx-material-study/1",
        "variants":[{"key":"a","changes":{"schema":"ftx-material-bindings/1","default":base}},
        {"key":"b","changes":{"schema":"ftx-material-bindings/1","default":thermal}},
        {"key":"c","changes":{"schema":"ftx-material-bindings/1","default":unavailable}}],
        "cutting":{"copies":1,"gap_mm":1.,"edge_margin_mm":0.,"end_margin_mm":0.,"seam_allowance_mm":0.}
    })).unwrap()
}
#[test]
fn file_free_study_matches_real_consumers_and_keeps_manufacturing_after_drape_refusal() {
    let form = form();
    let body = DrapeBody {
        frames: &Form,
        collision: (&form.vertices, &form.triangles),
        display: (&form.vertices, &form.triangles),
        arm_deg: 35.,
    };
    let p = Pattern::from_json(PATTERN).unwrap();
    let r = request();
    let input = serde_json::to_string(&r).unwrap();
    let mut progress = Vec::new();
    let output = on_body(&body, PATTERN, &input, SCENARIO, |s| {
        progress.push(s.to_owned())
    })
    .unwrap();
    let result: Value = serde_json::from_str(&output).unwrap();
    let scenario: Scenario = serde_json::from_str(SCENARIO).unwrap();
    let mut session = Session::new(&scenario, &body).unwrap();
    let direct = r
        .evaluate_with(&p, &session.descriptor(), |_, p| {
            session.evaluate(p, |_| {})
        })
        .unwrap();
    assert_eq!(result, serde_json::to_value(direct).unwrap());
    assert!(!progress.is_empty());
    let rows = result["simulations"].as_array().unwrap();
    assert_eq!(rows[0]["evaluation"]["status"], "available");
    assert_eq!(rows[1]["evaluation"]["status"], "available");
    assert_eq!(rows[2]["evaluation"]["status"], "refused");
    assert_eq!(
        result["manufacturing"]["alternatives"][2]["cutting"]["status"],
        "available"
    );
    let a = &rows[0]["evaluation"]["result"];
    let b = &rows[1]["evaluation"]["result"];
    assert_eq!(a["positions_sha256"], b["positions_sha256"]);
    assert_ne!(
        a["metrics"]["independent_surface_power_w"],
        b["metrics"]["independent_surface_power_w"]
    );
    assert_ne!(
        a["metrics"]["body_coverage_power_w"],
        b["metrics"]["body_coverage_power_w"]
    );
    assert_eq!(
        output,
        on_body(&body, PATTERN, &input, SCENARIO, |_| {}).unwrap()
    );
    // Replay every material from its frozen changes against the original, not a mutated predecessor.
    let mut replay = r.clone();
    for (v, row) in replay
        .variants
        .iter_mut()
        .zip(result["manufacturing"]["alternatives"].as_array().unwrap())
    {
        v.changes = serde_json::from_value(row["frozen_changes"].clone()).unwrap();
    }
    let replay: Value = serde_json::from_str(
        &on_body(
            &body,
            PATTERN,
            &serde_json::to_string(&replay).unwrap(),
            SCENARIO,
            |_| {},
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(result["simulations"], replay["simulations"]);
    assert_eq!(p.raw(), &serde_json::from_str::<Value>(PATTERN).unwrap());
}
#[test]
fn material_substitution_recomputes_draped_vapor_without_dry_thermal_readiness() {
    let form = form();
    let body = DrapeBody {
        frames: &Form,
        collision: (&form.vertices, &form.triangles),
        display: (&form.vertices, &form.triangles),
        arm_deg: 35.,
    };
    let mut pattern: Value = serde_json::from_str(PATTERN).unwrap();
    pattern["fabric"]["material"]["record"]["properties"]
        .as_object_mut()
        .unwrap()
        .remove("thermal_conductivity");
    let missing = pattern["fabric"]["material"].clone();
    let mut low = missing.clone();
    low["record"]["properties"]["vapor_resistance"] = json!({
        "value":20.0,"unit":"m2*Pa/W","source":"test","evidence":"estimate",
        "method":"Synthetic geometry-coupling test",
        "conditions":{"specimen":"synthetic-scenario-sheet","conditioning":"synthetic"}});
    let mut high = low.clone();
    high["record"]["properties"]["vapor_resistance"]["value"] = json!(40.0);
    let request = json!({"schema":"ftx-material-study/1","variants":[
        {"key":"low","changes":{"schema":"ftx-material-bindings/1","default":low}},
        {"key":"high","changes":{"schema":"ftx-material-bindings/1","default":high}},
        {"key":"missing","changes":{"schema":"ftx-material-bindings/1","default":missing}}]});
    let mut scenario: Value = serde_json::from_str(SCENARIO).unwrap();
    scenario["vapor_coverage"] = json!({
        "body_triangle_ids":[0],"instance_layers":{"band":"shell"},
        "settings":{"max_distance_m":0.08,"min_normal_cosine":0.1,
            "separation_tolerance_m":0.00001,"interfaces":[]},
        "conditions":{"boundaries":[{"inside_vapor_pressure_pa":2000.0,
            "outside_vapor_pressure_pa":1000.0,"inside_film_resistance_m2_pa_w":1.0,
            "outside_film_resistance_m2_pa_w":4.0,"latent_heat_j_kg":2_500_000.0}]}});
    let raw_pattern = pattern.to_string();
    let result: Value = serde_json::from_str(
        &on_body(
            &body,
            &raw_pattern,
            &request.to_string(),
            &scenario.to_string(),
            |_| {},
        )
        .unwrap(),
    )
    .unwrap();
    let rows = result["simulations"].as_array().unwrap();
    let row = |key: &str| {
        let row = rows.iter().find(|row| row["key"] == key).unwrap();
        assert_eq!(row["evaluation"]["status"], "available");
        &row["evaluation"]["result"]
    };
    let a = row("low");
    let b = row("high");
    assert_eq!(a["positions_sha256"], b["positions_sha256"]);
    assert_eq!(a["surface"]["status"], "refused");
    assert_eq!(a["coverage"]["status"], "refused");
    for simulated in [a, b] {
        let vapor = &simulated["vapor_coverage"];
        assert_eq!(vapor["status"], "available", "{vapor}");
        let report = &vapor["result"];
        assert!(report["coverage"]["covered_area_m2"].as_f64().unwrap() > 0.0);
        assert!(report["coverage"]["uncovered_area_m2"].as_f64().unwrap() > 0.0);
        let replay = report["replay_request_json"].as_str().unwrap();
        let standalone: Value =
            serde_json::from_str(&ftx_wasm::thermal::vapor_coverage(replay).unwrap()).unwrap();
        assert_eq!(&standalone, report);
    }
    let flow = |v: &Value| {
        v["vapor_coverage"]["result"]["coverage"]["covered_potential_mass_flow_kg_s"]
            .as_f64()
            .unwrap()
    };
    assert!(flow(a) > flow(b));
    assert_ne!(
        a["vapor_coverage"]["result"]["source_sha256"],
        b["vapor_coverage"]["result"]["source_sha256"]
    );
    assert_eq!(row("missing")["vapor_coverage"]["status"], "refused");
    for alternative in result["manufacturing"]["alternatives"].as_array().unwrap() {
        assert_eq!(alternative["binding"]["status"], "available");
        assert_eq!(
            alternative["binding"]["result"]["pattern"]["pieces"],
            pattern["pieces"]
        );
        assert_eq!(
            alternative["binding"]["result"]["pattern"]["seams"],
            pattern["seams"]
        );
    }
}

#[test]
fn aggregate_budget_refusals_do_not_create_free_successful_cells() {
    let form = form();
    let body = DrapeBody {
        frames: &Form,
        collision: (&form.vertices, &form.triangles),
        display: (&form.vertices, &form.triangles),
        arm_deg: 35.,
    };
    let mut scenario: Value = serde_json::from_str(SCENARIO).unwrap();
    scenario["max_vertex_frames"] = json!(1);
    let mut called = false;
    let result: Value = serde_json::from_str(
        &on_body(
            &body,
            PATTERN,
            &serde_json::to_string(&request()).unwrap(),
            &scenario.to_string(),
            |_| called = true,
        )
        .unwrap(),
    )
    .unwrap();
    assert!(!called);
    for row in result["simulations"].as_array().unwrap() {
        assert_eq!(row["evaluation"]["status"], "refused");
    }
    for row in result["manufacturing"]["alternatives"].as_array().unwrap() {
        assert_eq!(row["cutting"]["status"], "available");
    }
    scenario["options"]["unknown"] = json!(true);
    assert!(
        on_body(
            &body,
            PATTERN,
            &serde_json::to_string(&request()).unwrap(),
            &scenario.to_string(),
            |_| {}
        )
        .is_err()
    );
}

#[test]
fn computed_scenario_objectives_use_actual_heat_and_strain_without_weakening_admission() {
    let form = form();
    let body = DrapeBody {
        frames: &Form,
        collision: (&form.vertices, &form.triangles),
        display: (&form.vertices, &form.triangles),
        arm_deg: 35.,
    };
    let mut r = request();
    r.comparison = Some(
        serde_json::from_value(json!({"basis":"nominal", "objectives":[
        {"metric":"independent_surface_power_w","direction":"minimize"},
        {"metric":"simulated_cloth_mass_kg","direction":"minimize"}],
        "constraints":[{"metric":"max_absolute_edge_strain_pct","maximum":100.}],
        "required_consumers":["coverage"]}))
        .unwrap(),
    );
    let output: Value = serde_json::from_str(
        &on_body(
            &body,
            PATTERN,
            &serde_json::to_string(&r).unwrap(),
            SCENARIO,
            |_| {},
        )
        .unwrap(),
    )
    .unwrap();
    assert!(output["manufacturing"].get("comparison").is_none());
    assert_eq!(
        output["comparison"]["context_sha256"],
        output["request_sha256"]
    );
    assert_eq!(output["comparison"]["uncertainty_propagated"], false);
    for (candidate, simulation) in output["comparison"]["alternatives"]
        .as_array()
        .unwrap()
        .iter()
        .zip(output["simulations"].as_array().unwrap())
    {
        let actual = &simulation["evaluation"]["result"];
        let admitted = simulation["evaluation"]["status"] == "available"
            && actual["drape"]["settled"] == true
            && actual["metrics"]["simulation_gating_clear"] == true
            && actual["coverage"]["status"] == "available";
        assert_eq!(candidate["eligible"], admitted);
        if admitted {
            assert_eq!(
                candidate["readings"]["independent_surface_power_w"]["value"],
                actual["surface"]["result"]["steady"]["power_w"]
            );
            assert_eq!(
                candidate["readings"]["simulated_cloth_mass_kg"]["value"],
                actual["drape"]["mass_kg"]
            );
            let strain = &actual["drape"]["global_strain_pct"];
            assert_eq!(
                candidate["readings"]["max_absolute_edge_strain_pct"]["value"],
                strain["min"]
                    .as_f64()
                    .unwrap()
                    .abs()
                    .max(strain["max"].as_f64().unwrap().abs())
            );
        }
    }
    let original = &output["manufacturing"]["alternatives"];
    r.comparison = None;
    let baseline: Value = serde_json::from_str(
        &on_body(
            &body,
            PATTERN,
            &serde_json::to_string(&r).unwrap(),
            SCENARIO,
            |_| {},
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(original, &baseline["manufacturing"]["alternatives"]);
    assert_eq!(output["simulations"], baseline["simulations"]);
    assert_eq!(output["comparison"]["frontier"], json!([]));
    assert_eq!(
        output["simulations"][0]["evaluation"]["result"]["drape"]["settled"],
        false
    );
    let drape = &output["simulations"][0]["evaluation"]["result"]["drape"];
    assert_eq!(drape["numerical"]["settled"], false);
    let gating = drape["gating"].as_array().unwrap();
    assert_eq!(gating.len(), 1);
    assert!(
        gating[0]
            .as_str()
            .unwrap()
            .contains("not numerically settled")
    );
}

#[test]
fn joint_outfit_boundary_uses_real_contact_and_ordered_layer_heat_with_frozen_replay() {
    use ftx_drape::{run::DrapeOptions, study_input::PreparedInput};
    let form = form();
    let body = DrapeBody {
        frames: &Form,
        collision: (&form.vertices, &form.triangles),
        display: (&form.vertices, &form.triangles),
        arm_deg: 35.0,
    };
    let mut outfit: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-joint-outfit.json"
    ))
    .unwrap();
    for (g, r) in outfit["garments"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .zip([40.0, 43.0])
    {
        g["pattern"]["pieces"][0]["placement"]["radius_mm"] = json!(r);
    }
    let base = outfit["garments"][0]["pattern"]["fabric"]["material"].clone();
    let mut thermal = base.clone();
    thermal["record"]["properties"]["thermal_conductivity"]["value"] = json!(0.1);
    let request = json!({"schema":"ftx-material-study/1","variants":[
        {"key":"baseline","changes":{"schema":"ftx-material-bindings/1"}},
        {"key":"thermal","changes":{"schema":"ftx-material-bindings/1",
            "pieces":{"inner::band":thermal,"outer::band":thermal}}}]});
    let mut scenario: Value = serde_json::from_str(SCENARIO).unwrap();
    scenario["options"]["spacing"] = json!(10.0);
    scenario["options"]["self_contact"] =
        serde_json::to_value(DrapeOptions::default().contact).unwrap();
    scenario["coverage"]["body_triangle_ids"] = json!([24, 25]);
    scenario["coverage"]["instance_layers"] = json!({"inner::band":"lining","outer::band":"shell"});
    scenario["coverage"]["settings"]["interfaces"] = json!([{
        "inner":"lining","outer":"shell","resistance_m2_k_w":0.02,
        "basis":"Explicit synthetic interface, not inferred from garment order"}]);
    let options: Scenario = serde_json::from_value(scenario.clone()).unwrap();
    let prepared = PreparedInput::new(outfit.clone(), &options.options).unwrap();
    let output = on_body(
        &body,
        &outfit.to_string(),
        &request.to_string(),
        &scenario.to_string(),
        |_| {},
    )
    .unwrap();
    let report: Value = serde_json::from_str(&output).unwrap();
    let baseline = &report["simulations"][0]["evaluation"];
    let changed = &report["simulations"][1]["evaluation"];
    assert_eq!(baseline["status"], "available", "{baseline}");
    assert_eq!(changed["status"], "available", "{changed}");
    let baseline = &baseline["result"];
    let changed = &changed["result"];
    assert_eq!(baseline["drape"]["self_contact"]["enabled"], true);
    assert_eq!(baseline["positions_sha256"], changed["positions_sha256"]);
    assert_eq!(
        baseline["coverage"]["status"], "available",
        "{}",
        baseline["coverage"]
    );
    assert_eq!(
        changed["coverage"]["status"], "available",
        "{}",
        changed["coverage"]
    );
    assert!(
        baseline["coverage"]["result"]["projection"]["layout"]["regions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["layers"].as_array().is_some_and(|layers| {
                layers.len() == 2 && layers[0]["layer"] == "lining" && layers[1]["layer"] == "shell"
            })),
        "the fixture must actually conduct through both garments in series, not disjoint patches"
    );
    assert_ne!(
        baseline["metrics"]["body_coverage_power_w"],
        changed["metrics"]["body_coverage_power_w"]
    );
    // Same source and exact existing evaluators, not an independent WASM-specific calculation.
    let mut session = Session::new(&options, &body).unwrap();
    let mut descriptor = session.descriptor();
    prepared.annotate(&mut descriptor).unwrap();
    let request: Request = serde_json::from_value(request.clone()).unwrap();
    let direct = request
        .evaluate_with(prepared.pattern(), &descriptor, |_, p| {
            session.evaluate(p, |_| {})
        })
        .unwrap();
    assert_eq!(report, serde_json::to_value(direct).unwrap());
    let replay: Value = serde_json::from_str(
        &on_body(
            &body,
            report["scenario"]["outfit"]["pattern_json"]
                .as_str()
                .unwrap(),
            &serde_json::to_string(&request).unwrap(),
            &scenario.to_string(),
            |_| {},
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(replay["simulations"], report["simulations"]);
    assert!(replay["scenario"].get("outfit").is_none());
    scenario["options"]["self_contact"] = Value::Null;
    assert!(
        on_body(
            &body,
            &outfit.to_string(),
            &serde_json::to_string(&request).unwrap(),
            &scenario.to_string(),
            |_| {}
        )
        .is_err()
    );
}

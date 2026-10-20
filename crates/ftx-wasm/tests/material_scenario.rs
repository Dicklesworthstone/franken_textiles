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

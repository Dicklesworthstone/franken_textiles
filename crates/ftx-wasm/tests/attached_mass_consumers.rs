//! File-free browser boundaries use the real assembly and sewn-drape consumers for synthetic extras.
use ftx_core::{Vec3, det};
use ftx_drape::{
    Body, PartFrame,
    run::DrapeBody,
    scene::PartFrames,
    study::{Scenario, Session},
};
use ftx_export::material_study::Request;
use ftx_pattern::Pattern;
use ftx_wasm::materials::{self, scenario::on_body};
use serde_json::{Value, json};

const SCENARIO: &str = include_str!("../../../tests/fixtures/materials/synthetic-scenario.json");

fn extra() -> Value {
    json!({"id":"closure-set","mass":{"value":0.03,"unit":"kg","bounds":[0.028,0.032],
        "evidence":"estimate","source":"test","method":"Synthetic retained total group",
        "conditions":{"inventory":"one assembly total"}},
        "sources":{"test":{"reference":"test:attachment-consumers","accessed_on":"2026-10-20",
            "note":"Synthetic software input, not measured hardware"}},"allow_estimates":true})
}

fn inputs() -> (Value, Value) {
    let mut pattern: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-scenario-pattern.json"
    ))
    .unwrap();
    pattern["attached_mass"] = json!({"schema":"ftx-attached-mass/1","groups":[{
        "extra":extra(),"targets":[{"instance":"band","vertex":"a","fraction":0.25},
            {"instance":"band","vertex":"b","fraction":0.75}]}]});
    let light = pattern["fabric"]["material"].clone();
    let mut heavy = light.clone();
    heavy["record"]["properties"]["areal_density"]["value"] = json!(0.4);
    let request = json!({"schema":"ftx-material-study/1","variants":[
        {"key":"a-light","changes":{"schema":"ftx-material-bindings/1","default":light}},
        {"key":"b-heavy","changes":{"schema":"ftx-material-bindings/1","default":heavy}}],
        "assembly_mass":{"schema":"ftx-assembly-mass/1","inventory_complete":true,
            "seam_allowance_mm":0.0,"extras":[extra()]},
        "cutting":{"copies":1,"gap_mm":1.0,"edge_margin_mm":0.0,"end_margin_mm":0.0,"seam_allowance_mm":0.0}});
    (pattern, request)
}

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

#[test]
fn file_free_manufacturing_and_scenario_share_exact_source_mass_and_real_dynamics() {
    let (pattern, request) = inputs();
    let pattern_text = pattern.to_string();
    let request_text = request.to_string();
    let typed: Request = serde_json::from_value(request).unwrap();
    let direct = typed.evaluate(&Pattern::new(pattern.clone())).unwrap();
    let manufacturing = materials::study(&pattern_text, &request_text).unwrap();
    let manufacturing: Value = serde_json::from_str(&manufacturing).unwrap();
    assert_eq!(manufacturing, serde_json::to_value(direct).unwrap());
    for row in manufacturing["alternatives"].as_array().unwrap() {
        assert_eq!(
            row["assembly_mass"]["result"]["extras"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            row["assembly_mass"]["result"]["attached_inventory"],
            pattern["attached_mass"]
        );
    }
    let form = form();
    let body = DrapeBody {
        frames: &Form,
        collision: (&form.vertices, &form.triangles),
        display: (&form.vertices, &form.triangles),
        arm_deg: 35.,
    };
    let text = on_body(&body, &pattern_text, &request_text, SCENARIO, |_| {}).unwrap();
    let report: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["manufacturing"], manufacturing);
    let scenario: Scenario = serde_json::from_str(SCENARIO).unwrap();
    let mut session = Session::new(&scenario, &body).unwrap();
    let expected = typed
        .evaluate_with(
            &Pattern::new(pattern.clone()),
            &session.descriptor(),
            |_, p| session.evaluate(p, |_| {}),
        )
        .unwrap();
    assert_eq!(report, serde_json::to_value(expected).unwrap());
    let rows = report["simulations"].as_array().unwrap();
    for row in rows {
        assert_eq!(row["evaluation"]["status"], "available", "{row}");
        let drape = &row["evaluation"]["result"]["drape"];
        let mass = &drape["attached_mass"];
        assert_eq!(mass["input"], pattern["attached_mass"]);
        assert_eq!(mass["attached_mass_kg"], 0.03);
        assert_eq!(mass["cloth_mass_kg"], drape["mass_kg"]);
        assert!(
            (mass["total_inertial_mass_kg"].as_f64().unwrap()
                - mass["cloth_mass_kg"].as_f64().unwrap()
                - 0.03)
                .abs()
                < 1e-15
        );
        let targets = mass["groups"][0]["targets"].as_array().unwrap();
        assert_eq!(targets[0]["solver_vertex"], targets[1]["solver_vertex"]);
        assert_ne!(targets[0]["mesh_vertex"], targets[1]["mesh_vertex"]);
    }
    let light = &rows[0]["evaluation"]["result"];
    let heavy = &rows[1]["evaluation"]["result"];
    assert_eq!(
        2. * light["drape"]["mass_kg"].as_f64().unwrap(),
        heavy["drape"]["mass_kg"]
    );
    assert_eq!(
        light["drape"]["attached_mass"]["input_sha256"],
        heavy["drape"]["attached_mass"]["input_sha256"]
    );
    assert_ne!(light["positions_sha256"], heavy["positions_sha256"]);
    assert_eq!(
        text,
        on_body(&body, &pattern_text, &request_text, SCENARIO, |_| {}).unwrap()
    );
}

#[test]
fn unknown_or_denied_attachment_mass_refuses_browser_dynamics_without_erasing_manufacturing() {
    let form = form();
    let body = DrapeBody {
        frames: &Form,
        collision: (&form.vertices, &form.triangles),
        display: (&form.vertices, &form.triangles),
        arm_deg: 35.,
    };
    for denied in [false, true] {
        let (mut pattern, mut request) = inputs();
        request["assembly_mass"]["extras"] = json!([]);
        let extra = &mut pattern["attached_mass"]["groups"][0]["extra"];
        if denied {
            extra["allow_estimates"] = json!(false);
        } else {
            extra["mass"] = Value::Null;
        }
        let result: Value = serde_json::from_str(
            &on_body(
                &body,
                &pattern.to_string(),
                &request.to_string(),
                SCENARIO,
                |_| {},
            )
            .unwrap(),
        )
        .unwrap();
        let ordinary: Value = serde_json::from_str(
            &materials::study(&pattern.to_string(), &request.to_string()).unwrap(),
        )
        .unwrap();
        assert_eq!(result["manufacturing"], ordinary);
        for row in result["manufacturing"]["alternatives"].as_array().unwrap() {
            assert_eq!(row["binding"]["status"], "available");
            assert_eq!(row["cutting"]["status"], "available");
            assert_eq!(row["assembly_mass"]["status"], "available");
            assert!(
                row["assembly_mass"]["result"]["sheet_mass"]["kg"]
                    .as_f64()
                    .is_some()
            );
            assert!(row["assembly_mass"]["result"]["assembly_mass"]["kg"].is_null());
        }
        for row in result["simulations"].as_array().unwrap() {
            assert_eq!(row["evaluation"]["status"], "refused");
            assert!(
                row["evaluation"]["refusal"]["message"]
                    .as_str()
                    .unwrap()
                    .contains(if denied { "consent" } else { "unknown" })
            );
        }
    }
    let (pattern, mut request) = inputs();
    request["assembly_mass"]["extras"][0]["mass"]["value"] = json!(0.031);
    let conflicting: Value = serde_json::from_str(
        &on_body(
            &body,
            &pattern.to_string(),
            &request.to_string(),
            SCENARIO,
            |_| {},
        )
        .unwrap(),
    )
    .unwrap();
    for row in conflicting["manufacturing"]["alternatives"]
        .as_array()
        .unwrap()
    {
        assert_eq!(row["assembly_mass"]["status"], "refused");
        assert!(
            row["assembly_mass"]["refusal"]["message"]
                .as_str()
                .unwrap()
                .contains("conflicts")
        );
    }
    // The conflicting independent accounting request cannot replace the valid pattern attachment.
    for row in conflicting["simulations"].as_array().unwrap() {
        assert_eq!(row["evaluation"]["status"], "available");
        assert_eq!(
            row["evaluation"]["result"]["drape"]["attached_mass"]["attached_mass_kg"],
            0.03
        );
    }
}

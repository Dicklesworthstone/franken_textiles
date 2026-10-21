//! The browser import uses the native composer and ordinary material/drape consumers.
use ftx_core::{Vec3, det, receipt::sha256_hex};
use ftx_drape::{
    Body, PartFrame,
    outfit::{Input, prepare},
    run::{DrapeBody, DrapeOptions, drape},
    scene::PartFrames,
};
use ftx_pattern::Pattern;
use ftx_wasm::materials;
use serde_json::{Value, json};

const SOURCE: &str = include_str!("../../../tests/fixtures/materials/synthetic-joint-outfit.json");

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
fn skin() -> Body {
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
fn outfit_import_preserves_exact_sources_and_material_swaps_reach_the_same_joint_solver() {
    let mut original: Value = serde_json::from_str(SOURCE).unwrap();
    for (i, garment) in original["garments"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        garment["pattern"]["pieces"][0]["placement"]["radius_mm"] = json!(40.0 + i as f64 * 3.0);
    }
    let request = format!("\n{}\n", serde_json::to_string_pretty(&original).unwrap());
    let imported: Value =
        serde_json::from_str(&materials::outfit::prepare(&request).unwrap()).unwrap();
    assert_eq!(imported["request_json"], request);
    assert_eq!(imported["request_sha256"], sha256_hex(request.as_bytes()));
    assert_eq!(imported["self_contact_required"], true);
    let defaults = DrapeOptions::default();
    let options = DrapeOptions {
        self_contact: Some(defaults.contact),
        spacing: 10.0,
        sew_steps: 3,
        drape_steps: 3,
        step: 0.001,
        ..defaults
    };
    let input: Input = serde_json::from_str(&request).unwrap();
    let native = prepare(&input, &options).unwrap();
    let pattern_json = imported["pattern_json"].as_str().unwrap();
    assert_eq!(
        pattern_json,
        serde_json::to_string(native.pattern().raw()).unwrap()
    );
    assert_eq!(imported["composition"], *native.receipt());
    assert_eq!(
        imported["composition_json"],
        serde_json::to_string(native.receipt()).unwrap()
    );
    let mut heavy = original["garments"][0]["pattern"]["fabric"]["material"].clone();
    heavy["record"]["properties"]["areal_density"]["value"] = json!(0.4);
    let changes = json!({"schema":"ftx-material-bindings/1","default":heavy,
        "pieces":{"inner::band":heavy,"outer::band":heavy}});
    let bound: Value =
        serde_json::from_str(&materials::bind(pattern_json, &changes.to_string()).unwrap())
            .unwrap();
    assert_eq!(bound["geometry_changed"], false);
    assert_eq!(bound["drape_materials_with_self_contact"]["ready"], true);
    let altered = Pattern::from_json(bound["pattern_json"].as_str().unwrap()).unwrap();
    for (a, b) in native.pattern().raw()["pieces"]
        .as_array()
        .unwrap()
        .iter()
        .zip(altered.raw()["pieces"].as_array().unwrap())
    {
        for key in ["id", "vertices", "placement", "edges", "grain", "cut"] {
            assert_eq!(a[key], b[key]);
        }
    }
    let skin = skin();
    let body = DrapeBody {
        frames: &Form,
        collision: (&skin.vertices, &skin.triangles),
        display: (&skin.vertices, &skin.triangles),
        arm_deg: 35.,
    };
    let joint = native.drape(&json!({}), &body, &options, |_| {}).unwrap();
    let replay = drape(
        &Pattern::from_json(pattern_json).unwrap(),
        &json!({}),
        &body,
        &options,
        |_| {},
    )
    .unwrap();
    assert_eq!(joint.scene.solver.x, replay.scene.solver.x);
    assert_eq!(
        joint.scene.solver.model.mass,
        replay.scene.solver.model.mass
    );
    let heavy_run = drape(&altered, &json!({}), &body, &options, |_| {}).unwrap();
    assert!(
        (heavy_run.report["mass_kg"].as_f64().unwrap()
            - 2.0 * joint.report["mass_kg"].as_f64().unwrap())
        .abs()
            < 1e-15
    );
    assert_eq!(
        heavy_run.scene.mesh.instances,
        ["inner::band", "outer::band"]
    );
    assert_eq!(request, imported["request_json"]);
}

#[test]
fn outfit_import_refuses_incomplete_materials_and_preserves_independent_estimate_consent() {
    assert!(materials::outfit::prepare("{}").is_err());
    let original: Value = serde_json::from_str(SOURCE).unwrap();
    let mut denied = original.clone();
    denied["garments"][0]["pattern"]["fabric"]["material"]["allow_estimates"] = json!(false);
    assert!(materials::outfit::prepare(&denied.to_string()).is_err());
    let mut unpaired = original;
    unpaired["garments"][0]["pattern"]["fabric"]["material"]["record"]["properties"]["areal_density"]
        ["value"] = json!(0.4);
    assert!(
        materials::outfit::prepare(&unpaired.to_string())
            .unwrap_err()
            .message
            .contains("pairwise")
    );
}

//! Material axes and independent consumer coverage, using entirely synthetic sheet observations.
use ftx_core::Vec3;
use ftx_drape::material::{Fabric, resolve};
use ftx_drape::run::{DrapeBody, DrapeOptions, drape};
use ftx_drape::scene::PartFrames;
use ftx_drape::{Body, Membrane, PartFrame};
use ftx_pattern::Pattern;
use serde_json::{Value, json};

fn pattern() -> Value {
    let mut properties = serde_json::Map::new();
    for (name, value, unit) in [
        ("areal_density", 0.2, "kg/m2"),
        ("thickness", 0.0006, "m"),
        ("warp_stiffness", 100.0, "N/m"),
        ("weft_stiffness", 50.0, "N/m"),
        ("shear_stiffness", 10.0, "N/m"),
        ("poisson_warp_weft", 0.2, "1"),
        ("bending_rigidity", 3e-5, "N*m"),
        ("friction_skin", 0.2, "1"),
        ("friction_self", 0.3, "1"),
        ("thermal_conductivity", 0.04, "W/(m*K)"),
    ] {
        properties.insert(
            name.into(),
            json!({"value":value,"unit":unit,"source":"test",
            "evidence":"estimate","method":"Synthetic declaration, not a textile measurement"}),
        );
    }
    let w = 40.0 * std::f64::consts::TAU;
    json!({"schema":"fabric-pattern/1","units":"mm","fabric":{"material":{
        "id":"synthetic","allow_estimates":true,"record":{
            "id":"synthetic","name":"Synthetic","form":"woven","properties":properties,
            "sources":{"test":{"reference":"test:axes","note":"Synthetic inputs","accessed_on":"2026-10-13"}}}}},
        "pieces":[{"id":"band","cut":{"qty":1},"material":"shell",
            "vertices":{"a":[0,0],"b":[w,0],"c":[w,20],"d":[0,20]},
            "edges":[{"id":"bottom","from":"a","to":"b"},{"id":"right","from":"b","to":"c"},
                {"id":"top","from":"c","to":"d"},{"id":"left","from":"d","to":"a"}],
            "notches":[],"grain":[[0,0],[20,15]],
            "placement":{"part":"torso","radius_mm":40,"x0":w/2.0}}],
        "seams":[{"id":"join","a":"band/right","b":"band/left"}]})
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
        Some(20.0)
    }
    fn section_radius(&self, _: &str, _: f64) -> f64 {
        30.0
    }
}

#[test]
fn both_grain_encodings_drive_the_same_actual_orthotropic_drape() {
    let mut vertices = Vec::new();
    for z in [-0.02, 0.12] {
        for i in 0..16 {
            let (s, c) = ftx_core::det::sin_cos(i as f64 * std::f64::consts::TAU / 16.0);
            vertices.push(Vec3::new(0.03 * c, 0.03 * s, z));
        }
    }
    let mut tris = Vec::new();
    for i in 0..16 {
        let j = (i + 1) % 16;
        tris.extend([[i, j, j + 16], [i, j + 16, i + 16]]);
    }
    let surface = Body::new(vertices, tris, 0.01).unwrap();
    let body = DrapeBody {
        frames: &Form,
        collision: (&surface.vertices, &surface.triangles),
        display: (&surface.vertices, &surface.triangles),
        arm_deg: 35.0,
    };
    let opts = DrapeOptions {
        spacing: 24.0,
        sew_steps: 3,
        drape_steps: 3,
        step: 1e-3,
        ..DrapeOptions::default()
    };
    let mut raw = pattern();
    let a = drape(&Pattern::new(raw.clone()), &json!({}), &body, &opts, |_| {}).unwrap();
    raw["pieces"][0]["grain"] = json!({"p0":[0,0],"p1":[20,15]});
    let b = drape(&Pattern::new(raw.clone()), &json!({}), &body, &opts, |_| {}).unwrap();
    assert_eq!(a.scene.solver.model.rest, b.scene.solver.model.rest);
    assert_eq!(a.fold.frames, b.fold.frames);
    assert_eq!(a.report, b.report);
    raw["pieces"][0]["grain"] = json!([[0, 0], [-15, 20]]);
    let c = drape(&Pattern::new(raw), &json!({}), &body, &opts, |_| {}).unwrap();
    assert_eq!(a.scene.mesh.verts2d, c.scene.mesh.verts2d);
    assert_eq!(a.scene.weld, c.scene.weld);
    for (a, c) in a
        .scene
        .solver
        .model
        .mass
        .iter()
        .zip(&c.scene.solver.model.mass)
    {
        assert!((a - c).abs() < 1e-13 * a.abs());
    }
    assert_ne!(a.scene.solver.x, c.scene.solver.x);
}

#[test]
fn missing_and_malformed_material_axes_cannot_default_silently() {
    let opts = DrapeOptions::default();
    for grain in [
        json!(null),
        json!([[0, 0], [0, 0]]),
        json!([[0, 0]]),
        json!({"p0":[0,0],"p1":["bad",20]}),
    ] {
        let mut raw = pattern();
        raw["pieces"][0]["grain"] = grain;
        assert!(resolve(&Pattern::new(raw), &opts).is_err());
    }
    let mut raw = pattern();
    raw["pieces"][0].as_object_mut().unwrap().remove("grain");
    assert!(resolve(&Pattern::new(raw.clone()), &opts).is_err());
    raw["fabric"] = json!({}); // only the historical numeric law may infer the vertical axis
    resolve(&Pattern::new(raw), &opts).unwrap();
}

#[test]
fn optional_thermal_evidence_cannot_disable_characterized_mechanics() {
    let mut raw = pattern();
    let selection = &mut raw["fabric"]["material"];
    selection["allow_estimates"] = json!(false);
    for (name, property) in selection["record"]["properties"].as_object_mut().unwrap() {
        if name != "thermal_conductivity" {
            property["evidence"] = json!("measured"); // synthetic evidence-policy exercise only
        }
    }
    let input = Pattern::new(raw);
    let m = resolve(&input, &DrapeOptions::default()).unwrap();
    assert_eq!(m.report["thermal_basis"], "unavailable");
    assert!(m.report["dry_material_resistance_m2_k_w"].is_null());
    assert!(
        m.report["thermal_unavailable"]["message"]
            .as_str()
            .unwrap()
            .contains("estimate")
    );
    assert_eq!(m.options.contact.mu, 0.2);
    assert_eq!(m.fabric.tensile_n_m, 100.0);
    let record = ftx_pattern::textile_import::resolve_fabric(&input.raw()["fabric"])
        .unwrap()
        .unwrap();
    assert!(record.thermal_resistance().is_err());
}

#[test]
fn invalid_membranes_and_effective_fabrics_refuse_before_scene_construction() {
    let good = Membrane::orthotropic(100., 50., 0.2, 10., 1.0);
    good.validate().unwrap();
    for q in [
        [[0.; 3]; 3],
        [[1., 2., 0.], [2., 1., 0.], [0., 0., 1.]],
        [[1., 0.1, 0.], [0.2, 1., 0.], [0., 0., 1.]],
        [[f64::INFINITY; 3]; 3],
    ] {
        assert!(Membrane { q, ..good }.validate().is_err());
    }
    for transition in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(Membrane { transition, ..good }.validate().is_err());
    }
    for compression in [[0., 1.], [1., 1.01], [f64::NAN, 1.]] {
        assert!(
            Membrane {
                compression,
                ..good
            }
            .validate()
            .is_err()
        );
    }
    let mut fabric = Fabric::default();
    fabric.bending_nm = 0.0;
    fabric.validate().unwrap();
    for density in [0.0, f64::INFINITY, f64::NAN] {
        fabric.areal_kg_m2 = density;
        assert!(fabric.validate().is_err());
    }
    fabric = Fabric::default();
    for bending in [-1.0, f64::INFINITY, f64::NAN] {
        fabric.bending_nm = bending;
        assert!(fabric.validate().is_err());
    }
}

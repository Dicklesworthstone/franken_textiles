//! Real production drapes and heat solves on synthetic geometry; no fitted wearer or measured textile.
use ftx_core::{RefusalCode, Vec3};
use ftx_drape::{
    Body, PartFrame,
    run::{DrapeBody, DrapeOptions, drape},
    scene::PartFrames,
    study::{Scenario, Session},
    thermal::steady::coverage::request as coverage,
};
use ftx_pattern::Pattern;
use serde_json::json;

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
            let (s, c) = ftx_core::det::sin_cos(i as f64 * std::f64::consts::TAU / 16.);
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
fn body(form: &Body) -> DrapeBody<'_> {
    DrapeBody {
        frames: &Form,
        collision: (&form.vertices, &form.triangles),
        display: (&form.vertices, &form.triangles),
        arm_deg: 35.,
    }
}
fn pattern() -> Pattern {
    let w = 40. * std::f64::consts::TAU;
    let mut props = serde_json::Map::new();
    for (k, v, u) in [
        ("areal_density", 0.2, "kg/m2"),
        ("thickness", 0.0004, "m"),
        ("warp_stiffness", 640., "N/m"),
        ("weft_stiffness", 420., "N/m"),
        ("shear_stiffness", 95., "N/m"),
        ("poisson_warp_weft", 0.2, "1"),
        ("bending_rigidity", 2.8e-5, "N*m"),
        ("friction_skin", 0.24, "1"),
        ("friction_self", 0.36, "1"),
        ("thermal_conductivity", 0.05, "W/(m*K)"),
        ("width", 1.4, "m"),
    ] {
        props.insert(
            k.into(),
            json!({"value":v,"unit":u,"source":"test","evidence":"estimate",
            "method":"Synthetic software gate; not measured textile data"}),
        );
    }
    Pattern::new(json!({"schema":"fabric-pattern/1","units":"mm",
        "fabric":{"material":{"id":"synthetic-scenario-sheet","allow_estimates":true,"record":{
            "id":"synthetic-scenario-sheet","name":"Synthetic scenario sheet","form":"woven",
            "properties":props,"sources":{"test":{"reference":"test:scenario",
                "accessed_on":"2026-10-16","note":"Synthetic software parameters only"}}}}},
        "pieces":[{"id":"band","cut":{"qty":1},"grain":[[0.,0.],[0.,20.]],
            "vertices":{"a":[0.,0.],"b":[w,0.],"c":[w,20.],"d":[0.,20.]},
            "edges":[{"id":"bottom","from":"a","to":"b"},{"id":"right","from":"b","to":"c"},
                {"id":"top","from":"c","to":"d"},{"id":"left","from":"d","to":"a"}],
            "placement":{"part":"torso","radius_mm":40.,"x0":w/2.}}],
        "seams":[{"id":"join","a":"band/right","b":"band/left"}]}))
}
fn scenario() -> Scenario {
    let film = json!({"inside_c":30.,"outside_c":10.,"inside_resistance_m2_k_w":0.1,
        "outside_resistance_m2_k_w":0.2});
    serde_json::from_value(json!({"schema":"ftx-drape-scenario/1",
        "options":{"spacing":24.,"sew_steps":3,"drape_steps":3,"step":0.001},"fit_spec":{},
        "max_mesh_points":4096,"max_vertex_frames":1000000,
        "surface":{"schema":"ftx-steady-surface-conditions/1","boundaries":[film]},
        "coverage":{"body_triangle_ids":[0],"instance_layers":{"band":"shell"},
            "settings":{"max_distance_m":0.08,"min_normal_cosine":0.1,
                "separation_tolerance_m":0.00001,"cells_per_layer":1,"interfaces":[]},
            "conditions":{"boundaries":[film]}}}))
    .unwrap()
}
fn changed(pattern: &Pattern, key: &str, value: f64) -> Pattern {
    let mut raw = pattern.raw().clone();
    raw["fabric"]["material"]["record"]["properties"][key]["value"] = json!(value);
    Pattern::new(raw)
}

#[test]
fn composed_evaluation_equals_real_drape_surface_and_projected_coverage() {
    let form = form();
    let body = body(&form);
    let p = pattern();
    let s = scenario();
    let ordinary = drape(&p, &s.fit_spec, &body, &s.options, |_| {}).unwrap();
    let r = Session::new(&s, &body)
        .unwrap()
        .evaluate(&p, |_| {})
        .unwrap();
    assert_eq!(r["drape"], ordinary.report);
    assert_eq!(r["positions_m"], json!(ordinary.scene.solver.x));
    assert_eq!(r["surface"]["status"], "available", "{}", r["surface"]);
    assert_eq!(
        r["surface"]["result"],
        s.surface
            .as_ref()
            .unwrap()
            .evaluate(&ordinary.report)
            .unwrap()
    );
    let c = s.coverage.as_ref().unwrap();
    let direct = coverage::Request {
        schema: "ftx-steady-coverage-request/1".into(),
        report: ordinary.report,
        map: coverage::Map {
            body_positions_m: form.vertices,
            body_triangles: vec![form.triangles[0]],
            instance_layers: c.instance_layers.clone(),
            settings: c.settings.clone(),
        },
        conditions: c.conditions.clone(),
    }
    .evaluate()
    .unwrap();
    assert_eq!(r["coverage"]["status"], "available", "{}", r["coverage"]);
    assert_eq!(r["coverage"]["result"], direct);
    assert!(r["metrics"]["cloth_mass_kg"].as_f64().unwrap() > 0.);
    assert!(!r["physical_validation_claim"].as_bool().unwrap());
}

#[test]
fn substitutions_recompute_heat_and_mechanics_without_warm_starting() {
    let form = form();
    let body = body(&form);
    let p = pattern();
    let original = p.raw().clone();
    let s = scenario();
    let mut session = Session::new(&s, &body).unwrap();
    let a = session.evaluate(&p, |_| {}).unwrap();
    let b = session
        .evaluate(&changed(&p, "thermal_conductivity", 0.1), |_| {})
        .unwrap();
    assert_eq!(a["positions_m"], b["positions_m"]);
    assert_eq!(a["body_geometry_sha256"], b["body_geometry_sha256"]);
    assert_eq!(a["placements_sha256"], b["placements_sha256"]);
    assert_ne!(
        a["surface"]["result"]["model_sha256"],
        b["surface"]["result"]["model_sha256"]
    );
    assert_ne!(
        a["metrics"]["independent_surface_power_w"],
        b["metrics"]["independent_surface_power_w"]
    );
    assert_ne!(
        a["metrics"]["body_coverage_power_w"],
        b["metrics"]["body_coverage_power_w"]
    );
    let mechanical = session
        .evaluate(&changed(&p, "areal_density", 0.4), |_| {})
        .unwrap();
    assert_ne!(
        a["metrics"]["cloth_mass_kg"],
        mechanical["metrics"]["cloth_mass_kg"]
    );
    assert_ne!(a["positions_sha256"], mechanical["positions_sha256"]);
    assert_eq!(a, session.evaluate(&p, |_| {}).unwrap());
    assert_eq!(p.raw(), &original);
}

#[test]
fn missing_thermal_and_stale_fields_do_not_erase_valid_draping() {
    let form = form();
    let body = body(&form);
    let p = pattern();
    let mut s = scenario();
    let mut raw = p.raw().clone();
    raw["fabric"]["material"]["record"]["properties"]
        .as_object_mut()
        .unwrap()
        .remove("thermal_conductivity");
    let r = Session::new(&s, &body)
        .unwrap()
        .evaluate(&Pattern::new(raw), |_| {})
        .unwrap();
    assert_eq!(r["surface"]["status"], "refused");
    assert_eq!(r["coverage"]["status"], "refused");
    assert!(r["metrics"]["independent_surface_power_w"].is_null());
    assert!(r["metrics"]["cloth_mass_kg"].is_number());
    s.surface.as_mut().unwrap().model_sha256 = Some("0".repeat(64));
    let r = Session::new(&s, &body)
        .unwrap()
        .evaluate(&p, |_| {})
        .unwrap();
    assert_eq!(r["surface"]["status"], "refused");
    assert_eq!(r["coverage"]["status"], "available");
    s.coverage.as_mut().unwrap().body_triangle_ids = vec![0, 0];
    s.surface = None;
    let r = Session::new(&s, &body)
        .unwrap()
        .evaluate(&p, |_| {})
        .unwrap();
    assert_eq!(r["coverage"]["status"], "refused");
    assert_eq!(r["surface"]["status"], "not_requested");
}

#[test]
fn budgets_and_identity_are_checked_before_a_solver_starts() {
    let form = form();
    let body = body(&form);
    let p = pattern();
    let mut s = scenario();
    let original = Session::new(&s, &body).unwrap().descriptor();
    s.options.step *= 2.;
    assert_ne!(original, Session::new(&s, &body).unwrap().descriptor());
    let shifted: Vec<_> = form
        .vertices
        .iter()
        .map(|p| *p + Vec3::new(0.001, 0., 0.))
        .collect();
    let moved = DrapeBody {
        collision: (&shifted, &form.triangles),
        ..body
    };
    assert_ne!(
        original["body_geometry_sha256"],
        Session::new(&s, &moved).unwrap().descriptor()["body_geometry_sha256"]
    );
    s.max_mesh_points = 3;
    let mut called = false;
    assert_eq!(
        Session::new(&s, &body)
            .unwrap()
            .evaluate(&p, |_| called = true)
            .unwrap_err()
            .code,
        RefusalCode::BudgetExceeded
    );
    assert!(!called);
    s.max_mesh_points = 4096;
    s.max_vertex_frames = 1;
    assert_eq!(
        Session::new(&s, &body)
            .unwrap()
            .evaluate(&p, |_| called = true)
            .unwrap_err()
            .code,
        RefusalCode::BudgetExceeded
    );
    assert!(!called);
    s.options.lag_rounds = [usize::MAX, 0];
    assert!(s.validate().is_err());
    let mut raw = serde_json::to_value(scenario()).unwrap();
    raw["options"]["typo"] = json!(1);
    assert!(serde_json::from_value::<Scenario>(raw).is_err());
    assert!(
        serde_json::from_value::<DrapeOptions>(json!({"contact":{"dhat":0.1,"typo":1}})).is_err()
    );
}

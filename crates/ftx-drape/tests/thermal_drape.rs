//! Actual sewn-drape to thermal exchange checks using explicitly synthetic textile coefficients.
use ftx_core::{Vec3, det};
use ftx_drape::run::{DrapeBody, DrapeOptions, DrapeRun, drape};
use ftx_drape::scene::PartFrames;
use ftx_drape::{Body, PartFrame};
use ftx_pattern::Pattern;
use serde_json::{Value, json};

fn sheet(scale: f64) -> Value {
    let mut properties = serde_json::Map::new();
    for (name, value, unit) in [
        ("areal_density", 200.0 * scale, "g/m2"),
        ("thickness", 0.6, "mm"),
        ("warp_stiffness", 100.0 * scale * scale, "N/m"),
        ("weft_stiffness", 40.0 * scale * scale, "N/m"),
        ("shear_stiffness", 10.0 * scale * scale, "N/m"),
        ("poisson_warp_weft", 0.2, "1"),
        ("bending_rigidity", 3e-5 * scale, "N*m"),
        ("friction_skin", 0.2, "1"),
        ("friction_self", 0.3, "1"),
    ] {
        properties.insert(
            name.into(),
            json!({"value":value,"unit":unit,"source":"test",
            "evidence":"estimate","method":"Synthetic known answer, not an actual textile"}),
        );
    }
    json!({"id":"test-sheet", "allow_estimates":true, "record":{
        "id":"test-sheet", "name":"Synthetic sheet", "form":"woven", "properties":properties,
        "sources":{"test":{"reference":"test:piece-materials","note":"Synthetic inputs", "accessed_on":"2026-10-13"}}}})
}

fn band() -> Pattern {
    let w = 40.0 * std::f64::consts::PI;
    let pieces: Vec<_> = ["front", "back"]
        .into_iter()
        .enumerate()
        .map(|(i, id)| {
            json!({
                "id":id, "material":"cutting label only", "cut":{"qty":1},
                "vertices":{"a":[0,0],"b":[w,0],"c":[w,20],"d":[0,20]},
                "edges":[{"id":"bottom","from":"a","to":"b"},{"id":"right","from":"b","to":"c"},
                    {"id":"top","from":"c","to":"d"},{"id":"left","from":"d","to":"a"}],
                "notches":[],"grain":[[0,0],[0,20]],
                "placement":{"part":"torso","radius_mm":40,"x0":w/2.0,"theta_deg":180*i}
            })
        })
        .collect();
    Pattern::new(
        json!({"schema":"fabric-pattern/1","units":"mm", "fabric":{"material":sheet(1.0)},
        "pieces":pieces,"seams":[{"id":"right_join","a":"front/right","b":"back/left"},
        {"id":"left_join","a":"back/right","b":"front/left"}]}),
    )
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
fn body() -> Body {
    let mut vertices = Vec::new();
    for z in [-0.02, 0.12] {
        for i in 0..16 {
            let (s, c) = det::sin_cos(i as f64 * std::f64::consts::TAU / 16.);
            vertices.push(Vec3::new(0.03 * c, 0.03 * s, z));
        }
    }
    let mut tris = Vec::new();
    for i in 0..16 {
        let j = (i + 1) % 16;
        tris.extend([[i, j, j + 16], [i, j + 16, i + 16]]);
    }
    Body::new(vertices, tris, 0.01).unwrap()
}
fn options() -> DrapeOptions {
    DrapeOptions {
        spacing: 24.0,
        sew_steps: 3,
        drape_steps: 3,
        step: 0.001,
        ..DrapeOptions::default()
    }
}
fn run(p: &Pattern, opts: DrapeOptions) -> DrapeRun {
    let b = body();
    drape(
        p,
        &json!({}),
        &DrapeBody {
            frames: &Form,
            collision: (&b.vertices, &b.triangles),
            display: (&b.vertices, &b.triangles),
            arm_deg: 35.,
        },
        &opts,
        |_| {},
    )
    .unwrap()
}
fn thermal_sheet(r: f64, heat: f64) -> Value {
    let mut selection = sheet(1.0);
    for (key, value, unit) in [
        ("thermal_resistance", r, "m2*K/W"),
        ("specific_heat", heat, "J/(kg*K)"),
    ] {
        selection["record"]["properties"][key] = json!({"value":value,"unit":unit,
            "source":"test","evidence":"estimate","method":"Synthetic thermal known answer"});
    }
    selection
}

#[test]
fn actual_drape_exports_material_locked_heat_fields_without_changing_geometry() {
    use ftx_drape::thermal::{Boundary, surface::Surface};
    let mut raw = band().raw().clone();
    raw["fabric"]["material"] = thermal_sheet(0.2, 1000.);
    let uniform = run(&Pattern::new(raw.clone()), options());
    raw["pieces"][1]["fabric"] = json!({"material":thermal_sheet(0.6,2000.)});
    let mixed = run(&Pattern::new(raw), options());
    // Only transport changed. The actual mechanical solution is bit-for-bit identical.
    assert_eq!(uniform.scene.solver.x, mixed.scene.solver.x);
    let (u, _) = Surface::from_drape_report(&uniform.report).unwrap();
    let (s, x) = Surface::from_drape_report(&mixed.report).unwrap();
    assert_eq!(x, mixed.scene.solver.x);
    assert_eq!(s.column_count(), 2);
    let snapshot = s.snapshot();
    assert_eq!(snapshot, mixed.thermal_surface(4).unwrap().snapshot());
    let boundary = Boundary {
        inside_c: 30.,
        outside_c: 0.,
        inside_resistance: 0.1,
        outside_resistance: 0.2,
    };
    let result = s.steady(&x, &[boundary; 2]).unwrap();
    let mut expected_capacity = 0.;
    let mut actual_capacity = 0.;
    for (i, (e, &owner)) in mixed
        .scene
        .solver
        .model
        .membranes
        .iter()
        .zip(&mixed.scene.triangle_owner)
        .enumerate()
    {
        let expected = 30. / (0.3 + if owner == 0 { 0.2 } else { 0.6 });
        assert!((result.patches[i].inside_flux_w_m2 - expected).abs() < 1e-10);
        assert_eq!(snapshot["patches"][i]["vertices"], json!(e.v));
        assert_eq!(snapshot["patches"][i]["reference_area_m2"], json!(e.area));
        let col = snapshot["patches"][i]["column"].as_u64().unwrap() as usize;
        actual_capacity += e.area
            * snapshot["columns"][col]["cell_capacity_j_m2_k"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap())
                .sum::<f64>();
        expected_capacity += e.area * 0.2 * if owner == 0 { 1000. } else { 2000. };
    }
    assert!((actual_capacity - expected_capacity).abs() < 1e-12);
    assert!(result.inside_heat_w < u.steady(&x, &[boundary]).unwrap().inside_heat_w);
    assert_ne!(snapshot["sha256"], u.snapshot()["sha256"]);
    let saved = serde_json::to_string(&mixed.report).unwrap();
    let restored = Surface::from_drape_report(&serde_json::from_str(&saved).unwrap()).unwrap();
    assert_eq!(restored.0.snapshot(), snapshot);
    assert_eq!(restored.1, x);
    assert!(mixed.thermal_surface(usize::MAX).is_err());
}

#[test]
fn missing_thermal_data_preserves_mechanical_success_and_modified_receipts_refuse() {
    use ftx_drape::thermal::surface::Surface;
    let mechanical = run(&band(), options());
    assert_eq!(mechanical.report["thermal_surface"]["available"], false);
    assert!(Surface::from_drape_report(&mechanical.report).is_err());
    let mut raw = band().raw().clone();
    raw["fabric"]["material"] = thermal_sheet(0.2, 1000.);
    let solved = run(&Pattern::new(raw), options());
    let mut changed = solved.report.clone();
    changed["thermal_surface"]["positions_m"][0][0] = json!(999.);
    assert!(Surface::from_drape_report(&changed).is_err());
    changed = solved.report.clone();
    changed["material"]["record_sha256"] = json!("changed-after-draping");
    assert!(Surface::from_drape_report(&changed).is_err());
    let snapshot = solved.thermal_surface(4).unwrap().snapshot();
    let mut changed = snapshot.clone();
    changed["columns"][0]["layers"][0]["allow_estimates"] = json!(false);
    assert!(Surface::from_snapshot(&changed).is_err());
    changed = snapshot.clone();
    changed["columns"][0]["cell_capacity_j_m2_k"][0] = json!(999.);
    assert!(Surface::from_snapshot(&changed).is_err());
    changed = snapshot;
    changed["patches"][0]["column"] = json!(usize::MAX);
    assert!(Surface::from_snapshot(&changed).is_err());
}

#[test]
fn temperature_series_follow_sewn_drape_without_alternative_material_lookup() {
    use ftx_drape::thermal::{Boundary, surface::Surface};
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-variable-thermal.json"
    ))
    .unwrap();
    let mut selected = sheet(1.0);
    let record = &mut selected["record"];
    record["thermal_response"] = fixture["layers"][0]["record"]["thermal_response"].clone();
    for name in ["areal_density", "thickness"] {
        record["properties"][name]["conditions"] = json!({
            "specimen":"test:temperature-sheet", "conditioning":"synthetic-dry"
        });
    }
    let mut raw = band().raw().clone();
    raw["fabric"]["material"] = selected;
    let original_geometry = raw["pieces"].clone();
    let solved = run(&Pattern::new(raw.clone()), options());
    assert_eq!(solved.report["thermal_surface"]["available"], true);
    let (surface, x) = Surface::from_drape_report(&solved.report).unwrap();
    assert_eq!(
        surface.snapshot()["columns"][0]["schema"],
        "ftx-dry-thermal-model/2"
    );
    let b = Boundary {
        inside_c: 30.0,
        outside_c: 0.0,
        inside_resistance: 0.1,
        outside_resistance: 0.2,
    };
    let first = surface.steady(&x, &[b]).unwrap();
    let transient = surface
        .advance(&surface.initial(10.0).unwrap(), &x, 1.0, &[b])
        .unwrap();
    assert!(transient.balance_error_j.abs() < 1e-7);
    // Alter only the record's transport observation; drape geometry and mechanical inputs stay fixed.
    for p in raw["fabric"]["material"]["record"]["thermal_response"]["conductivity"]["points"]
        .as_array_mut()
        .unwrap()
    {
        p["mean"] = json!(p["mean"].as_f64().unwrap() * 2.0);
    }
    let changed = run(&Pattern::new(raw.clone()), options());
    let (new_surface, new_x) = Surface::from_drape_report(&changed.report).unwrap();
    assert_eq!(original_geometry, raw["pieces"]);
    assert_eq!(x, new_x);
    assert_ne!(
        surface.snapshot()["sha256"],
        new_surface.snapshot()["sha256"]
    );
    assert!(new_surface.steady(&new_x, &[b]).unwrap().inside_heat_w > first.inside_heat_w);
    assert!(
        new_surface
            .advance(&transient.state, &new_x, 1.0, &[b])
            .is_err()
    );
}

//! Actual sewn-drape -> saved files -> coverage CLI. All geometry/coefficients are synthetic.
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::OnceLock,
};

use ftx_core::{Vec3, det, receipt::sha256_hex};
use ftx_drape::{
    Body, PartFrame,
    run::{DrapeBody, DrapeOptions, drape},
    scene::PartFrames,
};
use ftx_pattern::Pattern;
use serde_json::{Value, json};

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
fn sheet(r: f64, variable: bool) -> Value {
    let mut p = serde_json::Map::new();
    for (key, value, unit) in [
        ("areal_density", 0.2, "kg/m2"),
        ("thickness", 0.0006, "m"),
        ("warp_stiffness", 100., "N/m"),
        ("weft_stiffness", 40., "N/m"),
        ("shear_stiffness", 10., "N/m"),
        ("poisson_warp_weft", 0.2, "1"),
        ("bending_rigidity", 3e-5, "N*m"),
        ("friction_skin", 0.2, "1"),
        ("friction_self", 0.3, "1"),
        ("thermal_resistance", r, "m2*K/W"),
        ("specific_heat", 1000., "J/(kg*K)"),
    ] {
        p.insert(
            key.into(),
            json!({"value":value,"unit":unit,"source":"test",
            "evidence":"estimate","method":"Synthetic integration test only"}),
        );
    }
    let mut selection = json!({"id":"test","allow_estimates":true,"record":{"id":"test","name":"Synthetic sheet",
        "form":"woven","properties":p,"sources":{"test":{"reference":"test:coverage-cli",
        "accessed_on":"2026-10-14","note":"No measured-fabric or fit claim"}}}});
    if variable {
        let record = &mut selection["record"];
        for name in ["thermal_resistance", "specific_heat"] {
            record["properties"].as_object_mut().unwrap().remove(name);
        }
        for name in ["thickness", "areal_density"] {
            record["properties"][name]["conditions"] =
                json!({"specimen":"test:coverage-cli","conditioning":"synthetic-dry"});
        }
        record["thermal_response"] = json!({"specimen":"test:coverage-cli","conditioning":"synthetic-dry",
            "conductivity":{"unit":"W/(m*K)","source":"test","evidence":"estimate","method":"Synthetic fixed R table",
                "points":[{"temperature_c":0.,"mean":0.0006/r},{"temperature_c":60.,"mean":0.0006/r}]},
            "specific_heat":{"unit":"J/(kg*K)","source":"test","evidence":"estimate","method":"Synthetic Cp(T)=1000+20*T",
                "points":[{"temperature_c":0.,"mean":1000.},{"temperature_c":60.,"mean":2200.}]}});
    }
    selection
}
fn sewn_report(shell_r: f64) -> Value {
    sewn_report_with_law(shell_r, false)
}
fn sewn_report_with_law(shell_r: f64, variable: bool) -> Value {
    let w = 40. * std::f64::consts::PI;
    let pieces: Vec<_> = ["front","back"].into_iter().enumerate().map(|(i,id)| json!({
        "id":id,"cut":{"qty":1},"vertices":{"a":[0,0],"b":[w,0],"c":[w,20],"d":[0,20]},
        "edges":[{"id":"bottom","from":"a","to":"b"},{"id":"right","from":"b","to":"c"},
            {"id":"top","from":"c","to":"d"},{"id":"left","from":"d","to":"a"}],
        "notches":[],"grain":[[0,0],[0,20]],"fabric":{"material":sheet(if i==0 {0.1} else {shell_r}, variable)},
        "placement":{"part":"torso","radius_mm":40,"x0":w/2.,"theta_deg":180*i}
    })).collect();
    let pattern = Pattern::new(json!({"schema":"fabric-pattern/1","units":"mm",
        "fabric":{"material":sheet(0.1, variable)},"pieces":pieces,"seams":[
        {"id":"right_join","a":"front/right","b":"back/left"},
        {"id":"left_join","a":"back/right","b":"front/left"}]}));
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
    let body = Body::new(vertices, tris, 0.01).unwrap();
    drape(
        &pattern,
        &json!({}),
        &DrapeBody {
            frames: &Form,
            collision: (&body.vertices, &body.triangles),
            display: (&body.vertices, &body.triangles),
            arm_deg: 35.,
        },
        &DrapeOptions {
            spacing: 24.,
            sew_steps: 3,
            drape_steps: 3,
            step: 0.001,
            ..DrapeOptions::default()
        },
        |_| {},
    )
    .unwrap()
    .report
}
fn report() -> Value {
    static REPORT: OnceLock<Value> = OnceLock::new();
    REPORT.get_or_init(|| sewn_report(0.4)).clone()
}
fn mapping(report: &Value) -> Value {
    let owners = report["thermal_surface"]["triangle_instance"]
        .as_array()
        .unwrap();
    let lining = owners.iter().position(|v| *v == 0).unwrap();
    let shell = owners.iter().position(|v| *v == 1).unwrap();
    json!({"schema":"ftx-drape-thermal-coverage/1",
        "drape_thermal_frame_sha256":report["thermal_surface"]["sha256"],
        "body_positions_m":[[0.,0.,0.],[0.01,0.,0.],[0.,0.01,0.]],
        "layout":{"schema":"ftx-thermal-coverage-layout/1",
            "mapping_basis":"Synthetic area allocation for integration testing; not inferred physical overlap",
            "body_vertex_count":3,"body_triangles":[[0,1,2]],"cells_per_layer":2,
            "regions":[{"id":"covered","body_triangle":0,"body_fraction":0.5,
                "layers":[{"layer":"lining","cloth_triangle":lining,"fraction":0.5},
                    {"layer":"shell","cloth_triangle":shell,"fraction":0.5}],
                "interface_resistance_m2_k_w":[0.07]}]}})
}
fn conditions() -> Value {
    json!({"schema":"ftx-thermal-coverage-conditions/1","boundaries":[{
        "inside_c":30.,"outside_c":0.,"inside_resistance_m2_k_w":0.1,"outside_resistance_m2_k_w":0.2}],
        "transient":{"initial_temperature_c":10.,"step_seconds":0.5,"steps":20,"record_every":5}})
}
fn invoke(r: &Value, m: &Value, c: &Value) -> (Output, PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "ftx-coverage-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    for (name, v) in [
        ("report.json", r),
        ("coverage.json", m),
        ("conditions.json", c),
    ] {
        fs::write(root.join(name), serde_json::to_vec(v).unwrap()).unwrap();
    }
    let output = Command::new(env!("CARGO_BIN_EXE_ftx-materials"))
        .arg("thermal-coverage")
        .args([
            root.join("report.json"),
            root.join("coverage.json"),
            root.join("conditions.json"),
        ])
        .output()
        .unwrap();
    // Keep test products: repository policy forbids deleting files without explicit permission.
    (output, root)
}
fn success(r: &Value, m: &Value, c: &Value) -> Value {
    let (o, _) = invoke(r, m, c);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(o.stderr.is_empty());
    serde_json::from_slice(&o.stdout).unwrap()
}
fn refused(r: &Value, m: &Value, c: &Value) {
    let (o, root) = invoke(r, m, c);
    assert_eq!(o.status.code(), Some(2));
    assert!(
        o.stdout.is_empty(),
        "no partial successful report on failure"
    );
    let error: Value = serde_json::from_slice(&o.stderr).unwrap();
    assert!(error["refusal"].is_object());
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(root.join("report.json")).unwrap()).unwrap(),
        *r
    );
}
fn near(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() < tol, "{a} != {b}");
}

#[test]
fn actual_sewn_drape_to_cli_computes_series_bare_and_transient_heat() {
    let r = report();
    let m = mapping(&r);
    let c = conditions();
    let out = success(&r, &m, &c);
    assert_eq!(
        out["drape_thermal_frame_sha256"],
        r["thermal_surface"]["sha256"]
    );
    assert_eq!(out["model"]["source"], r["thermal_surface"]["model"]);
    assert_eq!(out["comfort_claim"], false);
    assert_eq!(out["mapping_is_geometrically_verified"], false);
    let positions: Vec<Vec3> = serde_json::from_value(out["cloth_positions_m"].clone()).unwrap();
    let ab = 0.5 * 0.01 * 0.01 * 0.5;
    let mut resistance = 0.37 / ab;
    for (p, rvalue) in m["layout"]["regions"][0]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .zip([0.1, 0.4])
    {
        let index = p["cloth_triangle"].as_u64().unwrap() as usize;
        let face: [usize; 3] = serde_json::from_value(
            r["thermal_surface"]["model"]["patches"][index]["vertices"].clone(),
        )
        .unwrap();
        let [a, b, c] = face.map(|i| positions[i]);
        let a_cloth = 0.5 * (b - a).cross(c - a).norm() * 0.5;
        resistance += rvalue / a_cloth;
    }
    let bare = ab * 30. / 0.3;
    near(out["steady"]["bare_heat_w"].as_f64().unwrap(), bare, 1e-12);
    near(
        out["steady"]["inside_heat_w"].as_f64().unwrap(),
        bare + 30. / resistance,
        1e-10,
    );
    near(
        out["steady"]["covered_area_m2"].as_f64().unwrap(),
        ab,
        1e-15,
    );
    assert!(
        out["steady"]["unassigned_cloth_reference_area_m2"]
            .as_f64()
            .unwrap()
            > 0.
    );
    assert_eq!(out["transient"]["trace"].as_array().unwrap().len(), 5);
    near(
        out["transient"]["balance_error_j"].as_f64().unwrap(),
        0.,
        1e-8,
    );
    let mut unsigned = out.clone();
    unsigned.as_object_mut().unwrap().remove("sha256");
    assert_eq!(
        out["sha256"],
        sha256_hex(&serde_json::to_vec(&unsigned).unwrap())
    );
    assert_eq!(out, success(&r, &m, &c));
}

#[test]
fn thermal_only_material_substitution_preserves_drape_and_requires_explicit_remapping() {
    let a = report();
    let b = sewn_report(0.8);
    assert_eq!(
        a["thermal_surface"]["positions_m"],
        b["thermal_surface"]["positions_m"]
    );
    let mut c = conditions();
    c.as_object_mut().unwrap().remove("transient");
    let ma = mapping(&a);
    let mb = mapping(&b);
    refused(&b, &ma, &c);
    let x = success(&a, &ma, &c);
    let y = success(&b, &mb, &c);
    assert!(
        y["steady"]["inside_heat_w"].as_f64().unwrap()
            < x["steady"]["inside_heat_w"].as_f64().unwrap()
    );
    assert_eq!(x["steady"]["bare_heat_w"], y["steady"]["bare_heat_w"]);
    assert_ne!(x["model"]["sha256"], y["model"]["sha256"]);
    assert!(y.get("transient").is_none());
}

#[test]
fn tampered_sources_wrong_frames_and_overallocated_maps_publish_nothing() {
    let r = report();
    let m = mapping(&r);
    let c = conditions();
    let mut bad = r.clone();
    bad["thermal_surface"]["positions_m"][0][0] = json!(999.);
    refused(&bad, &m, &c);
    bad = r.clone();
    bad["material"]["basis"] = json!("changed");
    refused(&bad, &m, &c);
    let mut bad = m.clone();
    bad["drape_thermal_frame_sha256"] = json!("missing");
    refused(&r, &bad, &c);
    bad = m.clone();
    bad["layout"]["regions"][0]["body_fraction"] = json!(1.1);
    refused(&r, &bad, &c);
    bad = m.clone();
    bad["layout"]["regions"][0]["layers"][0]["fraction"] = json!(2.);
    refused(&r, &bad, &c);
    bad = m.clone();
    bad["body_positions_m"][1] = json!([0., 0., 0.]);
    refused(&r, &bad, &c);
    bad = m;
    bad["layout"]["cells_per_layer"] = json!(4096);
    refused(&r, &bad, &c);
}

#[test]
fn unsupported_physics_and_excessive_schedules_are_not_ignored() {
    let r = report();
    let m = mapping(&r);
    for (key, value) in [
        ("steps", json!(0)),
        ("steps", json!(100_001)),
        ("record_every", json!(0)),
        ("step_seconds", json!(0)),
        ("initial_temperature_c", json!(-274.)),
    ] {
        let mut c = conditions();
        c["transient"][key] = value;
        refused(&r, &m, &c);
    }
    let mut c = conditions();
    c["wind_speed"] = json!(4.);
    refused(&r, &m, &c);
    c = conditions();
    c["boundaries"] = json!([]);
    refused(&r, &m, &c);
    // A bare-only scene still scans source/body geometry on each step and must obey its work budget.
    let mut bare = m.clone();
    bare["layout"]["regions"] = json!([]);
    let mut large = bare.clone();
    large["layout"]["body_triangles"] = json!(
        (0..200)
            .map(|i| [3 * i, 3 * i + 1, 3 * i + 2])
            .collect::<Vec<_>>()
    );
    large["layout"]["body_vertex_count"] = json!(600);
    large["body_positions_m"] = json!(
        (0..200)
            .flat_map(|i| [
                [i as f64, 0., 0.],
                [i as f64 + 0.01, 0., 0.],
                [i as f64, 0.01, 0.]
            ])
            .collect::<Vec<_>>()
    );
    c = conditions();
    c["transient"]["steps"] = json!(100_000);
    refused(&r, &large, &c);
    let out = success(&r, &bare, &conditions());
    assert_eq!(out["final"]["stored_change_j"], 0.);
    assert_eq!(out["final"]["covered_area_m2"], 0.);
    c = conditions();
    c["boundaries"][0]["inside_resistance_m2_k_w"] = json!(0.);
    c["boundaries"][0]["outside_resistance_m2_k_w"] = json!(0.);
    refused(&r, &bare, &c);
}

#[test]
fn sewn_temperature_laws_reach_the_cli_and_conserve_integrated_reference_enthalpy() {
    let report = sewn_report_with_law(0.4, true);
    assert_eq!(
        report["thermal_surface"]["positions_m"],
        self::report()["thermal_surface"]["positions_m"]
    );
    let mapping = mapping(&report);
    let out = success(&report, &mapping, &conditions());
    let temperatures = out["final"]["temperature_c"][0].as_array().unwrap();
    let mut expected = 0.0;
    for (layer, portion) in mapping["layout"]["regions"][0]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let index = portion["cloth_triangle"].as_u64().unwrap() as usize;
        let reference = report["thermal_surface"]["model"]["patches"][index]["reference_area_m2"]
            .as_f64()
            .unwrap()
            * 0.5;
        // Two cells/layer, 0.2 kg/m2, independently integrated Cp(T)=1000+20*T from 10 C.
        for t in &temperatures[2 * layer..2 * layer + 2] {
            let t = t.as_f64().unwrap();
            expected += reference * 0.2 / 2.0 * (1000.0 * (t - 10.0) + 10.0 * (t * t - 100.0));
        }
    }
    near(
        out["transient"]["stored_energy_change_j"].as_f64().unwrap(),
        expected,
        1e-9,
    );
    near(
        out["transient"]["net_energy_into_storage_j"]
            .as_f64()
            .unwrap(),
        expected,
        1e-8,
    );
    let mut c = conditions();
    c["boundaries"][0]["outside_c"] = json!(-1.);
    refused(&report, &mapping, &c);
}

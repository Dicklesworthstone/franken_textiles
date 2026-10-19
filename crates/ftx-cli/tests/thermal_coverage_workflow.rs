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
    sewn_report_with_optics(shell_r, variable, None)
}
fn sewn_report_with_optics(shell_r: f64, variable: bool, emissivity: Option<f64>) -> Value {
    let material = |r| {
        let mut selection = sheet(r, variable);
        if let Some(value) = emissivity {
            selection["record"]["properties"]["emissivity"] = json!({
                "value":value,"unit":"1","source":"test","evidence":"estimate",
                "method":"Synthetic optical coefficient for consumer integration tests",
                "conditions":{"specimen":"test:coverage-cli","conditioning":"synthetic-dry",
                    "face":"public","spectral_basis":"total_hemispherical",
                    "temperature_min_c":"-80","temperature_max_c":"80"}});
        }
        selection
    };
    let w = 40. * std::f64::consts::PI;
    let pieces: Vec<_> = ["front","back"].into_iter().enumerate().map(|(i,id)| json!({
        "id":id,"cut":{"qty":1},"vertices":{"a":[0,0],"b":[w,0],"c":[w,20],"d":[0,20]},
        "edges":[{"id":"bottom","from":"a","to":"b"},{"id":"right","from":"b","to":"c"},
            {"id":"top","from":"c","to":"d"},{"id":"left","from":"d","to":"a"}],
        "notches":[],"grain":[[0,0],[0,20]],"fabric":{"material":material(if i==0 {0.1} else {shell_r})},
        "placement":{"part":"torso","radius_mm":40,"x0":w/2.,"theta_deg":180*i}
    })).collect();
    let pattern = Pattern::new(json!({"schema":"fabric-pattern/1","units":"mm",
        "fabric":{"material":material(0.1)},"pieces":pieces,"seams":[
        {"id":"right_join","a":"front/right","b":"back/left"},
        {"id":"left_join","a":"back/right","b":"front/left"}]}));
    let (vertices, tris) = cylinder();
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

fn cylinder() -> (Vec<Vec3>, Vec<[usize; 3]>) {
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
    (vertices, tris)
}

fn body_obj() -> String {
    let (vertices, faces) = cylinder();
    let mut text = String::from("# Synthetic body in metres, actual drape collision geometry\n");
    for p in vertices {
        text.push_str(&format!("v {} {} {}\n", p.x, p.y, p.z));
    }
    for [a, b, c] in faces {
        text.push_str(&format!("f {} {} {}\n", a + 1, b + 1, c + 1));
    }
    text
}

fn projection_input() -> Value {
    json!({"schema":"ftx-drape-thermal-projection/1",
        "instance_layers":{"front":"shell","back":"shell"},
        "settings":{"max_distance_m":0.025,"min_normal_cosine":0.25,
            "separation_tolerance_m":1e-8,"cells_per_layer":2,"interfaces":[]}})
}

fn map_invoke(report: &Value, body: &str, request: &Value) -> (Output, PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "ftx-projection-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    fs::write(
        root.join("report.json"),
        serde_json::to_vec(report).unwrap(),
    )
    .unwrap();
    fs::write(root.join("body.obj"), body).unwrap();
    fs::write(
        root.join("projection.json"),
        serde_json::to_vec(request).unwrap(),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ftx-materials"))
        .arg("thermal-map")
        .args([
            root.join("report.json"),
            root.join("body.obj"),
            root.join("projection.json"),
        ])
        .output()
        .unwrap();
    (output, root)
}

fn mapped(report: &Value) -> Value {
    let (out, _) = map_invoke(report, &body_obj(), &projection_input());
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn automatic_map_from_actual_sewn_drape_and_body_obj_reaches_heat_solver() {
    let r = report();
    let m = mapped(&r);
    let out = success(&r, &m, &conditions());
    assert_eq!(out["mapping_is_geometrically_verified"], true);
    assert_eq!(out["comfort_claim"], false);
    assert!(
        out["geometric_verification_scope"]
            .as_str()
            .unwrap()
            .contains("local-normal")
    );
    let covered = out["steady"]["covered_area_m2"].as_f64().unwrap();
    let body = out["steady"]["body_area_m2"].as_f64().unwrap();
    assert!(covered > 0.001 && covered < body, "{covered} of {body}");
    assert!(out["steady"]["bare_heat_w"].as_f64().unwrap() > 0.0);
    assert!(
        !m["projection"]["result"]["footprints"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(m["layout"], m["projection"]["result"]["layout"]);
    assert_eq!(mapped(&r), m);
    assert_eq!(success(&r, &m, &conditions()), out);
    near(
        out["transient"]["balance_error_j"].as_f64().unwrap(),
        0.,
        1e-8,
    );
}

#[test]
fn material_substitution_regenerates_geometric_map_without_manual_triangle_edits() {
    let a = report();
    let b = sewn_report(0.8);
    assert_eq!(
        a["thermal_surface"]["positions_m"],
        b["thermal_surface"]["positions_m"]
    );
    let ma = mapped(&a);
    let mb = mapped(&b);
    // Same instance names and projection controls. No caller-selected triangle IDs or fractions.
    assert_eq!(ma["projection"]["request"], mb["projection"]["request"]);
    assert_eq!(
        ma["projection"]["result"]["footprints"],
        mb["projection"]["result"]["footprints"]
    );
    assert_ne!(
        ma["projection"]["result"]["input_sha256"],
        mb["projection"]["result"]["input_sha256"]
    );
    refused(&b, &ma, &conditions());
    let x = success(&a, &ma, &conditions());
    let y = success(&b, &mb, &conditions());
    assert_eq!(
        x["steady"]["covered_area_m2"],
        y["steady"]["covered_area_m2"]
    );
    assert_eq!(x["steady"]["bare_heat_w"], y["steady"]["bare_heat_w"]);
    assert!(
        y["steady"]["inside_heat_w"].as_f64().unwrap()
            < x["steady"]["inside_heat_w"].as_f64().unwrap()
    );
}

#[test]
fn generated_geometry_and_allocation_receipts_are_recomputed_not_trusted() {
    let r = report();
    let m = mapped(&r);
    for change in [
        (|m: &mut Value| m["layout"]["regions"][0]["body_fraction"] = json!(0.01))
            as fn(&mut Value),
        |m| m["layout"]["regions"][0]["layers"][0]["fraction"] = json!(0.01),
        |m| {
            m["projection"]["result"]["footprints"][0]["hits"][0]["mid_surface_distance_m"][0] =
                json!(0.123)
        },
        |m| m["projection"]["result"]["footprints"][0]["body_polygon"][0] = json!([0.2, 0.2]),
        |m| m["body_positions_m"][0][0] = json!(0.031),
        |m| m["projection"]["request"]["settings"]["max_distance_m"] = json!(0.027),
        |m| m["projection"]["request"]["instance_layers"]["front"] = json!("different"),
    ] {
        let mut bad = m.clone();
        change(&mut bad);
        refused(&r, &bad, &conditions());
    }
    // Merely removing the proof may retain an authored map, but cannot claim geometric verification.
    let mut authored = m;
    authored.as_object_mut().unwrap().remove("projection");
    assert_eq!(
        success(&r, &authored, &conditions())["mapping_is_geometrically_verified"],
        false
    );
    authored["mapping_is_geometrically_verified"] = json!(true);
    refused(&r, &authored, &conditions());
}

#[test]
fn generated_maps_preserve_integrated_reference_mass_with_variable_heat_capacity() {
    let r = sewn_report_with_law(0.4, true);
    let m = mapped(&r);
    let out = success(&r, &m, &conditions());
    let mut expected = 0.0;
    let cells = m["layout"]["cells_per_layer"].as_u64().unwrap() as usize;
    for (i, region) in m["layout"]["regions"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        for (j, portion) in region["layers"].as_array().unwrap().iter().enumerate() {
            let ti = portion["cloth_triangle"].as_u64().unwrap() as usize;
            let area = r["thermal_surface"]["model"]["patches"][ti]["reference_area_m2"]
                .as_f64()
                .unwrap()
                * portion["fraction"].as_f64().unwrap();
            for cell in 0..cells {
                let t = out["final"]["temperature_c"][i][j * cells + cell]
                    .as_f64()
                    .unwrap();
                expected += 0.2 * area / cells as f64 * (1000. * (t - 10.) + 10. * (t * t - 100.));
            }
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
    let mut outside_domain = conditions();
    outside_domain["boundaries"][0]["outside_c"] = json!(-1.);
    refused(&r, &m, &outside_domain);
}

#[test]
fn map_creation_rejects_ambiguous_inputs_without_partial_output_or_file_changes() {
    let r = report();
    let input = projection_input();
    let obj = body_obj();
    let check = |r: &Value, body: &str, config: &Value| {
        let (o, root) = map_invoke(r, body, config);
        assert_eq!(
            o.status.code(),
            Some(2),
            "{}",
            String::from_utf8_lossy(&o.stdout)
        );
        assert!(o.stdout.is_empty());
        let error: Value = serde_json::from_slice(&o.stderr).unwrap();
        assert!(error["refusal"].is_object());
        assert_eq!(fs::read_to_string(root.join("body.obj")).unwrap(), body);
        for (name, expected) in [("report.json", r), ("projection.json", config)] {
            assert_eq!(
                serde_json::from_slice::<Value>(&fs::read(root.join(name)).unwrap()).unwrap(),
                *expected
            );
        }
    };
    for change in [
        (|v: &mut Value| {
            v["instance_layers"]
                .as_object_mut()
                .unwrap()
                .remove("front");
        }) as fn(&mut Value),
        |v| v["instance_layers"]["typo"] = json!("shell"),
        |v| v["instance_layers"]["front"] = json!(""),
        |v| v["settings"]["max_distance_m"] = json!(-0.1),
        |v| v["settings"]["wind_speed"] = json!(3.),
    ] {
        let mut bad = input.clone();
        change(&mut bad);
        check(&r, &obj, &bad);
    }
    check(
        &r,
        "v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nf 1 2 3 4\n",
        &input,
    );
    check(&r, "v 0 0 0 2\nv 1 0 0\nv 0 1 0\nf 1 2 3\n", &input);
    check(&r, "v NaN 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n", &input);
    check(&r, "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 4\n", &input);
    let mut changed = r;
    changed["thermal_surface"]["positions_m"][0][0] = json!(999.);
    check(&changed, &obj, &input);
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

fn body_pose(dz: f64, refine: bool) -> String {
    let (mut vertices, faces) = cylinder();
    for p in &mut vertices {
        p.z += dz;
    }
    let mut triangles = Vec::new();
    for [a, b, c] in faces {
        if refine {
            let index = vertices.len();
            vertices.push((vertices[a] + vertices[b] + vertices[c]) / 3.);
            triangles.extend([[a, b, index], [b, c, index], [c, a, index]]);
        } else {
            triangles.push([a, b, c]);
        }
    }
    let mut text = String::from("# Synthetic prescribed body pose; not a measured motion\n");
    for p in vertices {
        text.push_str(&format!("v {} {} {}\n", p.x, p.y, p.z));
    }
    for [a, b, c] in triangles {
        text.push_str(&format!("f {} {} {}\n", a + 1, b + 1, c + 1));
    }
    text
}
fn motion_map(report: &Value, dz: f64, refine: bool, cells: usize) -> Value {
    let mut request = projection_input();
    request["settings"]["cells_per_layer"] = json!(cells);
    let (output, _) = map_invoke(report, &body_pose(dz, refine), &request);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
fn motion_case(variable: bool) -> (PathBuf, Value, Value) {
    motion_case_from_report(sewn_report_with_law(0.4, variable))
}
fn motion_case_from_report(r: Value) -> (PathBuf, Value, Value) {
    let (output, root) = map_invoke(&r, &body_obj(), &projection_input());
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    fs::write(root.join("start.json"), output.stdout).unwrap();
    let moved = motion_map(&r, 0.003, true, 3);
    fs::write(root.join("moved.json"), serde_json::to_vec(&moved).unwrap()).unwrap();
    let cold = conditions()["boundaries"].clone();
    let mut reversed = cold.clone();
    reversed[0]["inside_c"] = json!(0.);
    reversed[0]["outside_c"] = json!(30.);
    let input = json!({"schema":"ftx-thermal-motion/1",
        "initial":{"kind":"uniform","time_seconds":0.,"drape_report":"report.json",
            "coverage":"start.json","temperature_c":10.},
        "frames":[{"time_seconds":2.,"drape_report":"report.json","coverage":"start.json",
            "boundaries":cold,"steps":4},
            {"time_seconds":4.,"drape_report":"report.json","coverage":"moved.json",
            "boundaries":reversed,"steps":4}]});
    (root, input, r)
}
fn motion_invoke(root: &std::path::Path, input: &Value) -> Output {
    let path = root.join(format!(
        "motion-{}.json",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::write(&path, serde_json::to_vec(input).unwrap()).unwrap();
    Command::new(env!("CARGO_BIN_EXE_ftx-materials"))
        .current_dir(std::env::temp_dir())
        .arg("thermal-motion")
        .arg(path)
        .output()
        .unwrap()
}
fn motion_success(root: &std::path::Path, input: &Value) -> Value {
    let output = motion_invoke(root, input);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    serde_json::from_slice(&output.stdout).unwrap()
}
fn motion_refused(root: &std::path::Path, input: &Value) {
    let report = fs::read(root.join("report.json")).unwrap();
    let output = motion_invoke(root, input);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        output.stdout.is_empty(),
        "failure must not publish earlier successful intervals"
    );
    assert!(serde_json::from_slice::<Value>(&output.stderr).unwrap()["refusal"].is_object());
    assert_eq!(report, fs::read(root.join("report.json")).unwrap());
}

#[test]
fn timed_motion_cli_carries_actual_sewn_drape_heat_across_body_pose_and_cell_changes() {
    let (root, input, r) = motion_case(true);
    let out = motion_success(&root, &input);
    assert_eq!(out, motion_success(&root, &input));
    assert_eq!(out["schema"], "ftx-thermal-motion-report/1");
    assert_eq!(out["trace"].as_array().unwrap().len(), 2);
    assert!(out["trace"][0]["remap"].is_null()); // exact identity does not numerically mix cells
    assert!(out["trace"][1]["remap"].is_object());
    assert_eq!(out["checkpoint"]["time_seconds"], 4.);
    assert_eq!(
        out["checkpoint"]["state"]["frame"]["source"],
        r["thermal_surface"]["model"]
    );
    let frame = &out["checkpoint"]["state"]["frame"];
    assert_eq!(frame["request"]["settings"]["cells_per_layer"], 3);
    // An independently integrated Cp(T)=1000+20*T, times rho_A=0.2 and reference area,
    // checks TOTAL final stored heat, including any tiny remap error, against the initial 10 C.
    let mut energy = 0.;
    for (i, region) in frame["projection"]["layout"]["regions"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        for (j, p) in region["layers"].as_array().unwrap().iter().enumerate() {
            let id = p["cloth_triangle"].as_u64().unwrap() as usize;
            let reference = frame["source"]["patches"][id]["reference_area_m2"]
                .as_f64()
                .unwrap();
            let fraction = p["fraction"].as_f64().unwrap();
            for cell in 0..3 {
                let t = out["final"]["temperature_c"][i][j * 3 + cell]
                    .as_f64()
                    .unwrap();
                energy += reference * fraction / 3. * (200. * (t - 10.) + 2. * (t * t - 100.));
            }
        }
    }
    near(
        out["material_energy_change_including_remap_j"]
            .as_f64()
            .unwrap(),
        energy,
        1e-8,
    );
    near(out["heat_balance_error_j"].as_f64().unwrap(), 0., 1e-8);
    near(
        out["energy"]["absolute_remap_residual_j"].as_f64().unwrap(),
        0.,
        1e-8,
    );
    assert_eq!(out["empirical_validation_claim"], false);
}

#[test]
fn motion_cli_restart_matches_uninterrupted_state_without_resetting_temperatures() {
    let (root, input, _) = motion_case(true);
    let complete = motion_success(&root, &input);
    let mut first = input.clone();
    first["frames"].as_array_mut().unwrap().truncate(1);
    let first = motion_success(&root, &first);
    fs::write(
        root.join("previous-run.json"),
        serde_json::to_vec_pretty(&first).unwrap(),
    )
    .unwrap();
    let mut resume = input.clone();
    resume["initial"] = json!({"kind":"resume","checkpoint":"previous-run.json"});
    resume["frames"] = json!([input["frames"][1]]);
    let second = motion_success(&root, &resume);
    assert_eq!(complete["checkpoint"], second["checkpoint"]);
    assert_eq!(complete["final"], second["final"]);
    assert_eq!(second["from_time_seconds"], 2.);
    for key in [
        "incoming_energy_j",
        "outgoing_energy_j",
        "physical_stored_change_j",
        "net_boundary_energy_j",
    ] {
        near(
            complete["energy"][key].as_f64().unwrap(),
            first["energy"][key].as_f64().unwrap() + second["energy"][key].as_f64().unwrap(),
            1e-12,
        );
    }
    // Deliberate uniform reinitialization is a NEW simulation and must give a different result.
    resume["initial"] = json!({"kind":"uniform","time_seconds":2.,"drape_report":"report.json",
        "coverage":"start.json","temperature_c":10.});
    assert_ne!(
        second["final"]["temperature_c"],
        motion_success(&root, &resume)["final"]["temperature_c"]
    );
}

#[test]
fn motion_cli_refuses_tampered_checkpoints_and_material_substitution_mid_history() {
    let (root, input, r) = motion_case(false);
    let mut first = input.clone();
    first["frames"].as_array_mut().unwrap().truncate(1);
    let saved = motion_success(&root, &first);
    let mut broken = saved.clone();
    broken["checkpoint"]["state"]["temperature_c"][0][0] = json!(999.);
    fs::write(
        root.join("broken.json"),
        serde_json::to_vec(&broken).unwrap(),
    )
    .unwrap();
    let mut resume = input.clone();
    resume["initial"] = json!({"kind":"resume","checkpoint":"broken.json"});
    resume["frames"] = json!([input["frames"][1]]);
    motion_refused(&root, &resume);
    // Valid new material + valid geometric mapping still cannot inherit the previous heat state.
    let other = sewn_report(0.8);
    assert_eq!(
        r["thermal_surface"]["positions_m"],
        other["thermal_surface"]["positions_m"]
    );
    fs::write(root.join("other.json"), serde_json::to_vec(&other).unwrap()).unwrap();
    fs::write(
        root.join("other-map.json"),
        serde_json::to_vec(&mapped(&other)).unwrap(),
    )
    .unwrap();
    let mut changed = input;
    changed["frames"][1]["drape_report"] = json!("other.json");
    changed["frames"][1]["coverage"] = json!("other-map.json");
    motion_refused(&root, &changed);
}

#[test]
fn motion_cli_rejects_changed_covered_domain_manual_maps_and_invalid_time_budgets() {
    let (root, input, r) = motion_case(false);
    let bare = motion_map(&r, 0.3, false, 2);
    assert!(bare["layout"]["regions"].as_array().unwrap().is_empty());
    fs::write(root.join("bare.json"), serde_json::to_vec(&bare).unwrap()).unwrap();
    fs::write(
        root.join("manual.json"),
        serde_json::to_vec(&mapping(&r)).unwrap(),
    )
    .unwrap();
    let mut corrupt: Value =
        serde_json::from_slice(&fs::read(root.join("start.json")).unwrap()).unwrap();
    corrupt["projection"]["result"]["layout"]["regions"][0]["body_fraction"] = json!(0.1);
    fs::write(
        root.join("corrupt.json"),
        serde_json::to_vec(&corrupt).unwrap(),
    )
    .unwrap();
    for change in [
        (|v: &mut Value| v["frames"][1]["coverage"] = json!("manual.json")) as fn(&mut Value),
        |v| v["frames"][1]["coverage"] = json!("corrupt.json"),
        |v| v["frames"][1]["time_seconds"] = json!(1.),
        |v| v["frames"][0]["steps"] = json!(0),
        |v| v["frames"][0]["steps"] = json!(100_001),
        |v| v["frames"][1]["boundaries"] = json!([]),
        |v| v["frames"][1]["boundaries"][0]["inside_c"] = json!(-274.),
        |v| v["initial"]["temperature_c"] = json!(-274.),
    ] {
        let mut bad = input.clone();
        change(&mut bad);
        motion_refused(&root, &bad);
    }
    // A pose that leaves the cloth outside body coverage is now a valid material-history state.
    // Without exposed_boundaries it is deliberately adiabatic, never reset to an ambient value.
    let mut exposed = input.clone();
    exposed["frames"][1]["coverage"] = json!("bare.json");
    let out = motion_success(&root, &exposed);
    assert!(out["final"]["temperature_c"].as_array().unwrap().is_empty());
    assert!(
        !out["final"]["exposed_temperature_c"][0]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let adiabatic = out["final"]["exposed_temperature_c"].clone();
    exposed["frames"][1]["exposed_boundaries"] = json!([{
        "inside_c":40.,"outside_c":40.,
        "inside_resistance_m2_k_w":0.05,"outside_resistance_m2_k_w":0.05
    }]);
    let heated = motion_success(&root, &exposed);
    assert_ne!(heated["final"]["exposed_temperature_c"], adiabatic);
    assert!(
        heated["trace"][1]["exposed_inside_heat_w"]
            .as_f64()
            .unwrap()
            > 0.
    );
    assert!(
        heated["energy"]["physical_stored_change_j"]
            .as_f64()
            .unwrap()
            .is_finite()
    );
    // Even a zero-temperature-cell, all-bare scene must account for body/source geometry work.
    let mut all_bare = input;
    all_bare["initial"]["coverage"] = json!("bare.json");
    all_bare["frames"] = json!([{"time_seconds":1.,"drape_report":"report.json","coverage":"bare.json",
        "steps":100_000,"boundaries":conditions()["boundaries"]}]);
    motion_refused(&root, &all_bare);
}

fn coverage_exterior(air_c: f64, ambient_c: f64) -> Value {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-exterior-exchange.json"
    ))
    .unwrap();
    let mut exterior = fixture["exterior_exchange"].clone();
    exterior["air_c"] = json!(air_c);
    exterior["radiation"]["ambient_c"] = json!(ambient_c);
    exterior["convection_conditions"] =
        json!("Synthetic prescribed coefficient on the declared geometry; no wind inference");
    exterior
}

fn exterior_motion_case() -> (PathBuf, Value) {
    let (root, mut input, report) =
        motion_case_from_report(sewn_report_with_optics(0.4, false, Some(0.8)));
    let bare = motion_map(&report, 0.3, false, 2);
    assert!(bare["layout"]["regions"].as_array().unwrap().is_empty());
    fs::write(root.join("bare.json"), serde_json::to_vec(&bare).unwrap()).unwrap();
    for (i, f) in input["frames"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        f["bare_boundaries"] = f["boundaries"].clone();
        let (air, radiant) = if i == 0 { (5., -10.) } else { (40., 55.) };
        f["boundaries"][0]["outside_c"] = json!(air);
        f["boundaries"][0]["outside_resistance_m2_k_w"] = json!(0.);
        f["exterior_exchange"] = json!([coverage_exterior(air, radiant)]);
    }
    let last = &mut input["frames"][1];
    last["coverage"] = json!("bare.json");
    last["exposed_boundaries"] = last["boundaries"].clone();
    last["exposed_exterior_exchange"] = last["exterior_exchange"].clone();
    (root, input)
}

#[test]
fn motion_cli_exterior_preserves_weather_and_heat_across_exposure_and_restart() {
    let (root, input) = exterior_motion_case();
    let complete = motion_success(&root, &input);
    assert_eq!(complete, motion_success(&root, &input));
    assert_eq!(
        complete["input"]["frames"][0]["exterior_exchange"],
        input["frames"][0]["exterior_exchange"]
    );
    assert_eq!(
        complete["input"]["frames"][1]["exposed_exterior_exchange"],
        input["frames"][1]["exposed_exterior_exchange"]
    );
    assert!(complete["trace"][0]["covered_area_m2"].as_f64().unwrap() > 0.);
    assert_eq!(complete["trace"][1]["covered_area_m2"], 0.);
    assert!(
        complete["energy"]["exterior_exchange"]["convective_energy_j"]
            .as_f64()
            .unwrap()
            > 0.
    );
    for key in ["convective_energy_j", "radiative_energy_j"] {
        assert!(
            complete["energy"]["exposed_exterior_exchange"][key]
                .as_f64()
                .unwrap()
                < 0.
        );
    }
    let faces = complete["final"]["exposed_exterior_exchange"]["triangles"]
        .as_array()
        .unwrap();
    assert!(!faces.is_empty());
    let mut convection = 0.;
    let mut radiation = 0.;
    for face in faces {
        let area = face["current_area_m2"].as_f64().unwrap();
        let flux = &face["flux"];
        let surface = flux["surface_c"].as_f64().unwrap();
        near(
            flux["convective_flux_w_m2"].as_f64().unwrap(),
            3. * (surface - 40.),
            1e-10,
        );
        assert_eq!(flux["radiation_enabled"], true);
        assert!(flux["balance_residual_w_m2"].as_f64().unwrap().abs() < 1e-6);
        convection += area * flux["convective_flux_w_m2"].as_f64().unwrap();
        radiation += area * flux["radiative_flux_w_m2"].as_f64().unwrap();
    }
    near(
        complete["final"]["exposed_exterior_exchange"]["convective_heat_w"]
            .as_f64()
            .unwrap(),
        convection,
        1e-10,
    );
    near(
        complete["final"]["exposed_exterior_exchange"]["radiative_heat_w"]
            .as_f64()
            .unwrap(),
        radiation,
        1e-10,
    );
    near(complete["heat_balance_error_j"].as_f64().unwrap(), 0., 1e-7);
    let mut first_input = input.clone();
    first_input["frames"].as_array_mut().unwrap().truncate(1);
    let first = motion_success(&root, &first_input);
    fs::write(
        root.join("exterior-previous.json"),
        serde_json::to_vec(&first).unwrap(),
    )
    .unwrap();
    let mut resume = input.clone();
    resume["initial"] = json!({"kind":"resume","checkpoint":"exterior-previous.json"});
    resume["frames"] = json!([input["frames"][1]]);
    let second = motion_success(&root, &resume);
    assert_eq!(complete["checkpoint"], second["checkpoint"]);
    assert_eq!(complete["final"], second["final"]);
    for mode in ["exterior_exchange", "exposed_exterior_exchange"] {
        for key in ["convective_energy_j", "radiative_energy_j"] {
            near(
                complete["energy"][mode][key].as_f64().unwrap(),
                first["energy"][mode][key].as_f64().unwrap_or(0.)
                    + second["energy"][mode][key].as_f64().unwrap_or(0.),
                1e-10,
            );
        }
    }
    resume["initial"] = json!({"kind":"uniform","time_seconds":2.,
        "drape_report":"report.json","coverage":"start.json","temperature_c":10.});
    let restarted = motion_success(&root, &resume);
    assert_ne!(
        second["final"]["exposed_temperature_c"],
        restarted["final"]["exposed_temperature_c"]
    );
    assert_eq!(complete["empirical_validation_claim"], false);
}

#[test]
fn motion_cli_exterior_refuses_conflicts_missing_bare_and_unqualified_optics() {
    let (root, input) = exterior_motion_case();
    for change in [
        (|v: &mut Value| {
            v["frames"][0]["boundaries"][0]["outside_resistance_m2_k_w"] = json!(0.2);
        }) as fn(&mut Value),
        |v| v["frames"][1]["exposed_boundaries"][0]["outside_c"] = json!(39.),
        |v| {
            v["frames"][0]
                .as_object_mut()
                .unwrap()
                .remove("bare_boundaries");
        },
        |v| {
            v["frames"][1]["exposed_exterior_exchange"] =
                json!([coverage_exterior(40., 55.), coverage_exterior(40., 55.)]);
        },
        |v| {
            v["frames"][0]["exterior_exchange"] =
                json!([coverage_exterior(5., -10.), coverage_exterior(5., -10.)]);
        },
        |v| {
            v["frames"][1]
                .as_object_mut()
                .unwrap()
                .remove("exposed_boundaries");
        },
        |v| v["frames"][0]["exterior_exchange"][0]["allow_estimates"] = json!(false),
        |v| {
            v["frames"][1]["exposed_exterior_exchange"][0]["radiation"]["exposed_face"] =
                json!("unqualified-face");
        },
        |v| v["frames"][0]["bare_boundaries"] = json!([]),
    ] {
        let mut bad = input.clone();
        change(&mut bad);
        motion_refused(&root, &bad);
    }
    let (root, legacy, _) = motion_case(false);
    let legacy = motion_success(&root, &legacy);
    for field in [
        "exterior_exchange",
        "bare_boundaries",
        "exposed_exterior_exchange",
    ] {
        assert!(legacy["input"]["frames"][0].get(field).is_none());
        assert!(legacy["trace"][0].get(field).is_none());
        assert!(legacy["energy"].get(field).is_none());
        assert!(legacy["final"].get(field).is_none());
    }
}

fn static_exterior_conditions() -> Value {
    let mut input = conditions();
    input["boundaries"][0]["outside_c"] = json!(25.);
    input["bare_boundaries"] = input["boundaries"].clone();
    input["boundaries"][0]["outside_resistance_m2_k_w"] = json!(0.);
    input["exterior_exchange"] = json!([coverage_exterior(25., 0.)]);
    input["transient"]["steps"] = json!(4);
    input["transient"]["record_every"] = json!(1);
    input
}

#[test]
fn static_coverage_exterior_preserves_geometry_optics_and_signed_energy() {
    let report = sewn_report_with_optics(0.4, true, Some(0.8));
    let mapping = mapped(&report);
    let input = static_exterior_conditions();
    let out = success(&report, &mapping, &input);
    assert_eq!(out["conditions"], input);
    assert_eq!(out["model"]["source"], report["thermal_surface"]["model"]);
    assert_eq!(out["mapping_is_geometrically_verified"], true);
    assert_eq!(out["comfort_claim"], false);
    assert_eq!(out, success(&report, &mapping, &input));

    let positions: Vec<Vec3> = serde_json::from_value(out["cloth_positions_m"].clone()).unwrap();
    let regions = mapping["layout"]["regions"].as_array().unwrap();
    let fields = out["final"]["exterior_exchange"]["regions"]
        .as_array()
        .unwrap();
    assert_eq!(fields.len(), regions.len());
    let mut convection = 0.;
    let mut radiation = 0.;
    let mut storage = 0.;
    let fourth = |t: f64| {
        let k = t + 273.15;
        k * k * k * k
    };
    for (i, (region, field)) in regions.iter().zip(fields).enumerate() {
        let portions = region["layers"].as_array().unwrap();
        let outer = portions.last().unwrap();
        let triangle = outer["cloth_triangle"].as_u64().unwrap() as usize;
        let face: [usize; 3] = serde_json::from_value(
            report["thermal_surface"]["model"]["patches"][triangle]["vertices"].clone(),
        )
        .unwrap();
        let [a, b, c] = face.map(|index| positions[index]);
        let area = 0.5 * (b - a).cross(c - a).norm() * outer["fraction"].as_f64().unwrap();
        assert_eq!(field["id"], region["id"]);
        assert_eq!(field["outer_cloth_triangle"], outer["cloth_triangle"]);
        assert_eq!(field["outer_layer"], outer["layer"]);
        near(field["outer_area_m2"].as_f64().unwrap(), area, 1e-14);
        let surface = field["flux"]["surface_c"].as_f64().unwrap();
        let qc = 3. * (surface - 25.);
        let qr = 0.8
            * ftx_drape::thermal::steady::exterior::STEFAN_BOLTZMANN
            * (fourth(surface) - fourth(0.));
        near(
            field["flux"]["convective_flux_w_m2"].as_f64().unwrap(),
            qc,
            1e-9,
        );
        near(
            field["flux"]["radiative_flux_w_m2"].as_f64().unwrap(),
            qr,
            1e-9,
        );
        near(
            field["convective_heat_w"].as_f64().unwrap(),
            area * qc,
            1e-10,
        );
        near(
            field["radiative_heat_w"].as_f64().unwrap(),
            area * qr,
            1e-10,
        );
        convection += area * qc;
        radiation += area * qr;
        // Independently integrate the fixture's Cp(T)=1000+20*T over retained reference mass.
        for (layer, portion) in portions.iter().enumerate() {
            let index = portion["cloth_triangle"].as_u64().unwrap() as usize;
            let reference =
                report["thermal_surface"]["model"]["patches"][index]["reference_area_m2"]
                    .as_f64()
                    .unwrap()
                    * portion["fraction"].as_f64().unwrap();
            for cell in 0..2 {
                let t = out["final"]["temperature_c"][i][2 * layer + cell]
                    .as_f64()
                    .unwrap();
                storage += reference / 2. * (200. * (t - 10.) + 2. * (t * t - 100.));
            }
        }
    }
    assert!(convection < 0.);
    assert!(radiation > 0.);
    let bare = out["final"]["bare_heat_w"].as_f64().unwrap();
    assert!(bare > 0.);
    near(
        out["final"]["outside_heat_w"].as_f64().unwrap(),
        convection + radiation + bare,
        1e-8,
    );
    near(
        out["transient"]["stored_energy_change_j"].as_f64().unwrap(),
        storage,
        1e-8,
    );
    let mut convective_j = 0.;
    let mut radiative_j = 0.;
    let mut bare_j = 0.;
    let mut previous = 0.;
    for sample in out["transient"]["trace"].as_array().unwrap().iter().skip(1) {
        let time = sample["time_seconds"].as_f64().unwrap();
        let dt = time - previous;
        previous = time;
        let exchange = &sample["exterior_exchange"];
        // Histories keep aggregate powers; complete outer-face fields are in steady/final.
        assert!(exchange.get("regions").is_none());
        assert!(exchange.get("field").is_none());
        convective_j += dt * exchange["convective_heat_w"].as_f64().unwrap();
        radiative_j += dt * exchange["radiative_heat_w"].as_f64().unwrap();
        bare_j += dt * sample["bare_heat_w"].as_f64().unwrap();
        near(
            exchange["convective_energy_j"].as_f64().unwrap(),
            convective_j,
            1e-10,
        );
        near(
            exchange["radiative_energy_j"].as_f64().unwrap(),
            radiative_j,
            1e-10,
        );
    }
    let exchange = &out["transient"]["exterior_exchange"];
    near(
        exchange["convective_energy_j"].as_f64().unwrap(),
        convective_j,
        1e-10,
    );
    near(
        exchange["radiative_energy_j"].as_f64().unwrap(),
        radiative_j,
        1e-10,
    );
    near(exchange["bare_energy_j"].as_f64().unwrap(), bare_j, 1e-10);
    near(
        out["transient"]["outgoing_energy_j"].as_f64().unwrap(),
        convective_j + radiative_j + bare_j,
        1e-8,
    );
    near(
        exchange["outside_energy_balance_error_j"].as_f64().unwrap(),
        0.,
        1e-8,
    );
    near(
        out["transient"]["balance_error_j"].as_f64().unwrap(),
        0.,
        1e-8,
    );

    let count = mapping["layout"]["body_triangles"]
        .as_array()
        .unwrap()
        .len();
    let mut explicit = input.clone();
    for field in ["boundaries", "bare_boundaries", "exterior_exchange"] {
        explicit[field] = json!(vec![input[field][0].clone(); count]);
    }
    let expanded = success(&report, &mapping, &explicit);
    for field in ["steady", "final", "transient", "model"] {
        assert_eq!(expanded[field], out[field]);
    }
    assert_ne!(expanded["sha256"], out["sha256"]); // Original boundary declarations stay frozen.
    let mut unsigned = out.clone();
    unsigned.as_object_mut().unwrap().remove("sha256");
    assert_eq!(
        out["sha256"],
        sha256_hex(&serde_json::to_vec(&unsigned).unwrap())
    );

    let reflective = sewn_report_with_optics(0.4, true, Some(0.1));
    assert_eq!(
        report["thermal_surface"]["positions_m"],
        reflective["thermal_surface"]["positions_m"]
    );
    refused(&reflective, &mapping, &input);
    let changed = success(&reflective, &mapped(&reflective), &input);
    assert_ne!(out["model"]["source"], changed["model"]["source"]);
    assert_ne!(
        out["final"]["temperature_c"],
        changed["final"]["temperature_c"]
    );
    assert_eq!(
        out["steady"]["bare_heat_w"],
        changed["steady"]["bare_heat_w"]
    );
}

#[test]
fn static_coverage_exterior_refuses_ambiguous_boundaries_and_preserves_legacy_fields() {
    let report = sewn_report_with_optics(0.4, false, Some(0.8));
    let mapping = mapping(&report);
    let input = static_exterior_conditions();
    for change in [
        (|v: &mut Value| v["boundaries"][0]["outside_resistance_m2_k_w"] = json!(0.2))
            as fn(&mut Value),
        |v| v["boundaries"][0]["outside_c"] = json!(24.),
        |v| {
            v.as_object_mut().unwrap().remove("bare_boundaries");
        },
        |v| v["bare_boundaries"][0]["inside_c"] = json!(29.),
        |v| v["exterior_exchange"] = json!([]),
        |v| {
            v["exterior_exchange"] = json!([coverage_exterior(25., 0.), coverage_exterior(25., 0.)])
        },
        |v| {
            v["bare_boundaries"] =
                json!([conditions()["boundaries"][0], conditions()["boundaries"][0]])
        },
        |v| v["exterior_exchange"][0]["allow_estimates"] = json!(false),
        |v| v["exterior_exchange"][0]["radiation"]["exposed_face"] = json!("unqualified-face"),
        |v| {
            v.as_object_mut().unwrap().remove("exterior_exchange");
        },
    ] {
        let mut invalid = input.clone();
        change(&mut invalid);
        refused(&report, &mapping, &invalid);
    }
    let unqualified = self::report();
    refused(&unqualified, &self::mapping(&unqualified), &input);
    let legacy = success(&report, &mapping, &conditions());
    assert!(legacy.get("exterior_exchange").is_none());
    for field in ["steady", "final", "transient"] {
        assert!(legacy[field].get("exterior_exchange").is_none());
    }
    assert!(
        legacy["transient"]["trace"]
            .as_array()
            .unwrap()
            .iter()
            .all(|v| { v.get("exterior_exchange").is_none() })
    );
}

#[test]
fn motion_cli_retains_declared_exterior_policy_when_cloth_returns_to_body_coverage() {
    let (root, mut input) = exterior_motion_case();
    let mut returned = input["frames"][1].clone();
    returned["time_seconds"] = json!(6.);
    returned["coverage"] = json!("start.json");
    input["frames"].as_array_mut().unwrap().push(returned);
    let out = motion_success(&root, &input);
    assert_eq!(out["trace"][1]["covered_area_m2"], 0.);
    assert!(out["trace"][2]["covered_area_m2"].as_f64().unwrap() > 0.);
    assert!(out["trace"][2]["remap"].is_object());
    assert_eq!(
        out["input"]["frames"][2]["exposed_exterior_exchange"],
        input["frames"][2]["exposed_exterior_exchange"]
    );
    assert!(out["final"]["exterior_exchange"].is_object());
    assert_eq!(out["checkpoint"]["time_seconds"], 6.);
    near(out["heat_balance_error_j"].as_f64().unwrap(), 0., 1e-7);
}

//! Portable material selection -> real drape -> saved-geometry heat workbench.
//! All geometry and material numbers are synthetic, never an empirical garment validation.
use ftx_core::{Vec3, det};
use ftx_drape::run::{DrapeBody, DrapeOptions, drape};
use ftx_drape::scene::PartFrames;
use ftx_drape::start::PartFrame;
use ftx_drape::thermal::steady::request::SurfaceConditions;
use ftx_pattern::Pattern;
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "ftx-crease-workflow-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn write(&self, file: &str, value: &Value) {
        fs::write(self.0.join(file), serde_json::to_vec(value).unwrap()).unwrap();
    }
    fn read(&self, file: &str) -> Value {
        serde_json::from_slice(&fs::read(self.0.join(file)).unwrap()).unwrap()
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ftx-materials"))
            .current_dir(&self.0)
            .args(args)
            .output()
            .unwrap()
    }
    fn ok(&self, args: &[&str]) -> Value {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    }
}
fn selection() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-plastic-bending.json"
    ))
    .unwrap()
}
fn membrane_selection() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-membrane-plasticity.json"
    ))
    .unwrap()
}
fn garment() -> Value {
    let w = 40. * std::f64::consts::PI;
    let pieces: Vec<_> = ["front", "back"].into_iter().enumerate().map(|(i,id)| json!({
        "id":id,"cut":{"qty":1},"vertices":{"a":[0,0],"b":[w,0],"c":[w,30],"d":[0,30]},
        "edges":[{"id":"bottom","from":"a","to":"b"},{"id":"right","from":"b","to":"c"},
            {"id":"top","from":"c","to":"d"},{"id":"left","from":"d","to":"a"}],
        "grain":[[0,0],[0,30]],"placement":{"part":"torso","radius_mm":40,"x0":w/2.,"theta_deg":180*i}
    })).collect();
    json!({"schema":"fabric-pattern/1","units":"mm","fabric":{"note":"retain source pattern"},
        "pieces":pieces,"seams":[{"id":"left","a":"front/right","b":"back/left"},
        {"id":"right","a":"back/right","b":"front/left"}]})
}

#[test]
fn binding_and_resolution_agree_on_incomplete_or_unapproved_crease_laws() {
    for (i, mut selected) in (0..5).map(|i| (i, selection())) {
        match i {
            0 => {
                selected["record"]["properties"]
                    .as_object_mut()
                    .unwrap()
                    .remove("bending_yield_warp");
            }
            1 => {
                selected["record"]["properties"]["bending_hardening_weft"]["conditions"]["specimen_id"] =
                    json!("other-lot");
            }
            2 => {
                selected["record"]["structure"]["bending_plasticity_model"] = json!("unknown/1");
            }
            3 => {
                selected["allow_estimates"] = json!(false);
            }
            _ => {
                selected["record"]["properties"]["bending_curvature_limit_twist"]["value"] =
                    json!(0.);
            }
        }
        let w = Workspace::new();
        w.write("pattern.json", &garment());
        w.write("selection.json", &selected);
        let result = w.ok(&["bind", "pattern.json", "selection.json", "bound.json"]);
        assert_eq!(result["simulation_ready"], false, "case {i}");
        assert_eq!(w.read("bound.json")["pieces"], garment()["pieces"]);
        let result = w.run(&["resolve", "bound.json"]);
        assert!(!result.status.success(), "case {i}");
        assert!(result.stdout.is_empty());
    }
}

#[test]
fn binding_preserves_geometry_and_replays_the_complete_source_locked_law() {
    let w = Workspace::new();
    w.write("pattern.json", &garment());
    w.write("selection.json", &selection());
    let bound = w.ok(&["bind", "pattern.json", "selection.json", "bound.json"]);
    assert_eq!(bound["simulation_ready"], true);
    assert_eq!(bound["geometry_changed"], false);
    let saved = w.read("bound.json");
    assert_eq!(saved["pieces"], garment()["pieces"]);
    assert_eq!(saved["seams"], garment()["seams"]);
    assert_eq!(saved["fabric"]["note"], "retain source pattern");
    let physical = w.ok(&["resolve", "bound.json"]);
    assert_eq!(physical["record_sha256"], bound["material_sha256"]);
    assert!(physical["bending_plasticity_law_sha256"].is_string());
    assert_eq!(physical, w.ok(&["resolve", "bound.json"]));
    let bytes = fs::read(w.0.join("bound.json")).unwrap();
    assert!(
        !w.run(&["bind", "pattern.json", "selection.json", "bound.json"])
            .status
            .success()
    );
    assert_eq!(bytes, fs::read(w.0.join("bound.json")).unwrap());
    let mut edited = saved;
    edited["fabric"]["material"]["record"]["properties"]["bending_yield_warp"]["value"] = json!(2.);
    w.write("edited.json", &edited);
    assert!(!w.run(&["resolve", "edited.json"]).status.success());
}

struct Frames;
impl PartFrames for Frames {
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

fn cylinder() -> (Vec<Vec3>, Vec<[usize; 3]>) {
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    for z in [-0.02, 0.12] {
        for i in 0..16 {
            let (s, c) = det::sin_cos(f64::from(i) * std::f64::consts::TAU / 16.);
            vertices.push(Vec3::new(0.03 * c, 0.03 * s, z));
        }
    }
    for i in 0..16 {
        let j = (i + 1) % 16;
        triangles.extend([[i, j, j + 16], [i, j + 16, i + 16]]);
    }
    (vertices, triangles)
}

#[test]
fn bound_material_drives_crease_drape_and_heat_uses_that_same_deformed_surface() {
    let w = Workspace::new();
    w.write("pattern.json", &garment());
    w.write("selection.json", &selection());
    w.ok(&["bind", "pattern.json", "selection.json", "bound.json"]);
    let pattern = Pattern::new(w.read("bound.json"));
    let (vertices, triangles) = cylinder();
    let body = DrapeBody {
        frames: &Frames,
        collision: (&vertices, &triangles),
        display: (&vertices, &triangles),
        arm_deg: 35.,
    };
    let options = DrapeOptions {
        spacing: 10.,
        sew_steps: 3,
        drape_steps: 3,
        step: 0.0005,
        ..Default::default()
    };
    let run = drape(&pattern, &json!({}), &body, &options, |_| {}).unwrap();
    assert_eq!(run.report["bending_history"]["accepted_steps"], 6);
    assert!(
        run.report["bending_history"]["plastic_dissipation_j"]
            .as_f64()
            .unwrap()
            > 0.
    );
    let conditions: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/steady-surface-conditions.json"
    ))
    .unwrap();
    let expected = serde_json::from_value::<SurfaceConditions>(conditions.clone())
        .unwrap()
        .evaluate(&run.report)
        .unwrap();
    w.write("report.json", &run.report);
    w.write("conditions.json", &conditions);
    let thermal = w.ok(&["thermal-steady-surface", "report.json", "conditions.json"]);
    assert_eq!(thermal, expected);
    assert_eq!(
        thermal["positions_m"],
        serde_json::to_value(&run.scene.solver.x).unwrap()
    );
    assert_eq!(
        thermal["request"]["report"]["material"],
        run.report["material"]
    );
    // 30 K across fixed film resistance 0.3 plus t/k = 0.015. Plastic work is not added as heat.
    let area = thermal["steady"]["area_m2"].as_f64().unwrap();
    let power = thermal["steady"]["power_w"].as_f64().unwrap();
    assert!((power - area * 30. / 0.315).abs() < 1e-10);
    assert_eq!(thermal["comfort_claim"], false);
    w.write("replay-report.json", &thermal["request"]["report"]);
    w.write("replay-conditions.json", &thermal["request"]["conditions"]);
    assert_eq!(
        thermal,
        w.ok(&[
            "thermal-steady-surface",
            "replay-report.json",
            "replay-conditions.json"
        ])
    );
    // Altering curvature geometry without regenerating the locked surface cannot enter heat transfer.
    let mut changed = run.report;
    changed["steady_thermal_surface"]["positions_m"][0][0] = json!(7.);
    w.write("changed.json", &changed);
    assert!(
        !w.run(&["thermal-steady-surface", "changed.json", "conditions.json"])
            .status
            .success()
    );
}

#[test]
fn membrane_binding_requires_the_complete_matched_law_and_preserves_its_source_lock() {
    for (i, mut selected) in (0..5).map(|i| (i, membrane_selection())) {
        match i {
            0 => {
                selected["record"]["properties"]
                    .as_object_mut()
                    .unwrap()
                    .remove("membrane_yield_warp");
            }
            1 => {
                selected["record"]["properties"]["warp_stiffness"]["conditions"]["specimen_id"] =
                    json!("another-coupon")
            }
            2 => selected["record"]["structure"]["membrane_plasticity_model"] = json!("unknown/1"),
            3 => selected["allow_estimates"] = json!(false),
            _ => {
                selected["record"]["properties"]["membrane_accumulation_limit_shear"]["value"] =
                    json!(0.)
            }
        }
        let w = Workspace::new();
        w.write("pattern.json", &garment());
        w.write("selection.json", &selected);
        let bound = w.ok(&["bind", "pattern.json", "selection.json", "bound.json"]);
        assert_eq!(bound["simulation_ready"], false, "case {i}");
        assert!(
            !w.run(&["resolve", "bound.json"]).status.success(),
            "case {i}"
        );
    }
    let w = Workspace::new();
    w.write("pattern.json", &garment());
    w.write("selection.json", &membrane_selection());
    let bound = w.ok(&["bind", "pattern.json", "selection.json", "bound.json"]);
    assert_eq!(bound["simulation_ready"], true);
    let report = w.ok(&["resolve", "bound.json"]);
    assert!(report["membrane_plasticity_law_sha256"].is_string());
    assert_eq!(report, w.ok(&["resolve", "bound.json"]));
    let mut edited = w.read("bound.json");
    assert_eq!(edited["pieces"], garment()["pieces"]);
    edited["fabric"]["material"]["record"]["properties"]["membrane_yield_warp"]["value"] =
        json!(1.);
    w.write("changed-law.json", &edited);
    assert!(!w.run(&["resolve", "changed-law.json"]).status.success());
}

#[test]
fn sewn_membrane_set_stays_piece_local_and_body_motion_keeps_accepted_history() {
    use ftx_drape::motion::{MotionOptions, advance_body};
    use ftx_drape::study::{Scenario, Session, motion};
    use ftx_pattern::material_membrane_plasticity::{AXES, PROPERTY_FAMILIES};

    let w = Workspace::new();
    let mut raw = garment();
    let mut elastic = membrane_selection();
    elastic["id"] = json!("synthetic-elastic-back");
    elastic["record"]["id"] = elastic["id"].clone();
    elastic["record"]["structure"]
        .as_object_mut()
        .unwrap()
        .remove("membrane_plasticity_model");
    for (family, _) in PROPERTY_FAMILIES {
        for axis in AXES {
            elastic["record"]["properties"]
                .as_object_mut()
                .unwrap()
                .remove(&format!("{family}_{axis}"));
        }
    }
    raw["pieces"][1]["fabric"] = json!({"material":elastic,"role":"lining"});
    let original = raw.clone();
    w.write("pattern.json", &raw);
    w.write(
        "changes.json",
        &json!({"schema":"ftx-material-bindings/1","default":membrane_selection()}),
    );
    let binding = w.ok(&["bind-changes", "pattern.json", "changes.json", "bound.json"]);
    assert_eq!(
        binding["drape_materials_without_self_contact"]["ready"],
        true
    );
    assert_eq!(binding["geometry_changed"], false);
    let pattern = Pattern::new(w.read("bound.json"));
    assert!(pattern.raw()["pieces"][1]["fabric"]["material"]["expected_sha256"].is_string());
    let (vertices, triangles) = cylinder();
    let body = DrapeBody {
        frames: &Frames,
        collision: (&vertices, &triangles),
        display: (&vertices, &triangles),
        arm_deg: 35.,
    };
    let options = DrapeOptions {
        spacing: 10.,
        sew_steps: 3,
        drape_steps: 3,
        step: 0.0005,
        ..Default::default()
    };
    let mut direct = drape(&pattern, &json!({}), &body, &options, |_| {}).unwrap();
    let resolved = w.ok(&["resolve", "bound.json"]);
    for (key, value) in resolved.as_object().unwrap() {
        assert_eq!(
            &direct.report["material"][key], value,
            "resolver field {key}"
        );
    }
    let instances = direct.report["material"]["instances"].as_array().unwrap();
    assert_eq!(instances.len(), 2);
    let expected_area = 40.0 * std::f64::consts::PI * 30.0 * 1e-6;
    for instance in instances {
        let piece = instance["piece_index"].as_u64().unwrap() as usize;
        assert_eq!(
            instance["record_sha256"],
            resolved["pieces"][piece]["record_sha256"]
        );
        assert!((instance["rest_area_m2"].as_f64().unwrap() - expected_area).abs() < 1e-12);
        assert!((instance["mass_kg"].as_f64().unwrap() - 0.2 * expected_area).abs() < 1e-12);
    }
    assert!((direct.report["mass_kg"].as_f64().unwrap() - 0.4 * expected_area).abs() < 1e-12);
    assert_eq!(
        direct.report["material"]["triangle_instance"],
        json!(direct.scene.triangle_owner)
    );
    assert_eq!(
        direct.report["material"]["membrane_plasticity_piece_count"],
        1
    );
    let initial = direct.report["membrane_plasticity_history"].clone();
    assert_eq!(initial["accepted_steps"], 6);
    assert!(initial["plastic_dissipation_j"].as_f64().unwrap() > 0.);
    let selected: Vec<_> = direct
        .scene
        .triangle_owner
        .iter()
        .enumerate()
        .filter(|(_, owner)| {
            pattern
                .instance(&direct.scene.mesh.instances[**owner])
                .unwrap()
                .piece_index
                == 0
        })
        .map(|(i, _)| i)
        .collect();
    let actual: Vec<_> = initial["elements"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["triangle"].as_u64().unwrap() as usize)
        .collect();
    assert_eq!(actual, selected);
    assert!(!actual.is_empty() && actual.len() < direct.scene.solver.model.membranes.len());
    assert!(initial["elements"].as_array().unwrap().iter().any(|e| {
        e["plastic_green_strain"]
            .as_array()
            .unwrap()
            .iter()
            .any(|q| q.as_f64().unwrap().abs() > 0.)
    }));

    let request: motion::Request = serde_json::from_value(json!({
        "schema":"ftx-drape-motion/1", "keyframes":[{"time_s":0.001,"arm_abduction_deg":35.}],
        "max_step_s":0.0005,"max_trials":16,"max_pairs":100000,"max_sample_vertices":10000
    }))
    .unwrap();
    let target: Vec<_> = vertices
        .iter()
        .map(|p| *p + Vec3::new(0., 0., 0.00005))
        .collect();
    let scenario = Scenario {
        schema: "ftx-drape-scenario/1".into(),
        options,
        fit_spec: json!({}),
        max_mesh_points: 4096,
        max_vertex_frames: 2_000_000,
        max_motion_trials: 4096,
        surface: None,
        vapor_surface: None,
        coverage: None,
        vapor_coverage: None,
        motion: Some(request.clone()),
    };
    let mut session = Session::new(&scenario, &body).unwrap();
    session
        .set_motion_path(motion::Path::new(
            &request,
            body.collision,
            vec![target.clone()],
        ))
        .unwrap();
    let result = session.evaluate(&pattern, |_| {}).unwrap();
    assert_eq!(result["drape"], direct.report);
    assert_eq!(
        result["motion"]["status"], "available",
        "{}",
        result["motion"]
    );
    let sample_budget = 10000 - vertices.len() - direct.scene.solver.x.len();
    let continued = advance_body(
        &mut direct.scene.solver,
        &target,
        0.001,
        MotionOptions {
            max_step_s: 0.0005,
            max_trials: 16,
            max_pairs: 100000,
            max_sample_vertices: sample_budget,
        },
    )
    .unwrap();
    let after = &result["motion"]["result"]["membrane_plasticity_history"];
    assert_eq!(
        *after,
        direct
            .scene
            .solver
            .membrane_plasticity_report()
            .unwrap()
            .unwrap()
    );
    assert_eq!(
        after["accepted_steps"].as_u64().unwrap(),
        6 + continued.samples.len() as u64
    );
    assert!(
        after["plastic_dissipation_j"].as_f64().unwrap()
            >= initial["plastic_dissipation_j"].as_f64().unwrap()
    );
    for (old, new) in initial["elements"]
        .as_array()
        .unwrap()
        .iter()
        .zip(after["elements"].as_array().unwrap())
    {
        assert_eq!(old["law_sha256"], new["law_sha256"]);
        for (a, b) in old["accumulated_plastic_green_strain"]
            .as_array()
            .unwrap()
            .iter()
            .zip(new["accumulated_plastic_green_strain"].as_array().unwrap())
        {
            assert!(b.as_f64().unwrap() >= a.as_f64().unwrap());
        }
    }
    assert_eq!(result["drape"]["membrane_plasticity_history"], initial);
    assert_eq!(raw, original);
    assert_eq!(result["physical_validation_claim"], false);
    // Explicit compression softening cannot silently change the raw Green constitutive law.
    let softened = DrapeOptions {
        compression: 0.1,
        ..scenario.options
    };
    assert!(drape(&pattern, &json!({}), &body, &softened, |_| {}).is_err());
}

#[test]
fn accepted_membrane_work_heats_only_qualified_sheets_and_excludes_initial_drape() {
    use ftx_drape::study::{Scenario, Session, motion};
    let selected = |fraction: f64| {
        let mut selection = membrane_selection();
        selection["record"]["properties"]["membrane_plastic_heat_fraction"] = json!({
            "value":fraction,"unit":"1","source":"test","evidence":"estimate",
            "method":"Synthetic irreversible-yield heat conversion, not measured textile data",
            "conditions":{"specimen_id":"artificial-stretch-A","conditioning":"synthetic constant dry state",
                "work_basis":"irreversible_membrane_plastic_dissipation",
                "temperature_min_c":"-20","temperature_max_c":"80"}});
        selection
    };
    let w = Workspace::new();
    let mut raw = garment();
    raw["pieces"][1]["fabric"] = json!({"material":selected(0.),"role":"lining"});
    w.write("pattern.json", &raw);
    w.write(
        "changes.json",
        &json!({"schema":"ftx-material-bindings/1","default":selected(0.8)}),
    );
    w.ok(&["bind-changes", "pattern.json", "changes.json", "bound.json"]);
    let pattern = Pattern::new(w.read("bound.json"));
    let (vertices, triangles) = cylinder();
    let body = DrapeBody {
        frames: &Frames,
        collision: (&vertices, &triangles),
        display: (&vertices, &triangles),
        arm_deg: 35.,
    };
    let target: Vec<_> = vertices
        .iter()
        .map(|p| *p + Vec3::new(0., 0., 0.00005))
        .collect();
    let scenario: Scenario = serde_json::from_value(json!({
        "schema":"ftx-drape-scenario/1","options":{"spacing":10.,"sew_steps":3,
            "drape_steps":3,"step":0.0005},"fit_spec":{},"max_mesh_points":4096,
        "max_vertex_frames":4000000,
        "motion":{"schema":"ftx-drape-motion/1",
            "keyframes":[{"time_s":0.001,"arm_abduction_deg":35.}],
            "max_step_s":0.0005,"max_trials":16,"max_pairs":100000,"max_sample_vertices":200000,
            "transient_surface":{"schema":"ftx-motion-surface-heat/1","initial_temperature_c":20.,
                "cells_per_layer":2,"max_step_s":0.00025,"max_cell_steps":5000000,
                "plastic_dissipation_heat":"uniform_reference_thickness",
                "boundaries":[{"inside_c":20.,"outside_c":20.,
                    "inside_resistance_m2_k_w":1.,"outside_resistance_m2_k_w":1.}]}}}))
    .unwrap();
    let evaluate = |s: &Scenario, p: &Pattern| {
        let mut session = Session::new(s, &body).unwrap();
        session
            .set_motion_path(motion::Path::new(
                s.motion.as_ref().unwrap(),
                body.collision,
                vec![target.clone()],
            ))
            .unwrap();
        session.evaluate(p, |_| {}).unwrap()
    };
    let result = evaluate(&scenario, &pattern);
    let moved = &result["motion"]["result"];
    assert_eq!(
        result["motion"]["status"], "available",
        "{}",
        result["motion"]
    );
    let thermal = &moved["transient_surface"];
    assert_eq!(thermal["status"], "available", "{thermal}");
    let thermal = &thermal["result"];
    let heating = &thermal["plastic_dissipation_heat"];
    assert!(heating["initial_plastic_dissipation_j"].as_f64().unwrap() > 0.);
    assert!(heating["supplied_heat_j"].as_f64().unwrap() > 0.);
    let initial = &moved["initial_sample"]["membrane_plastic_dissipation_j"];
    let last =
        &moved["samples"].as_array().unwrap().last().unwrap()["membrane_plastic_dissipation_j"];
    let ids = result["drape"]["material"]["triangle_instance"]
        .as_array()
        .unwrap();
    let instances = result["drape"]["material"]["instances"].as_array().unwrap();
    let mut expected_heat = 0.;
    let mut expected_work = 0.;
    for (i, owner) in ids.iter().enumerate() {
        let delta = last[i].as_f64().unwrap() - initial[i].as_f64().unwrap();
        expected_work += delta;
        let piece = instances[owner.as_u64().unwrap() as usize]["piece_index"]
            .as_u64()
            .unwrap();
        if piece == 0 {
            expected_heat += 0.8 * delta;
        } else {
            assert!(
                thermal["final"]["temperature_c"][i]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|t| (t.as_f64().unwrap() - 20.).abs() < 1e-10)
            );
        }
    }
    assert!((heating["supplied_heat_j"].as_f64().unwrap() - expected_heat).abs() < 1e-12);
    assert!((heating["plastic_dissipation_j"].as_f64().unwrap() - expected_work).abs() < 1e-12);
    assert!(
        (thermal["stored_change_j"].as_f64().unwrap()
            - thermal["net_boundary_energy_j"].as_f64().unwrap()
            - expected_heat)
            .abs()
            < 1e-10
    );
    assert!(thermal["balance_error_j"].as_f64().unwrap().abs() < 1e-10);
    assert_eq!(evaluate(&scenario, &pattern), result);
    let mut control = scenario.clone();
    control
        .motion
        .as_mut()
        .unwrap()
        .transient_surface
        .as_mut()
        .unwrap()
        .plastic_dissipation_heat = None;
    let cold = evaluate(&control, &pattern);
    for (a, b) in moved["samples"]
        .as_array()
        .unwrap()
        .iter()
        .zip(cold["motion"]["result"]["samples"].as_array().unwrap())
    {
        assert_eq!(a["positions_m"], b["positions_m"]);
        assert_eq!(a["solve"], b["solve"]);
    }
    assert_eq!(result["drape"], cold["drape"]);
    assert!(
        cold["motion"]["result"]["transient_surface"]["result"]["plastic_dissipation_heat"]
            .is_null()
    );
    // Permit exactly the first accepted sample's heat work, then refuse the next sample.
    // This exercises disposal of an already advanced private history, not an initial refusal.
    let trace = thermal["trace"].as_array().unwrap();
    assert!(trace.len() > 1);
    assert_eq!(trace.len(), moved["samples"].as_array().unwrap().len());
    let thermal_steps = thermal["thermal_steps"].as_u64().unwrap();
    let charged_steps = thermal["cell_steps"].as_u64().unwrap();
    let first_substeps = trace[0]["thermal_substeps"].as_u64().unwrap();
    assert!(first_substeps > 0 && first_substeps < thermal_steps);
    assert_eq!(charged_steps % thermal_steps, 0);
    let first_budget = first_substeps * (charged_steps / thermal_steps);
    assert!(first_budget < charged_steps);
    let mut limited = scenario.clone();
    limited
        .motion
        .as_mut()
        .unwrap()
        .transient_surface
        .as_mut()
        .unwrap()
        .max_cell_steps = usize::try_from(first_budget).unwrap();
    let exhausted = evaluate(&limited, &pattern);
    assert_eq!(exhausted["motion"]["status"], "available");
    assert_eq!(exhausted["drape"], result["drape"]);
    let completed = &exhausted["motion"]["result"];
    for key in [
        "samples",
        "final_positions_sha256",
        "final_velocities_m_s",
        "membrane_plasticity_history",
    ] {
        assert_eq!(completed[key], moved[key], "heat refusal changed {key}");
    }
    let refused = &completed["transient_surface"];
    assert_eq!(refused["status"], "refused");
    assert_eq!(refused["refusal"]["code"], "budget_exceeded");
    assert!(refused.get("result").is_none());
    assert!(refused.get("trace").is_none());
    // A stale conversion qualifier is local to heat; valid permanent deformation still completes.
    let mut incomplete = selected(0.8);
    incomplete["record"]["properties"]["membrane_plastic_heat_fraction"]["conditions"]["specimen_id"] =
        json!("other-specimen");
    w.write(
        "incomplete.json",
        &json!({"schema":"ftx-material-bindings/1","default":incomplete}),
    );
    w.ok(&[
        "bind-changes",
        "pattern.json",
        "incomplete.json",
        "incomplete-bound.json",
    ]);
    let failed_heat = evaluate(&scenario, &Pattern::new(w.read("incomplete-bound.json")));
    assert_eq!(failed_heat["motion"]["status"], "available");
    assert_eq!(
        failed_heat["motion"]["result"]["transient_surface"]["status"],
        "refused"
    );
    assert!(failed_heat["motion"]["result"]["transient_surface"]["result"].is_null());
    assert_eq!(
        failed_heat["motion"]["result"]["final_positions_sha256"],
        moved["final_positions_sha256"]
    );
}

#[test]
fn reversible_calibration_refuses_an_existing_permanent_membrane_law() {
    for (command, text) in [
        (
            "calibrate",
            include_str!("../../../tests/fixtures/materials/synthetic-swatch.json"),
        ),
        (
            "calibrate-nonlinear",
            include_str!("../../../tests/fixtures/materials/synthetic-nonlinear-swatch.json"),
        ),
        (
            "calibrate-relaxation",
            include_str!("../../../tests/fixtures/materials/synthetic-relaxation-history.json"),
        ),
    ] {
        let w = Workspace::new();
        let mut request: Value = serde_json::from_str(text).unwrap();
        request["material"] = membrane_selection()["record"].clone();
        w.write("request.json", &request);
        let result = w.run(&[command, "request.json", "--allow-estimates"]);
        assert!(!result.status.success(), "{command}");
        assert!(
            String::from_utf8_lossy(&result.stderr).contains("permanent"),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(result.stdout.is_empty());
        assert_eq!(w.read("request.json"), request);
    }
}

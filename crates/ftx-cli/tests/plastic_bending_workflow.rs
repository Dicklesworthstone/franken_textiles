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

#[test]
fn bound_material_drives_crease_drape_and_heat_uses_that_same_deformed_surface() {
    let w = Workspace::new();
    w.write("pattern.json", &garment());
    w.write("selection.json", &selection());
    w.ok(&["bind", "pattern.json", "selection.json", "bound.json"]);
    let pattern = Pattern::new(w.read("bound.json"));
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

//! Finished-band choices through both native commands and actual joint contact dynamics.
//! All band forces, sheet parameters, geometry and body data here are synthetic.
use ftx_core::{Vec3, det};
use ftx_drape::{
    Body, PartFrame,
    outfit::{Garment, Input, prepare},
    run::{DrapeBody, DrapeOptions, drape},
    scene::PartFrames,
};
use ftx_export::material_study::Request;
use ftx_pattern::{Pattern, material_binding::Changes};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

fn band(ea: f64) -> Value {
    let mut band: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-elastic-band.json"
    ))
    .unwrap();
    band["record"]["properties"]["axial_stiffness"]["value"] = json!(ea);
    band
}
fn options() -> DrapeOptions {
    DrapeOptions {
        spacing: 24.0,
        sew_steps: 3,
        drape_steps: 3,
        step: 0.001,
        elastic_ea: 1.0, // sourced coefficients must survive this different fallback
        self_contact: Some(DrapeOptions::default().contact),
        ..DrapeOptions::default()
    }
}
fn source() -> Pattern {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-friction-sliding.json"
    ))
    .unwrap();
    let mut sheet = fixture["target"]["materials"][0].clone();
    sheet["record"]["properties"]["width"] = json!({"value":1.0,"unit":"m",
        "source":"sheet","evidence":"estimate","method":"Synthetic stock width"});
    let garments = [("inner", 40.0, 0.7, 6.0), ("outer", 44.0, 1.1, 9.0)]
        .into_iter()
        .map(|(id, radius, fraction, ea)| {
            let width = std::f64::consts::TAU * radius;
            Garment {
                id: id.into(),
                pattern: json!({"schema":"fabric-pattern/1","units":"mm","fabric":{"material":sheet},
                    "pieces":[{"id":"waistband","role":"waistband","cut":{"qty":1},
                        "vertices":{"a":[0,0],"b":[width,0],"c":[width,20],"d":[0,20]},
                        "edges":[{"id":"bottom","from":"a","to":"b"},{"id":"end_r","from":"b","to":"c"},
                            {"id":"top","from":"c","to":"d"},{"id":"end_l","from":"d","to":"a"}],
                        "grain":[[0,0],[0,20]],"placement":{"part":"torso","radius_mm":radius,"x0":width/2.0}}],
                    "seams":[{"id":"close","a":"waistband/end_r","b":"waistband/end_l","reverse":true}],
                    "meta":{"style":{"elastic_overlap":10.0},"solved":{
                        "elastic_cut_mm":width*fraction+10.0,"elastic_material":band(ea)}}}),
            }
        })
        .collect();
    let prepared = prepare(
        &Input {
            schema: "ftx-outfit-drape/1".into(),
            garments,
            interfaces: vec![],
        },
        &options(),
    )
    .unwrap();
    Pattern::new(prepared.pattern().raw().clone())
}
fn changes(ea: f64) -> Value {
    json!({"schema":"ftx-material-bindings/1","elastic_casings":{"inner::waistband":band(ea)}})
}
fn study() -> Value {
    json!({"schema":"ftx-material-study/1","variants":[
        {"key":"soft","changes":changes(6.0)},{"key":"firm","changes":changes(24.0)}],
        "cutting":{"copies":1,"gap_mm":2.0,"edge_margin_mm":0.0,"end_margin_mm":0.0}})
}
fn workspace() -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "ftx-band-study-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&path).unwrap();
    path
}
fn command(dir: &Path, unified: bool, args: &[&str]) -> std::process::Output {
    let mut command = Command::new(if unified {
        env!("CARGO_BIN_EXE_ftx")
    } else {
        env!("CARGO_BIN_EXE_ftx-materials")
    });
    command.current_dir(dir);
    if unified {
        command.arg("materials");
    }
    command.args(args);
    if unified {
        command.arg("--json");
    }
    command.output().unwrap()
}
fn result(output: std::process::Output, unified: bool) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    if unified {
        value["result"].clone()
    } else {
        value
    }
}

#[test]
fn both_native_commands_bind_and_replay_exact_band_study_variants_without_editing_sources() {
    let source = source();
    let dir = workspace();
    let source_bytes = serde_json::to_vec_pretty(source.raw()).unwrap();
    fs::write(dir.join("pattern.json"), &source_bytes).unwrap();
    fs::write(
        dir.join("study.json"),
        serde_json::to_vec(&study()).unwrap(),
    )
    .unwrap();
    let expected = serde_json::to_value(
        serde_json::from_value::<Request>(study())
            .unwrap()
            .evaluate(&source)
            .unwrap(),
    )
    .unwrap();
    for unified in [false, true] {
        let report = result(
            command(&dir, unified, &["study", "pattern.json", "study.json"]),
            unified,
        );
        assert_eq!(report, expected);
        for row in report["alternatives"].as_array().unwrap() {
            let name = format!("{}-{unified}.json", row["key"].as_str().unwrap());
            fs::write(
                dir.join("changes.json"),
                serde_json::to_vec(&row["frozen_changes"]).unwrap(),
            )
            .unwrap();
            let bound = result(
                command(
                    &dir,
                    unified,
                    &["bind-changes", "pattern.json", "changes.json", &name],
                ),
                unified,
            );
            assert_eq!(
                bound["pattern_json"],
                row["binding"]["result"]["pattern_json"]
            );
            let saved = fs::read(dir.join(&name)).unwrap();
            assert_eq!(saved, bound["pattern_json"].as_str().unwrap().as_bytes());
            let pattern = Pattern::new(serde_json::from_slice(&saved).unwrap());
            let bands = ftx_drape::scene::elastic_casings(&pattern, 1.0).unwrap();
            assert_eq!(bands[0].ea, if row["key"] == "firm" { 24.0 } else { 6.0 });
            assert_eq!(bands[1].ea, 9.0);
            assert!(
                !command(
                    &dir,
                    unified,
                    &["bind-changes", "pattern.json", "changes.json", &name]
                )
                .status
                .success()
            );
            assert_eq!(saved, fs::read(dir.join(&name)).unwrap());
        }
    }
    assert_eq!(source_bytes, fs::read(dir.join("pattern.json")).unwrap());
}

#[test]
fn denied_band_refuses_without_writing_and_does_not_remove_a_valid_study_alternative() {
    let dir = workspace();
    fs::write(
        dir.join("pattern.json"),
        serde_json::to_vec(source().raw()).unwrap(),
    )
    .unwrap();
    let mut denied = changes(24.0);
    denied["elastic_casings"]["inner::waistband"]["allow_estimates"] = json!(false);
    fs::write(
        dir.join("changes.json"),
        serde_json::to_vec(&denied).unwrap(),
    )
    .unwrap();
    let mut request = study();
    request["variants"][1]["changes"] = denied;
    fs::write(
        dir.join("study.json"),
        serde_json::to_vec(&request).unwrap(),
    )
    .unwrap();
    for unified in [false, true] {
        let output = command(
            &dir,
            unified,
            &[
                "bind-changes",
                "pattern.json",
                "changes.json",
                "denied.json",
            ],
        );
        assert!(!output.status.success());
        assert!(!dir.join("denied.json").exists());
        let report = result(
            command(&dir, unified, &["study", "pattern.json", "study.json"]),
            unified,
        );
        let rows = report["alternatives"].as_array().unwrap();
        assert_eq!(rows[0]["key"], "firm");
        assert_eq!(rows[0]["binding"]["status"], "refused");
        assert_eq!(rows[1]["cutting"]["status"], "available");
    }
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
fn skin() -> Body {
    let mut vertices = Vec::new();
    for z in [-0.02, 0.12] {
        for i in 0..16 {
            let (s, c) = det::sin_cos(i as f64 * std::f64::consts::TAU / 16.0);
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
fn production_study_changes_only_one_bands_force_and_joint_dynamics_then_replays_exactly() {
    let source = source();
    let original = source.raw().clone();
    let skin = skin();
    let body = DrapeBody {
        frames: &Form,
        collision: (&skin.vertices, &skin.triangles),
        display: (&skin.vertices, &skin.triangles),
        arm_deg: 35.0,
    };
    let request: Request = serde_json::from_value(study()).unwrap();
    let mut runs = Vec::new();
    let report = request
        .evaluate_with(&source, &json!({"synthetic":true}), |key, pattern| {
            let run = drape(pattern, &json!({}), &body, &options(), |_| {})?;
            let value = json!({"key":key,"elastics":run.report["elastics"]});
            runs.push(run);
            Ok(value)
        })
        .unwrap();
    assert_eq!(runs.len(), 2); // key order: firm then soft
    let (firm, soft) = (&runs[0], &runs[1]);
    assert_eq!(firm.scene.mesh.verts2d, soft.scene.mesh.verts2d);
    assert_eq!(firm.scene.weld, soft.scene.weld);
    assert_eq!(firm.scene.solver.model.mass, soft.scene.solver.model.mass);
    assert_eq!(
        firm.scene.solver.model.membrane,
        soft.scene.solver.model.membrane
    );
    assert_eq!(firm.report["material"], soft.report["material"]);
    assert_ne!(firm.scene.solver.x, soft.scene.solver.x);
    let mut positive_force = false;
    for (index, group) in soft.scene.elastic_groups.iter().enumerate() {
        let ratio = if index == 0 { 4.0 } else { 1.0 };
        for ring in &soft.scene.elastic_rings[group.rings.clone()] {
            for (a, b) in soft.scene.solver.model.elastics[ring.clone()]
                .iter()
                .zip(&firm.scene.solver.model.elastics[ring.clone()])
            {
                assert_eq!(a.v, b.v);
                assert_eq!(a.rest, b.rest);
                assert_eq!(b.ea, ratio * a.ea);
                let force = a.tension(&soft.scene.solver.x);
                positive_force |= index == 0 && force > 0.0;
                assert!((b.tension(&soft.scene.solver.x) - ratio * force).abs() < 1e-12);
            }
        }
    }
    assert!(positive_force);
    let frozen: Changes = report.manufacturing.alternatives[0]
        .frozen_changes
        .clone()
        .unwrap();
    let replay = Pattern::new(frozen.apply(&source).unwrap()["pattern"].clone());
    let replay = drape(&replay, &json!({}), &body, &options(), |_| {}).unwrap();
    assert_eq!(firm.scene.solver.x, replay.scene.solver.x);
    assert_eq!(firm.scene.solver.v, replay.scene.solver.v);
    assert_eq!(firm.fold.frames, replay.fold.frames);
    let mut solver = runs.remove(0).scene.solver;
    let laws = solver.model.elastics.clone();
    let moved: Vec<_> = skin
        .vertices
        .iter()
        .map(|p| *p + Vec3::new(0., 0., 0.00001))
        .collect();
    ftx_drape::motion::advance_body(&mut solver, &moved, 0.001, Default::default()).unwrap();
    assert_eq!(laws, solver.model.elastics);
    solver
        .contact
        .as_ref()
        .unwrap()
        .audit_self_contact(&solver.x)
        .unwrap();
    assert_eq!(source.raw(), &original);
}

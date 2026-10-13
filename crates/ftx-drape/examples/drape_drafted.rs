//! Drapes a drafted garment from the oracle fixtures on an Anny body fitted to the reference wearer's
//! age and tape measurements: `cargo run --release -p ftx-drape --example drape_drafted -- OUT_DIR [case] [sew_steps]
//! [drape_steps]` (default case `zouave_bolero-920`). Needs the converted Anny model. Writes the body,
//! the start state and the draped garment as OBJ files, `sim_report.json`, and the fold-up as `fold.pc2`,
//! `mesh_map.json` and a self-contained `fold_viewer.html`. For experiments: `FTX_STEP` (s, default
//! 1/24), `FTX_MU` (0.4), `FTX_EPS_V` (m/s, 1e-3) and `FTX_COMPRESSION` (1). `ftx drape` does the same for any pattern.

use std::fmt::Write as _;
use std::time::Instant;

use ftx_body::{
    AnnyModel, BodyFrames, Pose, Wearer, default_model_path, fit_tape, pose_deltas, skin_mesh,
    tape_targets, trimmed_skin,
};
use ftx_core::Vec3;
use ftx_drape::animation::{mesh_map, pc2};
use ftx_drape::run::{DrapeBody, DrapeOptions, drape, viewer};
use ftx_pattern::Pattern;
use serde_json::Value;

fn obj(vertices: &[Vec3], faces: &[[usize; 3]]) -> String {
    let mut s = String::new();
    for v in vertices {
        let _ = writeln!(s, "v {} {} {}", v.x, v.y, v.z);
    }
    for f in faces {
        let _ = writeln!(s, "f {} {} {}", f[0] + 1, f[1] + 1, f[2] + 1);
    }
    s
}

fn env(name: &str, default: f64) -> f64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out = std::path::PathBuf::from(
        args.get(1)
            .expect("usage: drape_drafted OUT_DIR [case] [sew] [drape]"),
    );
    let case_id = args.get(2).map_or("zouave_bolero-920", String::as_str);
    let mut opts = DrapeOptions::default();
    if let Some(n) = args.get(3) {
        opts.sew_steps = n.parse().expect("sew steps");
    }
    if let Some(n) = args.get(4) {
        opts.drape_steps = n.parse().expect("drape steps");
    }
    opts.step = env("FTX_STEP", opts.step);
    opts.contact.mu = env("FTX_MU", opts.contact.mu);
    opts.contact.eps_v = env("FTX_EPS_V", opts.contact.eps_v);
    opts.compression = env("FTX_COMPRESSION", opts.compression);
    std::fs::create_dir_all(&out).expect("output directory");

    let drafts: Value = serde_json::from_str(
        &std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/oracle/pattern/drafts.json"
        ))
        .expect("drafts fixture"),
    )
    .expect("JSON");
    let case = drafts["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == case_id)
        .unwrap_or_else(|| panic!("no case {case_id}"));
    let meas = &case["input"]["measurements"];

    // An Anny body of that age (sexes averaged, as the reference wearer) fitted to its tape measurements,
    // in the drape pose.
    let model = AnnyModel::load(&default_model_path().expect("model path"), false)
        .unwrap_or_else(|r| panic!("{r}"));
    let wearer = Wearer::from_measurements(meas).unwrap_or_else(|r| panic!("{r}"));
    let pose = Pose::drape();
    let fit =
        fit_tape(&model, &wearer, &tape_targets(meas), &pose).unwrap_or_else(|r| panic!("{r}"));
    let rest = model.rest(&fit.params).unwrap();
    let posed = model
        .pose(&rest, &pose_deltas(&model, &rest, &pose, 1.0).unwrap())
        .unwrap();
    let skin = trimmed_skin(
        &model,
        &posed.vertices,
        &["wrist.R", "wrist.L", "foot.R", "foot.L"],
    )
    .unwrap_or_else(|r| panic!("{r}"));
    let full = skin_mesh(&posed.vertices, &model.data().faces).unwrap_or_else(|r| panic!("{r}"));
    let frames = BodyFrames::new(&model, &posed).unwrap_or_else(|r| panic!("{r}"));
    println!(
        "body: fitted in {} iterations ({}); landmarks (mm above floor): {:?}",
        fit.iterations,
        fit.residuals
            .iter()
            .map(|r| format!("{} {:.1} for {:.1}", r.name, r.body_mm, r.target_mm))
            .collect::<Vec<_>>()
            .join(", "),
        frames
            .landmarks
            .iter()
            .map(|(k, v)| format!("{k} {v:.0}"))
            .collect::<Vec<_>>()
    );
    std::fs::write(out.join("body.obj"), obj(&full.vertices, &full.triangles)).unwrap();

    let pattern = Pattern::new(case["input"]["pattern"].clone());
    let body = DrapeBody {
        frames: &frames,
        collision: (&skin.vertices, &skin.triangles),
        display: (&full.vertices, &full.triangles),
        arm_deg: pose.arm_abduction_deg.unwrap_or(Pose::A_POSE_DEG),
    };
    let t0 = Instant::now();
    let run = drape(&pattern, &case["input"]["fit_spec"], &body, &opts, |line| {
        println!("{line}")
    })
    .unwrap_or_else(|r| panic!("{r}"));
    println!("draped in {:.1} s", t0.elapsed().as_secs_f64());
    std::fs::write(
        out.join("start.obj"),
        obj(&run.scene.start.relaxed, &run.scene.mesh.faces),
    )
    .unwrap();
    std::fs::write(
        out.join("drape.obj"),
        obj(&run.scene.positions(), &run.scene.mesh.faces),
    )
    .unwrap();
    std::fs::write(
        out.join("sim_report.json"),
        serde_json::to_string_pretty(&run.report).unwrap(),
    )
    .unwrap();
    std::fs::write(out.join("fold.pc2"), pc2(&run.fold.frames)).unwrap();
    std::fs::write(
        out.join("mesh_map.json"),
        serde_json::to_string_pretty(&mesh_map(&run.scene.mesh, &run.fold)).unwrap(),
    )
    .unwrap();
    let template = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fold_viewer_template.html"
    ));
    let html = viewer(&run, &pattern, &body, template).unwrap_or_else(|e| panic!("{e}"));
    std::fs::write(out.join("fold_viewer.html"), &html).unwrap();
    println!(
        "fit: {} gating, {} advisory, settled {}",
        run.gating().len(),
        run.advisory().len(),
        run.report["settled"]
    );
    for f in run.gating() {
        println!("  GATING   {f}");
    }
    for f in run.advisory() {
        println!("  advisory {f}");
    }
}

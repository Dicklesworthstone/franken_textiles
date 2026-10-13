//! Drapes a drafted garment from the oracle fixtures on an Anny body of the reference wearer's stature:
//! `cargo run --release -p ftx-drape --example drape_drafted -- OUT_DIR [case] [sew_steps] [drape_steps]`
//! (default case `zouave_bolero-920`). Needs the converted Anny model. Writes the body, the start state
//! and the draped garment as OBJ files, and the fold-up as `fold.pc2`, `mesh_map.json` and a
//! self-contained `fold_viewer.html` (spec 04 §9). For experiments: `FTX_STEP` (s, default 1/24), `FTX_MU` (0.4),
//! `FTX_EPS_V` (m/s, 1e-3) and `FTX_DEBUG_STEPS` (print three steps' solver reports first).

use std::fmt::Write as _;
use std::time::Instant;

use ftx_body::{
    AnnyModel, BodyFrames, Phenotype, Pose, age_parameter_for_years, default_model_path,
    pose_deltas, skin_mesh, trimmed_skin,
};
use ftx_core::Vec3;
use ftx_drape::animation::{
    FOLD_FRAMES, FoldUp, ViewerExtras, ZIP_FRAMES, board_layout, fold_frames, mesh_map, pc2,
    viewer_html, viewer_payload, zip_frames,
};
use ftx_drape::metrics::{MetricsInput, metrics};
use ftx_drape::scene::{Fabric, PartFrames, build_scene, run_phase};
use ftx_drape::{ContactParams, NearestTree, build_garment};
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

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out = std::path::PathBuf::from(args.get(1).expect("usage: drape_drafted OUT_DIR [case]"));
    let case_id = args.get(2).map_or("zouave_bolero-920", String::as_str);
    let sew_steps: usize = args.get(3).map_or(24, |s| s.parse().expect("sew steps"));
    let drape_steps: usize = args.get(4).map_or(48, |s| s.parse().expect("drape steps"));
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
    let wearer = &case["input"]["measurements"]["wearer"];
    let stature = wearer["stature_mm"].as_f64().unwrap();
    let years = wearer["age_months"].as_f64().unwrap() / 12.0;

    // An Anny body of that age (sexes averaged, as the reference wearer) and stature.
    let model = AnnyModel::load(&default_model_path().expect("model path"), false)
        .unwrap_or_else(|r| panic!("{r}"));
    let base = Phenotype {
        gender: 0.5,
        age: age_parameter_for_years(years),
        ..Phenotype::default()
    };
    let height_of = |h: f64| {
        let p = model.params(Phenotype { height: h, ..base });
        model.anthropometry(&model.rest(&p).unwrap()).height * 1000.0
    };
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..40 {
        let mid = 0.5 * (lo + hi);
        if height_of(mid) < stature {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let params = model.params(Phenotype {
        height: 0.5 * (lo + hi),
        ..base
    });
    let rest = model.rest(&params).unwrap();
    let posed = model
        .pose(
            &rest,
            &pose_deltas(&model, &rest, &Pose::drape(), 1.0).unwrap(),
        )
        .unwrap();
    // The drape body: no hands or feet (no garment here covers them), openings capped.
    let skin = trimmed_skin(
        &model,
        &posed.vertices,
        &["wrist.R", "wrist.L", "foot.R", "foot.L"],
    )
    .unwrap_or_else(|r| panic!("{r}"));
    let full = skin_mesh(&posed.vertices, &model.data().faces).unwrap_or_else(|r| panic!("{r}"));
    println!(
        "drape body: {} vertices, {} triangles, {:.2} L (full skin {:.2} L)",
        skin.vertices.len(),
        skin.triangles.len(),
        skin.report.volume * 1e3,
        full.report.volume * 1e3
    );
    let frames_body = BodyFrames::new(&model, &posed).unwrap_or_else(|r| panic!("{r}"));
    println!(
        "body: {years:.1} y, stature {:.1} mm (height parameter {:.3}); landmarks (mm above floor): {:?}",
        height_of(0.5 * (lo + hi)),
        0.5 * (lo + hi),
        frames_body
            .landmarks
            .iter()
            .map(|(k, v)| format!("{k} {v:.0}"))
            .collect::<Vec<_>>()
    );
    std::fs::write(out.join("body.obj"), obj(&skin.vertices, &skin.triangles)).unwrap();

    // The garment.
    let pattern = Pattern::new(case["input"]["pattern"].clone());
    let mesh = build_garment(&pattern, 12.0).unwrap_or_else(|r| panic!("{r}"));
    let fabric = Fabric::of(&pattern);
    let contact = ContactParams {
        dhat: 1e-3,
        thickness: 1.5e-3,
        kappa: 100.0,
        mu: std::env::var("FTX_MU")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.4),
        eps_v: std::env::var("FTX_EPS_V")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1e-3),
    };
    let t0 = Instant::now();
    let mut scene = build_scene(
        &pattern,
        mesh,
        &frames_body,
        &skin.vertices,
        &skin.triangles,
        fabric,
        0.05,
        contact,
    )
    .unwrap_or_else(|r| panic!("{r}"));
    println!(
        "garment: {} instances, {} vertices ({} after welding), {} triangles ({} collapsed by welding); \
         start crossings untangled per round {:?}; start relaxed {} iterations, p95 {:?}; built in {:.1} s; fabric \
         {fabric:?}",
        scene.mesh.instances.len(),
        scene.mesh.verts2d.len(),
        scene.solver.x.len(),
        scene.mesh.faces.len(),
        scene.collapsed,
        scene.untangled,
        scene.start.relax_iterations,
        scene.start.block_p95,
        t0.elapsed().as_secs_f64()
    );
    std::fs::write(
        out.join("start.obj"),
        obj(&scene.start.relaxed, &scene.mesh.faces),
    )
    .unwrap();
    // Newton stops when ‖Δx‖∞/h < tol: 1e-4 m/s is 4 µm per 1/24 s step, ample for cloth.
    scene.solver.tol = 1e-4;
    let h = std::env::var("FTX_STEP")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1.0 / 24.0);
    if std::env::var_os("FTX_DEBUG_STEPS").is_some() {
        for _ in 0..3 {
            let t = Instant::now();
            let r = scene.solver.step(h).unwrap();
            println!("debug step: {:.1} s, {r:?}", t.elapsed().as_secs_f64());
        }
    }
    // The fold-up: board, fold, zip, then the solver's frames.
    let board = board_layout(&scene.mesh);
    let mut frames = fold_frames(&scene.mesh, &scene.placements, &board, FOLD_FRAMES);
    frames.extend(zip_frames(&scene.start.wrapped, &scene.start.relaxed));
    for (name, steps, g) in [("sew", sew_steps, 0.0), ("drape", drape_steps, 9.81)] {
        let t = Instant::now();
        let rep = run_phase(&mut scene, steps, h, g, |_, x| frames.push(x.to_vec()))
            .unwrap_or_else(|r| panic!("{r}"));
        println!(
            "{name}: {} steps, {} Newton iterations, {} unconverged, max speed {:.4} m/s, cloth offset \
             {:.2} mm, {:.1} s",
            rep.steps,
            rep.iterations,
            rep.unconverged,
            rep.max_speed,
            rep.thickness * 1e3,
            t.elapsed().as_secs_f64()
        );
        std::fs::write(
            out.join(format!("{name}.obj")),
            obj(&scene.positions(), &scene.mesh.faces),
        )
        .unwrap();
    }
    let gap = scene
        .solver
        .contact
        .as_ref()
        .unwrap()
        .min_gap(&scene.solver.x, 0.01)
        .unwrap_or(f64::INFINITY);
    println!("min cloth-body gap after the drape: {:.3} mm", gap * 1e3);

    // The fit verdict: the reference's metrics and flags on the baked frames, against the body the cloth
    // collided with (as the reference measures against its form): the hands and feet are not in it, so
    // a sleeve lying over a hand is a length question, not penetration.
    let seq = &frames[FOLD_FRAMES + ZIP_FRAMES..];
    let true_body = NearestTree::new(skin.vertices.clone(), skin.triangles.clone())
        .unwrap_or_else(|r| panic!("{r}"));
    let raw = pattern.raw();
    let level_y = |inst: &str, landmark: &str| -> Option<f64> {
        let i = pattern.instance(inst)?;
        let pl = &raw["pieces"][i.piece_index]["placement"];
        let part = pl["part"].as_str().unwrap_or("torso");
        let key = if part == "torso" {
            part.to_string()
        } else {
            format!("{part}_R")
        };
        let anchor = pl["y0_landmark"].as_str().unwrap_or("waist");
        Some(
            PartFrames::landmark_h(&frames_body, &key, landmark)?
                - PartFrames::landmark_h(&frames_body, &key, anchor)?
                + pl["y0"].as_f64().unwrap_or(0.0),
        )
    };
    let report = metrics(
        &scene.mesh,
        &MetricsInput {
            pattern: &pattern,
            seq,
            sew_end: sew_steps,
            wrapped: &scene.start.wrapped,
            body: &true_body,
            level_y: &level_y,
            fit_spec: &case["input"]["fit_spec"],
            spacing: 12.0,
            arm_deg: Pose::A_POSE_DEG,
        },
    );
    std::fs::write(
        out.join("sim_report.json"),
        serde_json::to_string_pretty(&report).unwrap(),
    )
    .unwrap();
    let strings = |v: &serde_json::Value| -> Vec<String> {
        v.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|s| s.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    };
    let (gating, all) = (strings(&report["gating"]), strings(&report["flags"]));
    let advisory: Vec<String> = all
        .iter()
        .filter(|f| !gating.contains(f))
        .cloned()
        .collect();
    println!(
        "fit: {} gating, {} advisory, settled {}",
        gating.len(),
        advisory.len(),
        report["settled"]
    );
    for f in &all {
        println!(
            "  {} {f}",
            if gating.contains(f) {
                "GATING"
            } else {
                "advisory"
            }
        );
    }

    let fold = FoldUp {
        frames,
        fold: FOLD_FRAMES,
        zip: ZIP_FRAMES,
        sew: sew_steps,
        drape: drape_steps,
    };
    std::fs::write(out.join("fold.pc2"), pc2(&fold.frames)).unwrap();
    std::fs::write(
        out.join("mesh_map.json"),
        serde_json::to_string_pretty(&mesh_map(&scene.mesh, &fold)).unwrap(),
    )
    .unwrap();
    let style = raw["style"].as_str().unwrap_or("");
    let title = raw["meta"]["title"].as_str().unwrap_or(style);
    let payload = viewer_payload(
        &scene.mesh,
        &fold,
        &ViewerExtras {
            title: if title.is_empty() { "fold-up" } else { title },
            style,
            body: (&full.vertices, &full.triangles),
            gating,
            advisory,
            settled: report["settled"].as_bool().unwrap_or(false),
            arm_deg: Pose::A_POSE_DEG,
        },
        150,
    );
    let template = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fold_viewer_template.html"
    ));
    let html = viewer_html(template, &payload).unwrap_or_else(|e| panic!("{e}"));
    std::fs::write(out.join("fold_viewer.html"), &html).unwrap();
    println!(
        "fold-up: {} frames ({} fold, {} zip, {sew_steps} sew, {drape_steps} drape); viewer {:.1} MB",
        fold.frames.len(),
        FOLD_FRAMES,
        ZIP_FRAMES,
        html.len() as f64 / 1e6
    );
}

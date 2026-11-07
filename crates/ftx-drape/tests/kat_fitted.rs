//! Known answers on fitted bodies (plan §11.9, gate G2):
//! - KAT 8, the armpit wedge: the drafted bolero at 80, 92 and 116 cm, each on the toddler fitted to its
//!   reference's nominal chart targets at fixed fitting scales, arms at 35°: no gating flag, settled,
//!   every hem within ±15 mm of its drafted
//!   level (the cloth caught between arm and torso is what destabilised the reference's form).
//! - KAT 10, two starts: the 92 cm bolero sewn in 12 and in 18 steps (two paths to the same garment); both
//!   without flags, and their readings apart by no more than the run-to-run spread the reference shows
//!   (hems 6 mm, landmark clearances 3 mm, seam strain p90 1.5 %). The spread is printed. Measured
//!   2026-10-13, with exact Newton to 1e-4 m/s and again with the shipped inexact Newton to 1e-3 m/s: the two
//!   paths settle into the same minimum, every reading equal at the report's precision (0.1 mm, 0.01 %).
//!
//! Needs the converted Anny model and takes minutes:
//! `cargo test -p ftx-drape --release --test kat_fitted -- --include-ignored --nocapture`.

use ftx_body::{
    AnnyModel, BodyFrames, Pose, Wearer, clothing_skin, default_model_path, fit_tape, pose_deltas,
    skin_mesh, tape_targets, trimmed_skin,
};
use ftx_drape::run::{DrapeBody, DrapeOptions, drape};
use ftx_pattern::Pattern;
use serde_json::Value;

fn drafts() -> Value {
    serde_json::from_str(
        &std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/oracle/pattern/drafts.json"
        ))
        .unwrap(),
    )
    .unwrap()
}

// These cloth KATs fix the nominal body used by their original hem bound and digest. Chart sigmas
// describe inference uncertainty, not a change to that synthetic geometry case. Production callers
// continue to use tape_targets directly, retaining every supplied sigma.
fn nominal_reference_targets(meas: &Value) -> Vec<ftx_body::TapeTarget> {
    let mut targets = tape_targets(meas).unwrap();
    for target in &mut targets {
        target.sigma_mm = None;
    }
    targets
}

/// Drapes a drafted case on the body fitted to its measurements; the report.
fn drape_case(model: &AnnyModel, case: &Value, opts: &DrapeOptions) -> Value {
    drape_result(model, case, opts).unwrap().report
}

/// Drapes a drafted case on the body fitted to its measurements.
fn drape_result(
    model: &AnnyModel,
    case: &Value,
    opts: &DrapeOptions,
) -> Result<ftx_drape::run::DrapeRun, ftx_core::Refusal> {
    drape_result_with_targets(
        model,
        case,
        opts,
        &nominal_reference_targets(&case["input"]["measurements"]),
    )
}

fn drape_result_with_targets(
    model: &AnnyModel,
    case: &Value,
    opts: &DrapeOptions,
    targets: &[ftx_body::TapeTarget],
) -> Result<ftx_drape::run::DrapeRun, ftx_core::Refusal> {
    let meas = &case["input"]["measurements"];
    let pose = Pose::drape();
    let fit = fit_tape(
        model,
        &Wearer::from_measurements(meas).unwrap(),
        targets,
        &pose,
    )
    .unwrap();
    let rest = model.rest(&fit.params).unwrap();
    let posed = model
        .pose(&rest, &pose_deltas(model, &rest, &pose, 1.0).unwrap())
        .unwrap();
    let skin = clothing_skin(model, &posed.vertices).unwrap();
    let full = skin_mesh(&posed.vertices, &model.data().faces).unwrap();
    let frames = BodyFrames::new(model, &posed).unwrap();
    let body = DrapeBody {
        frames: &frames,
        collision: (&skin.vertices, &skin.triangles),
        display: (&full.vertices, &full.triangles),
        arm_deg: Pose::A_POSE_DEG,
    };
    drape(
        &Pattern::new(case["input"]["pattern"].clone()),
        &case["input"]["fit_spec"],
        &body,
        opts,
        |_| {},
    )
}

/// A pattern with no fold-up placement (a fez, or placements emptied) refuses; it used to panic computing
/// metrics of an empty mesh.
#[test]
#[ignore = "needs the converted Anny model"]
fn no_fold_up_placement_refuses() {
    let fx = drafts();
    let model = AnnyModel::load(&default_model_path().unwrap(), false).unwrap();
    let mut case = fx["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "zouave_bolero-920")
        .unwrap()
        .clone();
    for piece in case["input"]["pattern"]["pieces"].as_array_mut().unwrap() {
        piece["placement"] = Value::Null;
    }
    let Err(err) = drape_result(&model, &case, &DrapeOptions::default()) else {
        panic!("a pattern with no placement draped");
    };
    assert_eq!(err.code, ftx_core::RefusalCode::InvalidDocument);
    assert!(
        err.message.contains("back") && err.message.contains("placement"),
        "{err}"
    );
}

#[test]
#[ignore = "needs the converted Anny model; chart-trouser self-contact start qualification"]
fn chart_trousers_self_contact_start() {
    let fx = drafts();
    let case = fx["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "zouave_trousers-920")
        .unwrap();
    let model = AnnyModel::load(&default_model_path().unwrap(), false).unwrap();
    let targets = tape_targets(&case["input"]["measurements"]).unwrap();
    let self_contact = ftx_drape::ContactParams {
        thickness: 2e-3,
        ..DrapeOptions::default().contact
    };
    let options = DrapeOptions {
        self_contact: Some(self_contact),
        sew_steps: 3,
        drape_steps: 3,
        ..DrapeOptions::default()
    };
    let run = drape_result_with_targets(&model, case, &options, &targets).unwrap();
    assert_eq!(run.scene.start_backtracked_from, Some(400));
    assert_eq!(run.scene.start.relax_iterations, 200);
    assert_eq!(run.report["relax_iters"], 200);
    assert_eq!(run.report["start_repair"]["rejected_relax_iters"], 400);
    assert_eq!(run.report["start_repair"]["accepted_relax_iters"], 200);
    assert!(run.scene.start.block_p95[0] > ftx_drape::start::RELAX_TARGET_P95);
    run.scene
        .solver
        .contact
        .as_ref()
        .unwrap()
        .audit_self_contact(&run.scene.solver.x)
        .unwrap();
    let refused = drape_result_with_targets(
        &model,
        case,
        &DrapeOptions {
            self_contact: Some(ftx_drape::ContactParams {
                thickness: 50e-3,
                ..self_contact
            }),
            ..options
        },
        &targets,
    )
    .err()
    .expect("no earlier checkpoint clears this contact offset");
    assert_eq!(refused.code, ftx_core::RefusalCode::IntersectingStart);
}

#[test]
#[ignore = "needs admitted Anny data; clothing-collider qualification"]
fn clothing_proxy_retains_source_vertices_and_passes_strict_admission() {
    let fx = drafts();
    let case = fx["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "zouave_bolero-920")
        .unwrap();
    let model = AnnyModel::load(&default_model_path().unwrap(), false).unwrap();
    let meas = &case["input"]["measurements"];
    let pose = Pose::drape();
    let fit = fit_tape(
        &model,
        &Wearer::from_measurements(meas).unwrap(),
        &nominal_reference_targets(meas),
        &pose,
    )
    .unwrap();
    let rest = model.rest(&fit.params).unwrap();
    let posed = model
        .pose(&rest, &pose_deltas(&model, &rest, &pose, 1.0).unwrap())
        .unwrap();
    let detailed = trimmed_skin(
        &model,
        &posed.vertices,
        &["wrist.R", "wrist.L", "foot.R", "foot.L"],
    )
    .unwrap();
    let refusal =
        ftx_drape::collider::audit_surface(&detailed.vertices, &detailed.triangles, 1_000_000)
            .unwrap_err();
    assert_eq!(refusal.code, ftx_core::RefusalCode::IntersectingStart);
    let proxy = ftx_body::clothing_skin(&model, &posed.vertices).unwrap();
    let basis = proxy.contact_proxy_basis();
    assert_eq!(
        basis["selected_trim_roots"],
        serde_json::json!([
            "wrist.R",
            "wrist.L",
            "foot.R",
            "foot.L",
            "levator05.R",
            "levator05.L"
        ])
    );
    assert!(basis["generated_cap_vertices"].as_u64().unwrap() > 0);
    assert_eq!(basis["source_vertices_moved"], false);
    assert_eq!(basis["physical_qualification"], false);
    for (&source, &point) in proxy.source_vertex.iter().zip(&proxy.vertices) {
        if source != usize::MAX {
            assert_eq!(point, posed.vertices[source]);
        }
    }
    eprintln!(
        "detailed/proxy vertices {}/{}, triangles {}/{}, volume {}/{} m3",
        detailed.vertices.len(),
        proxy.vertices.len(),
        detailed.triangles.len(),
        proxy.triangles.len(),
        detailed.report.volume,
        proxy.report.volume
    );
    ftx_drape::collider::audit_surface(&proxy.vertices, &proxy.triangles, 1_000_000).unwrap();
}

fn numbers(v: &Value) -> Vec<(String, f64)> {
    v.as_object()
        .map(|o| {
            o.iter()
                .filter_map(|(k, x)| x.as_f64().map(|f| (k.clone(), f)))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
#[ignore = "needs the converted Anny model; minutes"]
fn kat8_armpit_wedge_at_three_sizes() {
    let fx = drafts();
    let model = AnnyModel::load(&default_model_path().unwrap(), false).unwrap();
    let ids = [
        "zouave_bolero-800",
        "zouave_bolero-920",
        "zouave_bolero-1160",
    ];
    let reports: Vec<Value> = std::thread::scope(|s| {
        let runs: Vec<_> = ids
            .iter()
            .map(|id| {
                let case = fx["cases"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|c| c["id"] == *id)
                    .unwrap();
                let model = &model;
                s.spawn(move || drape_case(model, case, &DrapeOptions::default()))
            })
            .collect();
        runs.into_iter().map(|r| r.join().unwrap()).collect()
    });
    let mut fails = Vec::new();
    for (id, rep) in ids.iter().zip(&reports) {
        let rise = numbers(&rep["hem_rise_mm"]);
        println!(
            "{id}: gating {}, advisory {}, settled {}, offset {} m / {} mm, final body gap {:?} m, hem rise {rise:?}",
            rep["gating"],
            rep["flags"],
            rep["settled"],
            rep["numerical"]["drape"]["thickness"],
            rep["cloth_offset_mm"],
            rep["numerical"]["drape"]["samples"]
                .as_array()
                .and_then(|s| s.last())
                .map(|s| &s["contact"]["body"]["minimum_active_gap_m"])
        );
        if rep["gating"].as_array().is_none_or(|g| !g.is_empty()) {
            fails.push(format!("{id}: gating {}", rep["gating"]));
        }
        if rep["settled"] != true {
            fails.push(format!("{id}: not settled"));
        }
        for (hem, r) in rise {
            if r.abs() > 15.0 {
                fails.push(format!("{id}: {hem} rose {r:+.1} mm"));
            }
        }
    }
    assert!(fails.is_empty(), "{fails:?}");
}

#[test]
#[ignore = "needs the converted Anny model; minutes"]
fn kat10_two_starts() {
    let fx = drafts();
    let model = AnnyModel::load(&default_model_path().unwrap(), false).unwrap();
    let case = fx["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "zouave_bolero-920")
        .unwrap();
    let starts = [12, 18];
    let reports: Vec<Value> = std::thread::scope(|s| {
        let runs: Vec<_> = starts
            .iter()
            .map(|&sew| {
                let model = &model;
                s.spawn(move || {
                    let opts = DrapeOptions {
                        sew_steps: sew,
                        ..DrapeOptions::default()
                    };
                    drape_case(model, case, &opts)
                })
            })
            .collect();
        runs.into_iter().map(|r| r.join().unwrap()).collect()
    });
    let (a, b) = (&reports[0], &reports[1]);
    let mut fails = Vec::new();
    for rep in [a, b] {
        if rep["gating"].as_array().is_none_or(|g| !g.is_empty()) || rep["settled"] != true {
            fails.push(format!(
                "gating {} settled {}",
                rep["gating"], rep["settled"]
            ));
        }
    }
    let spread = |what: &str, xa: &Value, xb: &Value, bound: f64, fails: &mut Vec<String>| {
        let pairs: Vec<(String, f64)> = numbers(xa)
            .iter()
            .filter_map(|(k, v)| xb[k].as_f64().map(|w| (k.clone(), (v - w).abs())))
            .collect();
        if pairs.is_empty() {
            fails.push(format!("{what}: nothing to compare"));
            return;
        }
        let worst =
            pairs.iter().cloned().fold(
                (String::new(), -1.0),
                |m, (k, d)| if d > m.1 { (k, d) } else { m },
            );
        if worst.1 == 0.0 {
            // the two paths settle into the same minimum: equal at the report's precision
            println!(
                "  {what}: identical over {} readings at the report's precision",
                pairs.len()
            );
        } else {
            println!("  {what}: largest spread {:.2} ({})", worst.1, worst.0);
        }
        if worst.1 > bound {
            fails.push(format!(
                "{what}: {} apart by {:.2} (bound {bound})",
                worst.0, worst.1
            ));
        }
    };
    println!("two starts (12 and 18 sewing steps):");
    spread(
        "hem heights (mm)",
        &a["hem_heights_mm"],
        &b["hem_heights_mm"],
        6.0,
        &mut fails,
    );
    let field = |rep: &Value, group: &str, key: &str| -> Value {
        Value::Object(
            rep[group]
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| (k.clone(), v[key].clone()))
                .collect(),
        )
    };
    spread(
        "landmark clearance (mm)",
        &field(a, "landmarks", "clearance_mean_mm"),
        &field(b, "landmarks", "clearance_mean_mm"),
        3.0,
        &mut fails,
    );
    spread(
        "seam strain p90 (%)",
        &field(a, "seams", "seam_strain_p90_pct"),
        &field(b, "seams", "seam_strain_p90_pct"),
        1.5,
        &mut fails,
    );
    assert!(fails.is_empty(), "{fails:?}");
}

/// KAT 9 on a fitted body: the whole pipeline (the tape fit, the scene, three sewing and three draping steps
/// of the 92 cm bolero, the fewest the drape's metrics accept) pinned by the SHA-256 of the fitted
/// parameters and the last frame's positions. The same digest must come out of native debug, native release
/// and wasm32-wasip1 (`FTX_ANNY_MODEL` naming the model file inside the WASI sandbox). The solver settings
/// are the shipped ones, so the inexact Newton's paths are covered: these six steps take 1,569 conjugate-
/// gradient iterations, 38 factorizations and 86 projected iterations.
#[test]
#[ignore = "needs the converted Anny model"]
fn kat9_fitted_pipeline_digest() {
    let fx = drafts();
    let model = AnnyModel::load(&default_model_path().unwrap(), false).unwrap();
    let case = fx["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "zouave_bolero-920")
        .unwrap();
    let meas = &case["input"]["measurements"];
    let pose = Pose::drape();
    let fit = fit_tape(
        &model,
        &Wearer::from_measurements(meas).unwrap(),
        &nominal_reference_targets(meas),
        &pose,
    )
    .unwrap();
    let rest = model.rest(&fit.params).unwrap();
    let posed = model
        .pose(&rest, &pose_deltas(&model, &rest, &pose, 1.0).unwrap())
        .unwrap();
    let skin = clothing_skin(&model, &posed.vertices).unwrap();
    let full = skin_mesh(&posed.vertices, &model.data().faces).unwrap();
    let frames = BodyFrames::new(&model, &posed).unwrap();
    let body = DrapeBody {
        frames: &frames,
        collision: (&skin.vertices, &skin.triangles),
        display: (&full.vertices, &full.triangles),
        arm_deg: Pose::A_POSE_DEG,
    };
    // the shipped solver settings (tolerance, inexact Newton), shortened schedule
    let opts = DrapeOptions {
        sew_steps: 3,
        drape_steps: 3,
        ..DrapeOptions::default()
    };
    let run = drape(
        &Pattern::new(case["input"]["pattern"].clone()),
        &case["input"]["fit_spec"],
        &body,
        &opts,
        |_| {},
    )
    .unwrap();
    let last = run.fold.frames.last().unwrap();
    let bytes: Vec<u8> = fit
        .free
        .iter()
        .copied()
        .chain(last.iter().flat_map(|p| [p.x, p.y, p.z]))
        .flat_map(f64::to_le_bytes)
        .collect();
    let digest = ftx_core::receipt::sha256_hex(&bytes);
    println!("fitted pipeline digest {digest} ({} vertices)", last.len());
    assert!(last.iter().all(|p| p.is_finite()));
    assert_eq!(
        digest,
        "7693994660a8605f5d0081abffde860400c955b109f61d050042d49f1fcac336"
    );
}

//! KAT 7, the elastic ladder (plan §11.9; spec 04 REQ-ELASTIC): the drafted 92 cm trousers on the toddler
//! fitted to the reference wearer's tape, with the casing's elastic at 0.85, 0.95, 1.0 and 1.2 × the waist.
//! The first three grip (stretched on the body, their friction 2πμT, KAT 4's law, carries the whole
//! garment's weight) and keep the waistband within 25 mm of where it was drafted; at 1.2 the elastic cannot
//! hold the trousers up and the run says so (they then rest on the hips and crotch, so "fall" reads as an
//! elastic that does not grip, not as trousers on the floor). The looser the elastic, the lower the
//! waistband, to within 1 mm: the slack one rests on the hips about where they also stop the snug one
//! (measured 2026-10-13: 1.2 × sits 0.9 mm above 1.0 × at μ 0.3, 0.6 mm at 0.5). The ladder runs at two
//! body frictions, μ = 0.3 and 0.5 (the default 0.4 is UNCERTAIN until sourced), and must read the same at
//! both.
//!
//! Needs the converted Anny model and takes minutes:
//! `cargo test -p ftx-drape --release --test kat_elastic -- --include-ignored --nocapture`.

use ftx_body::{
    AnnyModel, BodyFrames, Pose, Wearer, clothing_skin, default_model_path, fit_tape, pose_deltas,
    skin_mesh, tape_targets,
};
use ftx_drape::run::{DrapeBody, DrapeOptions, drape};
use ftx_pattern::Pattern;
use serde_json::Value;

#[test]
#[ignore = "needs the converted Anny model; minutes"]
fn kat7_elastic_ladder() {
    let drafts: Value = serde_json::from_str(
        &std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/oracle/pattern/drafts.json"
        ))
        .unwrap(),
    )
    .unwrap();
    let case = drafts["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "zouave_trousers-920")
        .unwrap();
    let meas = &case["input"]["measurements"];
    let model = AnnyModel::load(&default_model_path().unwrap(), false).unwrap();
    let pose = Pose::drape();
    let fit = fit_tape(
        &model,
        &Wearer::from_measurements(meas).unwrap(),
        &tape_targets(meas),
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
    let waist = meas["fused"]["waist"]["value_mm"].as_f64().unwrap();
    let rungs = [0.85, 0.95, 1.0, 1.2];
    for mu in [0.3, 0.5] {
        let mut opts = DrapeOptions::default();
        opts.contact.mu = mu;
        let reports: Vec<Value> = std::thread::scope(|s| {
            let runs: Vec<_> = rungs
                .iter()
                .map(|&frac| {
                    let mut raw = case["input"]["pattern"].clone();
                    let overlap = raw["meta"]["style"]["elastic_overlap"].as_f64().unwrap();
                    raw["meta"]["solved"]["elastic_cut_mm"] = (frac * waist + overlap).into();
                    let (body, spec, opts) = (&body, &case["input"]["fit_spec"], &opts);
                    s.spawn(move || {
                        drape(&Pattern::new(raw), spec, body, opts, |_| {})
                            .unwrap_or_else(|e| panic!("mu {mu}, {frac} x waist: {e:?}"))
                            .report
                    })
                })
                .collect();
            runs.into_iter().map(|r| r.join().unwrap()).collect()
        });
        let mut shifts = Vec::new();
        for (frac, rep) in rungs.iter().zip(&reports) {
            let e = &rep["elastic"];
            let shift = rep["bands"]["waistband"]["shift_mm"].as_f64().unwrap();
            println!(
                "mu {mu}, {frac} x waist: elastic {e}, waistband {shift:+.1} mm, gating {}",
                rep["gating"]
            );
            let holds = e["holds"].as_bool().unwrap();
            let slack_flag = rep["flags"].as_array().unwrap().iter().any(|f| {
                f.as_str()
                    .is_some_and(|t| t.starts_with("the elastic does not grip"))
            });
            if *frac < 1.1 {
                assert!(
                    holds && !slack_flag,
                    "mu {mu}, {frac}: the elastic should grip"
                );
                assert!(
                    shift.abs() < 25.0,
                    "mu {mu}, {frac}: waistband {shift:+.1} mm"
                );
            } else {
                assert!(
                    !holds && slack_flag,
                    "mu {mu}, {frac}: the elastic should not grip"
                );
            }
            shifts.push(shift);
        }
        assert!(
            shifts.windows(2).all(|w| w[1] <= w[0] + 1.0),
            "mu {mu}: looser elastics should not sit higher: {shifts:?}"
        );
    }
}

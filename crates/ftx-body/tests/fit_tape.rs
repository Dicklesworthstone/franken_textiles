//! Fitting Anny to tape measurements (plan §9.4, gate G1b): targets drawn from the model itself are
//! recovered within 3 mm (girths) and 2 mm (lengths), and the reference toddler's measurements are fitted
//! with their residuals reported.
//!
//! Needs the converted model, so ignored by default:
//! `cargo test -p ftx-body --release --test fit_tape -- --include-ignored --nocapture`.

use ftx_body::{
    AnnyModel, BodyFrames, FREE, N_PHENOTYPE, N_TAPE, Pose, TAPE_FIELDS, TapeTarget, Wearer,
    default_model_path, fit_tape, is_girth, pose_deltas, tape_readings,
};

fn model() -> AnnyModel {
    AnnyModel::load(&default_model_path().expect("model path"), false)
        .expect("converted Anny model")
}

fn targets(values: &[f64; N_TAPE]) -> Vec<TapeTarget> {
    TAPE_FIELDS
        .iter()
        .zip(values)
        .map(|(n, v)| TapeTarget {
            name: (*n).to_string(),
            value_mm: *v,
        })
        .collect()
}

#[test]
#[ignore = "needs the converted Anny model"]
fn synthetic_targets_are_recovered() {
    let model = model();
    let faces: Vec<[usize; 3]> = model
        .data()
        .faces
        .iter()
        .map(|f| f.map(|i| i as usize))
        .collect();
    let pose = Pose::drape();
    // (years, gender, free parameters in FREE order)
    let cases: [(f64, f64, [f64; 19]); 4] = [
        (
            2.5,
            1.0,
            [
                0.40, 0.65, 0.6, 0.4, 0.3, 0.4, -0.2, -0.3, 0.1, 0.2, -0.1, 0.3, -0.2, 0.25, -0.15,
                0.2, -0.1, 0.15, -0.2,
            ],
        ),
        (
            4.0,
            0.0,
            [
                0.60, 0.35, 0.3, 0.55, -0.2, -0.3, 0.3, 0.2, -0.2, -0.25, 0.2, -0.1, 0.1, -0.2,
                0.2, -0.15, 0.25, -0.1, 0.2,
            ],
        ),
        (
            1.5,
            0.5,
            [
                0.50, 0.50, 0.5, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
                0.0, 0.0, 0.0,
            ],
        ),
        (
            30.0,
            1.0,
            [
                0.55, 0.60, 0.45, 0.6, 0.4, 0.2, 0.3, -0.2, 0.2, 0.3, 0.1, -0.2, 0.2, 0.15, 0.1,
                -0.2, 0.1, -0.15, 0.1,
            ],
        ),
    ];
    let mut fails = Vec::new();
    for (years, gender, th) in cases {
        let age = ftx_body::age_parameter_for_years(years);
        let mut p = model.params(ftx_body::Phenotype {
            gender,
            age,
            height: th[0],
            weight: th[1],
            muscle: th[2],
            proportions: th[3],
        });
        for (j, n) in FREE[N_PHENOTYPE..].iter().enumerate() {
            p.local_changes[model.local_change_index(n).unwrap()] = th[N_PHENOTYPE + j];
        }
        let rest = model.rest(&p).unwrap();
        let height = model.anthropometry(&rest).height;
        let posed = model
            .pose(&rest, &pose_deltas(&model, &rest, &pose, 1.0).unwrap())
            .unwrap();
        let frames = BodyFrames::new(&model, &posed).unwrap();
        let want = tape_readings(&faces, height, &posed, &frames).expect("sections");
        let fit = fit_tape(
            &model,
            &Wearer {
                years,
                gender,
                stature_mm: want[0],
            },
            &targets(&want),
            &pose,
        )
        .unwrap();
        println!(
            "{years} y, gender {gender}: {} iterations, free {:?}",
            fit.iterations,
            fit.free.map(|x| (x * 1000.0).round() / 1000.0)
        );
        for r in &fit.residuals {
            let tol = if is_girth(&r.name) { 3.0 } else { 2.0 };
            let d = r.body_mm - r.target_mm;
            println!(
                "  {:14} target {:7.1}  body {:7.1}  ({d:+.2})",
                r.name, r.target_mm, r.body_mm
            );
            if d.abs() > tol {
                fails.push(format!("{years} y {}: {d:+.2} mm", r.name));
            }
        }
    }
    assert!(fails.is_empty(), "not recovered: {fails:?}");
}

#[test]
#[ignore = "needs the converted Anny model"]
fn the_reference_toddler_is_fitted() {
    let model = model();
    let wearer = Wearer {
        years: 2.5,
        gender: 0.5,
        stature_mm: 920.0,
    };
    // the reference wearer's fused values (Ottobre chart priors at 92 cm), in TAPE_FIELDS order
    let want = [
        920.0, 540.0, 530.0, 580.0, 530.0, 372.5, 288.8, 210.6, 204.5, 146.9, 156.8, 109.8, 320.0,
        231.0, 225.0,
    ];
    let faces: Vec<[usize; 3]> = model
        .data()
        .faces
        .iter()
        .map(|f| f.map(|i| i as usize))
        .collect();
    let (p, _) = ftx_body::fit_stature(&model, &wearer).unwrap();
    let rest = model.rest(&p).unwrap();
    let posed = model
        .pose(
            &rest,
            &pose_deltas(&model, &rest, &Pose::drape(), 1.0).unwrap(),
        )
        .unwrap();
    let frames = BodyFrames::new(&model, &posed).unwrap();
    let before = tape_readings(&faces, model.anthropometry(&rest).height, &posed, &frames).unwrap();
    println!(
        "stature only: {:?}",
        TAPE_FIELDS
            .iter()
            .zip(before)
            .map(|(n, v)| format!("{n} {v:.1}"))
            .collect::<Vec<_>>()
    );
    let fit = fit_tape(&model, &wearer, &targets(&want), &Pose::drape()).unwrap();
    println!(
        "{} iterations, free {:?}",
        fit.iterations,
        fit.free.map(|x| (x * 1000.0).round() / 1000.0)
    );
    for r in &fit.residuals {
        println!(
            "  {:14} target {:7.1}  body {:7.1}  ({:+.1})",
            r.name,
            r.target_mm,
            r.body_mm,
            r.body_mm - r.target_mm
        );
    }
    // every measurement within 3 mm: the chart values are reachable, the priors keep the shape natural
    for r in &fit.residuals {
        assert!((r.body_mm - r.target_mm).abs() < 3.0, "{r:?}");
    }
}

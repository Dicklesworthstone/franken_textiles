//! Fitting Anny to tape measurements (plan §9.4, gate G1b): targets drawn from the model itself are
//! recovered within 3 mm (girths) and 2 mm (lengths), and the reference toddler's measurements are fitted
//! with their residuals reported.
//!
//! Needs the converted model, so ignored by default:
//! `cargo test -p ftx-body --release --test fit_tape -- --include-ignored --nocapture`.

use ftx_body::{
    AnnyModel, BodyFrames, FREE, N_PHENOTYPE, N_TAPE, Pose, TAPE_FIELDS, TapeTarget, Wearer,
    default_model_path, fit_sigma, fit_tape, is_girth, pose_deltas, tape_readings,
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
    let cases: [(f64, f64, [f64; 23]); 4] = [
        (
            2.5,
            1.0,
            [
                0.40, 0.65, 0.6, 0.4, 0.3, 0.4, -0.2, -0.3, 0.1, 0.2, -0.1, 0.3, -0.2, 0.25, -0.15,
                0.2, -0.1, 0.15, -0.2, 0.2, -0.2, 0.15, 0.15,
            ],
        ),
        (
            4.0,
            0.0,
            [
                0.60, 0.35, 0.3, 0.55, -0.2, -0.3, 0.3, 0.2, -0.2, -0.25, 0.2, -0.1, 0.1, -0.2,
                0.2, -0.15, 0.25, -0.1, 0.2, -0.2, 0.25, -0.1, -0.1,
            ],
        ),
        (
            1.5,
            0.5,
            [
                0.50, 0.50, 0.5, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
                0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            ],
        ),
        (
            30.0,
            1.0,
            [
                0.55, 0.60, 0.45, 0.6, 0.4, 0.2, 0.3, -0.2, 0.2, 0.3, 0.1, -0.2, 0.2, 0.15, 0.1,
                -0.2, 0.1, -0.15, 0.1, 0.1, 0.2, -0.15, -0.15,
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
        let mass = model.anthropometry(&rest).mass;
        let fit = fit_tape(
            &model,
            &Wearer {
                years,
                gender,
                stature_mm: want[0],
                weight_kg: Some(mass),
            },
            &targets(&want),
            &pose,
        )
        .unwrap();
        let dm = fit.mass_kg / mass - 1.0;
        println!(
            "  weight         target {mass:7.2}  body {:7.2}  ({:+.2} %)",
            fit.mass_kg,
            dm * 100.0
        );
        if dm.abs() > 0.01 {
            fails.push(format!("{years} y weight: {:+.2} %", dm * 100.0));
        }
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
        weight_kg: None,
    };
    // the reference wearer's fused values (Ottobre chart priors at 92 cm), in TAPE_FIELDS order
    let want = [
        920.0, 540.0, 530.0, 580.0, 530.0, 372.5, 288.8, 210.6, 204.5, 146.9, 156.8, 109.8, 320.0,
        231.0, 225.0, 251.2, 483.8, 80.0, 225.7, 396.6, 540.0,
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
    // every measurement within 3 mm, the definition-limited lengths within 2σ of the fit (the chart's
    // outseam and shoulder length are not both reachable with its crotch height); the priors keep the
    // shape natural
    for r in &fit.residuals {
        let tol = if fit_sigma(&r.name) > 3.0 {
            2.0 * fit_sigma(&r.name)
        } else {
            3.0
        };
        assert!((r.body_mm - r.target_mm).abs() < tol, "{r:?}");
    }
}

/// Fitting from little (plan §9.4): stature and weight only. Synthetic children and an adult of known
/// stature and mass are reached within 2 mm and 1 %, and a heavier wearer of the same stature gets a
/// heavier body (larger girths), not just a heavier number.
#[test]
#[ignore = "needs the converted Anny model"]
fn stature_and_weight_alone() {
    let model = model();
    let pose = Pose::drape();
    let stature_only = |stature: f64| {
        vec![TapeTarget {
            name: "stature".to_string(),
            value_mm: stature,
        }]
    };
    let mut waists = Vec::new();
    for (years, gender, stature, kg) in [
        (2.5, 0.5, 920.0, 12.0),
        (2.5, 0.5, 920.0, 15.5),
        (6.0, 0.0, 1160.0, 21.0),
        (35.0, 1.0, 1650.0, 62.0),
    ] {
        let fit = fit_tape(
            &model,
            &Wearer {
                years,
                gender,
                stature_mm: stature,
                weight_kg: Some(kg),
            },
            &stature_only(stature),
            &pose,
        )
        .unwrap();
        println!(
            "{years} y {stature} mm {kg} kg: body {:.1} mm {:.2} kg, waist {:.1} mm, free {:?}",
            fit.readings[0],
            fit.mass_kg,
            fit.readings[2],
            &fit.free[..4]
        );
        assert!((fit.readings[0] - stature).abs() < 2.0, "{years} y stature");
        assert!((fit.mass_kg / kg - 1.0).abs() < 0.01, "{years} y weight");
        if years == 2.5 {
            waists.push(fit.readings[2]);
        }
    }
    assert!(
        waists[1] > waists[0] + 20.0,
        "a heavier toddler has a larger waist: {waists:?}"
    );
}

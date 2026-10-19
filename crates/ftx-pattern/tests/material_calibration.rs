//! Synthetic constitutive identification, independent force oracle and evidence/refusal regressions.

use std::collections::BTreeMap;

use ftx_pattern::material_calibration::{Input, Partition, SCHEMA, Sample, calibrate};
use ftx_pattern::materials::{Evidence, Form, Material, Quantity, Source};

fn sample(id: &str, f: [[f64; 2]; 2], partition: Partition, q: [f64; 4]) -> Sample {
    // Independent constitutive oracle: S = Q E followed by matrix multiplication P = F S.
    let e11 = (f[0][0] * f[0][0] + f[1][0] * f[1][0] - 1.0) / 2.0;
    let e22 = (f[0][1] * f[0][1] + f[1][1] * f[1][1] - 1.0) / 2.0;
    let gamma = f[0][0] * f[0][1] + f[1][0] * f[1][1];
    let s = [
        [q[0] * e11 + q[2] * e22, q[3] * gamma],
        [q[3] * gamma, q[2] * e11 + q[1] * e22],
    ];
    let nominal_n_m = std::array::from_fn(|i| {
        std::array::from_fn(|j| Some(f[i][0] * s[0][j] + f[i][1] * s[1][j]))
    });
    Sample {
        id: id.into(),
        deformation: f,
        nominal_n_m,
        scale_n_m: 1.0,
        source: "synthetic".into(),
        evidence: Evidence::Estimate,
        partition,
    }
}

fn input() -> Input {
    let properties = [
        ("areal_density", 0.2, "kg/m2"),
        ("bending_rigidity", 3e-5, "N*m"),
        ("thickness", 0.0007, "m"),
        ("friction_skin", 0.3, "1"),
        ("friction_self", 0.5, "1"),
    ]
    .into_iter()
    .map(|(key, value, unit)| {
        (
            key.into(),
            Quantity {
                value,
                unit: unit.into(),
                evidence: Evidence::Estimate,
                source: "synthetic".into(),
                method: "Synthetic test parameter, not a physical measurement".into(),
                conditions: BTreeMap::new(),
                bounds: None,
            },
        )
    })
    .collect();
    let q = [9000.0, 4000.0, 600.0, 700.0];
    Input {
        schema: SCHEMA.into(),
        material: Material {
            id: "synthetic-sheet".into(),
            name: "Synthetic law, not a commercial textile".into(),
            form: Form::Woven,
            composition: vec![],
            composition_complete: false,
            structure: BTreeMap::new(),
            properties,
            membrane_response: None,
            membrane_relaxation: None,
            thermal_response: None,
            optical_faces: None,
            sources: BTreeMap::from([(
                "synthetic".into(),
                Source {
                    reference: "test:known-answer-orthotropic".into(),
                    accessed_on: "2026-10-14".into(),
                    note: "Synthetic ground truth, not supplier or laboratory measurements".into(),
                },
            )]),
        },
        method: "Synthetic matrix oracle".into(),
        conditions: BTreeMap::new(),
        fitted_on: "2026-10-14".into(),
        allow_estimated_observations: true,
        replace_existing: false,
        max_rms_scaled: 1e-8,
        max_peak_scaled: 1e-8,
        samples: vec![
            sample("warp", [[1.08, 0.0], [0.0, 0.99]], Partition::Fit, q),
            sample("weft", [[0.99, 0.0], [0.0, 1.12]], Partition::Fit, q),
            sample("shear", [[1.0, 0.07], [0.0, 1.0]], Partition::Fit, q),
            sample(
                "biaxial-held-out",
                [[1.03, 0.02], [0.01, 1.06]],
                Partition::Check,
                q,
            ),
        ],
    }
}

fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-9, "{a} != {b}");
}

#[test]
fn recovers_the_solver_law_in_finite_strain_not_engineering_strain() {
    let fit = calibrate(&input()).unwrap();
    near(fit.stiffness_n_m[0][0], 9000.0);
    near(fit.stiffness_n_m[1][1], 4000.0);
    near(fit.stiffness_n_m[0][1], 600.0);
    near(fit.stiffness_n_m[2][2], 700.0);
    let m = fit.material.mechanics(true).unwrap();
    near(m.warp_n_m, 8910.0);
    near(m.weft_n_m, 3960.0);
    near(m.poisson, 0.15);
    near(m.shear_n_m, 700.0);
    near(m.areal_kg_m2, 0.2);
    assert_eq!(fit.check.unwrap().components, 4);
    assert!(fit.missing_mechanics.is_empty());
    assert!(fit.material.mechanics(false).is_err());
}

#[test]
fn frame_rotation_does_not_change_material_parameters() {
    let mut rotated = input();
    for s in &mut rotated.samples {
        s.deformation = [s.deformation[1].map(|v| -v), s.deformation[0]];
        s.nominal_n_m = [s.nominal_n_m[1].map(|v| v.map(|x| -x)), s.nominal_n_m[0]];
    }
    let a = calibrate(&input()).unwrap();
    let b = calibrate(&rotated).unwrap();
    for (x, y) in a
        .stiffness_n_m
        .iter()
        .flatten()
        .zip(b.stiffness_n_m.iter().flatten())
    {
        near(*x, *y);
    }
}

#[test]
fn no_measurement_defaults_and_no_implicit_estimate_permission() {
    let mut raw = input();
    raw.allow_estimated_observations = false;
    assert!(calibrate(&raw).is_err());
    raw = input();
    raw.material.properties.clear();
    let fit = calibrate(&raw).unwrap();
    assert_eq!(
        fit.missing_mechanics,
        [
            "areal_density",
            "bending_rigidity",
            "thickness",
            "friction_skin",
            "friction_self"
        ]
    );
    assert!(fit.material.mechanics(true).is_err());
    raw = input();
    for s in &mut raw.samples {
        s.nominal_n_m[0][1] = None;
        s.nominal_n_m[1][0] = None;
    }
    // The P11 shear contribution is still observed, so the model remains identifiable.
    let fit = calibrate(&raw).unwrap();
    assert_eq!(fit.fit.components, 6);
    assert!(fit.residuals.iter().all(|r| r.residual_n_m[0][1].is_none()));
}

#[test]
fn held_out_failures_refuse_and_do_not_leak_into_identification() {
    let baseline = calibrate(&input()).unwrap();
    let mut raw = input();
    raw.samples[3].nominal_n_m[0][0] = Some(9000.0);
    assert!(
        calibrate(&raw)
            .unwrap_err()
            .to_string()
            .contains("held-out")
    );
    raw.max_rms_scaled = 1e5;
    raw.max_peak_scaled = 1e5;
    let fit = calibrate(&raw).unwrap();
    assert_eq!(fit.stiffness_n_m, baseline.stiffness_n_m);
    raw.samples.pop();
    assert!(calibrate(&raw).unwrap().check.is_none());
}

#[test]
fn rejects_unidentifiable_and_nonphysical_laws() {
    let mut raw = input();
    raw.samples.truncate(1);
    assert!(calibrate(&raw).is_err());
    let q = [1000.0, 1000.0, 2000.0, 100.0];
    raw = input();
    for s in &mut raw.samples {
        *s = sample(&s.id, s.deformation, s.partition, q);
    }
    assert!(
        calibrate(&raw)
            .unwrap_err()
            .to_string()
            .contains("positive definite")
    );
    raw = input();
    // Two differently scaled but proportional strain states and one shear load: Q11/Q22/Q12
    // cannot all be identified if only a single normal-strain ratio is observed.
    raw.samples = vec![sample(
        "one",
        [[1.1, 0.0], [0.0, 1.1]],
        Partition::Fit,
        [9000.0, 4000.0, 600.0, 700.0],
    )];
    assert!(calibrate(&raw).is_err());
}

#[test]
fn malformed_and_unsourced_observations_fail_before_publication() {
    let mutations: Vec<fn(&mut Input)> = vec![
        |i| i.samples[0].id.clear(),
        |i| i.samples[1].id = i.samples[0].id.clone(),
        |i| i.samples[0].source = "missing".into(),
        |i| i.samples[0].scale_n_m = 0.0,
        |i| i.samples[0].scale_n_m = f64::NAN,
        |i| i.samples[0].nominal_n_m[0][0] = Some(f64::INFINITY),
        |i| i.samples[0].nominal_n_m = [[None; 2]; 2],
        |i| i.samples[0].deformation = [[1.0, 0.0], [0.0, -1.0]],
        |i| i.samples[0].deformation = [[1.0, 0.0], [0.0, 0.0]],
        |i| i.samples[0].deformation[0][0] = f64::NAN,
        |i| i.max_peak_scaled = 0.0,
        |i| i.material.form = Form::Yarn,
    ];
    for change in mutations {
        let mut raw = input();
        change(&mut raw);
        assert!(calibrate(&raw).is_err());
    }
}

#[test]
fn provenance_is_replayable_and_replacement_requires_explicit_consent() {
    let raw = input();
    let fit = calibrate(&raw).unwrap();
    let text = serde_json::to_string(&fit).unwrap();
    let replay =
        calibrate(&serde_json::from_value(serde_json::to_value(&raw).unwrap()).unwrap()).unwrap();
    assert_eq!(text, serde_json::to_string(&replay).unwrap());
    let coefficient = fit.material.properties["warp_stiffness"].clone();
    assert_eq!(coefficient.evidence, Evidence::Estimate);
    assert!(
        fit.material.sources[&coefficient.source]
            .reference
            .ends_with(&fit.input_sha256)
    );
    let mut raw = input();
    raw.material = fit.material;
    assert!(calibrate(&raw).is_err());
    raw.replace_existing = true;
    let replaced = calibrate(&raw).unwrap();
    assert_eq!(
        replaced.input.material.properties["warp_stiffness"].value,
        coefficient.value
    );
}

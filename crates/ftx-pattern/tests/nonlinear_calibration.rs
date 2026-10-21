//! Independent synthetic forces identify the production nonlinear law, not measured fabric data.
use ftx_pattern::{
    material_calibration::{
        Partition, Sample, fitted_domain,
        nonlinear::{Input, calibrate},
    },
    material_catalog::Selection,
    materials::{Catalog, Evidence},
};
use serde_json::json;

fn stress(e: [f64; 3]) -> [f64; 3] {
    let axial = |x: f64, compression: f64, low: f64, high: f64| {
        if x < 0.0 {
            compression * x
        } else if x <= 0.12 {
            low * x
        } else {
            low * 0.12 + high * (x - 0.12)
        }
    };
    let shear = e[2].abs();
    [
        axial(e[0], 100., 200., 800.) + 10. * e[1],
        axial(e[1], 80., 150., 600.) + 10. * e[0],
        e[2].signum()
            * if shear <= 0.4 {
                30. * shear
            } else {
                12. + 90. * (shear - 0.4)
            },
    ]
}

fn sample(id: usize, f: [[f64; 2]; 2], partition: Partition) -> Sample {
    let [[a, b], [c, d]] = f;
    let e = [
        0.5 * (a * a + c * c - 1.),
        0.5 * (b * b + d * d - 1.),
        a * b + c * d,
    ];
    let [x, y, s] = stress(e);
    Sample {
        id: format!("synthetic-{id}"),
        deformation: f,
        nominal_n_m: [
            [Some(a * x + b * s), Some(a * s + b * y)],
            [Some(c * x + d * s), Some(c * s + d * y)],
        ],
        scale_n_m: 1.,
        source: "synthetic".into(),
        evidence: Evidence::Estimate,
        partition,
    }
}

fn input() -> Input {
    let mut input: Input = serde_json::from_value(json!({
        "schema":"ftx-nonlinear-swatch-calibration/1",
        "material":{"id":"synthetic-nonlinear","name":"Synthetic software test","form":"knit",
            "structure":{"specimen_id":"synthetic-nonlinear"},
            "sources":{"synthetic":{"reference":"test:independent-analytic-forces","accessed_on":"2026-10-18","note":"Synthetic numerical acquisition, not a measured textile."}}},
        "method":"Independent synthetic nominal forces from an explicit piecewise-linear Green-strain law.",
        "conditions":{"specimen_id":"synthetic-nonlinear","conditioning":"synthetic dry reference"},
        "fitted_on":"2026-10-18","samples":[],"interior_knots":[[0.,0.12],[0.,0.12],[-0.4,0.,0.4]],
        "allow_estimated_observations":true,"max_rms_scaled":1e-8,"max_peak_scaled":1e-7
    })).unwrap();
    for axis in 0..2 {
        for lambda in [0.8, 0.9, 1.1, 1.2] {
            let mut f = [[1., 0.], [0., 1.]];
            f[axis][axis] = lambda;
            input
                .samples
                .push(sample(input.samples.len(), f, Partition::Fit));
        }
    }
    for gamma in [-0.8_f64, -0.2, 0.2, 0.8] {
        input.samples.push(sample(
            input.samples.len(),
            [[1., gamma], [0., (1. - gamma * gamma).sqrt()]],
            Partition::Fit,
        ));
    }
    input.samples.push(sample(
        input.samples.len(),
        [[1.1, 0.1], [0.05, 0.95]],
        Partition::Check,
    ));
    input
}

#[test]
fn nonlinear_forces_recover_compression_toe_stiffening_shear_and_coupling() {
    let source = input();
    let report = calibrate(&source).unwrap();
    assert_eq!(report.parameter_count, 11);
    assert_eq!(report.fit.components, 48);
    assert_eq!(report.check.as_ref().unwrap().components, 4);
    assert!(report.fit.rms_scaled < 1e-10);
    let law = report.material.compiled_membrane(true).unwrap().unwrap();
    assert_eq!(law.strain_range(), report.fitted_strain_range);
    assert_eq!(
        fitted_domain(&report.material).unwrap().unwrap().ranges(),
        report.fitted_strain_range
    );
    for e in [[-0.1, 0.1, 0.3], [0.17, -0.08, -0.6], [0.03, 0.18, 0.7]] {
        for (actual, expected) in law
            .try_evaluate(e)
            .unwrap()
            .stress
            .into_iter()
            .zip(stress(e))
        {
            assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
        }
    }
    assert!(law.try_evaluate([0.5, 0., 0.]).is_err());
    assert!(report.material.compiled_membrane(false).is_err());
    assert!(
        report
            .missing_mechanics
            .contains(&"areal_density".to_owned())
    );
    assert!(!report.material.properties.contains_key("warp_stiffness"));
    assert_eq!(
        serde_json::to_value(source).unwrap(),
        serde_json::to_value(&report.input).unwrap()
    );
    let catalog = Catalog {
        schema: ftx_pattern::materials::CATALOG_SCHEMA.into(),
        materials: vec![],
    };
    let frozen = catalog
        .resolve(&Selection {
            id: report.material.id.clone(),
            record: Some(report.material.clone()),
            allow_estimates: true,
            ..Selection::default()
        })
        .unwrap()
        .frozen_selection();
    let replay: Selection = serde_json::from_str(&serde_json::to_string(&frozen).unwrap()).unwrap();
    assert_eq!(
        catalog
            .resolve(&replay)
            .unwrap()
            .frozen_selection()
            .expected_sha256,
        frozen.expected_sha256
    );
    let mut changed = replay;
    changed
        .record
        .as_mut()
        .unwrap()
        .membrane_response
        .as_mut()
        .unwrap()
        .warp
        .points[0][1] *= 1.01;
    assert!(catalog.resolve(&changed).is_err());
}

#[test]
fn nonlinear_fitted_support_cannot_be_dropped_or_mixed_after_publication() {
    let material = calibrate(&input()).unwrap().material;
    for mutation in 0..7 {
        let mut bad = material.clone();
        let response = bad.membrane_response.as_mut().unwrap();
        match mutation {
            0 => {
                response.warp.conditions.remove("fitted_green_strain_range");
            }
            1 => {
                response
                    .coupling
                    .conditions
                    .remove("fitted_green_strain_range");
            }
            2 => {
                response.shear.conditions.insert(
                    "fitted_green_strain_range".into(),
                    "[[-1,1],[-1,1],[-1,1]]".into(),
                );
            }
            3 => {
                response.weft.source = "synthetic".into();
            }
            4 => {
                response.coupling.conditions.insert(
                    "fitted_green_strain_range".into(),
                    "[[1,-1],[-1,1],[-1,1]]".into(),
                );
            }
            _ => {
                // Source identity independently preserves the fit marker if every bound is lost.
                for curve in [&mut response.warp, &mut response.weft, &mut response.shear] {
                    curve.conditions.remove("fitted_green_strain_range");
                }
                response
                    .coupling
                    .conditions
                    .remove("fitted_green_strain_range");
                if mutation == 6 {
                    bad.sources
                        .get_mut(&response.coupling.source)
                        .unwrap()
                        .reference =
                        "local:ftx-strip-calibration/synthetic-support-regression".into();
                }
            }
        }
        assert!(fitted_domain(&bad).is_err(), "mutation {mutation}");
        assert!(bad.validate().is_err(), "mutation {mutation}");
        assert!(bad.mechanics(true).is_err(), "mutation {mutation}");
    }
    let mut authored = material;
    let response = authored.membrane_response.as_mut().unwrap();
    authored
        .sources
        .get_mut(&response.coupling.source)
        .unwrap()
        .reference = "test:independently-authored-nonlinear-law".into();
    for curve in [&mut response.warp, &mut response.weft, &mut response.shear] {
        curve.conditions.remove("fitted_green_strain_range");
    }
    response
        .coupling
        .conditions
        .remove("fitted_green_strain_range");
    assert!(fitted_domain(&authored).unwrap().is_none());
    authored.validate().unwrap();
}

#[test]
fn missing_force_channels_remain_unobserved_and_rotation_preserves_identification() {
    let mut source = input();
    // Axial tests still measure lateral reaction; shear tests only measure P12. Other sensors
    // are genuinely absent. Removing the lateral channels as well makes coupling unidentifiable.
    for s in &mut source.samples[..8] {
        s.nominal_n_m[0][1] = None;
        s.nominal_n_m[1][0] = None;
    }
    for s in &mut source.samples[8..12] {
        s.nominal_n_m = [[None, s.nominal_n_m[0][1]], [None, None]];
    }
    let report = calibrate(&source).unwrap();
    assert_eq!(report.fit.components, 20);
    assert_eq!(report.residuals[0].residual_n_m[0][1], None);
    assert!(
        (report
            .material
            .membrane_response
            .as_ref()
            .unwrap()
            .coupling
            .value
            - 10.)
            .abs()
            < 1e-10
    );
    let mut rotated = input();
    // Exact 90-degree spatial rotation: the material axes and Green strains do not change.
    for s in &mut rotated.samples {
        let [[a, b], [c, d]] = s.deformation;
        s.deformation = [[-c, -d], [a, b]];
        let [[p, q], [r, t]] = s.nominal_n_m;
        s.nominal_n_m = [[r.map(|v| -v), t.map(|v| -v)], [p, q]];
    }
    let other = calibrate(&rotated).unwrap();
    let a = report
        .material
        .compiled_membrane(true)
        .unwrap()
        .unwrap()
        .try_evaluate([0.15, 0.1, 0.6])
        .unwrap();
    let b = other
        .material
        .compiled_membrane(true)
        .unwrap()
        .unwrap()
        .try_evaluate([0.15, 0.1, 0.6])
        .unwrap();
    for (a, b) in a.stress.into_iter().zip(b.stress) {
        assert!((a - b).abs() < 1e-9);
    }
}

#[test]
fn held_out_results_never_tune_the_fit_or_extend_its_domain() {
    let source = input();
    let original = calibrate(&source).unwrap();
    let mut changed = source.clone();
    changed.max_peak_scaled = 100.;
    changed.max_rms_scaled = 100.;
    changed.samples.last_mut().unwrap().nominal_n_m[0][0] = Some(45.);
    let report = calibrate(&changed).unwrap();
    assert_eq!(
        report
            .material
            .membrane_response
            .as_ref()
            .unwrap()
            .warp
            .points,
        original
            .material
            .membrane_response
            .as_ref()
            .unwrap()
            .warp
            .points
    );
    assert!(report.check.unwrap().rms_scaled > 1.);
    changed.max_peak_scaled = 1e-7;
    changed.max_rms_scaled = 1e-8;
    assert!(
        calibrate(&changed)
            .unwrap_err()
            .message
            .contains("held-out")
    );
    changed = source;
    *changed.samples.last_mut().unwrap() = sample(100, [[1.4, 0.], [0., 1.]], Partition::Check);
    assert!(
        calibrate(&changed)
            .unwrap_err()
            .message
            .contains("FIT strain domain")
    );
}

#[test]
fn unidentifiable_or_unsourced_branches_refuse_without_material_defaults() {
    let mut source = input();
    source.allow_estimated_observations = false;
    assert!(calibrate(&source).is_err());
    source = input();
    source.samples.retain(|s| {
        s.partition == Partition::Fit && s.deformation[0][0] >= 1. && s.deformation[1][1] >= 1.
    });
    assert!(calibrate(&source).is_err());
    source = input();
    source.interior_knots[0] = vec![0., 0.05, 0.06, 0.07, 0.08, 0.09, 0.1, 0.12];
    assert!(calibrate(&source).is_err());
    source = input();
    source
        .conditions
        .insert("specimen_id".into(), "different specimen".into());
    assert!(calibrate(&source).is_err());
    source = input();
    source.interior_knots[0] = vec![0.12, 0.];
    assert!(calibrate(&source).is_err());
    source = input();
    for sample in &mut source.samples {
        for row in &mut sample.nominal_n_m {
            for value in row.iter_mut().flatten() {
                *value = -*value;
            }
        }
    }
    assert!(calibrate(&source).is_err());
}

#[test]
fn explicit_replacement_retains_old_evidence_and_does_not_retain_old_linear_response() {
    let mut source = input();
    source.material.properties.insert("warp_stiffness".into(),serde_json::from_value(json!({"value":123.,"unit":"N/m","evidence":"estimate","source":"synthetic","method":"Old synthetic observation"})).unwrap());
    assert!(calibrate(&source).is_err());
    source.replace_existing = true;
    let report = calibrate(&source).unwrap();
    assert_eq!(
        report.input.material.properties["warp_stiffness"].value,
        123.
    );
    assert!(report.material.properties.is_empty());
}

#[test]
fn explicit_replacement_cannot_discard_an_existing_area_response() {
    let mut source = input();
    let example: serde_json::Value =
        serde_json::from_str(include_str!("../../../examples/area-response/enabled.json")).unwrap();
    let selection: Selection = serde_json::from_value(example["material"].clone()).unwrap();
    let material = selection.record.unwrap();
    source.material.membrane_response = material.membrane_response;
    source.material.sources.extend(material.sources);
    source.replace_existing = true;
    source.material.validate().unwrap();
    assert!(
        source
            .material
            .compiled_membrane(true)
            .unwrap()
            .unwrap()
            .area_ratio_range()
            .is_some()
    );
    let refusal = calibrate(&source).unwrap_err();
    assert_eq!(refusal.code, ftx_core::RefusalCode::InvalidDocument);
    assert!(
        refusal
            .message
            .contains("cannot identify or discard an existing area response")
    );
}

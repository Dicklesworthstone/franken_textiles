//! Independent analytic swatches identify the production joint law; no measured-fabric claim.
use ftx_core::RefusalCode;
use ftx_pattern::{
    material_calibration::{
        Partition, Sample, fitted_domain,
        nonlinear::{Input, calibrate},
    },
    material_response::{FITTED_AREA_RANGE_KEY, physical_area_ratio},
    materials::Evidence,
};

fn input() -> Input {
    serde_json::from_str(include_str!(
        "../../../examples/area-identification/swatch.json"
    ))
    .unwrap()
}

fn strain(sample: &Sample) -> [f64; 3] {
    let [[a, b], [c, d]] = sample.deformation;
    [
        0.5 * (a * a + c * c - 1.0),
        0.5 * (b * b + d * d - 1.0),
        a * b + c * d,
    ]
}

// Independent closed-form derivative of W=k1(J-1)^2+k2(ln J)^2. This test oracle
// deliberately does not call the production constitutive evaluator or its fitting basis.
fn analytic_forces(sample: &mut Sample, k1: f64, k2: f64) {
    let [x, y, z] = strain(sample);
    let j = ((1.0 + 2.0 * x) * (1.0 + 2.0 * y) - z * z).sqrt();
    let factor = 2.0 * (k1 * (j - 1.0) + k2 * j.ln() / j) / j;
    let s11 = if x < 0.0 { 200.0 } else { 350.0 } * x + 20.0 * y + factor * (1.0 + 2.0 * y);
    let s22 = if y < 0.0 { 150.0 } else { 250.0 } * y + 20.0 * x + factor * (1.0 + 2.0 * x);
    let s12 = if z < 0.0 { 70.0 } else { 90.0 } * z - factor * z;
    let [[a, b], [c, d]] = sample.deformation;
    sample.nominal_n_m = [
        [Some(a * s11 + b * s12), Some(a * s12 + b * s22)],
        [Some(c * s11 + d * s12), Some(c * s12 + d * s22)],
    ];
}

#[test]
fn independent_nominal_forces_recover_nine_joint_parameters_and_exact_fit_support() {
    let input = input();
    let report = calibrate(&input).unwrap();
    assert_eq!(report.parameter_count, 9);
    assert_eq!(report.fit.components, 24);
    assert_eq!(report.check.as_ref().unwrap().components, 4);
    assert!(report.excitation_pivot > 1e-3);
    assert!(report.fit.rms_scaled < 1e-9);
    assert!(report.check.as_ref().unwrap().rms_scaled < 1e-9);
    let response = report.material.membrane_response.as_ref().unwrap();
    let area = response.area_response.as_ref().unwrap();
    assert!((response.coupling.value - 20.0).abs() < 1e-8);
    assert!((area.quadratic.value - 40.0).abs() < 1e-8);
    assert!((area.log_squared.value - 15.0).abs() < 1e-8);
    for (curve, slopes) in [
        (&response.warp, [200.0, 350.0]),
        (&response.weft, [150.0, 250.0]),
        (&response.shear, [70.0, 90.0]),
    ] {
        for &[e, stress] in &curve.points {
            let slope = if e < 0.0 { slopes[0] } else { slopes[1] };
            assert!((stress - slope * e).abs() < 1e-8);
        }
        assert_eq!(curve.source, area.quadratic.source);
        assert_eq!(curve.evidence, Evidence::Estimate);
    }
    for quantity in [&response.coupling, &area.quadratic, &area.log_squared] {
        assert_eq!(quantity.source, area.quadratic.source);
        assert_eq!(quantity.evidence, Evidence::Estimate);
        assert_eq!(quantity.unit, "N/m");
        assert!(quantity.bounds.is_none());
    }
    let expected = input
        .samples
        .iter()
        .filter(|sample| sample.partition == Partition::Fit)
        .fold([1.0_f64, 1.0_f64], |[lo, hi], sample| {
            let j = physical_area_ratio(strain(sample)).unwrap();
            [lo.min(j), hi.max(j)]
        });
    assert_eq!(report.fitted_area_ratio_range, Some(expected));
    assert_eq!(area.area_ratio_range, expected);
    assert_eq!(
        fitted_domain(&report.material).unwrap().unwrap().ranges(),
        report.fitted_strain_range
    );
    let law = report.material.compiled_membrane(true).unwrap().unwrap();
    assert!(law.try_evaluate([-0.15, -0.10, 0.20]).is_err());
    assert!(report.material.compiled_membrane(false).is_err());
    assert!(report.missing_mechanics.contains(&"areal_density".into()));
    assert!(!report.material.properties.contains_key("bending_warp"));
    assert_eq!(
        serde_json::to_value(&input).unwrap(),
        serde_json::to_value(&report.input).unwrap()
    );
}

#[test]
fn held_out_observations_do_not_change_coefficients_or_expand_area_support() {
    let mut input = input();
    let baseline = calibrate(&input).unwrap();
    input.samples.last_mut().unwrap().nominal_n_m[0][0] = Some(1000.0);
    assert!(calibrate(&input).unwrap_err().message.contains("held-out"));
    input.max_rms_scaled = 2000.0;
    input.max_peak_scaled = 2000.0;
    let perturbed = calibrate(&input).unwrap();
    let before = baseline.material.membrane_response.as_ref().unwrap();
    let after = perturbed.material.membrane_response.as_ref().unwrap();
    assert_eq!(before.warp.points, after.warp.points);
    assert_eq!(before.weft.points, after.weft.points);
    assert_eq!(before.shear.points, after.shear.points);
    assert_eq!(before.coupling.value, after.coupling.value);
    let before = before.area_response.as_ref().unwrap();
    let after = after.area_response.as_ref().unwrap();
    assert_eq!(before.quadratic.value, after.quadratic.value);
    assert_eq!(before.log_squared.value, after.log_squared.value);
    assert_eq!(
        baseline.fitted_area_ratio_range,
        perturbed.fitted_area_ratio_range
    );
    // Each coordinate remains inside its FIT envelope, but this combination has lower J.
    let last = input.samples.last_mut().unwrap();
    let a = 0.72_f64.sqrt();
    let b = 0.19 / a;
    last.deformation = [[a, b], [0.0, (0.81 - b * b).sqrt()]];
    assert_eq!(calibrate(&input).unwrap_err().code, RefusalCode::OutOfRange);
}

#[test]
fn area_identification_refuses_unqualified_states_and_unphysical_solutions() {
    let source = input();
    for mutation in 0..7 {
        let mut bad = source.clone();
        match mutation {
            0 => {
                bad.conditions.remove("specimen");
            }
            1 => {
                bad.conditions
                    .insert("specimen".into(), "different-specimen".into());
            }
            2 => {
                bad.area_response.as_mut().unwrap().model = "unrecognized/1".into();
            }
            3 => {
                bad.area_response.as_mut().unwrap().area_ratio_range = [0.9, 1.1];
            }
            4 => {
                bad.allow_estimated_observations = false;
            }
            5 => {
                bad.samples[0].source = "missing-source".into();
            }
            _ => {
                for sample in &mut bad.samples {
                    analytic_forces(sample, -40.0, 15.0);
                }
            }
        }
        assert!(calibrate(&bad).is_err(), "mutation {mutation}");
    }
    let mut unexcited = source;
    for sample in &mut unexcited.samples {
        // No transverse force observations: the six acquisition rows cannot identify nine
        // independent unknowns, even though a least-squares pseudoinverse has a solution.
        sample.nominal_n_m[0][1] = None;
        sample.nominal_n_m[1] = [None, None];
    }
    assert!(calibrate(&unexcited).is_err());
}

#[test]
fn replacement_retains_prior_evidence_and_cannot_widen_inherited_support() {
    let mut input = input();
    input.conditions.insert("temperature_c".into(), "20".into());
    let first = calibrate(&input).unwrap();
    input.material = first.material.clone();
    assert!(calibrate(&input).is_err());
    input.replace_existing = true;
    let replacement = calibrate(&input).unwrap();
    assert_eq!(
        replacement.fitted_area_ratio_range,
        first.fitted_area_ratio_range
    );
    assert!(
        first
            .material
            .sources
            .keys()
            .all(|id| replacement.material.sources.contains_key(id))
    );
    assert_eq!(
        serde_json::to_value(&replacement.input.material).unwrap(),
        serde_json::to_value(&first.material).unwrap()
    );
    input
        .conditions
        .insert("conditioning".into(), "different-state".into());
    assert!(calibrate(&input).is_err());
    input.conditions = first.input.conditions.clone();
    input.conditions.remove("temperature_c");
    assert!(calibrate(&input).is_err());
    input.conditions = first.input.conditions;
    let response = input.material.membrane_response.as_mut().unwrap();
    // An independently narrowed, consistently source-bearing prior remains authoritative.
    let narrower = [0.8, 1.4];
    let text = serde_json::to_string(&narrower).unwrap();
    for curve in [&mut response.warp, &mut response.weft, &mut response.shear] {
        curve
            .conditions
            .insert(FITTED_AREA_RANGE_KEY.into(), text.clone());
    }
    response
        .coupling
        .conditions
        .insert(FITTED_AREA_RANGE_KEY.into(), text.clone());
    let area = response.area_response.as_mut().unwrap();
    area.area_ratio_range = narrower;
    for conditions in [
        &mut area.conditions,
        &mut area.quadratic.conditions,
        &mut area.log_squared.conditions,
    ] {
        conditions.insert(FITTED_AREA_RANGE_KEY.into(), text.clone());
    }
    input.material.validate().unwrap();
    assert_eq!(calibrate(&input).unwrap_err().code, RefusalCode::OutOfRange);
}

#[test]
fn identified_area_group_cannot_be_dropped_mixed_or_widened() {
    let material = calibrate(&input()).unwrap().material;
    for mutation in 0..8 {
        let mut bad = material.clone();
        let response = bad.membrane_response.as_mut().unwrap();
        match mutation {
            0 => {
                response.area_response = None;
            }
            1 => {
                response.warp.conditions.remove(FITTED_AREA_RANGE_KEY);
            }
            2 => {
                response.area_response.as_mut().unwrap().quadratic.source = "synthetic".into();
            }
            3 => {
                response.area_response.as_mut().unwrap().area_ratio_range = [0.7, 1.5];
            }
            4 => {
                response
                    .area_response
                    .as_mut()
                    .unwrap()
                    .log_squared
                    .conditions
                    .remove(FITTED_AREA_RANGE_KEY);
            }
            5 => {
                response
                    .coupling
                    .conditions
                    .remove("fitted_green_strain_range");
            }
            _ => {
                for curve in [&mut response.warp, &mut response.weft, &mut response.shear] {
                    curve.conditions.remove(FITTED_AREA_RANGE_KEY);
                }
                response.coupling.conditions.remove(FITTED_AREA_RANGE_KEY);
                if mutation == 6 {
                    response.area_response = None;
                } else {
                    let area = response.area_response.as_mut().unwrap();
                    for conditions in [
                        &mut area.conditions,
                        &mut area.quadratic.conditions,
                        &mut area.log_squared.conditions,
                    ] {
                        conditions.remove(FITTED_AREA_RANGE_KEY);
                    }
                }
            }
        }
        assert!(fitted_domain(&bad).is_err(), "mutation {mutation}");
        assert!(bad.validate().is_err(), "mutation {mutation}");
        assert!(bad.compiled_membrane(true).is_err(), "mutation {mutation}");
    }
}

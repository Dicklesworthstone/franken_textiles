//! Synthetic known answers for moment/curvature identification; not textile measurements.

use std::collections::BTreeMap;

use ftx_pattern::material_bending::NORMAL_MODEL;
use ftx_pattern::material_bending_calibration::{Input, SCHEMA, Sample, calibrate};
use ftx_pattern::material_calibration::Partition;
use ftx_pattern::materials::{Evidence, Form, Material, Quantity, Source};

fn material() -> Material {
    Material {
        id: "synthetic-bending-sheet".into(),
        name: "Synthetic bending sheet, not a real textile".into(),
        form: Form::Woven,
        composition: vec![],
        composition_complete: false,
        structure: BTreeMap::new(),
        properties: BTreeMap::new(),
        membrane_response: None,
        membrane_relaxation: None,
        thermal_response: None,
        sources: BTreeMap::from([(
            "fixture".into(),
            Source {
                reference: "test:synthetic-bending-oracle".into(),
                accessed_on: "2026-10-15".into(),
                note: "Independent synthetic moment/curvature oracle; not measured fabric data".into(),
            },
        )]),
    }
}

fn moment(k: [f64; 3], d: [f64; 4]) -> [f64; 3] {
    [
        d[0] * k[0] + d[2] * k[1],
        d[2] * k[0] + d[1] * k[1],
        d[3] * k[2],
    ]
}

fn sample(id: &str, k: [f64; 3], d: [f64; 4], partition: Partition) -> Sample {
    Sample {
        id: id.into(),
        curvature_m_inv: k,
        moment_n: moment(k, d).map(Some),
        scale_n: 1e-4,
        source: "fixture".into(),
        evidence: Evidence::Measured,
        partition,
    }
}

fn input() -> Input {
    let d = [4.0e-4, 1.5e-4, 5.0e-5, 8.0e-5];
    Input {
        schema: SCHEMA.into(),
        material: material(),
        method: "Synthetic direct bending-resultant oracle".into(),
        conditions: BTreeMap::from([("purpose".into(), "software known answer".into())]),
        fitted_on: "2026-10-15".into(),
        samples: vec![
            sample("warp", [8.0, 0.0, 0.0], d, Partition::Fit),
            sample("weft", [0.0, -6.0, 0.0], d, Partition::Fit),
            sample("biaxial", [3.0, 4.0, 0.0], d, Partition::Fit),
            sample("twist", [0.0, 0.0, 9.0], d, Partition::Fit),
            sample("held-out", [-2.0, 5.0, 3.0], d, Partition::Check),
        ],
        allow_estimated_observations: false,
        replace_existing: false,
        max_rms_scaled: 1e-7,
        max_peak_scaled: 1e-7,
    }
}

fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-12, "{a} != {b}");
}

#[test]
fn recovers_the_exact_solver_tensor_and_held_out_coupon() {
    let out = calibrate(&input()).unwrap();
    near(out.bending_nm[0][0], 4.0e-4);
    near(out.bending_nm[1][1], 1.5e-4);
    near(out.bending_nm[0][1], 5.0e-5);
    near(out.bending_nm[2][2], 8.0e-5);
    assert_eq!(out.material.structure["bending_model"], NORMAL_MODEL);
    assert_eq!(out.check.unwrap().components, 3);
    let q = &out.material.properties["bending_warp"];
    assert_eq!(q.evidence, Evidence::Estimate);
    assert_eq!(q.unit, "N*m");
}

#[test]
fn existing_preferred_curvature_is_subtracted_before_fitting() {
    let mut raw = input();
    let preferred = [1.0, -2.0, 0.5];
    for (key, value) in [
        ("preferred_curvature_warp", preferred[0]),
        ("preferred_curvature_weft", preferred[1]),
        ("preferred_curvature_twist", preferred[2]),
    ] {
        raw.material.properties.insert(
            key.into(),
            Quantity {
                value,
                unit: "1/m".into(),
                evidence: Evidence::Measured,
                source: "fixture".into(),
                method: "Synthetic preferred-curvature known answer".into(),
                conditions: BTreeMap::new(),
                bounds: None,
            },
        );
    }
    let d = [4.0e-4, 1.5e-4, 5.0e-5, 8.0e-5];
    for s in &mut raw.samples {
        let strain: [f64; 3] =
            std::array::from_fn(|i| s.curvature_m_inv[i] - preferred[i]);
        s.moment_n = moment(strain, d).map(Some);
    }
    let out = calibrate(&raw).unwrap();
    assert_eq!(out.preferred_curvature_m_inv, preferred);
    near(out.bending_nm[0][0], d[0]);
    near(out.bending_nm[1][1], d[1]);
    near(out.bending_nm[0][1], d[2]);
    near(out.bending_nm[2][2], d[3]);
}

#[test]
fn incomplete_or_unexcited_inputs_refuse_instead_of_guessing() {
    let mut raw = input();
    raw.samples.retain(|s| s.id == "warp");
    assert!(calibrate(&raw).is_err());
    raw = input();
    raw.material.properties.insert(
        "preferred_curvature_warp".into(),
        Quantity {
            value: 1.0,
            unit: "1/m".into(),
            evidence: Evidence::Measured,
            source: "fixture".into(),
            method: "Synthetic partial target".into(),
            conditions: BTreeMap::new(),
            bounds: None,
        },
    );
    assert!(calibrate(&raw).is_err());
}

#[test]
fn nonphysical_tensor_and_bad_held_out_data_refuse() {
    let mut raw = input();
    let unstable = [1.0e-4, 1.0e-4, 2.0e-4, 8.0e-5];
    for s in &mut raw.samples {
        s.moment_n = moment(s.curvature_m_inv, unstable).map(Some);
    }
    assert!(calibrate(&raw).is_err());

    raw = input();
    raw.samples.last_mut().unwrap().moment_n[0] = Some(1.0);
    assert!(calibrate(&raw).is_err());
}

#[test]
fn replacement_and_estimated_observation_require_explicit_consent() {
    let fitted = calibrate(&input()).unwrap();
    let mut raw = input();
    raw.material = fitted.material;
    assert!(calibrate(&raw).is_err());
    raw.replace_existing = true;
    assert!(calibrate(&raw).is_ok());

    raw = input();
    raw.samples[0].evidence = Evidence::Estimate;
    assert!(calibrate(&raw).is_err());
    raw.allow_estimated_observations = true;
    assert!(calibrate(&raw).is_ok());
}

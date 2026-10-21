//! Independent area-energy answers and derivative/domain/source checks using synthetic coefficients.

use std::collections::BTreeMap;

use ftx_pattern::{
    material_response::{AREA_MODEL, AreaResponse, CompiledMembrane},
    materials::{Material, Quantity},
};

fn fixture() -> Material {
    let mut material: Material = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-nonlinear.json"
    ))
    .unwrap();
    let conditions = BTreeMap::from([
        ("specimen".into(), "synthetic-area-coupon".into()),
        ("conditioning".into(), "synthetic reference state".into()),
    ]);
    let response = material.membrane_response.as_mut().unwrap();
    for actual in [
        &mut response.warp.conditions,
        &mut response.weft.conditions,
        &mut response.shear.conditions,
        &mut response.coupling.conditions,
    ] {
        *actual = conditions.clone();
    }
    let quantity = |value| Quantity {
        value,
        unit: "N/m".into(),
        evidence: ftx_pattern::materials::Evidence::Estimate,
        source: "test".into(),
        method: "Synthetic area-energy coefficient; not an empirical textile".into(),
        conditions: conditions.clone(),
        bounds: None,
    };
    response.area_response = Some(AreaResponse {
        model: AREA_MODEL.into(),
        quadratic: quantity(40.0),
        log_squared: quantity(15.0),
        area_ratio_range: [0.6, 1.6],
        conditions,
    });
    material
}

fn compiled(material: &Material) -> CompiledMembrane {
    material.compiled_membrane(true).unwrap().unwrap()
}

fn near(actual: f64, expected: f64, relative: f64) {
    assert!(
        (actual - expected).abs() <= relative * (1.0 + expected.abs()),
        "actual {actual}, expected {expected}"
    );
}

#[test]
fn published_area_energy_and_reference_tangent_use_the_exact_prefactor() {
    let material = fixture();
    let law = compiled(&material);
    let e = [0.2, -0.1, 0.1];
    let area = law.evaluate_area(e).unwrap();
    let j = (1.4_f64 * 0.8 - 0.1 * 0.1).sqrt();
    near(
        area.energy,
        40.0 * (j - 1.0).powi(2) + 15.0 * j.ln().powi(2),
        1e-13,
    );
    let first = 80.0 * (j - 1.0) + 30.0 * j.ln() / j;
    for (actual, derivative) in area.stress.into_iter().zip([0.8 / j, 1.4 / j, -0.1 / j]) {
        near(actual, first * derivative, 1e-13);
    }
    let origin = law.evaluate_area([0.0; 3]).unwrap();
    assert_eq!(origin.energy, 0.0);
    assert_eq!(origin.stress, [0.0; 3]);
    assert_eq!(
        origin.tangent,
        [[110.0, 110.0, 0.0], [110.0, 110.0, 0.0], [0.0; 3]]
    );
    assert_eq!(law.origin_stiffness(), [310.0, 210.0, 120.0, 40.0]);
    let [e1, e2, nu, g] = law.reference_moduli().unwrap();
    let denominator = 1.0 - nu * nu * e2 / e1;
    for (actual, expected) in [e1 / denominator, e2 / denominator, nu * e2 / denominator, g]
        .into_iter()
        .zip(law.origin_stiffness())
    {
        near(actual, expected, 1e-13);
    }
}

#[test]
fn exact_area_and_combined_derivatives_match_independent_differences() {
    let law = compiled(&fixture());
    for area_only in [true, false] {
        // The base cubic has continuous tangent, but a third-derivative jump at its knots.
        // Check its combined derivatives inside intervals; the area term is smooth at zero.
        let states = if area_only {
            [
                [0.0; 3],
                [0.2, -0.1, 0.1],
                [-0.12, 0.13, -0.17],
                [0.2, 0.2, 0.0],
            ]
        } else {
            [
                [0.011, 0.017, 0.009],
                [0.2, -0.11, 0.11],
                [-0.12, 0.13, -0.17],
                [0.21, 0.19, 0.013],
            ]
        };
        for e in states {
            let evaluate = |e| {
                if area_only {
                    law.evaluate_area(e).unwrap()
                } else {
                    law.evaluate(e).unwrap()
                }
            };
            let exact = evaluate(e);
            for i in 0..3 {
                let mut plus = e;
                let mut minus = e;
                plus[i] += 1e-7;
                minus[i] -= 1e-7;
                let a = evaluate(plus);
                let b = evaluate(minus);
                near((a.energy - b.energy) / 2e-7, exact.stress[i], 2e-6);
                for k in 0..3 {
                    near(
                        (a.stress[k] - b.stress[k]) / 2e-7,
                        exact.tangent[k][i],
                        5e-6,
                    );
                    assert_eq!(exact.tangent[k][i], exact.tangent[i][k]);
                }
            }
        }
    }
    // Positive area coefficients do not guarantee a PSD material tangent in Green coordinates.
    assert!(law.evaluate_area([0.2, 0.2, 0.0]).unwrap().tangent[2][2] < 0.0);
}

#[test]
fn isochoric_stretch_and_simple_shear_have_no_area_stress_or_energy() {
    let law = compiled(&fixture());
    // F=diag(2,1/2), and F=[[1,1/2],[0,1]], respectively. Both have J=1.
    for e in [[1.5, -0.375, 0.0], [0.0, 0.125, 0.5]] {
        let area = law.evaluate_area(e).unwrap();
        assert_eq!(area.energy, 0.0);
        assert_eq!(area.stress, [0.0; 3]);
    }
    let e = 1e-14;
    let tiny = law.evaluate_area([e, 0.0, 0.0]).unwrap();
    assert!((tiny.energy / (55.0 * e * e) - 1.0).abs() < 1e-12);
    assert!((tiny.stress[0] / (110.0 * e) - 1.0).abs() < 1e-12);
}

#[test]
fn area_and_strain_domains_intersect_without_clamping_or_orientation_claims() {
    let law = compiled(&fixture());
    assert_eq!(law.area_ratio_range(), Some([0.6, 1.6]));
    assert!(law.check_domain([0.3, 0.3, 0.0]).is_ok());
    for e in [
        [0.300001, 0.3, 0.0],
        [-0.21, -0.21, 0.0],
        [-0.5, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, f64::NAN],
    ] {
        assert!(!law.contains(e));
        assert!(law.check_domain(e).is_err());
        assert!(law.evaluate(e).is_none());
    }
    assert!(law.evaluate_area([-1.0, -1.0, 0.0]).is_none()); // positive determinant, negative C
    // The compiled base's own strain range remains an independent restriction.
    assert!(law.evaluate_area([3.0, -0.43, 0.0]).is_some());
    assert!(law.evaluate([3.0, -0.43, 0.0]).is_none());
}

#[test]
fn area_evidence_units_shared_conditions_and_model_are_required() {
    let base = fixture();
    assert!(base.compiled_membrane(false).is_err());
    for edit in 0..12 {
        let mut material = base.clone();
        let response = material.membrane_response.as_mut().unwrap();
        let area = response.area_response.as_mut().unwrap();
        match edit {
            0 => area.model = "ordinary_neo_hookean".into(),
            1 => area.quadratic.value = -1.0,
            2 => area.log_squared.unit = "Pa".into(),
            3 => area.quadratic.source = "missing-source".into(),
            4 => area.quadratic.method.clear(),
            5 => area.quadratic.conditions.clear(),
            6 => {
                response
                    .shear
                    .conditions
                    .insert("specimen".into(), "different".into());
            }
            7 => {
                area.conditions.remove("conditioning");
            }
            8 => area.area_ratio_range = [0.0, 2.0],
            9 => area.area_ratio_range = [1.01, 2.0],
            10 => {
                area.quadratic.value = 0.0;
                area.log_squared.value = 0.0;
            }
            11 => area.log_squared.bounds = Some([-1.0, 20.0]),
            _ => unreachable!(),
        }
        assert!(material.validate().is_err(), "case {edit}");
    }
    let mut zero = base;
    zero.membrane_response
        .as_mut()
        .unwrap()
        .area_response
        .as_mut()
        .unwrap()
        .quadratic
        .value = 0.0;
    assert!(zero.validate().is_ok());
}

#[test]
fn absent_area_keeps_legacy_serialization_and_separable_arithmetic() {
    let material: Material = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-nonlinear.json"
    ))
    .unwrap();
    let json = serde_json::to_value(&material).unwrap();
    assert!(json["membrane_response"].get("area_response").is_none());
    let law = compiled(&material);
    assert_eq!(law.area_ratio_range(), None);
    for e in [[0.0; 3], [0.2, -0.1, 0.1], [-0.12, 0.13, -0.17]] {
        assert_eq!(law.evaluate(e), law.evaluate_separable(e));
    }
}

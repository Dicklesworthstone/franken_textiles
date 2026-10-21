//! Independent tensile-force/virtual-work checks; no empirical textile qualification.
use ftx_core::RefusalCode;
use ftx_drape::strip::{Geometry, Options, simulate};
use ftx_pattern::{
    material_catalog::{ResolvedMaterial, Selection},
    materials::{CATALOG_SCHEMA, Catalog},
};
use serde_json::{Value, json};

fn source(e1: f64, e2: f64, nu: f64, shear: f64) -> Value {
    let mut properties = serde_json::Map::new();
    for (name, value, unit) in [
        ("warp_stiffness", e1, "N/m"),
        ("weft_stiffness", e2, "N/m"),
        ("poisson_warp_weft", nu, "1"),
        ("shear_stiffness", shear, "N/m"),
    ] {
        properties.insert(
            name.into(),
            json!({"value":value,"unit":unit,"source":"test",
            "evidence":"estimate","method":"Synthetic known answer, not a textile measurement"}),
        );
    }
    json!({"id":"strip-test","allow_estimates":true,"record":{"id":"strip-test","name":"Synthetic membrane only",
        "form":"knit","properties":properties,"sources":{"test":{"reference":"test:strip",
        "accessed_on":"2026-10-20","note":"Invented coefficients; no mass, bending or contact properties"}}}})
}
fn resolved(v: Value) -> ResolvedMaterial {
    let selection: Selection = serde_json::from_value(v).unwrap();
    Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: vec![],
    }
    .resolve(&selection)
    .unwrap()
}
fn geometry() -> Geometry {
    Geometry {
        gauge_m: 0.1,
        width_m: 0.06,
        warp_angle_deg: 0.0,
        length_cells: 8,
        width_cells: 6,
    }
}

#[test]
fn uncoupled_warp_and_weft_match_nominal_force_not_second_piola_stress() {
    let material = resolved(source(200., 80., 0., 30.));
    for (angle, modulus) in [(0., 200.), (90., 80.), (180., 200.), (-90., 80.)] {
        let geometry = Geometry {
            warp_angle_deg: angle,
            ..geometry()
        };
        let report = simulate(
            &material,
            &geometry,
            &[0., 0.1, 0.3, 0.1, 0.],
            &Options::default(),
        )
        .unwrap();
        for sample in &report.samples {
            let stretch = 1. + sample.extension_ratio;
            // Constant homogeneous solution is exact ONLY for aligned, uncoupled normal response.
            let e = (stretch * stretch - 1.) / 2.;
            let force = geometry.width_m * modulus * e * stretch;
            let energy = 0.5 * geometry.gauge_m * geometry.width_m * modulus * e * e;
            assert!((sample.axial_force_n - force).abs() < 1e-10);
            assert!((sample.energy_j - energy).abs() < 1e-11);
            assert!(sample.reaction_balance_n.norm() < 1e-10);
            for (p, rest) in sample.positions_m.iter().zip(&report.reference_positions_m) {
                assert!((p.x - rest.x * stretch).abs() < 1e-11);
                assert!((p.y - rest.y).abs() < 1e-11);
                assert_eq!(p.z, 0.);
            }
        }
        assert!(
            material.mechanics().is_err(),
            "no invented mass/bending/friction should be needed"
        );
    }
}

#[test]
fn full_width_grips_cause_nonuniform_strain_and_allow_interior_contraction() {
    let material = resolved(source(200., 80., 0.4, 30.));
    let g = geometry();
    let r = simulate(&material, &g, &[0.05, 0.1, 0.2], &Options::default()).unwrap();
    let s = r.samples.last().unwrap();
    let stride = g.length_cells + 1;
    for j in 0..=g.width_cells {
        for i in [j * stride, j * stride + g.length_cells] {
            assert_eq!(s.positions_m[i].y, r.reference_positions_m[i].y);
        }
    }
    let mid = g.length_cells / 2;
    assert!(
        s.positions_m[g.width_cells * stride + mid].y - s.positions_m[mid].y < g.width_m - 0.0001
    );
    assert!(s.strain_range[0][1] - s.strain_range[0][0] > 0.001);
    assert!(s.strain_range[1][0] < -0.001);
    assert!(s.free_residual_n <= Options::default().force_tolerance_n);
    let stretch = 1.2;
    let homogeneous_free_force = g.width_m * 200. * 0.5 * (stretch * stretch - 1.) * stretch;
    assert!(
        s.axial_force_n > homogeneous_free_force * 1.001,
        "grip constraint is not a free uniaxial test"
    );
}

#[test]
fn clamp_reaction_is_the_derivative_of_relaxed_energy_and_scales_with_width() {
    let material = resolved(source(200., 110., 0.25, 40.));
    let g = Geometry {
        warp_angle_deg: 35.,
        ..geometry()
    };
    let opts = Options {
        correction_tolerance_m: 1e-12,
        ..Options::default()
    };
    let h = 1e-5;
    let r = simulate(&material, &g, &[0.15 - h, 0.15, 0.15 + h], &opts).unwrap();
    let derivative = (r.samples[2].energy_j - r.samples[0].energy_j) / (2. * h * g.gauge_m);
    assert!((derivative - r.samples[1].axial_force_n).abs() < 1e-6);
    let big = Geometry {
        gauge_m: g.gauge_m * 3.,
        width_m: g.width_m * 3.,
        ..g.clone()
    };
    let scaled = simulate(&material, &big, &[0.15], &opts).unwrap();
    assert!((scaled.samples[0].axial_force_n / r.samples[1].axial_force_n - 3.).abs() < 1e-8);
    assert!((scaled.samples[0].energy_j / r.samples[1].energy_j - 9.).abs() < 1e-8);
}

#[test]
fn bias_rotation_and_mesh_refinement_preserve_the_physical_response() {
    let m = resolved(source(200., 110., 0.2, 40.));
    let g = Geometry {
        warp_angle_deg: 45.,
        ..geometry()
    };
    let a = simulate(&m, &g, &[0.1], &Options::default()).unwrap();
    let b = simulate(
        &m,
        &Geometry {
            warp_angle_deg: -45.,
            ..g.clone()
        },
        &[0.1],
        &Options::default(),
    )
    .unwrap();
    assert!((a.samples[0].axial_force_n - b.samples[0].axial_force_n).abs() < 1e-8);
    let refined = simulate(
        &m,
        &Geometry {
            length_cells: 16,
            width_cells: 12,
            ..g
        },
        &[0.1],
        &Options::default(),
    )
    .unwrap();
    assert!((a.samples[0].axial_force_n / refined.samples[0].axial_force_n - 1.).abs() < 0.02);
    assert_eq!(
        serde_json::to_value(&a).unwrap(),
        serde_json::to_value(
            simulate(
                &m,
                &Geometry {
                    warp_angle_deg: 45.,
                    ..geometry()
                },
                &[0.1],
                &Options::default()
            )
            .unwrap()
        )
        .unwrap()
    );
}

#[test]
fn nonlinear_source_law_reaches_the_same_element_kernel_without_extrapolation() {
    let record: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-nonlinear.json"
    ))
    .unwrap();
    let m = resolved(json!({"id":record["id"],"record":record,"allow_estimates":true}));
    let r = simulate(&m, &geometry(), &[0.02, 0.05, 0.1], &Options::default()).unwrap();
    assert!(r.samples[2].axial_force_n > r.samples[1].axial_force_n);
    let mut bounded = source(200., 80., 0., 30.);
    bounded["record"]["membrane_response"] =
        serde_json::to_value(m.material().membrane_response.as_ref().unwrap()).unwrap();
    bounded["record"]["properties"] = json!({});
    bounded["record"]["membrane_response"]["warp"]["points"] =
        json!([[-0.1, -20.], [0., 0.], [0.01, 2.]]);
    assert!(simulate(&resolved(bounded), &geometry(), &[0.1], &Options::default()).is_err());
}

#[test]
fn source_policy_history_input_and_global_work_fail_without_partial_responses() {
    let m = resolved(source(200., 80., 0.3, 30.));
    for extensions in [&[][..], &[f64::NAN], &[-0.01], &[1.1]] {
        assert!(simulate(&m, &geometry(), extensions, &Options::default()).is_err());
    }
    assert!(
        simulate(
            &m,
            &Geometry {
                length_cells: usize::MAX,
                ..geometry()
            },
            &[0.1],
            &Options::default()
        )
        .is_err()
    );
    let mut v = source(200., 80., 0.3, 30.);
    v["allow_estimates"] = json!(false);
    assert!(simulate(&resolved(v), &geometry(), &[0.1], &Options::default()).is_err());
    let mut v = source(200., 80., 0.3, 30.);
    v["record"]["structure"] = json!({"membrane_plasticity_model":"unsupported-history"});
    assert!(simulate(&resolved(v), &geometry(), &[0.1], &Options::default()).is_err());
    let opts = Options {
        max_newton: 1,
        ..Options::default()
    };
    assert_eq!(
        simulate(&m, &geometry(), &[0.1], &opts).unwrap_err().code,
        RefusalCode::NotConverged
    );
    let uncoupled = resolved(source(200., 80., 0., 30.));
    let opts = Options {
        max_total_newton: 1,
        ..Options::default()
    };
    assert_eq!(
        simulate(&uncoupled, &geometry(), &[0., 0.1], &opts)
            .unwrap_err()
            .code,
        RefusalCode::BudgetExceeded
    );
}

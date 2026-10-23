//! Numerical/material-contract tests. All coefficients are artificial, not measurements of textiles.
use ftx_pattern::material_bending::{BendingTensor, NORMAL_MODEL};
use ftx_pattern::material_catalog::Selection;
use ftx_pattern::material_plasticity::{AXES, MODEL, PROPERTY_FAMILIES, PlasticBending};
use ftx_pattern::materials::Catalog;
use serde_json::json;

fn law(coupling: f64, hardening: [f64; 3]) -> PlasticBending {
    PlasticBending::new(
        BendingTensor::new(2., 3., coupling, 4.)
            .unwrap()
            .normal_projected(),
        [1., 2., 3.],
        hardening,
        [100.; 3],
        [1000.; 3],
    )
    .unwrap()
}
fn near(a: f64, b: f64, tol: f64) {
    assert!(
        (a - b).abs() <= tol * (1. + a.abs() + b.abs()),
        "{a} != {b}"
    );
}

#[test]
fn independent_return_map_hardening_and_reversal_have_closed_form_answers() {
    let m = law(0., [1., 0., 2.]).pristine();
    let k = [2., -2., 3.];
    let r = m.response(k, true).unwrap();
    let u = m.advance(k).unwrap();
    let delta = [1., -4. / 3., 1.5];
    let expected_moments = [2., -2., 6.];
    for i in 0..3 {
        near(u.memory.plastic_curvature()[i], delta[i], 1e-14);
        near(u.memory.accumulated_curvature()[i], delta[i].abs(), 1e-14);
        near(r.moment_n[i], expected_moments[i], 1e-14);
    }
    near(r.tangent_nm[0][0], 2. / 3., 1e-14);
    near(r.tangent_nm[1][1], 0., 1e-14);
    near(r.tangent_nm[2][2], 4. / 3., 1e-14);
    near(u.dissipated_j_m2, 1. + 8. / 3. + 4.5, 1e-14);
    // Incremental potential = new stored energy - OLD hardening + rate-independent dissipation.
    near(
        r.energy,
        u.memory.response(k, false).unwrap().energy + u.dissipated_j_m2,
        1e-14,
    );
    assert_eq!(m.plastic_curvature(), [0.; 3]); // trial calls never write to the old state
    let rest = u.memory.plastic_curvature();
    let unloaded = u.memory.advance(rest).unwrap();
    assert_eq!(unloaded.memory, u.memory);
    assert_eq!(unloaded.dissipated_j_m2, 0.);
    assert_eq!(
        unloaded.memory.response(rest, false).unwrap().moment_n,
        [0.; 3]
    );
    // Reversal adds accumulated history; it does not erase hardening or change the law's sign.
    let reverse = u.memory.advance([-2., 2., -3.]).unwrap();
    for i in 0..3 {
        assert!(reverse.memory.accumulated_curvature()[i] > u.memory.accumulated_curvature()[i]);
    }
    assert!(reverse.dissipated_j_m2 > 0.);
}

#[test]
fn coupled_active_set_and_exact_condensed_derivatives() {
    let m = law(0.7, [0.4, 0.8, 0.2]).pristine();
    // Cover all signed regimes, both single and coupled normal plasticity, and independent twist.
    for a in [-3., -0.1, 0., 0.1, 3.] {
        for b in [-2., -0.1, 0., 0.1, 2.] {
            for c in [-1.8, -0.05, 0., 0.05, 1.8] {
                let k = [a, b, c];
                let r = m.response(k, true).unwrap();
                for j in 0..3 {
                    let eps = 1e-5;
                    let mut plus = k;
                    let mut minus = k;
                    plus[j] += eps;
                    minus[j] -= eps;
                    let p = m.response(plus, true).unwrap();
                    let q = m.response(minus, true).unwrap();
                    near(r.moment_n[j], (p.energy - q.energy) / (2. * eps), 1e-8);
                    for i in 0..3 {
                        near(
                            r.tangent_nm[i][j],
                            (p.moment_n[i] - q.moment_n[i]) / (2. * eps),
                            1e-8,
                        );
                        assert_eq!(r.tangent_nm[i][j], r.tangent_nm[j][i]);
                    }
                }
                let u = m.advance(k).unwrap();
                near(
                    r.energy,
                    u.memory.response(k, false).unwrap().energy + u.dissipated_j_m2,
                    1e-12,
                );
                assert!(u.dissipated_j_m2 >= 0.);
            }
        }
    }
}

#[test]
fn subyield_elasticity_monotone_partition_and_repeated_load_do_not_reset_history() {
    let law = law(-0.5, [0.1, 0.2, 0.3]);
    let initial = law.pristine();
    let e = initial.response([0.1, -0.1, 0.1], true).unwrap();
    assert_eq!(e.tangent_nm, law.tensor().matrix());
    assert_eq!(initial.advance([0.1, -0.1, 0.1]).unwrap().memory, initial);
    // Independent direction: constitutive rate independence under subdivision of a monotonic path.
    let law = law_zero_coupling();
    let target = [3., 0., 0.];
    let direct = law.pristine().advance(target).unwrap();
    let mut split = law.pristine();
    let mut dissipated = 0.;
    for i in 1..=30 {
        let u = split.advance([f64::from(i) * 0.1, 0., 0.]).unwrap();
        dissipated += u.dissipated_j_m2;
        split = u.memory;
    }
    for i in 0..3 {
        near(
            split.plastic_curvature()[i],
            direct.memory.plastic_curvature()[i],
            1e-13,
        );
    }
    near(dissipated, direct.dissipated_j_m2, 1e-13);
    let before = split.clone();
    for _ in 0..100 {
        split = split.advance(target).unwrap().memory;
    }
    for i in 0..3 {
        near(
            split.plastic_curvature()[i],
            before.plastic_curvature()[i],
            1e-13,
        );
    }
}
fn law_zero_coupling() -> PlasticBending {
    law(0., [0.1, 0.2, 0.3])
}

#[test]
fn domain_and_numeric_errors_do_not_clamp_or_modify_memory() {
    let tensor = BendingTensor::new(2., 3., 0.2, 4.)
        .unwrap()
        .normal_projected();
    for y in [[0., 1., 1.], [-1., 1., 1.], [f64::INFINITY, 1., 1.]] {
        assert!(PlasticBending::new(tensor, y, [0.; 3], [5.; 3], [1.; 3]).is_err());
    }
    assert!(PlasticBending::new(tensor, [1.; 3], [-1.; 3], [5.; 3], [1.; 3]).is_err());
    assert!(
        PlasticBending::new(
            BendingTensor::new(2., 3., 0., 4.).unwrap(),
            [1.; 3],
            [0.; 3],
            [5.; 3],
            [1.; 3]
        )
        .is_err()
    );
    let m = PlasticBending::new(tensor, [1.; 3], [0.; 3], [5.; 3], [0.1; 3])
        .unwrap()
        .pristine();
    let original = m.clone();
    for k in [[f64::NAN, 0., 0.], [6., 0., 0.], [1., 0., 0.]] {
        assert!(m.advance(k).is_err());
        assert_eq!(m, original);
    }
    let overflow = PlasticBending::new(
        BendingTensor::new(1e300, 1e300, 0., 1e300)
            .unwrap()
            .normal_projected(),
        [1.; 3],
        [1.; 3],
        [1e300; 3],
        [1e300; 3],
    )
    .unwrap()
    .pristine();
    assert!(overflow.response([1e200; 3], true).is_err());
}

fn selection() -> Selection {
    let mut s: Selection = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-panel-bending.json"
    ))
    .unwrap();
    let m = s.record.as_mut().unwrap();
    m.structure
        .insert("bending_model".into(), NORMAL_MODEL.into());
    m.structure
        .insert("bending_plasticity_model".into(), MODEL.into());
    for (family, unit) in PROPERTY_FAMILIES {
        for axis in AXES {
            m.properties.insert(format!("{family}_{axis}"), serde_json::from_value(json!({
                "value":1.0,"unit":unit,"evidence":"estimate","source":"test","method":"Synthetic plasticity test"
            })).unwrap());
        }
    }
    for q in m.properties.values_mut() {
        q.conditions
            .insert("specimen_id".into(), "artificial-coupon-A".into());
        q.conditions
            .insert("conditioning".into(), "synthetic constant state".into());
    }
    s
}

#[test]
fn canonical_resolution_keeps_units_source_evidence_and_frozen_identity() {
    let db = Catalog::builtin().unwrap();
    let mut s = selection();
    s.record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("bending_curvature_limit_warp")
        .unwrap()
        .unit = "1/cm".into();
    let r = db.resolve(&s).unwrap();
    let law = r.bending_plasticity().unwrap().unwrap();
    assert_eq!(
        r.authored().properties["bending_curvature_limit_warp"].unit,
        "1/cm"
    );
    assert_eq!(
        r.material().properties["bending_curvature_limit_warp"].value,
        100.
    );
    assert_eq!(
        law,
        db.resolve(&r.frozen_selection())
            .unwrap()
            .bending_plasticity()
            .unwrap()
            .unwrap()
    );
    s.record
        .as_mut()
        .unwrap()
        .sources
        .get_mut("test")
        .unwrap()
        .note
        .push_str(" different specimen revision");
    assert_ne!(
        law.fingerprint(),
        db.resolve(&s)
            .unwrap()
            .bending_plasticity()
            .unwrap()
            .unwrap()
            .fingerprint()
    );
    s.allow_estimates = false;
    assert!(db.resolve(&s).unwrap().bending_plasticity().is_err());
    let mut frozen = r.frozen_selection();
    frozen
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("bending_yield_warp")
        .unwrap()
        .value *= 2.;
    assert!(db.resolve(&frozen).is_err());
}

#[test]
fn missing_or_mixed_characterizations_cannot_activate_a_crease_model() {
    let db = Catalog::builtin().unwrap();
    for (family, _) in PROPERTY_FAMILIES {
        for axis in AXES {
            let mut s = selection();
            s.record
                .as_mut()
                .unwrap()
                .properties
                .remove(&format!("{family}_{axis}"));
            assert!(db.resolve(&s).unwrap().bending_plasticity().is_err());
        }
    }
    for identity in ["specimen_id", "conditioning"] {
        let mut s = selection();
        s.record
            .as_mut()
            .unwrap()
            .properties
            .get_mut("bending_yield_warp")
            .unwrap()
            .conditions
            .insert(identity.into(), "different".into());
        assert!(db.resolve(&s).unwrap().bending_plasticity().is_err());
    }
    let mut s = selection();
    s.record.as_mut().unwrap().structure.insert(
        "bending_model".into(),
        ftx_pattern::material_bending::MODEL.into(),
    );
    assert!(db.resolve(&s).unwrap().bending_plasticity().is_err());
    s.record
        .as_mut()
        .unwrap()
        .structure
        .remove("bending_plasticity_model");
    assert!(
        db.resolve(&s)
            .unwrap()
            .bending_plasticity()
            .unwrap()
            .is_none()
    );
}

fn heat_selection() -> Selection {
    let mut s = selection();
    s.record.as_mut().unwrap().properties.insert("bending_plastic_heat_fraction".into(),
        serde_json::from_value(json!({"value":0.75,"unit":"1","evidence":"estimate","source":"test",
            "method":"Synthetic irreversible-curvature conversion, not measured textile data",
            "conditions":{"specimen_id":"artificial-coupon-A","conditioning":"synthetic constant state",
                "work_basis":"irreversible_bending_plastic_dissipation",
                "temperature_min_c":"0","temperature_max_c":"80"}})).unwrap());
    s
}

#[test]
fn bending_heat_conversion_retains_source_units_and_frozen_law_identity() {
    let catalog = Catalog::builtin().unwrap();
    let mut s = heat_selection();
    let q = s
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("bending_plastic_heat_fraction")
        .unwrap();
    q.value = 75.;
    q.unit = "%".into();
    q.bounds = Some([50., 100.]);
    let material = catalog.resolve(&s).unwrap();
    let heat = material.bending_plastic_heat_fraction().unwrap().unwrap();
    assert_eq!(heat.fraction, 0.75);
    assert_eq!(heat.temperature_range_c, [0., 80.]);
    assert_eq!(
        material.material().properties["bending_plastic_heat_fraction"].bounds,
        Some([0.5, 1.])
    );
    assert_eq!(
        material.authored().properties["bending_plastic_heat_fraction"].unit,
        "%"
    );
    assert_eq!(
        material.authored().properties["bending_plastic_heat_fraction"].value,
        75.
    );
    assert_eq!(
        catalog
            .resolve(&material.frozen_selection())
            .unwrap()
            .bending_plastic_heat_fraction()
            .unwrap(),
        Some(heat)
    );
    for fraction in [0., 1.] {
        let mut s = heat_selection();
        s.record
            .as_mut()
            .unwrap()
            .properties
            .get_mut("bending_plastic_heat_fraction")
            .unwrap()
            .value = fraction;
        let changed = catalog.resolve(&s).unwrap();
        assert_eq!(
            changed
                .bending_plastic_heat_fraction()
                .unwrap()
                .unwrap()
                .fraction,
            fraction
        );
        assert_ne!(
            material
                .bending_plasticity()
                .unwrap()
                .unwrap()
                .fingerprint(),
            changed.bending_plasticity().unwrap().unwrap().fingerprint()
        );
        assert_eq!(
            material.bending_response().unwrap(),
            changed.bending_response().unwrap()
        );
    }
}

#[test]
fn bending_heat_requires_separate_qualified_evidence_without_changing_mechanics() {
    use ftx_pattern::materials::Evidence;
    let catalog = Catalog::builtin().unwrap();
    let missing = catalog.resolve(&selection()).unwrap();
    assert!(missing.bending_plasticity().is_ok());
    assert!(missing.bending_plastic_heat_fraction().is_err());
    for (key, value) in [
        ("specimen_id", "another coupon"),
        ("conditioning", "another state"),
        ("work_basis", "total_mechanical_energy"),
        ("temperature_min_c", "NaN"),
        ("temperature_max_c", "inf"),
        ("temperature_min_c", "-274"),
        ("temperature_min_c", "81"),
        ("temperature_max_c", ""),
    ] {
        let mut s = heat_selection();
        s.record
            .as_mut()
            .unwrap()
            .properties
            .get_mut("bending_plastic_heat_fraction")
            .unwrap()
            .conditions
            .insert(key.into(), value.into());
        let material = catalog.resolve(&s).unwrap();
        assert!(material.bending_plasticity().is_ok());
        assert!(
            material.bending_plastic_heat_fraction().is_err(),
            "{key}={value}"
        );
    }
    for (value, bounds) in [
        (-0.1, None),
        (1.1, None),
        (0.75, Some([-0.1, 1.])),
        (0.75, Some([0., 1.1])),
    ] {
        let mut s = heat_selection();
        let q = s
            .record
            .as_mut()
            .unwrap()
            .properties
            .get_mut("bending_plastic_heat_fraction")
            .unwrap();
        q.value = value;
        q.bounds = bounds;
        let material = catalog.resolve(&s).unwrap();
        assert!(material.bending_plasticity().is_ok());
        assert!(material.bending_plastic_heat_fraction().is_err());
    }
    let mut s = heat_selection();
    // Isolate the conversion consent gate; the fixture remains explicitly artificial.
    for (key, q) in &mut s.record.as_mut().unwrap().properties {
        if key != "bending_plastic_heat_fraction" {
            q.evidence = Evidence::Published;
        }
    }
    s.allow_estimates = false;
    let denied = catalog.resolve(&s).unwrap();
    assert!(denied.bending_plasticity().is_ok());
    assert!(denied.bending_plastic_heat_fraction().is_err());
    s.allow_estimates = true;
    assert!(
        catalog
            .resolve(&s)
            .unwrap()
            .bending_plastic_heat_fraction()
            .is_ok()
    );
    s.record
        .as_mut()
        .unwrap()
        .structure
        .remove("bending_plasticity_model");
    assert!(
        catalog
            .resolve(&s)
            .unwrap()
            .bending_plastic_heat_fraction()
            .unwrap()
            .is_none()
    );
}

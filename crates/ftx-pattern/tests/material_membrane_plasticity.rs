//! Synthetic known answers for bounded raw-Green plasticity; no textile measurements are implied.
use ftx_pattern::{
    material_catalog::Selection,
    material_membrane_plasticity::{AXES, MODEL, PROPERTY_FAMILIES, PlasticMembrane},
    materials::{Catalog, Material},
};
use serde_json::json;

fn diagonal() -> PlasticMembrane {
    PlasticMembrane::new(
        [[1000.0, 0.0, 0.0], [0.0, 500.0, 0.0], [0.0, 0.0, 250.0]],
        [10.0, 8.0, 5.0],
        [100.0, 50.0, 25.0],
        [0.2; 3],
        [0.5; 3],
    )
    .unwrap()
}

fn coupled() -> PlasticMembrane {
    PlasticMembrane::new(
        [[1000.0, 200.0, 0.0], [200.0, 600.0, 0.0], [0.0, 0.0, 250.0]],
        [10.0, 8.0, 5.0],
        [100.0, 50.0, 25.0],
        [0.2; 3],
        [0.5; 3],
    )
    .unwrap()
}

fn near(a: f64, b: f64, tolerance: f64) {
    assert!(
        (a - b).abs() <= tolerance * (1.0 + a.abs() + b.abs()),
        "{a} != {b}"
    );
}

#[test]
fn independent_directional_yield_unloading_and_storage_have_closed_form_answers() {
    let initial = diagonal().pristine();
    let strain = [0.05, -0.04, 0.06];
    let response = initial.response(strain, true).unwrap();
    let update = initial.advance(strain).unwrap();
    let plastic = [2.0 / 55.0, -6.0 / 275.0, 2.0 / 55.0];
    for i in 0..3 {
        near(update.memory.plastic_strain()[i], plastic[i], 1e-14);
        near(
            update.memory.accumulated_strain()[i],
            plastic[i].abs(),
            1e-14,
        );
        near(
            response.stress[i],
            [150.0 / 11.0, -100.0 / 11.0, 65.0 / 11.0][i],
            1e-14,
        );
        near(
            response.tangent[i][i],
            [1000.0 / 11.0, 500.0 / 11.0, 250.0 / 11.0][i],
            1e-14,
        );
    }
    near(update.dissipated_j_m2, 0.72, 1e-14);
    near(
        response.energy,
        update.memory.response(strain, false).unwrap().energy + 0.72,
        1e-14,
    );
    assert_eq!(initial.plastic_strain(), [0.0; 3]);
    let rest = update.memory.plastic_strain();
    let unloaded = update.memory.advance(rest).unwrap();
    assert_eq!(unloaded.memory, update.memory);
    assert_eq!(unloaded.dissipated_j_m2, 0.0);
    assert_eq!(
        unloaded.memory.response(rest, false).unwrap().stress,
        [0.0; 3]
    );
    // Permanent Green strain is not engineering extension: the warp rest stretch is sqrt(1+2z).
    near(
        (1.0 + 2.0 * rest[0]).sqrt(),
        (59.0_f64 / 55.0).sqrt(),
        1e-14,
    );
    assert!(unloaded.memory.response(rest, false).unwrap().energy > 0.0);
}

#[test]
fn coupled_yield_keeps_cross_tangent_and_can_activate_an_initially_onyield_direction() {
    let initial = coupled().pristine();
    let response = initial.response([0.05, 0.03, 0.0], true).unwrap();
    let update = initial.advance([0.05, 0.03, 0.0]).unwrap();
    // Independent inverse of [[1100,200],[200,650]], determinant 675000.
    near(update.memory.plastic_strain()[0], 25900.0 / 675000.0, 1e-14);
    near(update.memory.plastic_strain()[1], 12800.0 / 675000.0, 1e-14);
    near(response.stress[0], 10.0 + 100.0 * 25900.0 / 675000.0, 1e-14);
    near(response.stress[1], 8.0 + 50.0 * 12800.0 / 675000.0, 1e-14);
    near(
        response.tangent[0][0],
        100.0 - 6_500_000.0 / 675000.0,
        1e-14,
    );
    near(response.tangent[1][1], 50.0 - 2_750_000.0 / 675000.0, 1e-14);
    near(response.tangent[0][1], 1_000_000.0 / 675000.0, 1e-14);
    assert_eq!(response.tangent[0][1], response.tangent[1][0]);
    let reverse_weft = initial.advance([0.05, -0.03, 0.0]).unwrap();
    // Initial S22=-8 is exactly at its yield boundary; warp return activates negative weft flow.
    near(
        reverse_weft.memory.plastic_strain()[0],
        22100.0 / 675000.0,
        1e-14,
    );
    near(
        reverse_weft.memory.plastic_strain()[1],
        -6800.0 / 675000.0,
        1e-14,
    );
}

#[test]
fn condensed_energy_derivatives_and_nonzero_history_energy_balance_are_consistent() {
    let memory = coupled()
        .pristine()
        .advance([0.03, 0.02, 0.025])
        .unwrap()
        .memory;
    let original = memory.clone();
    let old_hardening = memory
        .response(memory.plastic_strain(), false)
        .unwrap()
        .energy;
    for warp in [-0.071, 0.004, 0.067] {
        for weft in [-0.043, 0.006, 0.039] {
            for shear in [-0.047, 0.003, 0.041] {
                let strain = [warp, weft, shear];
                let response = memory.response(strain, true).unwrap();
                for j in 0..3 {
                    let epsilon = 1e-7;
                    let mut plus = strain;
                    let mut minus = strain;
                    plus[j] += epsilon;
                    minus[j] -= epsilon;
                    let p = memory.response(plus, true).unwrap();
                    let m = memory.response(minus, true).unwrap();
                    near(
                        response.stress[j],
                        (p.energy - m.energy) / (2.0 * epsilon),
                        1e-7,
                    );
                    for i in 0..3 {
                        near(
                            response.tangent[i][j],
                            (p.stress[i] - m.stress[i]) / (2.0 * epsilon),
                            1e-7,
                        );
                        assert_eq!(response.tangent[i][j], response.tangent[j][i]);
                    }
                }
                let next = memory.advance(strain).unwrap();
                near(
                    response.energy + old_hardening,
                    next.memory.response(strain, false).unwrap().energy + next.dissipated_j_m2,
                    1e-12,
                );
                assert!(next.dissipated_j_m2 >= 0.0);
            }
        }
    }
    assert_eq!(memory, original);
}

#[test]
fn monotone_partition_repeated_load_and_reversal_preserve_irreversible_history() {
    let initial = diagonal().pristine();
    let direct = initial.advance([0.05, 0.0, 0.0]).unwrap();
    let mut split = initial.clone();
    let mut dissipation = 0.0;
    for i in 1..=20 {
        let next = split.advance([f64::from(i) * 0.0025, 0.0, 0.0]).unwrap();
        dissipation += next.dissipated_j_m2;
        split = next.memory;
    }
    near(
        split.plastic_strain()[0],
        direct.memory.plastic_strain()[0],
        1e-13,
    );
    near(dissipation, direct.dissipated_j_m2, 1e-13);
    for _ in 0..20 {
        let next = split.advance([0.05, 0.0, 0.0]).unwrap();
        near(next.dissipated_j_m2, 0.0, 1e-13);
        split = next.memory;
    }
    let reverse = direct.memory.advance([-0.03, 0.0, 0.0]).unwrap();
    near(reverse.memory.plastic_strain()[0], -7.0 / 605.0, 1e-14);
    near(reverse.memory.accumulated_strain()[0], 51.0 / 605.0, 1e-14);
    near(reverse.dissipated_j_m2, 58.0 / 121.0, 1e-14);
    near(
        reverse
            .memory
            .response([-0.03, 0.0, 0.0], false)
            .unwrap()
            .stress[0],
        -2230.0 / 121.0,
        1e-14,
    );
    let elastic = initial.advance([0.005, 0.0, 0.0]).unwrap();
    assert_eq!(elastic.memory, initial);
    assert_eq!(elastic.dissipated_j_m2, 0.0);
}

#[test]
fn physical_metric_history_and_numerical_limits_refuse_without_rewriting_state() {
    let q = diagonal().tensor();
    for bad_yield in [[0.0; 3], [-1.0; 3], [f64::NAN; 3]] {
        assert!(PlasticMembrane::new(q, bad_yield, [0.0; 3], [0.2; 3], [0.3; 3]).is_err());
    }
    assert!(PlasticMembrane::new(q, [1.0; 3], [-1.0; 3], [0.2; 3], [0.3; 3]).is_err());
    assert!(PlasticMembrane::new(q, [1.0; 3], [0.0; 3], [0.0; 3], [0.3; 3]).is_err());
    for bad_q in [
        [[1000.0, 1.0, 0.0], [0.0, 500.0, 0.0], [0.0, 0.0, 250.0]],
        [[1000.0, 800.0, 0.0], [800.0, 500.0, 0.0], [0.0, 0.0, 250.0]],
        [[1000.0, 0.0, 1.0], [0.0, 500.0, 0.0], [1.0, 0.0, 250.0]],
    ] {
        assert!(PlasticMembrane::new(bad_q, [1.0; 3], [0.0; 3], [0.2; 3], [0.3; 3]).is_err());
    }
    let law = PlasticMembrane::new(q, [1.0; 3], [0.0; 3], [2.0; 3], [0.01; 3]).unwrap();
    for strain in [[-0.5, 0.0, 0.0], [0.0, 0.0, 1.0], [f64::NAN, 0.0, 0.0]] {
        assert!(!law.contains(strain));
        assert!(law.pristine().response(strain, true).is_err());
    }
    let memory = law.pristine();
    let original = memory.clone();
    assert!(memory.advance([0.1, 0.0, 0.0]).is_err());
    assert_eq!(memory, original);
    // Physical current C=diag(0.2,0.6), but unconstrained box return would give z11=-0.579.
    // Refuse that impossible stress-free metric instead of clamping it or fabricating rest lengths.
    let metric = PlasticMembrane::new(
        [
            [1000.0, 900.0, 0.0],
            [900.0, 1000.0, 0.0],
            [0.0, 0.0, 250.0],
        ],
        [1.0, 100.0, 1.0],
        [0.0; 3],
        [1.0; 3],
        [1.0; 3],
    )
    .unwrap();
    assert!(metric.contains([-0.4, -0.2, 0.0]));
    assert!(metric.pristine().response([-0.4, -0.2, 0.0], false).is_ok());
    assert!(metric.pristine().advance([-0.4, -0.2, 0.0]).is_err());
}

fn selection() -> Selection {
    let mut selection: Selection = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-panel-bending.json"
    ))
    .unwrap();
    let material = selection.record.as_mut().unwrap();
    material
        .structure
        .insert("membrane_plasticity_model".into(), MODEL.into());
    for ((family, unit), value) in PROPERTY_FAMILIES.into_iter().zip([10.0, 100.0, 0.2, 0.5]) {
        for axis in AXES {
            material.properties.insert(
                format!("{family}_{axis}"),
                serde_json::from_value(json!({
                    "value":value,"unit":unit,"evidence":"estimate","source":"test",
                    "method":"Synthetic raw Green plasticity; not measured textile behavior"
                }))
                .unwrap(),
            );
        }
    }
    for quantity in material.properties.values_mut() {
        quantity
            .conditions
            .insert("specimen_id".into(), "synthetic-green-A".into());
        quantity
            .conditions
            .insert("conditioning".into(), "synthetic fixed state".into());
    }
    selection
}

#[test]
fn source_compilation_preserves_units_and_binds_full_material_identity_without_mechanics_recursion()
{
    let catalog = Catalog::builtin().unwrap();
    let mut selection = selection();
    let material = selection.record.as_mut().unwrap();
    let yield_warp = material.properties.get_mut("membrane_yield_warp").unwrap();
    yield_warp.value = 0.01;
    yield_warp.unit = "kN/m".into();
    let limit = material
        .properties
        .get_mut("membrane_strain_limit_warp")
        .unwrap();
    limit.value = 20.0;
    limit.unit = "%".into();
    let resolved = catalog.resolve(&selection).unwrap();
    let law = resolved.membrane_plasticity().unwrap().unwrap();
    assert_eq!(
        resolved.authored().properties["membrane_yield_warp"].unit,
        "kN/m"
    );
    assert_eq!(
        resolved.material().properties["membrane_yield_warp"].value,
        10.0
    );
    assert_eq!(
        resolved.material().properties["membrane_strain_limit_warp"].value,
        0.2
    );
    assert_eq!(
        law.tensor(),
        resolved
            .material()
            .mechanics(true)
            .unwrap()
            .origin_tangent()
    );
    assert_eq!(
        law,
        catalog
            .resolve(&resolved.frozen_selection())
            .unwrap()
            .membrane_plasticity()
            .unwrap()
            .unwrap()
    );
    let mut changed = selection.clone();
    changed
        .record
        .as_mut()
        .unwrap()
        .sources
        .get_mut("test")
        .unwrap()
        .note
        .push_str(" different source revision");
    assert_ne!(
        law.fingerprint(),
        catalog
            .resolve(&changed)
            .unwrap()
            .membrane_plasticity()
            .unwrap()
            .unwrap()
            .fingerprint()
    );
    let mut frozen = resolved.frozen_selection();
    frozen
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("membrane_yield_warp")
        .unwrap()
        .value = 11.0;
    assert!(catalog.resolve(&frozen).is_err());
    selection.allow_estimates = false;
    let denied = catalog.resolve(&selection).unwrap();
    assert!(denied.membrane_plasticity().is_err());
    assert!(denied.material().mechanics(false).is_err());
}

#[test]
fn incomplete_mixed_and_unsupported_history_cannot_silently_fall_back_to_elasticity() {
    let catalog = Catalog::builtin().unwrap();
    for (family, _) in PROPERTY_FAMILIES {
        for axis in AXES {
            let mut candidate = selection();
            candidate
                .record
                .as_mut()
                .unwrap()
                .properties
                .remove(&format!("{family}_{axis}"));
            let resolved = catalog.resolve(&candidate).unwrap();
            assert!(resolved.membrane_plasticity().is_err());
            assert!(resolved.material().mechanics(true).is_err());
        }
    }
    for identity in ["specimen_id", "conditioning"] {
        for key in ["warp_stiffness", "membrane_hardening_shear"] {
            let mut candidate = selection();
            candidate
                .record
                .as_mut()
                .unwrap()
                .properties
                .get_mut(key)
                .unwrap()
                .conditions
                .insert(identity.into(), "another coupon/state".into());
            let resolved = catalog.resolve(&candidate).unwrap();
            assert!(resolved.membrane_plasticity().is_err());
            assert!(resolved.material().mechanics(true).is_err());
        }
    }
    let mut unknown = selection();
    unknown
        .record
        .as_mut()
        .unwrap()
        .structure
        .insert("membrane_plasticity_model".into(), "unknown".into());
    assert!(
        catalog
            .resolve(&unknown)
            .unwrap()
            .material()
            .mechanics(true)
            .is_err()
    );
    let nonlinear: Material = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-nonlinear.json"
    ))
    .unwrap();
    let nonlinear = Selection {
        id: nonlinear.id.clone(),
        record: Some(nonlinear),
        allow_estimates: true,
        ..Selection::default()
    };
    let relaxation: Selection = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-relaxation.json"
    ))
    .unwrap();
    for mut incompatible in [nonlinear, relaxation] {
        incompatible
            .record
            .as_mut()
            .unwrap()
            .structure
            .insert("membrane_plasticity_model".into(), MODEL.into());
        let resolved = catalog.resolve(&incompatible).unwrap();
        assert!(resolved.membrane_plasticity().is_err());
        assert!(resolved.material().mechanics(true).is_err());
    }
    let mut absent = selection();
    absent
        .record
        .as_mut()
        .unwrap()
        .structure
        .remove("membrane_plasticity_model");
    assert!(
        catalog
            .resolve(&absent)
            .unwrap()
            .membrane_plasticity()
            .unwrap()
            .is_none()
    );
}

//! Synthetic constitutive known answers for an explicitly qualified parallel network.
//! These tests establish software behavior, not empirical textile calibration.
use ftx_pattern::{
    material_catalog::{ResolvedMaterial, Selection},
    material_membrane_plasticity::PlasticMembrane,
    material_relaxation::{CompiledRelaxation, Initialization},
    materials::{CATALOG_SCHEMA, Catalog, Evidence},
};

fn selection() -> Selection {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-parallel-membrane.json"
    ))
    .unwrap()
}

fn resolve(selected: &Selection) -> ResolvedMaterial {
    Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: Vec::new(),
    }
    .resolve(selected)
    .unwrap()
}

fn laws() -> (PlasticMembrane, CompiledRelaxation) {
    let material = resolve(&selection());
    (
        material.membrane_plasticity().unwrap().unwrap(),
        material.membrane_relaxation().unwrap().unwrap(),
    )
}

fn near(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance * (1.0 + actual.abs() + expected.abs()),
        "{actual} differs from {expected}"
    );
}

#[test]
fn complete_source_identity_and_both_domains_define_the_joint_lock() {
    let mut selected = selection();
    let material = selected.record.as_mut().unwrap();
    material
        .properties
        .get_mut("membrane_strain_limit_warp")
        .unwrap()
        .value = 0.05;
    material.membrane_relaxation.as_mut().unwrap().strain_range[1] = [-0.02, 0.03];
    let resolved = resolve(&selected);
    let plastic = resolved.membrane_plasticity().unwrap().unwrap();
    let relaxation = resolved.membrane_relaxation().unwrap().unwrap();
    assert!(plastic.accepts_parallel(&relaxation));
    assert_eq!(plastic.parallel_material_sha256(), Some(resolved.sha256()));
    assert_eq!(
        relaxation.parallel_material_sha256(),
        Some(resolved.sha256())
    );
    for strain in [[0.05, 0.03, 0.0], [-0.05, -0.02, 0.0]] {
        assert!(plastic.contains(strain));
        assert!(relaxation.contains(strain));
    }
    for strain in [
        [0.051, 0.0, 0.0],
        [0.0, 0.031, 0.0],
        [0.0, -0.021, 0.0],
        [0.0, 0.0, 0.301],
    ] {
        assert!(!plastic.contains(strain));
        assert!(!relaxation.contains(strain));
    }
    let mut changed = selected.clone();
    changed
        .record
        .as_mut()
        .unwrap()
        .name
        .push_str("; revised source label");
    let other = resolve(&changed);
    let other_relaxation = other.membrane_relaxation().unwrap().unwrap();
    assert_eq!(
        relaxation.origin_tangent(),
        other_relaxation.origin_tangent()
    );
    assert!(!plastic.accepts_parallel(&other_relaxation));
    assert_ne!(relaxation.fingerprint(), other_relaxation.fingerprint());
    assert_ne!(
        plastic.fingerprint(),
        other.membrane_plasticity().unwrap().unwrap().fingerprint()
    );
    let replay = resolve(&resolved.frozen_selection());
    assert_eq!(plastic, replay.membrane_plasticity().unwrap().unwrap());
    assert_eq!(relaxation, replay.membrane_relaxation().unwrap().unwrap());
}

#[test]
fn joint_admission_requires_explicit_network_and_every_matching_observation() {
    let check = |selected: Selection| {
        let material = resolve(&selected);
        assert!(material.membrane_plasticity().is_err());
        assert!(material.membrane_relaxation().is_err());
        assert!(material.mechanics().is_err());
    };
    let mut missing = selection();
    missing
        .record
        .as_mut()
        .unwrap()
        .membrane_relaxation
        .as_mut()
        .unwrap()
        .parallel_plasticity = None;
    check(missing);
    let mut missing = selection();
    missing
        .record
        .as_mut()
        .unwrap()
        .structure
        .remove("membrane_plasticity_model");
    check(missing);
    let mut mismatch = selection();
    mismatch
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("membrane_yield_weft")
        .unwrap()
        .conditions
        .insert("conditioning".into(), "another moisture state".into());
    check(mismatch);
    let mut missing = selection();
    missing
        .record
        .as_mut()
        .unwrap()
        .membrane_relaxation
        .as_mut()
        .unwrap()
        .branches[0]
        .relaxation_time
        .conditions
        .remove("specimen_id");
    check(missing);
    let mut mismatch = selection();
    mismatch
        .record
        .as_mut()
        .unwrap()
        .membrane_relaxation
        .as_mut()
        .unwrap()
        .parallel_plasticity
        .as_mut()
        .unwrap()
        .conditions
        .insert("temperature_c".into(), "20".into());
    check(mismatch);

    let selected = selection();
    let material = selected.record.as_ref().unwrap();
    let resolved = resolve(&selected);
    assert!(
        material
            .membrane_relaxation
            .as_ref()
            .unwrap()
            .compile(
                &material.sources,
                true,
                resolved.mechanics().unwrap().origin_tangent()
            )
            .is_err()
    );
}

#[test]
fn network_estimate_consent_is_independent_of_coefficient_consent() {
    let mut selected = selection();
    selected.allow_estimates = false;
    let material = selected.record.as_mut().unwrap();
    for quantity in material.properties.values_mut() {
        quantity.evidence = Evidence::Published;
    }
    for branch in &mut material.membrane_relaxation.as_mut().unwrap().branches {
        branch.stiffness_ratio.evidence = Evidence::Published;
        branch.relaxation_time.evidence = Evidence::Published;
    }
    assert!(resolve(&selected).mechanics().is_err());
    selected
        .record
        .as_mut()
        .unwrap()
        .membrane_relaxation
        .as_mut()
        .unwrap()
        .parallel_plasticity
        .as_mut()
        .unwrap()
        .evidence = Evidence::Published;
    assert!(resolve(&selected).mechanics().is_ok());
}

#[test]
fn relaxed_maxwell_initialization_does_not_anneal_the_plastic_branch() {
    let mut selected = selection();
    selected
        .record
        .as_mut()
        .unwrap()
        .membrane_relaxation
        .as_mut()
        .unwrap()
        .initialization = Initialization::RelaxedStart;
    let material = resolve(&selected);
    let relaxation = material
        .membrane_relaxation()
        .unwrap()
        .unwrap()
        .initialize([0.1, 0.0, 0.0])
        .unwrap();
    let plastic = material.membrane_plasticity().unwrap().unwrap().pristine();
    assert_eq!(
        relaxation.response([0.1, 0.0, 0.0], None).unwrap().stress,
        [0.0; 3]
    );
    assert_eq!(plastic.plastic_strain(), [0.0; 3]);
    near(
        plastic.response([0.1, 0.0, 0.0], false).unwrap().stress[0],
        100.0,
        1e-14,
    );
    near(
        plastic
            .advance([0.1, 0.0, 0.0])
            .unwrap()
            .memory
            .plastic_strain()[0],
        80.0 / 1100.0,
        1e-14,
    );
}

#[test]
fn plastic_flow_uses_equilibrium_stress_while_total_strain_drives_maxwell_memory() {
    let (plastic_law, relaxation_law) = laws();
    let mut plastic = plastic_law.pristine();
    let mut relaxation = relaxation_law.initialize([0.0; 3]).unwrap();
    let (mut permanent, mut accumulated, mut viscous): (f64, f64, f64) = (0.0, 0.0, 0.0);
    let (mut plastic_work, mut viscous_work) = (0.0, 0.0);
    let (mut actual_plastic_work, mut actual_viscous_work) = (0.0, 0.0);
    let (h, tau, stiffness, branch, yield_stress, hardening) =
        (0.01, 0.05, 1000.0, 2000.0, 20.0, 100.0);
    let rest = 80.0 / 1100.0;
    for (i, strain) in std::iter::repeat_n(0.1, 8)
        .chain(std::iter::repeat_n(rest, 80))
        .enumerate()
    {
        let trial = stiffness * (strain - permanent);
        let delta = trial.signum()
            * (trial.abs() - yield_stress - hardening * accumulated).max(0.0)
            / (stiffness + hardening);
        permanent += delta;
        accumulated += delta.abs();
        plastic_work += yield_stress * delta.abs();
        let a = tau / (tau + h);
        let gap = strain - viscous;
        viscous_work += a * (1.0 - a) * branch * gap * gap;
        viscous = a * viscous + (1.0 - a) * strain;
        let e = [strain, 0.0, 0.0];
        let plastic_response = plastic.response(e, true).unwrap();
        let viscous_response = relaxation.response(e, Some(h)).unwrap();
        near(
            plastic_response.stress[0],
            stiffness * (strain - permanent),
            1e-12,
        );
        near(
            viscous_response.stress[0],
            branch * (strain - viscous),
            1e-12,
        );
        let next_plastic = plastic.advance(e).unwrap();
        let next_relaxation = relaxation.advance(e, h).unwrap();
        assert!(next_plastic.dissipated_j_m2 >= 0.0);
        assert!(next_relaxation.dissipated_j_m2 >= 0.0);
        actual_plastic_work += next_plastic.dissipated_j_m2;
        actual_viscous_work += next_relaxation.dissipated_j_m2;
        plastic = next_plastic.memory;
        relaxation = next_relaxation.memory;
        near(plastic.plastic_strain()[0], permanent, 1e-12);
        near(plastic.accumulated_strain()[0], accumulated, 1e-12);
        near(relaxation.viscous_strains()[0][0], viscous, 1e-12);
        near(
            plastic.response(e, false).unwrap().energy
                + relaxation.response(e, None).unwrap().energy,
            0.5 * stiffness * (strain - permanent).powi(2)
                + 0.5 * hardening * accumulated.powi(2)
                + 0.5 * branch * (strain - viscous).powi(2),
            1e-12,
        );
        if i == 0 {
            near(permanent, rest, 1e-14);
            assert!(viscous_response.stress[0] > plastic_response.stress[0]);
        }
    }
    near(permanent, rest, 1e-12);
    near(plastic_work, yield_stress * rest, 1e-12);
    near(actual_plastic_work, plastic_work, 1e-12);
    near(actual_viscous_work, viscous_work, 1e-12);
    assert!(viscous_work > 0.0);
    assert!(relaxation.response([rest, 0.0, 0.0], None).unwrap().stress[0].abs() < 0.0001);
}

#[test]
fn combined_incremental_potential_has_the_sum_of_consistent_branch_derivatives() {
    let (plastic_law, relaxation_law) = laws();
    let plastic = plastic_law.pristine();
    let relaxation = relaxation_law.initialize([0.0; 3]).unwrap();
    let e = [0.1, 0.02, 0.08];
    let h = 0.03;
    let eps = 1e-6;
    let p = plastic.response(e, true).unwrap();
    let r = relaxation.response(e, Some(h)).unwrap();
    for i in 0..3 {
        let mut plus = e;
        let mut minus = e;
        plus[i] += eps;
        minus[i] -= eps;
        let pp = plastic.response(plus, true).unwrap();
        let pm = plastic.response(minus, true).unwrap();
        let rp = relaxation.response(plus, Some(h)).unwrap();
        let rm = relaxation.response(minus, Some(h)).unwrap();
        near(
            (pp.energy + rp.energy - pm.energy - rm.energy) / (2.0 * eps),
            p.stress[i] + r.stress[i],
            1e-8,
        );
        for j in 0..3 {
            near(
                (pp.stress[j] + rp.stress[j] - pm.stress[j] - rm.stress[j]) / (2.0 * eps),
                p.tangent[j][i] + r.tangent[j][i],
                1e-8,
            );
        }
    }
    assert_eq!(plastic.plastic_strain(), [0.0; 3]);
    assert_eq!(relaxation.viscous_strains(), &[[0.0; 3]]);
}

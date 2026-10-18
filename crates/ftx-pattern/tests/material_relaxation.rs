//! Synthetic standard-linear-solid and generalized-Maxwell answers; no empirical textile claims.

use ftx_pattern::material_relaxation::{Initialization, OrthotropicBranch, Relaxation};
use ftx_pattern::materials::Source;
use serde_json::json;
use std::collections::BTreeMap;

fn source() -> BTreeMap<String, Source> {
    serde_json::from_value(json!({"test": {"reference":"test:maxwell-known-answers",
        "accessed_on":"2026-10-14","note":"Synthetic model parameters, not textile measurements"}}))
    .unwrap()
}

fn law() -> Relaxation {
    serde_json::from_value(
        json!({"model":"green_maxwell/1", "initialization":"stress_free_reference",
        "strain_range":[[-0.4,1.0],[-0.4,1.0],[-1.0,1.0]],"source":"test",
        "method":"Synthetic stress relaxation law; equilibrium stiffness is the reference tangent",
        "branches":[{"id":"slow", "stiffness_ratio":{"value":2.,"unit":"1","source":"test",
            "evidence":"estimate","method":"synthetic test"},
            "relaxation_time":{"value":1.,"unit":"s","source":"test",
            "evidence":"estimate","method":"synthetic test"}}]}),
    )
    .unwrap()
}

const Q: [[f64; 3]; 3] = [[1000., 50., 0.], [50., 500., 0.], [0., 0., 200.]];
fn near(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() < tol, "{a} != {b}");
}

#[test]
fn held_strain_relaxes_as_the_independent_backward_euler_answer() {
    let compiled = law().compile(&source(), true, Q).unwrap();
    let mut memory = compiled.initialize([0.; 3]).unwrap();
    let e = [0.1, 0., 0.];
    let mut factor = 1.;
    for _ in 0..20 {
        let trial = memory.response(e, Some(0.1)).unwrap();
        factor /= 1.1;
        near(trial.stress[0], 200. * factor, 1e-12);
        let update = memory.advance(e, 0.1).unwrap();
        assert!(update.dissipated_j_m2 >= 0.);
        memory = update.memory;
        near(
            memory.response(e, None).unwrap().stress[0],
            trial.stress[0],
            1e-12,
        );
    }
}

#[test]
fn constant_stress_creeps_then_recovers_with_temporal_convergence() {
    // Independent standard linear solid: E_inf=1000, E_branch=2000, tau_branch=1.
    // epsilon(t)=sigma/E_inf*(1-2/3 exp(-t/3)). Creep retardation time is 3, not 1.
    let solve = |dt: f64, steps: usize| {
        let q = [[1000., 0., 0.], [0., 500., 0.], [0., 0., 200.]];
        let compiled = law().compile(&source(), true, q).unwrap();
        let mut m = compiled.initialize([0.; 3]).unwrap();
        let mut e = 0.;
        for _ in 0..steps {
            let r = m.response([0.; 3], Some(dt)).unwrap();
            e = (100. - r.stress[0]) / (1000. + r.tangent[0][0]);
            m = m.advance([e, 0., 0.], dt).unwrap().memory;
        }
        let loaded = e;
        for _ in 0..steps * 10 {
            let r = m.response([0.; 3], Some(dt)).unwrap();
            e = -r.stress[0] / (1000. + r.tangent[0][0]);
            assert!(e >= 0. && e < loaded);
            m = m.advance([e, 0., 0.], dt).unwrap().memory;
        }
        assert!(e < 1e-5);
        loaded
    };
    let exact = 0.1 * (1. - (2. / 3.) * (-1.0_f64).exp());
    let coarse = solve(0.1, 30);
    let fine = solve(0.01, 300);
    assert!((fine - exact).abs() < (coarse - exact).abs() / 9.);
    assert!(fine > 100. / 3000. && fine < 0.1);
}

#[test]
fn response_gradient_tangent_and_condensed_energy_match_finite_differences() {
    let mut l = law();
    let mut extra = l.branches[0].clone();
    extra.id = "fast".into();
    extra.relaxation_time.value = 0.05;
    extra.stiffness_ratio.value = 0.3;
    l.branches.push(extra);
    let c = l.compile(&source(), true, Q).unwrap();
    let m = c
        .initialize([0.; 3])
        .unwrap()
        .advance([0.02, 0.01, 0.03], 0.07)
        .unwrap()
        .memory;
    let e = [0.1, -0.02, 0.06];
    let r = m.response(e, Some(0.13)).unwrap();
    for j in 0..3 {
        let (mut p, mut n) = (e, e);
        p[j] += 1e-6;
        n[j] -= 1e-6;
        let a = m.response(p, Some(0.13)).unwrap();
        let b = m.response(n, Some(0.13)).unwrap();
        near((a.energy - b.energy) / 2e-6, r.stress[j], 1e-7);
        for i in 0..3 {
            near((a.stress[i] - b.stress[i]) / 2e-6, r.tangent[i][j], 1e-7);
        }
    }
    let up = m.advance(e, 0.13).unwrap();
    // Eliminated incremental potential = endpoint stored energy + one-half viscous step work.
    near(r.energy, up.stored_j_m2 + 0.5 * up.dissipated_j_m2, 1e-12);
}

#[test]
fn initialization_is_explicit_and_trials_or_failed_steps_never_consume_history() {
    let mut l = law();
    l.initialization = Initialization::RelaxedStart;
    let c = l.compile(&source(), true, Q).unwrap();
    let m = c.initialize([0.1, 0., 0.]).unwrap();
    assert_eq!(m.response([0.1, 0., 0.], None).unwrap().stress, [0.; 3]);
    let old = m.clone();
    for _ in 0..5 {
        m.response([0.2, 0., 0.], Some(0.1)).unwrap();
    }
    for h in [0., -1., f64::NAN, f64::INFINITY] {
        assert!(m.advance([0.1, 0., 0.], h).is_err());
    }
    assert!(m.advance([2., 0., 0.], 0.1).is_err());
    assert_eq!(m, old);
    assert!(c.initialize([-0.5, 0., 0.]).is_err());
}

#[test]
fn source_permissions_units_stability_and_parameter_ranges_are_enforced() {
    assert!(law().compile(&source(), false, Q).is_err());
    assert!(law().compile(&BTreeMap::new(), true, Q).is_err());
    let changes: Vec<fn(&mut Relaxation)> = vec![
        |l| l.branches.clear(),
        |l| l.branches.push(l.branches[0].clone()),
        |l| l.branches[0].relaxation_time.unit = "ms".into(),
        |l| l.branches[0].stiffness_ratio.value = -1.,
        |l| l.branches[0].relaxation_time.value = 0.,
        |l| l.branches[0].stiffness_ratio.bounds = Some([-1., 3.]),
        |l| l.strain_range[0][0] = -0.5,
        |l| l.strain_range[2][1] = f64::INFINITY,
        |l| l.branches[0].stiffness_ratio.value = f64::MAX,
    ];
    for edit in changes {
        let mut l = law();
        edit(&mut l);
        assert!(l.compile(&source(), true, Q).is_err());
    }
    for q in [
        [[0.; 3]; 3],
        [[1., 2., 0.], [2., 1., 0.], [0., 0., 1.]],
        [[1., 1., 0.], [0., 1., 0.], [0., 0., 1.]],
    ] {
        assert!(law().compile(&source(), true, q).is_err());
    }
}

#[test]
fn identity_and_extreme_time_ratios_are_deterministic_and_finite() {
    let l = law();
    let c = l.compile(&source(), true, Q).unwrap();
    let replay: Relaxation = serde_json::from_str(&serde_json::to_string(&l).unwrap()).unwrap();
    assert_eq!(c, replay.compile(&source(), true, Q).unwrap());
    let mut changed = l.clone();
    changed.branches[0].relaxation_time.value = 2.;
    assert_ne!(
        c.fingerprint(),
        changed.compile(&source(), true, Q).unwrap().fingerprint()
    );
    let m = c.initialize([0.; 3]).unwrap();
    for h in [f64::MIN_POSITIVE, 1e-10, 1e10, f64::MAX] {
        let u = m.advance([0.1, 0., 0.], h).unwrap();
        assert!(u.stored_j_m2.is_finite() && u.dissipated_j_m2.is_finite());
    }
}

fn orthotropic_law() -> Relaxation {
    let mut l = law();
    l.branches.clear();
    l.conditions = BTreeMap::from([
        ("specimen_id".into(), "synthetic-directional-coupon".into()),
        ("conditioning".into(), "synthetic-dry-reference".into()),
    ]);
    l.orthotropic_branches = [
        ("warp", [2000., 0., 0., 0.], 0.05),
        ("weft", [0., 1000., 0., 0.], 0.2),
        ("bias", [0., 0., 0., 400.], 0.7),
    ]
    .into_iter()
    .map(|(id, [a, b, c, g], tau)| {
        let q = |v, unit| {
            json!({"value":v,"unit":unit,"source":"test",
            "evidence":"estimate","method":"Synthetic directional known answer",
            "conditions":l.conditions})
        };
        serde_json::from_value::<OrthotropicBranch>(json!({"id":id,
            "stiffness":{"warp":q(a,"N/m"),"weft":q(b,"N/m"),
                "coupling":q(c,"N/m"),"shear":q(g,"N/m")},
            "relaxation_time":q(tau,"s")}))
        .unwrap()
    })
    .collect();
    l
}

#[test]
fn independent_directional_spectra_follow_three_closed_form_hold_and_unload_answers() {
    let compiled = orthotropic_law().compile(&source(), true, Q).unwrap();
    assert!(compiled.has_orthotropic_branches());
    let mut memory = compiled.initialize([0.; 3]).unwrap();
    let e = [0.1, -0.05, 0.2];
    let stiffness = [2000., 1000., 400.];
    let tau = [0.05, 0.2, 0.7];
    let mut factors = [1.; 3];
    for _ in 0..20 {
        let potential = memory.response(e, Some(0.01)).unwrap().energy;
        let update = memory.advance(e, 0.01).unwrap();
        near(
            potential,
            update.stored_j_m2 + 0.5 * update.dissipated_j_m2,
            1e-12,
        );
        memory = update.memory;
        let r = memory.response(e, None).unwrap();
        for j in 0..3 {
            factors[j] *= tau[j] / (tau[j] + 0.01);
            near(r.stress[j], stiffness[j] * e[j] * factors[j], 2e-12);
        }
    }
    let initial_unload = memory.response([0.; 3], None).unwrap().stress;
    for j in 0..3 {
        near(
            initial_unload[j],
            -stiffness[j] * e[j] * (1. - factors[j]),
            2e-12,
        );
    }
    let mut unload_factors = [1.; 3];
    let mut previous_energy = memory.response([0.; 3], None).unwrap().energy;
    for _ in 0..20 {
        let update = memory.advance([0.; 3], 0.01).unwrap();
        assert!(update.dissipated_j_m2 >= 0. && update.stored_j_m2 < previous_energy);
        previous_energy = update.stored_j_m2;
        memory = update.memory;
        for j in 0..3 {
            unload_factors[j] *= tau[j] / (tau[j] + 0.01);
            near(
                memory.response([0.; 3], None).unwrap().stress[j],
                initial_unload[j] * unload_factors[j],
                2e-12,
            );
        }
    }
}

#[test]
fn coupled_directional_tensors_have_consistent_derivatives_and_nonnegative_null_mode_storage() {
    let mut l = orthotropic_law();
    l.orthotropic_branches.truncate(1);
    let stiffness = &mut l.orthotropic_branches[0].stiffness;
    stiffness.warp.value = 1000.;
    stiffness.weft.value = 500.;
    stiffness.coupling.value = -200.;
    let m = l
        .compile(&source(), true, Q)
        .unwrap()
        .initialize([0.; 3])
        .unwrap()
        .advance([0.02, 0.01, 0.03], 0.04)
        .unwrap()
        .memory;
    let e = [0.08, -0.03, 0.1];
    let r = m.response(e, Some(0.07)).unwrap();
    for j in 0..3 {
        let (mut plus, mut minus) = (e, e);
        plus[j] += 1e-6;
        minus[j] -= 1e-6;
        let p = m.response(plus, Some(0.07)).unwrap();
        let n = m.response(minus, Some(0.07)).unwrap();
        near((p.energy - n.energy) / 2e-6, r.stress[j], 1e-7);
        for i in 0..3 {
            near((p.stress[i] - n.stress[i]) / 2e-6, r.tangent[i][j], 1e-7);
        }
    }
    // Rank-one coupled normal response and an explicitly inactive shear direction.
    let stiffness = &mut l.orthotropic_branches[0].stiffness;
    stiffness.warp.value = 100.;
    stiffness.weft.value = 100.;
    stiffness.coupling.value = 100.;
    let m = l
        .compile(&source(), true, Q)
        .unwrap()
        .initialize([0.; 3])
        .unwrap();
    let r = m.response([0.2, -0.2, 0.7], Some(0.1)).unwrap();
    assert_eq!(r.energy, 0.);
    assert_eq!(r.stress, [0.; 3]);
    assert_eq!(
        m.advance([0.2, -0.2, 0.7], 0.1).unwrap().dissipated_j_m2,
        0.
    );
}

#[test]
fn directional_laws_refuse_missing_evidence_mixed_conditions_and_unstable_tensors() {
    assert!(orthotropic_law().compile(&source(), false, Q).is_err());
    let changes: Vec<fn(&mut Relaxation)> = vec![
        |l| {
            l.conditions.remove("specimen_id");
        },
        |l| {
            l.orthotropic_branches[0].stiffness.warp.conditions.clear();
        },
        |l| {
            l.orthotropic_branches[0]
                .relaxation_time
                .conditions
                .insert("conditioning".into(), "wet".into());
        },
        |l| l.orthotropic_branches[0].stiffness.warp.unit = "Pa".into(),
        |l| l.orthotropic_branches[0].stiffness.warp.source = "missing".into(),
        |l| l.orthotropic_branches[0].stiffness.warp.value = -1.,
        |l| l.orthotropic_branches[0].stiffness.warp.value = 0.,
        |l| l.orthotropic_branches[0].stiffness.coupling.value = 1.,
        |l| l.orthotropic_branches[0].stiffness.coupling.bounds = Some([1., 2.]),
        |l| l.orthotropic_branches[0].relaxation_time.value = 0.,
        |l| l.orthotropic_branches[1].id = "warp".into(),
    ];
    for edit in changes {
        let mut l = orthotropic_law();
        edit(&mut l);
        assert!(l.compile(&source(), true, Q).is_err());
    }
    let mut l = orthotropic_law();
    l.orthotropic_branches[0].stiffness.weft.value = 1.;
    l.orthotropic_branches[0].stiffness.coupling.value = 50.;
    assert!(l.compile(&source(), true, Q).is_err());
    let mut l = orthotropic_law();
    for i in 0..14 {
        let mut b = l.orthotropic_branches[0].clone();
        b.id = format!("extra-{i}");
        l.orthotropic_branches.push(b);
    }
    assert!(l.compile(&source(), true, Q).is_err());
}

#[test]
fn directional_and_legacy_source_identities_replay_without_implicit_coefficients() {
    let legacy = law();
    assert!(
        !serde_json::to_value(&legacy)
            .unwrap()
            .as_object()
            .unwrap()
            .contains_key("orthotropic_branches")
    );
    assert!(
        !legacy
            .compile(&source(), true, Q)
            .unwrap()
            .has_orthotropic_branches()
    );
    let mut l = orthotropic_law();
    let mut proportional = legacy.branches[0].clone();
    proportional.stiffness_ratio.conditions = l.conditions.clone();
    proportional.relaxation_time.conditions = l.conditions.clone();
    l.branches.push(proportional);
    let c = l.compile(&source(), true, Q).unwrap();
    let replay: Relaxation = serde_json::from_str(&serde_json::to_string(&l).unwrap()).unwrap();
    assert_eq!(c, replay.compile(&source(), true, Q).unwrap());
    l.orthotropic_branches[1].relaxation_time.value *= 2.;
    assert_ne!(
        c.fingerprint(),
        l.compile(&source(), true, Q).unwrap().fingerprint()
    );
    l.orthotropic_branches[1].id.clone_from(&l.branches[0].id);
    assert!(l.compile(&source(), true, Q).is_err());
}

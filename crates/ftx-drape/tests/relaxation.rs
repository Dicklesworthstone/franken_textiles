//! Maxwell history through actual triangles, Newton steps, sewn scenes and body-motion continuation.
//! Every material and body in these tests is synthetic, not empirical textile/wearer evidence.
use ftx_core::{RefusalCode, Vec2, Vec3};
use ftx_drape::material::resolve;
use ftx_drape::motion::{MotionOptions, advance_body};
use ftx_drape::run::{DrapeBody, DrapeOptions, drape};
use ftx_drape::scene::PartFrames;
use ftx_drape::{Body, ClothModel, ContactParams, PartFrame, Solver};
use ftx_pattern::Pattern;
use ftx_pattern::material_catalog::Selection;
use ftx_pattern::material_relaxation::{OrthotropicBranch, OrthotropicStiffness};
use ftx_pattern::materials::Catalog;
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn selection() -> Selection {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-relaxation.json"
    ))
    .unwrap()
}
fn solver(fixed: bool, relaxation: bool) -> Solver {
    let mut select = selection();
    if !relaxation {
        select.record.as_mut().unwrap().membrane_relaxation = None;
    }
    selected_solver(select, fixed)
}
fn selected_solver(select: Selection, fixed: bool) -> Solver {
    let db = Catalog::builtin().unwrap();
    let m = db.resolve(&select).unwrap();
    let p = Pattern::new(json!({"fabric":{"material":select}}));
    let fabric = resolve(&p, &DrapeOptions::default()).unwrap().fabric;
    let rest = vec![Vec2::ZERO, Vec2::new(0.1, 0.), Vec2::new(0., 0.1)];
    let mut model =
        ClothModel::new(rest.clone(), &[[0, 1, 2]], fabric.membrane(1.), 0., 0.2).unwrap();
    model
        .set_membrane_responses(&[m.membrane_response().unwrap()])
        .unwrap();
    let x = vec![Vec3::ZERO, Vec3::new(0.11, 0., 0.), Vec3::new(0., 0.1, 0.)];
    let mut s = Solver::new(
        model,
        x,
        &[[true; 3], [fixed, true, true], [true; 3]],
        Vec3::ZERO,
    )
    .with_relaxation(&[m.membrane_relaxation().unwrap()])
    .unwrap();
    s.tol = 1e-8;
    s.max_newton = 100;
    s
}

fn directional_selection() -> Selection {
    let mut selected = selection();
    let record = selected.record.as_mut().unwrap();
    let conditions = BTreeMap::from([
        ("specimen_id".into(), "synthetic-directional-sheet".into()),
        ("conditioning".into(), "synthetic-reference".into()),
    ]);
    for quantity in record.properties.values_mut() {
        quantity.conditions = conditions.clone();
    }
    let datum = record.properties["warp_stiffness"].clone();
    let quantity = |value, unit: &str| {
        let mut q = datum.clone();
        q.value = value;
        q.unit = unit.into();
        q.method = "Synthetic directional model, not measured textile data".into();
        q
    };
    let law = record.membrane_relaxation.as_mut().unwrap();
    law.branches.clear();
    law.conditions = conditions;
    law.orthotropic_branches = [("warp", 2000., 0., 0.05), ("weft", 0., 1000., 0.2)]
        .into_iter()
        .map(|(id, warp, weft, tau)| OrthotropicBranch {
            id: id.into(),
            stiffness: OrthotropicStiffness {
                warp: quantity(warp, "N/m"),
                weft: quantity(weft, "N/m"),
                coupling: quantity(0., "N/m"),
                shear: quantity(0., "N/m"),
            },
            relaxation_time: quantity(tau, "s"),
        })
        .collect();
    selected
}
fn near(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() < tol, "{a} != {b}");
}

#[test]
fn clamped_reaction_relaxes_at_fixed_geometry_with_exact_step_clock() {
    let mut s = solver(true, true);
    let eq = solver(true, false).elastic_forces();
    let original = s.x.clone();
    let initial = s.elastic_forces();
    let mut factor = 1.;
    for i in 1..=20 {
        let step = s.step(0.01).unwrap();
        assert!(step.converged);
        factor /= 1.2;
        let force = s.elastic_forces();
        for j in 0..3 {
            near(
                force[j].x - eq[j].x,
                (initial[j].x - eq[j].x) * factor,
                1e-10,
            );
        }
        assert_eq!(s.x, original);
        let history = s.relaxation_report().unwrap().unwrap();
        assert_eq!(history["accepted_steps"], i);
        near(
            history["time_seconds"].as_f64().unwrap(),
            f64::from(i) * 0.01,
            1e-14,
        );
        assert!(history["viscous_step_work_j"].as_f64().unwrap() > 0.);
    }
    assert!(s.static_solve().is_err());
    assert!(s.settle(0.01, 0.1, 1e-3, 3, 10).is_err());
}

#[test]
fn rejected_newton_step_rolls_back_geometry_velocity_and_memory_then_can_retry() {
    let mut s = solver(false, true);
    let x = s.x.clone();
    let v = s.v.clone();
    let history = s.relaxation_report().unwrap();
    s.max_newton = 0;
    assert_eq!(s.step(0.005).unwrap_err().code, RefusalCode::NotConverged);
    assert_eq!(s.x, x);
    assert_eq!(s.v, v);
    assert_eq!(s.relaxation_report().unwrap(), history);
    s.max_newton = 100;
    let mut clean = solver(false, true);
    s.step(0.005).unwrap();
    clean.step(0.005).unwrap();
    assert_eq!(s.x, clean.x);
    assert_eq!(s.v, clean.v);
    assert_eq!(
        s.relaxation_report().unwrap(),
        clean.relaxation_report().unwrap()
    );
    let before = s.relaxation_report().unwrap();
    for h in [0., -1., f64::NAN, f64::INFINITY] {
        assert!(s.step(h).is_err());
    }
    assert_eq!(s.relaxation_report().unwrap(), before);
}

#[test]
fn material_only_relaxation_selection_changes_free_dynamics_and_clones_preserve_history() {
    let mut memory = solver(false, true);
    let mut equilibrium = solver(false, false);
    for _ in 0..4 {
        memory.step(0.005).unwrap();
        equilibrium.step(0.005).unwrap();
    }
    assert_ne!(memory.x, equilibrium.x);
    assert_eq!(memory.model.mass, equilibrium.model.mass);
    assert_eq!(memory.model.membrane, equilibrium.model.membrane);
    let mut continuation = memory.clone();
    for _ in 0..5 {
        memory.step(0.005).unwrap();
        continuation.step(0.005).unwrap();
    }
    assert_eq!(memory.x, continuation.x);
    assert_eq!(
        memory.relaxation_report().unwrap(),
        continuation.relaxation_report().unwrap()
    );
    assert_eq!(
        memory.relaxation_report().unwrap().unwrap()["accepted_steps"],
        9
    );
}

#[test]
fn history_cannot_be_rebound_or_reused_after_material_or_rest_frame_edits() {
    let mut s = solver(true, true);
    s.step(0.01).unwrap();
    let db = Catalog::builtin().unwrap();
    let law = db
        .resolve(&selection())
        .unwrap()
        .membrane_relaxation()
        .unwrap();
    assert!(s.clone().with_relaxation(&[law]).is_err());
    let mut changed = s.clone();
    changed.model.membrane.q[0][0] *= 2.;
    assert!(changed.step(0.01).is_err());
    let mut changed = s.clone();
    changed.model.membranes[0].g[0][0] *= 2.;
    assert!(changed.step(0.01).is_err());
    s.x[1].x = 1.;
    assert!(s.step(0.01).is_err());
}

#[test]
fn motion_admission_preserves_accepted_history_and_rolls_back_failed_body_steps() {
    let mut s = solver(true, true)
        .with_contact(
            Body::new(
                vec![
                    Vec3::new(-1., -1., -0.1),
                    Vec3::new(1., -1., -0.1),
                    Vec3::new(0., 1., -0.1),
                ],
                vec![[0, 1, 2]],
                0.1,
            )
            .unwrap(),
            ContactParams {
                mu: 0.,
                ..DrapeOptions::default().contact
            },
        )
        .unwrap();
    s.step(0.01).unwrap();
    let target: Vec<_> = s
        .contact
        .as_ref()
        .unwrap()
        .body
        .vertices
        .iter()
        .map(|p| *p + Vec3::new(0., 0., -0.01))
        .collect();
    advance_body(&mut s, &target, 0.02, MotionOptions::default()).unwrap();
    let history = s.relaxation_report().unwrap().unwrap();
    near(history["time_seconds"].as_f64().unwrap(), 0.03, 1e-14);
    assert!(history["accepted_steps"].as_u64().unwrap() >= 2);
    let body = s.contact.as_ref().unwrap().body.vertices.clone();
    let invalid = vec![Vec3::ZERO];
    assert!(advance_body(&mut s, &invalid, 0.02, MotionOptions::default()).is_err());
    assert_eq!(history, s.relaxation_report().unwrap().unwrap());
    assert_eq!(body, s.contact.as_ref().unwrap().body.vertices);
}

struct Form;
impl PartFrames for Form {
    fn frame(&self, _: &str) -> Option<PartFrame> {
        Some(PartFrame {
            origin: Vec3::ZERO,
            up: Vec3::new(0., 0., 1.),
            front: Vec3::new(0., -1., 0.),
            lat: Vec3::new(1., 0., 0.),
        })
    }
    fn landmark_h(&self, _: &str, _: &str) -> Option<f64> {
        Some(20.)
    }
    fn section_radius(&self, _: &str, _: f64) -> f64 {
        30.
    }
}
fn pattern() -> Pattern {
    let w = 40. * std::f64::consts::PI;
    let pieces:Vec<Value> = ["front","back"].into_iter().enumerate().map(|(i,id)| json!({
        "id":id,"cut":{"qty":1},"vertices":{"a":[0,0],"b":[w,0],"c":[w,20],"d":[0,20]},
        "edges":[{"id":"bottom","from":"a","to":"b"},{"id":"right","from":"b","to":"c"},
            {"id":"top","from":"c","to":"d"},{"id":"left","from":"d","to":"a"}],
        "grain":[[0,0],[0,20]],"placement":{"part":"torso","radius_mm":40,"x0":w/2.,"theta_deg":180*i}
    })).collect();
    Pattern::new(
        json!({"schema":"fabric-pattern/1","units":"mm","fabric":{"material":selection()},
        "pieces":pieces,"seams":[{"id":"join1","a":"front/right","b":"back/left"},
        {"id":"join2","a":"back/right","b":"front/left"}]}),
    )
}

#[test]
fn source_selection_reaches_only_assigned_retained_triangles_in_real_sewn_scene() {
    let mut input: ftx_pattern::material_relaxation::calibration::Input =
        serde_json::from_str(include_str!(
            "../../../tests/fixtures/materials/synthetic-orthotropic-relaxation-history.json"
        ))
        .unwrap();
    let mut selections = vec![selection(), directional_selection()];
    for identify_times in [false, true] {
        if identify_times {
            input.orthotropic = Some(
                serde_json::from_value(json!({
                    "time_bounds_seconds": [[0.1,0.9],[1.,4.],[5.,24.]],
                    "zero_threshold_n_m": 1e-7
                }))
                .unwrap(),
            );
        }
        let fitted = ftx_pattern::material_relaxation::calibration::calibrate(&input).unwrap();
        let calibrated = Catalog::builtin()
            .unwrap()
            .resolve(&Selection {
                id: fitted.material.id.clone(),
                record: Some(fitted.material),
                allow_estimates: true,
                ..Selection::default()
            })
            .unwrap()
            .frozen_selection();
        selections.push(calibrated);
    }
    for selected in selections {
        check_sewn_selection(selected);
    }
}

fn check_sewn_selection(selected: Selection) {
    let parallel = selected
        .record
        .as_ref()
        .unwrap()
        .membrane_relaxation
        .as_ref()
        .is_some_and(|law| law.parallel_plasticity.is_some());
    let mut p = pattern().raw().clone();
    p["fabric"]["material"] = json!(selected);
    let mut equilibrium = selected;
    equilibrium.record.as_mut().unwrap().membrane_relaxation = None;
    equilibrium.expected_sha256 = None;
    p["pieces"][1]["fabric"] = json!({"material":equilibrium});
    let opts = DrapeOptions {
        spacing: 24.,
        sew_steps: 3,
        drape_steps: 3,
        step: 0.001,
        ..DrapeOptions::default()
    };
    let surface = Body::new(
        vec![
            Vec3::new(-1., -1., -1.),
            Vec3::new(1., -1., -1.),
            Vec3::new(0., 1., -1.),
        ],
        vec![[0, 1, 2]],
        0.1,
    )
    .unwrap();
    let body = DrapeBody {
        frames: &Form,
        collision: (&surface.vertices, &surface.triangles),
        display: (&surface.vertices, &surface.triangles),
        arm_deg: 35.,
    };
    let run = drape(&Pattern::new(p.clone()), &json!({}), &body, &opts, |_| {}).unwrap();
    let history = &run.report["membrane_history"];
    assert_eq!(history["accepted_steps"], 6);
    assert!(!history["elements"].as_array().unwrap().is_empty());
    for entry in history["elements"].as_array().unwrap() {
        let triangle = entry["triangle"].as_u64().unwrap() as usize;
        assert_eq!(
            run.scene.mesh.instances[run.scene.triangle_owner[triangle]],
            "front"
        );
    }
    if parallel {
        let plastic = &run.report["membrane_plasticity_history"];
        assert_eq!(plastic["accepted_steps"], 6);
        for entry in plastic["elements"].as_array().unwrap() {
            let triangle = entry["triangle"].as_u64().unwrap() as usize;
            let front = run.scene.mesh.instances[run.scene.triangle_owner[triangle]] == "front";
            assert_eq!(entry.get("parallel_network").is_some(), front);
        }
    }
    p["fabric"]["material"] = json!(equilibrium);
    let eq = drape(&Pattern::new(p), &json!({}), &body, &opts, |_| {}).unwrap();
    assert!(eq.report.get("membrane_history").is_none());
    assert_eq!(eq.scene.mesh.verts2d, run.scene.mesh.verts2d);
    assert_eq!(eq.scene.weld, run.scene.weld);
    assert_eq!(eq.scene.solver.model.mass, run.scene.solver.model.mass);
    assert_ne!(eq.scene.solver.x, run.scene.solver.x);
}

#[test]
fn qualified_parallel_network_follows_real_sewn_piece_ownership() {
    let selected = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-parallel-membrane.json"
    ))
    .unwrap();
    check_sewn_selection(selected);
}

#[test]
fn material_directional_recovery_changes_actual_reactions_and_dynamics_without_changing_geometry() {
    let selected = directional_selection();
    let mut equilibrium = selected.clone();
    equilibrium.record.as_mut().unwrap().membrane_relaxation = None;
    let mut s = selected_solver(selected.clone(), true);
    let mut eq = selected_solver(equilibrium, true);
    s.x[2].y = 0.11;
    eq.x[2].y = 0.11;
    let original = s.x.clone();
    let initial = s.elastic_forces();
    let eq_force = eq.elastic_forces();
    let mut factor = [1.; 2];
    for _ in 0..20 {
        assert!(s.step(0.01).unwrap().converged);
        factor[0] /= 1.2;
        factor[1] /= 1.05;
        let force = s.elastic_forces();
        near(
            force[1].x - eq_force[1].x,
            (initial[1].x - eq_force[1].x) * factor[0],
            2e-11,
        );
        near(
            force[2].y - eq_force[2].y,
            (initial[2].y - eq_force[2].y) * factor[1],
            2e-11,
        );
        assert_eq!(s.x, original);
    }
    let mut changed = selected.clone();
    changed
        .record
        .as_mut()
        .unwrap()
        .membrane_relaxation
        .as_mut()
        .unwrap()
        .orthotropic_branches[0]
        .relaxation_time
        .value = 0.5;
    let db = Catalog::builtin().unwrap();
    let a = db.resolve(&selected).unwrap();
    let b = db.resolve(&changed).unwrap();
    assert_ne!(a.sha256(), b.sha256());
    assert_eq!(
        a.sha256(),
        db.resolve(&a.frozen_selection()).unwrap().sha256()
    );
    let mut fast = selected_solver(selected, false);
    let mut slow = selected_solver(changed, false);
    for _ in 0..4 {
        fast.step(0.005).unwrap();
        slow.step(0.005).unwrap();
    }
    assert_eq!(fast.model.rest, slow.model.rest);
    assert_eq!(fast.model.mass, slow.model.mass);
    assert_eq!(fast.model.membrane, slow.model.membrane);
    assert_ne!(fast.x, slow.x);
    let mut continued = fast.clone();
    fast.step(0.005).unwrap();
    continued.step(0.005).unwrap();
    assert_eq!(fast.x, continued.x);
    assert_eq!(
        fast.relaxation_report().unwrap(),
        continued.relaxation_report().unwrap()
    );
    let accepted = fast.relaxation_report().unwrap();
    let positions = fast.x.clone();
    fast.max_newton = 0;
    assert_eq!(
        fast.step(0.005).unwrap_err().code,
        RefusalCode::NotConverged
    );
    assert_eq!(fast.x, positions);
    assert_eq!(fast.relaxation_report().unwrap(), accepted);
}

#[test]
fn directional_recovery_cannot_mix_equilibrium_specimens_or_nonlinear_conditions() {
    let db = Catalog::builtin().unwrap();
    let mut selected = directional_selection();
    selected
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("warp_stiffness")
        .unwrap()
        .conditions
        .insert("specimen_id".into(), "different-lot".into());
    let material = db.resolve(&selected).unwrap();
    assert!(material.mechanics().is_err());
    assert!(material.membrane_relaxation().is_err());

    let mut nonlinear: ftx_pattern::materials::Material = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-nonlinear.json"
    ))
    .unwrap();
    let directional = directional_selection().record.unwrap();
    let conditions = directional
        .membrane_relaxation
        .as_ref()
        .unwrap()
        .conditions
        .clone();
    nonlinear.membrane_relaxation = directional.membrane_relaxation;
    let response = nonlinear.membrane_response.as_mut().unwrap();
    response.warp.conditions = conditions.clone();
    response.weft.conditions = conditions.clone();
    response.shear.conditions = conditions.clone();
    response.coupling.conditions = conditions;
    selected.id.clone_from(&nonlinear.id);
    selected.record = Some(nonlinear);
    db.resolve(&selected)
        .unwrap()
        .membrane_relaxation()
        .unwrap();
    selected
        .record
        .as_mut()
        .unwrap()
        .membrane_response
        .as_mut()
        .unwrap()
        .weft
        .conditions
        .insert("conditioning".into(), "wet".into());
    assert!(
        db.resolve(&selected)
            .unwrap()
            .membrane_relaxation()
            .is_err()
    );
}

#[test]
fn no_history_preserves_existing_snapshot_locks_and_estimated_history_requires_consent() {
    let db = Catalog::builtin().unwrap();
    let mut selected = selection();
    let a = db.resolve(&selected).unwrap();
    assert_eq!(
        a.sha256(),
        db.resolve(&a.frozen_selection()).unwrap().sha256()
    );
    selected
        .record
        .as_mut()
        .unwrap()
        .membrane_relaxation
        .as_mut()
        .unwrap()
        .branches[0]
        .relaxation_time
        .value *= 2.;
    assert_ne!(a.sha256(), db.resolve(&selected).unwrap().sha256());
    for p in selected.record.as_mut().unwrap().properties.values_mut() {
        p.evidence = ftx_pattern::materials::Evidence::Published;
    }
    selected.allow_estimates = false;
    let denied = db.resolve(&selected).unwrap();
    assert!(
        denied
            .mechanics()
            .unwrap_err()
            .message
            .contains("estimates")
    );
    assert!(denied.membrane_relaxation().is_err());
}

#[test]
fn captured_viscous_work_matches_independent_dashpot_quadrature_not_spring_energy_loss() {
    let mut s = solver(true, true);
    let initial = s.relaxation_report().unwrap().unwrap();
    let stored0 = initial["stored_branch_energy_j"].as_f64().unwrap();
    let e = s.model.membranes[0].strain(&s.x);
    let q = s.model.material(0).q;
    let reference_area = s.model.membranes[0].area;
    let work0 = reference_area
        * (0..3)
            .map(|i| e[i] * (0..3).map(|j| 2. * q[i][j] * e[j]).sum::<f64>())
            .sum::<f64>();
    let (tau, dt) = (0.05_f64, 0.01_f64);
    let a = tau / (tau + dt);
    let mut remaining = 1.;
    let mut expected = 0.;
    for _ in 0..20 {
        remaining *= a;
        expected += (dt / tau) * remaining * remaining * work0;
        s.step(dt).unwrap();
        let captured = s.membrane_relaxation_dissipation_j().unwrap().unwrap();
        assert_eq!(captured.len(), 1);
        near(captured[0], expected, 1e-13);
        let report = s.relaxation_report().unwrap().unwrap();
        assert_eq!(captured[0], report["viscous_step_work_j"].as_f64().unwrap());
        let stored = report["stored_branch_energy_j"].as_f64().unwrap();
        assert!(
            stored0 - stored > captured[0],
            "backward Euler numerical loss must not be converted"
        );
    }
    let before = s.membrane_relaxation_dissipation_j().unwrap();
    let state = format!("{s:?}");
    assert!(s.step(f64::NAN).is_err());
    assert_eq!(format!("{s:?}"), state);
    assert_eq!(s.membrane_relaxation_dissipation_j().unwrap(), before);
    s.model.compression[0] = [0.5; 2];
    assert!(s.membrane_relaxation_dissipation_j().is_err());
}

#[test]
fn relaxation_work_capture_preserves_motion_and_charges_every_retained_scalar() {
    use ftx_drape::motion::{
        DissipationCapture, advance_body_with_dissipation, advance_body_with_work,
    };
    let body = Body::new(
        vec![
            Vec3::new(-1., -1., -0.1),
            Vec3::new(1., -1., -0.1),
            Vec3::new(0., 1., -0.1),
        ],
        vec![[0, 1, 2]],
        0.1,
    )
    .unwrap();
    let p = ContactParams {
        thickness: 0.,
        mu: 0.,
        ..DrapeOptions::default().contact
    };
    let original = solver(true, true).with_contact(body, p).unwrap();
    let target: Vec<_> = original
        .contact
        .as_ref()
        .unwrap()
        .body
        .vertices
        .iter()
        .map(|p| *p + Vec3::new(0., 0., 0.0001))
        .collect();
    let opts = MotionOptions {
        max_step_s: 0.01,
        max_sample_vertices: 21,
        ..Default::default()
    };
    let capture = DissipationCapture {
        plastic: false,
        relaxation: true,
        bending: false,
    };
    let mut captured = original.clone();
    let result = advance_body_with_work(&mut captured, &target, 0.03, opts, capture).unwrap();
    assert_eq!(result.samples.len(), 3);
    let mut previous = 0.;
    for sample in &result.samples {
        let values = sample.membrane_relaxation_dissipation_j.as_ref().unwrap();
        assert_eq!(values.len(), 1);
        assert!(values[0] > previous);
        previous = values[0];
        assert!(sample.membrane_plastic_dissipation_j.is_none());
    }
    let mut ordinary = original.clone();
    let geometry_budget = MotionOptions {
        max_sample_vertices: 18,
        ..opts
    };
    let old = advance_body(&mut ordinary, &target, 0.03, geometry_budget).unwrap();
    assert_eq!(format!("{ordinary:?}"), format!("{captured:?}"));
    for (a, b) in old.samples.iter().zip(&result.samples) {
        assert!(a.membrane_relaxation_dissipation_j.is_none());
        assert_eq!(a.cloth, b.cloth);
        assert_eq!(a.body, b.body);
        assert_eq!(a.solve, b.solve);
    }
    let mut limited = original.clone();
    assert_eq!(
        advance_body_with_work(&mut limited, &target, 0.03, geometry_budget, capture)
            .unwrap_err()
            .code,
        RefusalCode::BudgetExceeded
    );
    assert_eq!(format!("{limited:?}"), format!("{original:?}"));
    // Old plastic-only capture must not acquire a Maxwell allocation or a stricter budget.
    let plastic =
        advance_body_with_dissipation(&mut limited, &target, 0.03, geometry_budget).unwrap();
    assert!(
        plastic
            .samples
            .iter()
            .all(|p| p.membrane_relaxation_dissipation_j.is_none())
    );
}

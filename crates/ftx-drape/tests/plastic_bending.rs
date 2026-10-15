//! Real panel kinematics, sparse Newton, accepted history, sewn ownership and continued motion.
//! All materials, specimen bounds and body geometry here are explicitly synthetic.
use ftx_core::{RefusalCode, Vec2, Vec3, det};
use ftx_drape::bending::BendingPatch;
use ftx_drape::material::resolve;
use ftx_drape::motion::{MotionOptions, advance_body};
use ftx_drape::run::{DrapeBody, DrapeOptions, drape};
use ftx_drape::scene::PartFrames;
use ftx_drape::{ClothModel, Membrane, PartFrame, Solver, hex_grid};
use ftx_pattern::Pattern;
use ftx_pattern::material_bending::BendingTensor;
use ftx_pattern::material_catalog::Selection;
use ftx_pattern::material_plasticity::PlasticBending;
use ftx_pattern::materials::Catalog;
use serde_json::json;

fn near(a: f64, b: f64, tol: f64) {
    assert!(
        (a - b).abs() <= tol * (1. + a.abs() + b.abs()),
        "{a} != {b}"
    );
}
fn selection() -> Selection {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-plastic-bending.json"
    ))
    .unwrap()
}
fn points() -> [Vec2; 6] {
    [
        [0., 0.],
        [0.2, 0.],
        [0., 0.15],
        [0.2, 0.15],
        [0.4, 0.],
        [0., 0.3],
    ]
    .map(|p| Vec2::new(p[0], p[1]))
}
fn curved(p: Vec2) -> Vec3 {
    Vec3::new(
        1.3 * p.x + 0.4 * p.x * p.x + 0.2 * p.x * p.y,
        0.8 * p.y + 0.3 * p.y * p.y,
        0.6 * p.x * p.x + 0.2 * p.x * p.y - 0.4 * p.y * p.y,
    )
}

#[test]
fn exact_condensed_patch_gradient_hessian_and_objectivity_with_accepted_memory() {
    let tensor = BendingTensor::new(2., 3., 0.7, 4.)
        .unwrap()
        .normal_projected()
        .with_preferred_curvature([0.05, -0.02, 0.01])
        .unwrap();
    let law = PlasticBending::new(
        tensor,
        [0.2, 0.1, 0.3],
        [0.4, 0.5, 0.6],
        [100.; 3],
        [1000.; 3],
    )
    .unwrap();
    let memory = law.pristine().advance([-0.5, 0.4, -0.3]).unwrap().memory;
    let patch = BendingPatch::new([0, 1, 2, 3, 4, 5], points(), tensor).unwrap();
    let x = points().map(curved);
    for incremental in [true, false] {
        let energy = |x: &[Vec3]| {
            patch.reference_area()
                * memory
                    .response(patch.bending_strain(x).unwrap(), incremental)
                    .unwrap()
                    .energy
        };
        let local = |x: &[Vec3]| {
            patch
                .normal_local_with_response(x, |k| memory.response(k, incremental))
                .unwrap()
        };
        let r = local(&x);
        near(r.energy, energy(&x), 1e-14);
        for j in 0..18 {
            let mut a = x;
            let mut b = x;
            let step = Vec3::new(
                f64::from(j % 3 == 0),
                f64::from(j % 3 == 1),
                f64::from(j % 3 == 2),
            ) * 1e-6;
            a[j / 3] += step;
            b[j / 3] -= step;
            near(r.grad[j], (energy(&a) - energy(&b)) / 2e-6, 2e-7);
            let (ap, bp) = (local(&a), local(&b));
            for i in 0..18 {
                near(r.hess[i * 18 + j], (ap.grad[i] - bp.grad[i]) / 2e-6, 4e-6);
                near(r.hess[i * 18 + j], r.hess[j * 18 + i], 1e-12);
            }
        }
        let rotate = |p: Vec3| Vec3::new(-p.y, p.x, p.z);
        let moved = x.map(|p| rotate(p) + Vec3::new(2., -3., 4.));
        let rigid = local(&moved);
        near(rigid.energy, r.energy, 1e-11);
        for i in 0..6 {
            let expected = rotate(Vec3::new(
                r.grad[3 * i],
                r.grad[3 * i + 1],
                r.grad[3 * i + 2],
            ));
            for k in 0..3 {
                near(rigid.grad[3 * i + k], expected[k], 1e-10);
            }
        }
    }
    assert!(memory.plastic_curvature().iter().any(|v| v.abs() > 0.));
}

fn solver(fixed: bool, plastic: bool, maxwell: bool) -> Solver {
    let mut selected = selection();
    if maxwell {
        let maxwell: Selection = serde_json::from_str(include_str!(
            "../../../tests/fixtures/materials/synthetic-relaxation.json"
        ))
        .unwrap();
        selected.record.as_mut().unwrap().membrane_relaxation =
            maxwell.record.unwrap().membrane_relaxation;
    }
    let m = Catalog::builtin().unwrap().resolve(&selected).unwrap();
    let fabric = resolve(
        &Pattern::new(json!({"fabric":{"material":selected}})),
        &DrapeOptions::default(),
    )
    .unwrap()
    .fabric;
    let (rest, tris) = hex_grid(0.1, 0.06, 0.025, Vec2::ZERO);
    let mut model = ClothModel::new(rest.clone(), &tris, fabric.membrane(1.), 0., 0.2).unwrap();
    model
        .set_panel_bending(
            &rest,
            &tris,
            &(0..rest.len()).collect::<Vec<_>>(),
            &vec![m.bending_response().unwrap(); tris.len()],
        )
        .unwrap();
    let x: Vec<_> = rest
        .iter()
        .map(|p| Vec3::new(p.x, p.y, 2. * p.x * p.x))
        .collect();
    let pins: Vec<_> = rest.iter().map(|p| [fixed || p.x == 0.; 3]).collect();
    let law = plastic.then(|| m.bending_plasticity().unwrap().unwrap());
    let mut solver = Solver::new(model, x, &pins, Vec3::ZERO)
        .with_bending_plasticity(&vec![law; tris.len()])
        .unwrap()
        .with_relaxation(&vec![m.membrane_relaxation().unwrap(); tris.len()])
        .unwrap();
    // Use the production 1e-6 m/s Newton-step tolerance. Requiring a 1e-11 m update
    // at these sub-millisecond steps stalls at numerical energy-resolution limits.
    solver.max_newton = 100;
    solver
}

#[test]
fn clamped_folding_records_permanent_curvature_without_double_counting_elastic_energy() {
    let mut s = solver(true, true, false);
    let x = s.x.clone();
    let initial = s.panel_bending_energy().unwrap();
    near(
        initial,
        s.model
            .panel_bending_patches()
            .iter()
            .map(|p| p.energy(&x))
            .sum(),
        1e-14,
    );
    let f = s.elastic_forces();
    s.step(0.001).unwrap();
    let history = s.bending_plasticity_report().unwrap().unwrap();
    assert_eq!(s.x, x);
    assert_eq!(history["accepted_steps"], 1);
    assert!(history["plastic_dissipation_j"].as_f64().unwrap() > 0.);
    assert!(
        history["patches"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["plastic_curvature_m_inv"][0].as_f64().unwrap().abs() > 0.1)
    );
    assert!(s.panel_bending_energy().unwrap() < initial);
    assert_ne!(s.elastic_forces(), f);
    near(
        s.panel_bending_energy().unwrap(),
        history["stored_elastic_and_hardening_energy_j"]
            .as_f64()
            .unwrap(),
        1e-14,
    );
    assert!(s.static_solve().is_err());
    assert!(s.settle(0.001, 0.01, 1e-4, 3, 10).is_err());
    let diss = history["plastic_dissipation_j"].as_f64().unwrap();
    for _ in 0..10 {
        s.step(0.001).unwrap();
    }
    near(
        s.bending_plasticity_report().unwrap().unwrap()["plastic_dissipation_j"]
            .as_f64()
            .unwrap(),
        diss,
        1e-12,
    );
    // A prescribed reverse bend accumulates history instead of annealing the original crease.
    for p in &mut s.x {
        p.z = -p.z;
    }
    s.step(0.001).unwrap();
    assert!(
        s.bending_plasticity_report().unwrap().unwrap()["plastic_dissipation_j"]
            .as_f64()
            .unwrap()
            > diss
    );
}

#[test]
fn failed_steps_roll_back_both_memories_and_continuation_is_deterministic() {
    let mut s = solver(false, true, true);
    s.step(0.0005).unwrap();
    let mut clean = s.clone();
    let (x, v, plastic, maxwell) = (
        s.x.clone(),
        s.v.clone(),
        s.bending_plasticity_report().unwrap(),
        s.relaxation_report().unwrap(),
    );
    s.max_newton = 0;
    assert_eq!(s.step(0.0005).unwrap_err().code, RefusalCode::NotConverged);
    assert_eq!(s.x, x);
    assert_eq!(s.v, v);
    assert_eq!(s.bending_plasticity_report().unwrap(), plastic);
    assert_eq!(s.relaxation_report().unwrap(), maxwell);
    s.max_newton = 100;
    for _ in 0..4 {
        s.step(0.0005).unwrap();
        clean.step(0.0005).unwrap();
    }
    assert_eq!(s.x, clean.x);
    assert_eq!(s.v, clean.v);
    assert_eq!(
        s.bending_plasticity_report().unwrap(),
        clean.bending_plasticity_report().unwrap()
    );
    assert_eq!(
        s.relaxation_report().unwrap(),
        clean.relaxation_report().unwrap()
    );
    assert_eq!(
        s.bending_plasticity_report().unwrap().unwrap()["accepted_steps"],
        5
    );
    let before = s.bending_plasticity_report().unwrap();
    for h in [0., -1., f64::NAN, f64::INFINITY] {
        assert!(s.step(h).is_err());
    }
    assert_eq!(s.bending_plasticity_report().unwrap(), before);
}

#[test]
fn material_only_change_affects_sparse_dynamics_not_mass_metric_or_seam_policy() {
    let mut plastic = solver(false, true, false);
    let mut elastic = solver(false, false, false);
    assert_eq!(plastic.x, elastic.x);
    assert_eq!(plastic.model.mass, elastic.model.mass);
    assert_eq!(plastic.model.membranes, elastic.model.membranes);
    for _ in 0..5 {
        plastic.step(0.001).unwrap();
        elastic.step(0.001).unwrap();
    }
    assert_ne!(plastic.x, elastic.x);
    assert!(plastic.x.iter().all(|p| p.is_finite()));
    assert!(elastic.bending_plasticity_report().unwrap().is_none());
    for i in 0..plastic.model.hinges.len() {
        assert_eq!(
            plastic.model.hinge_rigidity(i),
            elastic.model.hinge_rigidity(i)
        );
    }
}

#[test]
fn stale_stencils_rest_metrics_and_invalid_material_bindings_refuse() {
    let mut s = solver(true, true, false);
    s.step(0.001).unwrap();
    let before = s.bending_plasticity_report().unwrap();
    assert!(
        s.clone()
            .with_bending_plasticity(&vec![None; s.model.membranes.len()])
            .is_err()
    );
    let mut changed = s.clone();
    changed.model.membranes[0].g[0][0] += 1.;
    assert!(changed.step(0.001).is_err());
    assert!(changed.bending_plasticity_report().is_err());
    assert_eq!(s.bending_plasticity_report().unwrap(), before);
    let rest = vec![Vec2::ZERO, Vec2::new(0.1, 0.), Vec2::new(0., 0.1)];
    let model = ClothModel::new(
        rest.clone(),
        &[[0, 1, 2]],
        Membrane::orthotropic(100., 100., 0., 50., 1.),
        0.,
        0.2,
    )
    .unwrap();
    let bare = Solver::new(
        model,
        rest.iter().map(|p| Vec3::new(p.x, p.y, 0.)).collect(),
        &[[true; 3]; 3],
        Vec3::ZERO,
    );
    let law = Catalog::builtin()
        .unwrap()
        .resolve(&selection())
        .unwrap()
        .bending_plasticity()
        .unwrap();
    assert!(bare.with_bending_plasticity(&[law]).is_err());
}

struct Frames;
impl PartFrames for Frames {
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
fn garment() -> Pattern {
    let w = 40. * std::f64::consts::PI;
    let pieces:Vec<_>=["front","back"].into_iter().enumerate().map(|(i,id)|json!({
        "id":id,"cut":{"qty":1},"vertices":{"a":[0,0],"b":[w,0],"c":[w,30],"d":[0,30]},
        "edges":[{"id":"bottom","from":"a","to":"b"},{"id":"right","from":"b","to":"c"},
            {"id":"top","from":"c","to":"d"},{"id":"left","from":"d","to":"a"}],
        "grain":[[0,0],[0,30]],"placement":{"part":"torso","radius_mm":40,"x0":w/2.,"theta_deg":180*i}
    })).collect();
    let mut elastic = selection();
    elastic
        .record
        .as_mut()
        .unwrap()
        .structure
        .remove("bending_plasticity_model");
    let mut p = json!({"schema":"fabric-pattern/1","units":"mm","fabric":{"material":selection()},
        "pieces":pieces,"seams":[{"id":"left","a":"front/right","b":"back/left"},
        {"id":"right","a":"back/right","b":"front/left"}]});
    p["pieces"][1]["fabric"] = json!({"material":elastic});
    Pattern::new(p)
}

#[test]
fn ordinary_sewn_drape_selects_per_piece_crease_history_and_body_motion_retains_it() {
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    for z in [-0.02, 0.12] {
        for i in 0..16 {
            let (s, c) = det::sin_cos(f64::from(i) * std::f64::consts::TAU / 16.);
            vertices.push(Vec3::new(0.03 * c, 0.03 * s, z));
        }
    }
    for i in 0..16 {
        let j = (i + 1) % 16;
        triangles.extend([[i, j, j + 16], [i, j + 16, i + 16]]);
    }
    let body = DrapeBody {
        frames: &Frames,
        collision: (&vertices, &triangles),
        display: (&vertices, &triangles),
        arm_deg: 35.,
    };
    let opts = DrapeOptions {
        spacing: 10.,
        sew_steps: 3,
        drape_steps: 3,
        step: 0.0005,
        ..Default::default()
    };
    let mut run = drape(&garment(), &json!({}), &body, &opts, |_| {}).unwrap();
    let history = &run.report["bending_history"];
    assert_eq!(history["accepted_steps"], 6);
    assert!(history["plastic_dissipation_j"].as_f64().unwrap() > 0.);
    assert_eq!(run.report["material"]["plastic_bending_piece_count"], 1);
    assert!(run.scene.mesh.verts2d.len() > run.scene.solver.x.len()); // real sewn/welded pattern
    for p in history["patches"].as_array().unwrap() {
        let i = p["triangle"].as_u64().unwrap() as usize;
        assert_eq!(
            run.scene.mesh.instances[run.scene.triangle_owner[i]],
            "front"
        );
    }
    let shifted: Vec<_> = vertices
        .iter()
        .map(|p| *p + Vec3::new(0., 0., 1e-6))
        .collect();
    advance_body(
        &mut run.scene.solver,
        &shifted,
        0.0005,
        MotionOptions::default(),
    )
    .unwrap();
    let continued = run
        .scene
        .solver
        .bending_plasticity_report()
        .unwrap()
        .unwrap();
    assert!(continued["accepted_steps"].as_u64().unwrap() > 6);
    assert!(
        continued["plastic_dissipation_j"].as_f64().unwrap()
            >= history["plastic_dissipation_j"].as_f64().unwrap()
    );
    let (x, v, body_before) = (
        run.scene.solver.x.clone(),
        run.scene.solver.v.clone(),
        run.scene
            .solver
            .contact
            .as_ref()
            .unwrap()
            .body
            .vertices
            .clone(),
    );
    assert!(
        advance_body(
            &mut run.scene.solver,
            &[Vec3::ZERO],
            0.0005,
            MotionOptions::default()
        )
        .is_err()
    );
    assert_eq!(run.scene.solver.x, x);
    assert_eq!(run.scene.solver.v, v);
    assert_eq!(
        run.scene.solver.contact.as_ref().unwrap().body.vertices,
        body_before
    );
    assert_eq!(
        run.scene
            .solver
            .bending_plasticity_report()
            .unwrap()
            .unwrap(),
        continued
    );
}

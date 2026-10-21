//! Independent curved-surface and planar-stretch answers; all constitutive inputs are synthetic.
use ftx_core::{Vec2, Vec3, det};
use ftx_drape::bending::BendingPatch;
use ftx_drape::{ClothModel, Membrane, Solver, hex_grid};
use ftx_pattern::material_bending::{BendingTensor, NORMAL_MODEL};

fn law() -> BendingTensor {
    BendingTensor::new(0.002, 0.0005, -0.0001, 0.0003)
        .unwrap()
        .normal_projected()
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
fn patch() -> BendingPatch {
    BendingPatch::new([0, 1, 2, 3, 4, 5], points(), law()).unwrap()
}
fn surface(p: Vec2) -> Vec3 {
    let (u, v) = (p.x, p.y);
    Vec3::new(
        1.3 * u + 0.4 * u * u + 0.2 * u * v,
        0.8 * v + 0.3 * v * v,
        0.6 * u * u + 0.2 * u * v - 0.4 * v * v,
    )
}
fn near(a: f64, b: f64, tol: f64) {
    assert!(
        (a - b).abs() <= tol * (1. + a.abs() + b.abs()),
        "{a} vs {b}"
    );
}
fn model(rest: &[Vec2], tris: &[[usize; 3]], law: Option<BendingTensor>) -> ClothModel {
    let mut m = ClothModel::new(
        rest.to_vec(),
        tris,
        Membrane::orthotropic(100., 100., 0., 40., 1.),
        0.0002,
        0.2,
    )
    .unwrap();
    m.set_panel_bending(
        rest,
        tris,
        &(0..rest.len()).collect::<Vec<_>>(),
        &vec![law; tris.len()],
    )
    .unwrap();
    m
}

#[test]
fn normal_curvature_matches_independent_quadratic_surface_at_triangle_centroid() {
    let p = patch();
    let x = points().map(surface);
    let u = 0.2 / 3.;
    let v = 0.15 / 3.;
    let tu = Vec3::new(1.3 + 0.8 * u + 0.2 * v, 0., 1.2 * u + 0.2 * v);
    let tv = Vec3::new(0.2 * u, 0.8 + 0.6 * v, 0.2 * u - 0.8 * v);
    let n = tu.cross(tv).normalized();
    let expected = [
        n.dot(Vec3::new(0.8, 0., 1.2)),
        n.dot(Vec3::new(0., 0.6, -0.8)),
        n.dot(Vec3::new(0.4, 0., 0.4)),
    ];
    for (a, b) in p.normal_curvature(&x).unwrap().into_iter().zip(expected) {
        near(a, b, 2e-13);
    }
    let d = law().matrix();
    let mut e = 0.;
    for i in 0..3 {
        for j in 0..3 {
            e += 0.5 * 0.015 * expected[i] * d[i][j] * expected[j];
        }
    }
    near(p.energy(&x), e, 2e-15);
    assert_eq!(p.model(), NORMAL_MODEL);
}

#[test]
fn arbitrary_planar_nonuniform_stretch_has_no_bending_energy_or_force() {
    let x = points().map(|p| {
        let q = surface(p);
        Vec3::new(q.x, q.y, 0.)
    });
    let p = patch();
    assert_eq!(p.energy(&x), 0.);
    assert!(p.local(&x).grad.iter().all(|v| *v == 0.));
    let legacy = BendingPatch::new(
        [0, 1, 2, 3, 4, 5],
        points(),
        BendingTensor::new(0.002, 0.0005, -0.0001, 0.0003).unwrap(),
    )
    .unwrap();
    assert!(legacy.energy(&x) > 1e-6); // retain the old approximation only on explicit old selections
    let tilted = x.map(|p| Vec3::new(0.6 * p.x, p.y, 0.8 * p.x) + Vec3::new(3., -2., 1.));
    assert!(patch().energy(&tilted) < 1e-26);
    assert!(patch().local(&tilted).grad.iter().all(|v| v.abs() < 1e-12));
}

#[test]
fn exact_normal_derivatives_match_independent_finite_differences() {
    let p = patch();
    let x = points().map(surface);
    let l = p.local(&x);
    near(l.energy, p.energy(&x), 1e-17);
    assert!(!l.psd); // geometric stiffness is not silently classified as positive semidefinite
    let eps = 2e-6;
    for a in 0..18 {
        let mut plus = x;
        let mut minus = x;
        let mut delta = Vec3::ZERO;
        match a % 3 {
            0 => delta.x = eps,
            1 => delta.y = eps,
            _ => delta.z = eps,
        }
        plus[a / 3] += delta;
        minus[a / 3] -= delta;
        near(
            l.grad[a],
            (p.energy(&plus) - p.energy(&minus)) / (2. * eps),
            1e-8,
        );
        let gp = p.local(&plus).grad;
        let gm = p.local(&minus).grad;
        for b in 0..18 {
            near(l.hess[b * 18 + a], (gp[b] - gm[b]) / (2. * eps), 3e-7);
            near(l.hess[b * 18 + a], l.hess[a * 18 + b], 1e-12);
        }
    }
    for k in 0..3 {
        near((0..6).map(|i| l.grad[3 * i + k]).sum(), 0., 1e-14);
    }
}

#[test]
fn rigid_motion_preserves_energy_and_rotates_force() {
    let x = points().map(surface);
    let rotate = |p: Vec3| Vec3::new(p.z, p.x, p.y);
    let moved = x.map(|p| rotate(p) + Vec3::new(8., -4., 2.));
    let a = patch().local(&x);
    let b = patch().local(&moved);
    near(a.energy, b.energy, 1e-14);
    for i in 0..6 {
        let expected = rotate(Vec3::new(
            a.grad[3 * i],
            a.grad[3 * i + 1],
            a.grad[3 * i + 2],
        ));
        for k in 0..3 {
            near(expected[k], b.grad[3 * i + k], 1e-11);
        }
    }
}

#[test]
fn stretched_cylinders_converge_in_reference_covariant_curvature() {
    let radius = 0.2;
    let stretch = 1.6;
    for angle in [0., 0.37, 1.1] {
        let (s, c) = det::sin_cos(angle);
        let k = [
            stretch * stretch * c * c / radius,
            stretch * stretch * s * s / radius,
            2. * stretch * stretch * s * c / radius,
        ];
        let d = law().matrix();
        let mut expected = 0.;
        for i in 0..3 {
            for j in 0..3 {
                expected += 0.5 * 0.12 * 0.12 * k[i] * d[i][j] * k[j];
            }
        }
        let error = |h| {
            let (rest, tris) = hex_grid(0.12, 0.12, h, Vec2::ZERO);
            let m = model(&rest, &tris, Some(law()));
            let x: Vec<_> = rest
                .iter()
                .map(|p| {
                    let u = c * p.x + s * p.y;
                    let v = -s * p.x + c * p.y;
                    let (a, b) = det::sin_cos(stretch * u / radius);
                    Vec3::new(radius * a, 0.8 * v, radius * (1. - b))
                })
                .collect();
            m.check_panel_bending_domains(&x).unwrap();
            let actual: f64 = m.panel_bending_patches().iter().map(|p| p.energy(&x)).sum();
            (actual - expected).abs() / expected
        };
        let coarse = error(0.03);
        let fine = error(0.005);
        assert!(
            fine < 0.005 && fine < coarse / 5.,
            "{angle}: {coarse} -> {fine}"
        );
    }
}

#[test]
fn actual_planar_model_keeps_membrane_forces_without_spurious_bending() {
    let (rest, tris) = hex_grid(0.08, 0.06, 0.02, Vec2::ZERO);
    let normal = model(&rest, &tris, Some(law()));
    let scalar_zero = ClothModel::new(rest.clone(), &tris, normal.membrane, 0., 0.2).unwrap();
    let x: Vec<_> = rest
        .iter()
        .map(|p| Vec3::new(p.x + 2. * p.x * p.x, p.y + 3. * p.y * p.y, 0.))
        .collect();
    let fixed = vec![[false; 3]; x.len()];
    let a = Solver::new(normal, x.clone(), &fixed, Vec3::ZERO);
    let b = Solver::new(scalar_zero, x, &fixed, Vec3::ZERO);
    assert_eq!(a.model.mass, b.model.mass);
    let fa = a.elastic_forces();
    let fb = b.elastic_forces();
    for (x, y) in fa.iter().zip(fb) {
        near((*x - y).norm(), 0., 1e-12);
    }
    near(
        a.model.elastic_energy(&a.x),
        b.model.elastic_energy(&b.x),
        1e-14,
    );
}

#[test]
fn singular_frames_refuse_before_simulation_and_leave_state_unchanged() {
    let p = patch();
    let x = points().map(|p| Vec3::new(p.x, 0., 0.));
    assert!(p.check_domain(&x).is_err());
    assert!(p.normal_curvature(&x).is_err());
    assert!(!p.energy(&x).is_finite());
    let (rest, tris) = hex_grid(0.08, 0.06, 0.02, Vec2::ZERO);
    let m = model(&rest, &tris, Some(law()));
    let x: Vec<_> = rest.iter().map(|p| Vec3::new(p.x, 0., 0.)).collect();
    let mut s = Solver::new(m, x.clone(), &vec![[false; 3]; x.len()], Vec3::ZERO);
    let before = format!("{:?}", s.model);
    let v = s.v.clone();
    assert_eq!(
        s.step(0.001).unwrap_err().code,
        ftx_core::RefusalCode::OutOfRange
    );
    assert_eq!(s.x, x);
    assert_eq!(s.v, v);
    assert_eq!(format!("{:?}", s.model), before);
}

#[test]
fn normal_bending_reaches_sparse_newton_and_changes_material_only_dynamics() {
    let (rest, tris) = hex_grid(0.08, 0.06, 0.02, Vec2::ZERO);
    let x: Vec<_> = rest.iter().map(|p| Vec3::new(p.x, p.y, 0.)).collect();
    let fixed: Vec<_> = rest.iter().map(|p| [p.x < 0.021; 3]).collect();
    let make = |mult| {
        let d = BendingTensor::new(0.002 * mult, 0.0005 * mult, -0.0001 * mult, 0.0003 * mult)
            .unwrap()
            .normal_projected();
        let mut s = Solver::new(
            model(&rest, &tris, Some(d)),
            x.clone(),
            &fixed,
            Vec3::new(0., 0., -2.),
        );
        s.tol = 1e-7;
        s
    };
    let mut soft = make(0.1);
    let mut stiff = make(20.);
    for _ in 0..5 {
        assert!(soft.step(0.005).unwrap().converged);
        assert!(stiff.step(0.005).unwrap().converged);
    }
    assert_eq!(soft.model.mass, stiff.model.mass);
    assert_eq!(soft.model.membranes, stiff.model.membranes);
    assert!(
        soft.x.iter().map(|p| p.z).sum::<f64>() < stiff.x.iter().map(|p| p.z).sum::<f64>() - 1e-5
    );
    soft.model.check_panel_bending_domains(&soft.x).unwrap();
    stiff.model.check_panel_bending_domains(&stiff.x).unwrap();
}

#[test]
fn geometric_hessian_projection_supports_all_six_patch_vertices() {
    let x = points().map(surface);
    let l = patch().local(&x);
    let (eigenvalues, _) = ftx_drape::eigen::jacobi_eigh(&l.hess, 18);
    assert!(eigenvalues.iter().any(|v| *v < -1e-6));
    let mut h = l.hess;
    assert!(ftx_drape::eigen::project_psd_translation_free(&mut h, 6));
    let (eigenvalues, _) = ftx_drape::eigen::jacobi_eigh(&h, 18);
    assert!(eigenvalues.iter().all(|v| *v > -1e-10));
    for a in 0..18 {
        for k in 0..3 {
            near((0..6).map(|i| h[a * 18 + 3 * i + k]).sum(), 0., 1e-10);
        }
    }
}

struct Frames;
impl ftx_drape::scene::PartFrames for Frames {
    fn frame(&self, _: &str) -> Option<ftx_drape::PartFrame> {
        Some(ftx_drape::PartFrame {
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

#[test]
fn source_selected_kinematics_survive_sewing_per_piece_ownership_and_motion() {
    use ftx_drape::run::{DrapeBody, DrapeOptions, drape};
    use serde_json::{Value, json};
    let selection: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-panel-bending.json"
    ))
    .unwrap();
    let mut normal = selection.clone();
    normal["record"]["structure"]["bending_model"] = json!(NORMAL_MODEL);
    for (key, value) in ftx_pattern::material_bending::PREFERRED_CURVATURE_PROPERTIES
        .into_iter()
        .zip([0.5, -0.2, 0.1])
    {
        let mut datum = normal["record"]["properties"]["bending_warp"].clone();
        datum["value"] = json!(value);
        datum["unit"] = json!("1/m");
        normal["record"]["properties"][key] = datum;
    }
    let w = 40. * std::f64::consts::PI;
    let pieces: Vec<_> = ["front", "back"]
        .iter()
        .enumerate()
        .map(|(i, id)| {
            json!({
                "id":id,"cut":{"qty":1},"vertices":{"a":[0,0],"b":[w,0],"c":[w,30],"d":[0,30]},
                "edges":[{"id":"bottom","from":"a","to":"b"},{"id":"right","from":"b","to":"c"},
                    {"id":"top","from":"c","to":"d"},{"id":"left","from":"d","to":"a"}],
                "grain":[[0,0],[0,30]],
                "placement":{"part":"torso","radius_mm":40,"x0":w/2.,"theta_deg":180*i}
            })
        })
        .collect();
    let mut raw = json!({"schema":"fabric-pattern/1","units":"mm","fabric":{"material":normal},
        "pieces":pieces,"seams":[{"id":"left","a":"front/right","b":"back/left"},
        {"id":"right","a":"back/right","b":"front/left"}]});
    raw["pieces"][1]["fabric"] = json!({"material":selection});
    let pattern = ftx_pattern::Pattern::new(raw);
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    for z in [-0.02, 0.12] {
        for i in 0..16 {
            let (s, c) = det::sin_cos(i as f64 * std::f64::consts::TAU / 16.);
            vertices.push(Vec3::new(0.03 * c, 0.03 * s, z));
        }
    }
    for i in 0..16 {
        let j = (i + 1) % 16;
        triangles.extend([[i, j, j + 16], [i, j + 16, i + 16]]);
    }
    let b = DrapeBody {
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
    let mut result = drape(&pattern, &json!({}), &b, &opts, |_| {}).unwrap();
    let field = &result.scene.solver.model;
    assert!(result.scene.mesh.verts2d.len() > field.mass.len());
    assert_eq!(result.report["panel_bending"]["model"], "per_patch");
    assert_eq!(
        result.report["panel_bending"]["models"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    for (patch, &i) in field
        .panel_bending_patches()
        .iter()
        .zip(field.panel_bending_triangles())
    {
        assert_eq!(
            patch.preferred_curvature(),
            if result.scene.triangle_owner[i] == 0 {
                [0.5, -0.2, 0.1]
            } else {
                [0.0; 3]
            }
        );
        assert_eq!(
            patch.model(),
            if result.scene.triangle_owner[i] == 0 {
                NORMAL_MODEL
            } else {
                ftx_pattern::material_bending::MODEL
            }
        );
    }
    let records = result.report["material"]["records"].as_object().unwrap();
    assert!(records.values().any(|r| r["bending_model"] == NORMAL_MODEL));
    let before = result.scene.solver.clone();
    let shifted: Vec<_> = vertices
        .iter()
        .map(|v| *v + Vec3::new(0., 0., 1e-6))
        .collect();
    ftx_drape::motion::advance_body(
        &mut result.scene.solver,
        &shifted,
        0.0005,
        Default::default(),
    )
    .unwrap();
    result
        .scene
        .solver
        .model
        .check_panel_bending_domains(&result.scene.solver.x)
        .unwrap();
    assert_eq!(before.model.mass, result.scene.solver.model.mass);
    assert_eq!(
        before.model.panel_bending_triangles(),
        result.scene.solver.model.panel_bending_triangles()
    );
}

#[test]
fn preferred_curvature_has_independent_zero_and_flat_energy_answers() {
    let x = points().map(surface);
    let u = 0.2 / 3.;
    let v = 0.15 / 3.;
    let n = Vec3::new(1.3 + 0.8 * u + 0.2 * v, 0., 1.2 * u + 0.2 * v)
        .cross(Vec3::new(0.2 * u, 0.8 + 0.6 * v, 0.2 * u - 0.8 * v))
        .normalized();
    let target = [
        n.dot(Vec3::new(0.8, 0., 1.2)),
        n.dot(Vec3::new(0., 0.6, -0.8)),
        n.dot(Vec3::new(0.4, 0., 0.4)),
    ];
    let d = law().with_preferred_curvature(target).unwrap();
    let p = BendingPatch::new([0, 1, 2, 3, 4, 5], points(), d).unwrap();
    assert!(p.energy(&x) < 1e-28);
    assert!(p.local(&x).grad.iter().all(|v| v.abs() < 1e-13));
    let flat = points().map(|v| Vec3::new(v.x, v.y, 0.));
    let matrix = d.matrix();
    let mut expected = 0.;
    for i in 0..3 {
        for j in 0..3 {
            expected += 0.5 * 0.015 * target[i] * matrix[i][j] * target[j];
        }
    }
    near(p.energy(&flat), expected, 1e-15);
    // A geometric plane is no longer bending-stress-free when the material prefers a curved form.
    assert!(p.local(&flat).grad.iter().any(|v| v.abs() > 1e-6));
    let reflected = x.map(|v| Vec3::new(v.x, v.y, -v.z));
    let opposite = BendingPatch::new(
        [0, 1, 2, 3, 4, 5],
        points(),
        law().with_preferred_curvature(target.map(|v| -v)).unwrap(),
    )
    .unwrap();
    assert!(opposite.energy(&reflected) < 1e-28);
    assert!(p.energy(&reflected) > 1e-6);
}

#[test]
fn preferred_curvature_derivatives_include_the_changing_normal() {
    let p = BendingPatch::new(
        [0, 1, 2, 3, 4, 5],
        points(),
        law().with_preferred_curvature([2., -1., 0.6]).unwrap(),
    )
    .unwrap();
    let x = points().map(surface);
    let l = p.local(&x);
    near(l.energy, p.energy(&x), 1e-17);
    let eps = 2e-6;
    for i in 0..18 {
        let mut plus = x;
        let mut minus = x;
        let delta = [
            Vec3::new(eps, 0., 0.),
            Vec3::new(0., eps, 0.),
            Vec3::new(0., 0., eps),
        ][i % 3];
        plus[i / 3] += delta;
        minus[i / 3] -= delta;
        near(
            l.grad[i],
            (p.energy(&plus) - p.energy(&minus)) / (2. * eps),
            1e-8,
        );
        let gp = p.local(&plus).grad;
        let gm = p.local(&minus).grad;
        for j in 0..18 {
            near(l.hess[j * 18 + i], (gp[j] - gm[j]) / (2. * eps), 4e-7);
        }
    }
    let moved = x.map(|v| Vec3::new(v.z, v.x, v.y) + Vec3::new(7., -3., 2.));
    near(p.energy(&moved), p.energy(&x), 1e-14);
    let zero = BendingPatch::new(
        [0, 1, 2, 3, 4, 5],
        points(),
        law().with_preferred_curvature([0.; 3]).unwrap(),
    )
    .unwrap();
    assert_eq!(zero.energy(&x), patch().energy(&x));
    assert_eq!(zero.local(&x).grad, patch().local(&x).grad);
    assert_eq!(zero.local(&x).hess, patch().local(&x).hess);
}

#[test]
fn preferred_curvature_drives_opposite_curl_without_gravity_or_geometry_edits() {
    let (rest, tris) = hex_grid(0.08, 0.06, 0.02, Vec2::ZERO);
    let x: Vec<_> = rest.iter().map(|p| Vec3::new(p.x, p.y, 0.)).collect();
    let fixed: Vec<_> = rest.iter().map(|p| [p.x < 0.021; 3]).collect();
    let make = |curvature| {
        let d = law().with_preferred_curvature([curvature, 0., 0.]).unwrap();
        let mut s = Solver::new(model(&rest, &tris, Some(d)), x.clone(), &fixed, Vec3::ZERO);
        s.tol = 1e-8;
        s
    };
    let mut up = make(4.);
    let mut down = make(-4.);
    let mut flat = make(0.);
    let initial = up.model.elastic_energy(&up.x);
    for _ in 0..8 {
        assert!(up.step(0.005).unwrap().converged);
        assert!(down.step(0.005).unwrap().converged);
        assert!(flat.step(0.005).unwrap().converged);
    }
    assert_eq!(up.model.mass, down.model.mass);
    assert_eq!(up.model.membranes, down.model.membranes);
    assert_eq!(up.model.rest, down.model.rest);
    assert!(up.x.iter().map(|p| p.z).sum::<f64>() > 1e-5);
    for ((u, d), f) in up.x.iter().zip(&down.x).zip(&flat.x) {
        near(u.x, d.x, 1e-11);
        near(u.y, d.y, 1e-11);
        near(u.z, -d.z, 1e-11);
        near(f.z, 0., 1e-13);
    }
    assert!(up.model.elastic_energy(&up.x) < initial);
}

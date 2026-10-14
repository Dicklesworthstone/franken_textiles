//! Orthotropic bending known answers and actual sparse cloth dynamics; all materials are synthetic.
use ftx_core::{Vec2, Vec3, det};
use ftx_drape::{ClothModel, Membrane, Solver, hex_grid};
use ftx_pattern::material_bending::BendingTensor;

fn membrane() -> Membrane {
    Membrane::orthotropic(1000., 1000., 0.0, 200., 1.0)
}
fn tensor(scale: f64) -> BendingTensor {
    BendingTensor::new(
        0.002 * scale,
        0.0005 * scale,
        0.0001 * scale,
        0.0003 * scale,
    )
    .unwrap()
}
fn model(rest: &[Vec2], tris: &[[usize; 3]], law: Option<BendingTensor>) -> ClothModel {
    let mut m = ClothModel::new(rest.to_vec(), tris, membrane(), 0.0002, 0.2).unwrap();
    m.set_panel_bending(
        rest,
        tris,
        &(0..rest.len()).collect::<Vec<_>>(),
        &vec![law; tris.len()],
    )
    .unwrap();
    m
}
fn bending_energy(m: &ClothModel, x: &[Vec3]) -> f64 {
    m.panel_bending_patches().iter().map(|p| p.energy(x)).sum()
}

#[test]
fn cylinders_converge_to_directional_plate_energy_including_bias_twist() {
    for angle in [0.0, 0.37, std::f64::consts::FRAC_PI_2] {
        let (s, c) = det::sin_cos(angle);
        let radius = 0.20;
        let k = [c * c / radius, s * s / radius, 2.0 * c * s / radius];
        let d = tensor(1.0).matrix();
        let mut expected = 0.0;
        for i in 0..3 {
            for j in 0..3 {
                expected += 0.5 * 0.12 * 0.12 * k[i] * d[i][j] * k[j];
            }
        }
        let evaluate = |h| {
            let (rest, tris) = hex_grid(0.12, 0.12, h, Vec2::ZERO);
            let m = model(&rest, &tris, Some(tensor(1.0)));
            let x: Vec<_> = rest
                .iter()
                .map(|p| {
                    let u = c * p.x + s * p.y;
                    let v = -s * p.x + c * p.y;
                    let (sin, cos) = det::sin_cos(u / radius);
                    Vec3::new(radius * sin, v, radius * (1.0 - cos))
                })
                .collect();
            assert!(
                m.hinges
                    .iter()
                    .enumerate()
                    .all(|(i, _)| m.hinge_rigidity(i) == 0.0)
            );
            (bending_energy(&m, &x) - expected).abs() / expected
        };
        let coarse = evaluate(0.03);
        let fine = evaluate(0.0075);
        assert!(fine < 0.003, "angle {angle}: {coarse} -> {fine}");
        assert!(fine < coarse / 3.0, "angle {angle}: {coarse} -> {fine}");
    }
}

#[test]
fn full_model_forces_are_the_derivative_of_membrane_plus_panel_energy() {
    let (rest, tris) = hex_grid(0.08, 0.06, 0.02, Vec2::ZERO);
    let m = model(&rest, &tris, Some(tensor(1.0)));
    let x: Vec<_> = rest
        .iter()
        .map(|p| Vec3::new(p.x, p.y, 0.4 * p.x * p.x - 0.2 * p.x * p.y))
        .collect();
    let solver = Solver::new(m, x.clone(), &vec![[false; 3]; x.len()], Vec3::ZERO);
    let forces = solver.elastic_forces();
    let eps = 1e-7;
    for i in 0..x.len() {
        for (k, delta) in [
            Vec3::new(eps, 0., 0.),
            Vec3::new(0., eps, 0.),
            Vec3::new(0., 0., eps),
        ]
        .into_iter()
        .enumerate()
        {
            let mut plus = x.clone();
            let mut minus = x.clone();
            plus[i] += delta;
            minus[i] -= delta;
            let num = -(solver.model.elastic_energy(&plus) - solver.model.elastic_energy(&minus))
                / (2.0 * eps);
            assert!(
                (forces[i][k] - num).abs() < 2e-7 * (1.0 + num.abs()),
                "{i}/{k}: {} vs {num}",
                forces[i][k]
            );
        }
    }
    assert!(forces.iter().fold(Vec3::ZERO, |a, b| a + *b).norm() < 1e-10);
}

#[test]
fn material_only_bending_changes_actual_newton_motion_and_none_is_bit_identical() {
    let (rest, tris) = hex_grid(0.12, 0.06, 0.02, Vec2::ZERO);
    let x: Vec<_> = rest.iter().map(|p| Vec3::new(p.x, p.y, 0.0)).collect();
    let fixed: Vec<_> = rest.iter().map(|p| [p.x < 0.021; 3]).collect();
    let make = |m| Solver::new(m, x.clone(), &fixed, Vec3::new(0., 0., -9.81));
    let mut soft = make(model(&rest, &tris, Some(tensor(0.1))));
    let mut stiff = make(model(&rest, &tris, Some(tensor(20.0))));
    assert_eq!(soft.model.mass, stiff.model.mass);
    let mut legacy = make(ClothModel::new(rest.clone(), &tris, membrane(), 0.0002, 0.2).unwrap());
    let mut absent = make(model(&rest, &tris, None));
    for _ in 0..10 {
        assert!(soft.step(0.01).unwrap().converged);
        assert!(stiff.step(0.01).unwrap().converged);
        assert_eq!(legacy.step(0.01).unwrap(), absent.step(0.01).unwrap());
        assert_eq!(legacy.x, absent.x);
        assert_eq!(legacy.v, absent.v);
    }
    let sag = |s: &Solver| s.x.iter().map(|p| p.z).sum::<f64>();
    assert!(
        sag(&soft) < sag(&stiff) - 0.001,
        "{} vs {}",
        sag(&soft),
        sag(&stiff)
    );
    assert_eq!(soft.model.membranes, stiff.model.membranes);
}

#[test]
fn binding_is_atomic_and_invalidates_previously_built_sparse_structure() {
    let (rest, tris) = hex_grid(0.08, 0.06, 0.02, Vec2::ZERO);
    let x: Vec<_> = rest
        .iter()
        .map(|p| Vec3::new(p.x, p.y, 0.2 * p.x * p.x))
        .collect();
    let fixed: Vec<_> = rest.iter().map(|p| [p.x < 0.021; 3]).collect();
    let m = model(&rest, &tris, None);
    let mut solver = Solver::new(m, x, &fixed, Vec3::new(0., 0., -1.));
    assert!(solver.step(0.01).unwrap().converged);
    let weld: Vec<_> = (0..rest.len()).collect();
    let mut laws = vec![Some(tensor(1.0)); tris.len()];
    laws[0] = None;
    let before = format!("{:?}", solver.model);
    assert!(
        solver
            .model
            .set_panel_bending(&rest, &tris, &weld, &laws)
            .is_err()
    );
    assert_eq!(before, format!("{:?}", solver.model));
    laws[0] = Some(tensor(1.0));
    solver
        .model
        .set_panel_bending(&rest, &tris, &weld, &laws)
        .unwrap();
    assert!(solver.step(0.01).unwrap().converged); // wider couplings must not scatter into old CSR
    assert!(
        solver
            .model
            .set_panel_bending(&rest, &tris, &weld, &laws)
            .is_err()
    );
}

#[test]
fn tensor_bending_coexists_with_transactional_membrane_history() {
    use ftx_pattern::{material_catalog::Selection, materials::Catalog};
    let selection: Selection = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-relaxation.json"
    ))
    .unwrap();
    let record = Catalog::builtin().unwrap().resolve(&selection).unwrap();
    let q = record.mechanics().unwrap();
    let (rest, tris) = hex_grid(0.08, 0.06, 0.02, Vec2::ZERO);
    let mut m = ClothModel::new(
        rest.clone(),
        &tris,
        Membrane::orthotropic(q.warp_n_m, q.weft_n_m, q.poisson, q.shear_n_m, 1.0),
        q.bending_nm,
        q.areal_kg_m2,
    )
    .unwrap();
    m.set_panel_bending(
        &rest,
        &tris,
        &(0..rest.len()).collect::<Vec<_>>(),
        &vec![Some(tensor(1.0)); tris.len()],
    )
    .unwrap();
    let x: Vec<_> = rest.iter().map(|p| Vec3::new(p.x, p.y, 0.0)).collect();
    let fixed: Vec<_> = rest.iter().map(|p| [p.x < 0.021; 3]).collect();
    let mut s = Solver::new(m, x, &fixed, Vec3::new(0., 0., -1.))
        .with_relaxation(&vec![record.membrane_relaxation().unwrap(); tris.len()])
        .unwrap();
    assert!(s.step(0.001).unwrap().converged);
    let mut twin = s.clone();
    assert_eq!(s.step(0.001).unwrap(), twin.step(0.001).unwrap());
    assert_eq!(s.x, twin.x);
    assert_eq!(
        s.relaxation_report().unwrap(),
        twin.relaxation_report().unwrap()
    );
    let before = s.x.clone();
    let history = s.relaxation_report().unwrap();
    s.max_newton = 0;
    assert!(s.step(0.001).is_err());
    assert_eq!(s.x, before);
    assert_eq!(s.relaxation_report().unwrap(), history);
}

#[test]
fn small_slope_cantilever_approaches_independent_beam_tip_deflection() {
    // With D12=0 the width-independent free-side solution is w_tip = q L^4/(8 D11).
    // Keep deflections tiny so membrane and non-isometric corrections are negligible. The two
    // clamped vertex columns approximate w(0)=w'(0)=0; their boundary error must shrink on refinement.
    let length: f64 = 0.12;
    let gravity = 0.001;
    let rigidity = 0.002;
    let expected = 0.2 * gravity * det::powi(length, 4) / (8.0 * rigidity);
    let solve = |h| {
        let (rest, tris) = hex_grid(length, 0.06, h, Vec2::ZERO);
        let m = model(
            &rest,
            &tris,
            Some(BendingTensor::new(rigidity, rigidity, 0.0, rigidity / 2.0).unwrap()),
        );
        let x = rest.iter().map(|p| Vec3::new(p.x, p.y, 0.0)).collect();
        let fixed: Vec<_> = rest.iter().map(|p| [true, true, p.x < 0.51 * h]).collect();
        let mut s = Solver::new(m, x, &fixed, Vec3::new(0.0, 0.0, -gravity));
        s.tol = 1e-12;
        let report = s.static_solve().unwrap();
        assert!(report.converged, "{report:?}");
        let tip: Vec<_> = rest
            .iter()
            .enumerate()
            .filter(|(_, p)| (p.x - length).abs() < 1e-10)
            .map(|(i, _)| -s.x[i].z)
            .collect();
        let actual = tip.iter().sum::<f64>() / tip.len() as f64;
        (actual - expected).abs() / expected
    };
    let coarse = solve(0.02);
    let fine = solve(0.005);
    assert!(fine < 0.12, "relative tip error {coarse} -> {fine}");
    assert!(fine < coarse * 0.6, "relative tip error {coarse} -> {fine}");
}

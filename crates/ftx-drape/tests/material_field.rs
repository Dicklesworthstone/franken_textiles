//! Synthetic heterogeneous-sheet known answers; no empirical fabric or sewn-seam claim.
use ftx_core::{Vec2, Vec3};
use ftx_drape::cloth::SheetMaterial;
use ftx_drape::{ClothModel, Membrane, Solver};
use ftx_pattern::material_catalog::Selection;
use ftx_pattern::material_response::CompiledMembrane;
use ftx_pattern::materials::{Catalog, Material};

fn sheet(scale: f64) -> SheetMaterial {
    SheetMaterial {
        membrane: Membrane::orthotropic(800.0 * scale, 200.0 * scale, 0.2, 100.0 * scale, 1.0),
        bending_nm: 0.002 * scale,
        areal_kg_m2: 0.2 * scale,
    }
}
fn geometry() -> (Vec<[usize; 3]>, Vec<[Vec2; 3]>, Vec<Vec3>) {
    // Different triangle areas (1/2 and 1) exercise area-weighted hinge coefficients.
    let rest = [
        Vec2::ZERO,
        Vec2::new(1.0, 0.0),
        Vec2::new(0.0, 1.0),
        Vec2::new(1.0, -2.0),
    ];
    let tris = vec![[0, 1, 2], [1, 0, 3]];
    let local = tris.iter().map(|t| t.map(|i| rest[i])).collect();
    let x = rest.iter().map(|p| Vec3::new(p.x, p.y, 0.0)).collect();
    (tris, local, x)
}
fn model(materials: &[SheetMaterial]) -> ClothModel {
    let (tris, local, _) = geometry();
    ClothModel::from_material_triangles(4, &tris, &local, materials).unwrap()
}

fn nonlinear_record() -> Material {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-nonlinear.json"
    ))
    .unwrap()
}

fn nonlinear(scale: f64) -> CompiledMembrane {
    let mut record = nonlinear_record();
    let response = record.membrane_response.as_mut().unwrap();
    for curve in [&mut response.warp, &mut response.weft, &mut response.shear] {
        for point in &mut curve.points {
            point[1] *= scale;
        }
    }
    response.coupling.value *= scale;
    response.compile(&record.sources, true).unwrap()
}

#[test]
fn per_triangle_mass_and_directional_laws_survive_a_shared_weld() {
    let a = sheet(1.0);
    let b = sheet(4.0);
    let model = model(&[a, b]);
    assert!(model.is_heterogeneous());
    let (ma, mb) = (a.areal_kg_m2 * 0.5, b.areal_kg_m2);
    for (got, expected) in
        model
            .mass
            .iter()
            .zip([(ma + mb) / 3.0, (ma + mb) / 3.0, ma / 3.0, mb / 3.0])
    {
        assert!((got - expected).abs() < 1e-15);
    }
    assert_eq!(model.material(0), a.membrane);
    assert_eq!(model.material(1), b.membrane);
    assert!((model.mass.iter().sum::<f64>() - ma - mb).abs() < 1e-15);
    assert!((model.hinge_rigidity(0) - (0.5 * a.bending_nm + b.bending_nm) / 1.5).abs() < 1e-16);
}

#[test]
fn homogeneous_field_is_bit_identical_through_newton() {
    let (tris, local, mut x) = geometry();
    let m = sheet(1.0);
    let plain =
        ClothModel::from_triangles(4, &tris, &local, m.membrane, m.bending_nm, m.areal_kg_m2)
            .unwrap();
    let field = model(&[m, m]);
    assert!(!field.is_heterogeneous());
    assert_eq!(plain.mass, field.mass);
    x[2].z = 0.02;
    assert_eq!(plain.elastic_energy(&x), field.elastic_energy(&x));
    let pins = [[true; 3], [true; 3], [false; 3], [false; 3]];
    let mut a = Solver::new(plain, x.clone(), &pins, Vec3::new(0.0, 0.0, -9.81));
    let mut b = Solver::new(field, x, &pins, Vec3::new(0.0, 0.0, -9.81));
    for _ in 0..4 {
        assert_eq!(a.step(0.005).unwrap(), b.step(0.005).unwrap());
        assert_eq!(a.x, b.x);
        assert_eq!(a.v, b.v);
        assert_eq!(a.elastic_forces(), b.elastic_forces());
    }
}

#[test]
fn heterogeneous_elastic_forces_match_the_actual_energy_gradient() {
    let mut m = model(&[sheet(1.0), sheet(7.0)]);
    m.set_compression(&[0.2, 0.03]).unwrap();
    assert_eq!(m.material(0).compression, [0.2; 2]);
    assert_eq!(m.material(1).q, sheet(7.0).membrane.q);
    let (_, _, mut x) = geometry();
    x[1].x = 1.02;
    x[2].z = 0.03;
    x[3].y = -2.04;
    let solver = Solver::new(m, x.clone(), &[[false; 3]; 4], Vec3::ZERO);
    let f = solver.elastic_forces();
    let eps = 1e-6;
    for i in 0..4 {
        for k in 0..3 {
            let mut plus = x.clone();
            let mut minus = x.clone();
            let delta = match k {
                0 => Vec3::new(eps, 0.0, 0.0),
                1 => Vec3::new(0.0, eps, 0.0),
                _ => Vec3::new(0.0, 0.0, eps),
            };
            plus[i] += delta;
            minus[i] -= delta;
            let numeric = -(solver.model.elastic_energy(&plus)
                - solver.model.elastic_energy(&minus))
                / (2.0 * eps);
            assert!(
                (f[i][k] - numeric).abs() < 1e-6 * (1.0 + numeric.abs()),
                "{i}/{k}: {} != {numeric}",
                f[i][k]
            );
        }
    }
    assert!(f.into_iter().fold(Vec3::ZERO, |a, b| a + b).norm() < 1e-11);
}

#[test]
fn substitution_changes_only_assigned_elements_and_is_atomic_on_failure() {
    let mut m = model(&[sheet(1.0), sheet(4.0)]);
    let before = format!("{m:?}");
    let mut invalid = sheet(2.0);
    invalid.membrane.q[0][1] = f64::NAN;
    assert!(m.set_materials(&[sheet(1.0), invalid]).is_err());
    assert_eq!(format!("{m:?}"), before);
    assert!(m.set_materials(&[sheet(1.0)]).is_err());
    assert_eq!(format!("{m:?}"), before);
    let first = m.material(0);
    let area = m.membranes.iter().map(|e| e.area).collect::<Vec<_>>();
    let old_unshared = m.mass[2];
    m.set_materials(&[sheet(1.0), sheet(8.0)]).unwrap();
    assert_eq!(m.material(0), first);
    assert_eq!(m.mass[2], old_unshared);
    assert_eq!(area, m.membranes.iter().map(|e| e.area).collect::<Vec<_>>());
    assert_eq!(m.material(1), sheet(8.0).membrane);
    m.set_materials(&[sheet(3.0); 2]).unwrap();
    assert!(!m.is_heterogeneous());
    assert_eq!(m.material(1), sheet(3.0).membrane);
    assert_eq!(m.hinge_rigidity(0), sheet(3.0).bending_nm);
}

#[test]
fn material_replacement_clears_old_nonlinear_laws_before_binding_new_mixed_response() {
    let sheets = [sheet(2.0), sheet(6.0)];
    let mut replaced = model(&[sheet(1.0), sheet(4.0)]);
    replaced
        .set_membrane_responses(&[Some(nonlinear(1.0)), Some(nonlinear(3.0))])
        .unwrap();
    let (_, _, mut x) = geometry();
    x[1].x = 1.12;
    x[2].z = 0.03;
    let old_energy = replaced.elastic_energy(&x);
    replaced.set_materials(&sheets).unwrap();
    assert!(replaced.membrane_response(0).is_none());
    assert!(replaced.membrane_response(1).is_none());
    let mut fresh = model(&sheets);
    assert_eq!(replaced.elastic_energy(&x), fresh.elastic_energy(&x));
    assert_ne!(replaced.elastic_energy(&x), old_energy);

    // Scene construction binds the new nonlinear field after effective sheets and gather factors.
    // An omitted response must remain linear even when that triangle used to have a nonlinear law.
    let responses = [None, Some(nonlinear(7.0))];
    replaced.set_membrane_responses(&responses).unwrap();
    fresh.set_membrane_responses(&responses).unwrap();
    assert!(replaced.membrane_response(0).is_none());
    assert_eq!(replaced.membrane_response(1), responses[1].as_ref());
    let pins = [[true; 3], [true; 3], [false; 3], [false; 3]];
    let mut a = Solver::new(replaced, x.clone(), &pins, Vec3::new(0.0, 0.0, -9.81));
    let mut b = Solver::new(fresh, x, &pins, Vec3::new(0.0, 0.0, -9.81));
    for _ in 0..3 {
        assert_eq!(a.elastic_forces(), b.elastic_forces());
        assert_eq!(a.step(0.001).unwrap(), b.step(0.001).unwrap());
        assert_eq!(a.x, b.x);
        assert_eq!(a.v, b.v);
    }
}

#[test]
fn failed_material_replacement_keeps_nonlinear_response_and_coefficients_atomic() {
    let mut m = model(&[sheet(1.0), sheet(4.0)]);
    m.set_membrane_responses(&[Some(nonlinear(1.0)), None])
        .unwrap();
    let before = format!("{m:?}");
    let (_, _, mut x) = geometry();
    x[1].x = 1.08;
    let energy = m.elastic_energy(&x);
    let mut underflow = sheet(2.0);
    // Passes material validation but fails mass assembly, after the first triangle is processed.
    underflow.areal_kg_m2 = f64::from_bits(1);
    for materials in [vec![sheet(2.0)], vec![sheet(2.0), underflow]] {
        assert!(m.set_materials(&materials).is_err());
        assert_eq!(format!("{m:?}"), before);
        assert_eq!(m.elastic_energy(&x), energy);
    }
}

#[test]
fn replacing_nonlinear_material_cannot_reuse_accepted_maxwell_history() {
    let mut selection: Selection = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-relaxation.json"
    ))
    .unwrap();
    let mut record = nonlinear_record();
    record.membrane_relaxation = selection
        .record
        .as_ref()
        .unwrap()
        .membrane_relaxation
        .clone();
    selection.id.clone_from(&record.id);
    selection.record = Some(record);
    let resolved = Catalog::builtin().unwrap().resolve(&selection).unwrap();
    let mechanics = resolved.mechanics().unwrap();
    let sheet = SheetMaterial {
        membrane: Membrane::orthotropic(
            mechanics.warp_n_m,
            mechanics.weft_n_m,
            mechanics.poisson,
            mechanics.shear_n_m,
            1.0,
        ),
        bending_nm: mechanics.bending_nm,
        areal_kg_m2: mechanics.areal_kg_m2,
    };
    let mut m = model(&[sheet; 2]);
    m.set_membrane_responses(&vec![resolved.membrane_response().unwrap(); 2])
        .unwrap();
    let (_, _, mut x) = geometry();
    x[1].x = 1.02;
    let mut solver = Solver::new(m, x, &[[true; 3]; 4], Vec3::ZERO)
        .with_relaxation(&vec![resolved.membrane_relaxation().unwrap(); 2])
        .unwrap();
    solver.step(0.01).unwrap();
    assert!(solver.relaxation_report().unwrap().is_some());
    // Identical scalar coefficients cannot hide removal of the old nonlinear source law.
    solver.model.set_materials(&[sheet; 2]).unwrap();
    let positions = solver.x.clone();
    let velocities = solver.v.clone();
    assert!(solver.relaxation_report().is_err());
    assert!(solver.step(0.01).is_err());
    assert_eq!(solver.x, positions);
    assert_eq!(solver.v, velocities);
}

#[test]
fn malformed_and_unrepresentable_fields_refuse() {
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let mut m = sheet(1.0);
        m.areal_kg_m2 = bad;
        assert!(m.validate().is_err());
    }
    let mut m = sheet(1.0);
    m.membrane.q[1][1] = -1.0;
    assert!(m.validate().is_err());
    m = sheet(1.0);
    m.bending_nm = 0.0;
    assert!(m.validate().is_ok());
    m.areal_kg_m2 = f64::MIN_POSITIVE;
    let local = [[Vec2::ZERO, Vec2::new(1e-100, 0.0), Vec2::new(0.0, 1e-100)]];
    assert!(ClothModel::from_material_triangles(3, &[[0, 1, 2]], &local, &[m]).is_err());
}

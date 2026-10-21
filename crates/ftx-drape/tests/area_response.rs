//! Physical-area membrane propagation and independent virtual-work checks; synthetic data only.

use std::collections::BTreeMap;

use ftx_core::{Vec2, Vec3};
use ftx_drape::{ClothModel, Membrane, MembraneElement, Solver, strip};
use ftx_pattern::{
    material_catalog::Selection,
    material_response::{AREA_MODEL, AreaResponse, CompiledMembrane},
    materials::{Evidence, Material, Quantity},
};

fn material(area: bool) -> Material {
    let mut record: Material = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-nonlinear.json"
    ))
    .unwrap();
    let response = record.membrane_response.as_mut().unwrap();
    let conditions = BTreeMap::from([
        ("specimen".into(), "synthetic-area-strip".into()),
        ("conditioning".into(), "synthetic reference state".into()),
    ]);
    for actual in [
        &mut response.warp.conditions,
        &mut response.weft.conditions,
        &mut response.shear.conditions,
        &mut response.coupling.conditions,
    ] {
        *actual = conditions.clone();
    }
    if area {
        let quantity = |value| Quantity {
            value,
            unit: "N/m".into(),
            evidence: Evidence::Estimate,
            source: "test".into(),
            method: "Synthetic physical-area coefficient".into(),
            bounds: None,
            conditions: conditions.clone(),
        };
        response.area_response = Some(AreaResponse {
            model: AREA_MODEL.into(),
            quadratic: quantity(40.0),
            log_squared: quantity(15.0),
            area_ratio_range: [0.6, 1.6],
            conditions,
        });
    }
    record
}

fn compiled(area: bool) -> CompiledMembrane {
    material(area).compiled_membrane(true).unwrap().unwrap()
}

fn membrane(law: &CompiledMembrane, compression: f64) -> Membrane {
    let [a, b, c, g] = law.origin_stiffness();
    Membrane {
        q: [[a, c, 0.0], [c, b, 0.0], [0.0, 0.0, g]],
        compression: [compression; 2],
        transition: 0.01,
    }
}

fn near(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance * (1.0 + expected.abs()),
        "actual {actual:e}, expected {expected:e}"
    );
}

fn element() -> MembraneElement {
    MembraneElement::new(
        [0, 1, 2],
        [Vec2::ZERO, Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0)],
    )
    .unwrap()
}

#[test]
fn element_area_acts_on_physical_metric_with_exact_spatial_derivatives() {
    let law = compiled(true);
    let base = compiled(false);
    let element = element();
    let positions = [
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(0.91, 0.04, 0.02),
        Vec3::new(0.02, 1.06, 0.03),
    ];
    for compression in [1.0, 0.01] {
        let m = membrane(&law, compression);
        let actual = element.response_local(&positions, &law, &m);
        let base_only = element.response_local(&positions, &base, &m);
        let physical = element.strain(&positions);
        let expected_area = law.evaluate_area(physical).unwrap();
        near(
            actual.energy - base_only.energy,
            element.area * expected_area.energy,
            1e-13,
        );
        assert!(
            !actual.psd,
            "area term cannot borrow the separable PSD shortcut"
        );
        near(
            actual.energy,
            element.response_energy(&positions, &law, &m),
            1e-13,
        );
        let h = 1e-6;
        for coordinate in 0..9 {
            let mut plus = positions;
            let mut minus = positions;
            let delta = match coordinate % 3 {
                0 => Vec3::new(h, 0.0, 0.0),
                1 => Vec3::new(0.0, h, 0.0),
                _ => Vec3::new(0.0, 0.0, h),
            };
            plus[coordinate / 3] += delta;
            minus[coordinate / 3] -= delta;
            let p = element.response_local(&plus, &law, &m);
            let n = element.response_local(&minus, &law, &m);
            near(
                (p.energy - n.energy) / (2.0 * h),
                actual.grad[coordinate],
                2e-7,
            );
            for component in 0..9 {
                near(
                    (p.grad[component] - n.grad[component]) / (2.0 * h),
                    actual.hess[component * 9 + coordinate],
                    2e-6,
                );
            }
        }
    }
}

#[test]
fn rigid_motion_preserves_area_energy_and_rotates_physical_forces() {
    let law = compiled(true);
    let m = membrane(&law, 0.1);
    let element = element();
    let x = [
        Vec3::ZERO,
        Vec3::new(1.1, 0.02, 0.03),
        Vec3::new(0.04, 0.95, 0.02),
    ];
    let rotate = |p: Vec3| Vec3::new(-p.y, p.z, -p.x);
    let transformed = x.map(|p| rotate(p) + Vec3::new(0.3, -0.4, 0.8));
    let first = element.response_local(&x, &law, &m);
    let second = element.response_local(&transformed, &law, &m);
    near(second.energy, first.energy, 1e-12);
    for i in 0..3 {
        let force = rotate(Vec3::new(
            first.grad[3 * i],
            first.grad[3 * i + 1],
            first.grad[3 * i + 2],
        ));
        for k in 0..3 {
            near(second.grad[3 * i + k], force[k], 1e-11);
        }
    }
}

#[test]
fn gathered_domain_refusal_preserves_solver_state() {
    let law = compiled(true);
    let mut model = ClothModel::new(
        vec![Vec2::ZERO, Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0)],
        &[[0, 1, 2]],
        membrane(&law, 0.01),
        0.0,
        1.0,
    )
    .unwrap();
    model.set_membrane_responses(&[Some(law)]).unwrap();
    let x = vec![
        Vec3::ZERO,
        Vec3::new(0.5, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
    ];
    let refusal = model.check_membrane_domains(&x).unwrap_err();
    assert!(refusal.message.contains("area-ratio"));
    assert!(model.elastic_energy(&x).is_nan());
    let mut solver = Solver::new(model, x.clone(), &[[true; 3]; 3], Vec3::ZERO);
    assert!(solver.step(0.001).is_err());
    assert_eq!(solver.x, x);
    assert_eq!(solver.v, vec![Vec3::ZERO; 3]);
}

fn request(area: bool, extension: f64) -> strip::Request {
    let record = material(area);
    strip::Request {
        schema: "ftx-strip-experiment/1".into(),
        material: Selection {
            id: record.id.clone(),
            record: Some(record),
            allow_estimates: true,
            ..Selection::default()
        },
        fixture: strip::Fixture {
            gauge_length_m: 0.1,
            width_m: 0.06,
            warp_axis: [0.0, 1.0],
            cells: [4, 6],
        },
        extensions: vec![extension],
        controls: strip::Controls::default(),
    }
}

#[test]
fn ordinary_frozen_strip_changes_contraction_and_reactions_with_virtual_work() {
    let reference = strip::evaluate(&request(false, 0.1)).unwrap();
    let actual = strip::evaluate(&request(true, 0.1)).unwrap();
    assert_ne!(reference.input_sha256, actual.input_sha256);
    assert_eq!(
        reference.states[0].positions_m.len(),
        actual.states[0].positions_m.len()
    );
    assert!(actual.states[0].mid_width_m < reference.states[0].mid_width_m - 1e-5);
    assert!((actual.states[0].force_n - reference.states[0].force_n).abs() > 1e-4);
    assert_eq!(
        serde_json::to_value(&actual).unwrap(),
        serde_json::to_value(strip::evaluate(&actual.input).unwrap()).unwrap()
    );
    let h = 1e-5;
    let before = strip::evaluate(&request(true, 0.1 - h)).unwrap();
    let after = strip::evaluate(&request(true, 0.1 + h)).unwrap();
    let derivative = (after.states[0].energy_j - before.states[0].energy_j) / (2.0 * h * 0.1);
    near(derivative, actual.states[0].force_n, 2e-6);
}

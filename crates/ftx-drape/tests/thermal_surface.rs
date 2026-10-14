//! Synthetic surface energy checks; no empirical textile or human comfort claims.

use ftx_core::Vec3;
use ftx_drape::thermal::{
    Boundary, DryThermalModel,
    surface::{Patch, Surface},
};
use ftx_pattern::material_catalog::Selection;
use ftx_pattern::materials::Catalog;
use serde_json::json;

fn column(id: &str, r: f64, capacity: f64) -> DryThermalModel {
    let s: Selection = serde_json::from_value(json!({
        "id":id,"allow_estimates":true,"record":{
            "id":id,"name":"Synthetic sheet","form":"woven",
            "sources":{"test":{"reference":"test:surface-energy","accessed_on":"2026-10-14",
                "note":"Synthetic known answer, not textile measurements"}},
            "properties":{
                "thermal_resistance":{"value":r,"unit":"m2*K/W","evidence":"estimate","source":"test","method":"synthetic"},
                "areal_density":{"value":0.2,"unit":"kg/m2","evidence":"estimate","source":"test","method":"synthetic"},
                "specific_heat":{"value":capacity/0.2,"unit":"J/(kg*K)","evidence":"estimate","source":"test","method":"synthetic"}
            }
        }
    })).unwrap();
    DryThermalModel::new(
        vec![Catalog::builtin().unwrap().resolve(&s).unwrap()],
        vec![],
        1,
    )
    .unwrap()
}
fn patch() -> Patch {
    Patch {
        vertices: [0, 1, 2],
        reference_area_m2: 0.5,
        column: 0,
    }
}
fn mesh(stretch: f64) -> Vec<Vec3> {
    vec![
        Vec3::ZERO,
        Vec3::new(stretch, 0., 0.),
        Vec3::new(0., 1., 0.),
    ]
}
fn boundary() -> Boundary {
    Boundary {
        inside_c: 30.,
        outside_c: 0.,
        inside_resistance: 0.1,
        outside_resistance: 0.2,
    }
}
fn surface(id: &str, r: f64, c: f64) -> Surface {
    Surface::new(vec![column(id, r, c)], 3, vec![patch()]).unwrap()
}
fn near(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol, "{a} != {b}");
}

#[test]
fn deformed_one_cell_matches_reference_mass_backward_euler() {
    let s = surface("a", 0.4, 200.);
    let old = s.initial(10.).unwrap();
    let g1 = 1. / 0.3;
    let g2 = 1. / 0.4;
    for j in [0.25, 1., 2., 4.] {
        let result = s.advance(&old, &mesh(j), 5., &[boundary()]).unwrap();
        let c = 200. * 0.5;
        let area = 0.5 * j;
        let expected = (c / 5. * 10. + area * g1 * 30.) / (c / 5. + area * (g1 + g2));
        near(result.state.temperature_c()[0][0], expected, 1e-12);
        near(result.stored_change_j, c * (expected - 10.), 1e-10);
        near(
            result.stored_change_j,
            5. * (result.inside_heat_w - result.outside_heat_w),
            1e-10,
        );
        near(result.balance_error_j, 0., 1e-10);
    }
    assert_eq!(old.temperature_c(), &[vec![10.]]);
}

#[test]
fn stretch_changes_exchange_area_not_total_material_heat_capacity() {
    let s = surface("a", 0.4, 200.);
    let equilibrium = s.steady(&mesh(1.), &[boundary()]).unwrap();
    let stretched = s.steady(&mesh(3.), &[boundary()]).unwrap();
    near(
        stretched.inside_heat_w,
        3. * equilibrium.inside_heat_w,
        1e-12,
    );
    assert_eq!(
        stretched.state.temperature_c(),
        equilibrium.state.temperature_c()
    );
    let mut old = s.initial(0.).unwrap();
    let mut stored = 0.;
    for n in 0..200 {
        // Change pose/area repeatedly, never remap or recreate the material temperature state.
        let result = s
            .advance(
                &old,
                &mesh(if n % 2 == 0 { 1. } else { 3. }),
                20.,
                &[boundary()],
            )
            .unwrap();
        stored += result.stored_change_j;
        old = result.state;
    }
    near(
        old.temperature_c()[0][0],
        equilibrium.state.temperature_c()[0][0],
        1e-9,
    );
    near(stored, 100. * old.temperature_c()[0][0], 1e-8);
}

#[test]
fn heterogeneous_patches_have_their_own_materials_and_boundaries() {
    let s = Surface::new(
        vec![column("shell", 0.4, 200.), column("bridge", 0.1, 50.)],
        4,
        vec![
            patch(),
            Patch {
                vertices: [1, 3, 2],
                reference_area_m2: 0.5,
                column: 1,
            },
        ],
    )
    .unwrap();
    let positions = vec![
        Vec3::ZERO,
        Vec3::new(1., 0., 0.),
        Vec3::new(0., 1., 0.),
        Vec3::new(1., 1., 0.),
    ];
    let outside = Boundary {
        outside_c: 10.,
        ..boundary()
    };
    let steady = s.steady(&positions, &[boundary(), outside]).unwrap();
    near(steady.patches[0].inside_flux_w_m2, 30. / 0.7, 1e-12);
    near(steady.patches[1].inside_flux_w_m2, 20. / 0.4, 1e-12);
    near(steady.inside_heat_w, 0.5 * (30. / 0.7 + 20. / 0.4), 1e-12);
    let result = s
        .advance(
            &s.initial(5.).unwrap(),
            &positions,
            2.,
            &[boundary(), outside],
        )
        .unwrap();
    assert_ne!(
        result.state.temperature_c()[0],
        result.state.temperature_c()[1]
    );
    near(result.balance_error_j, 0., 1e-9);
}

#[test]
fn explicit_series_layers_share_one_area_and_retain_all_their_capacity() {
    let a = column("lining", 0.1, 50.);
    let b = column("fill", 0.5, 300.);
    let catalog = Catalog::builtin().unwrap();
    let layers = [&a, &b].into_iter().map(|m| {
        let receipt = &m.snapshot()["layers"][0];
        let s: Selection = serde_json::from_value(json!({"id":receipt["authored"]["id"],
            "record":receipt["authored"],"allow_estimates":true,"expected_sha256":receipt["sha256"]})).unwrap();
        catalog.resolve(&s).unwrap()
    }).collect();
    let stack = DryThermalModel::new(layers, vec![0.2], 3).unwrap();
    let s = Surface::new(vec![stack], 3, vec![patch()]).unwrap();
    let out = s.steady(&mesh(1.), &[boundary()]).unwrap();
    near(out.area_m2, 0.5, 1e-15);
    near(out.inside_heat_w, 0.5 * 30. / 1.1, 1e-12);
    assert_eq!(out.state.temperature_c()[0].len(), 6);
    let step = s
        .advance(&s.initial(0.).unwrap(), &mesh(2.), 1., &[boundary()])
        .unwrap();
    let independent: f64 = step.state.temperature_c()[0]
        .iter()
        .enumerate()
        .map(|(i, t)| 0.5 * (if i < 3 { 50. } else { 300. }) / 3. * t)
        .sum();
    near(step.stored_change_j, independent, 1e-9);
    near(step.balance_error_j, 0., 1e-9);
}

#[test]
fn rigid_motion_is_objective_and_material_substitution_invalidates_old_state() {
    let s = surface("a", 0.4, 200.);
    let old = s.initial(10.).unwrap();
    let a = s.advance(&old, &mesh(1.), 1., &[boundary()]).unwrap();
    let moved: Vec<_> = mesh(1.)
        .iter()
        .map(|p| Vec3::new(4. + p.z, 2. + p.x, -3. + p.y))
        .collect();
    let b = s.advance(&old, &moved, 1., &[boundary()]).unwrap();
    assert_eq!(a.state.temperature_c(), b.state.temperature_c());
    assert_eq!(a.inside_heat_w, b.inside_heat_w);
    for other in [
        surface("a", 0.8, 200.),
        surface("a", 0.4, 400.),
        surface("other-source", 0.4, 200.),
    ] {
        assert_ne!(s.snapshot()["sha256"], other.snapshot()["sha256"]);
        assert!(other.advance(&old, &mesh(1.), 1., &[boundary()]).is_err());
    }
}

#[test]
fn malformed_meshes_steps_and_boundaries_refuse_without_changing_state() {
    let s = surface("a", 0.4, 200.);
    let old = s.initial(10.).unwrap();
    for mesh in [mesh(0.), mesh(f64::NAN), mesh(f64::MAX), vec![]] {
        assert!(s.advance(&old, &mesh, 1., &[boundary()]).is_err());
    }
    for dt in [0., -1., f64::NAN, f64::INFINITY] {
        assert!(s.advance(&old, &mesh(1.), dt, &[boundary()]).is_err());
    }
    assert!(s.steady(&mesh(1.), &[]).is_err());
    assert!(
        s.steady(
            &mesh(1.),
            &[Boundary {
                inside_c: -274.,
                ..boundary()
            }]
        )
        .is_err()
    );
    for p in [
        Patch {
            vertices: [0, 0, 1],
            ..patch()
        },
        Patch {
            vertices: [0, 1, 3],
            ..patch()
        },
        Patch {
            column: 1,
            ..patch()
        },
        Patch {
            reference_area_m2: 0.,
            ..patch()
        },
    ] {
        assert!(Surface::new(vec![column("a", 0.4, 200.)], 3, vec![p]).is_err());
    }
    assert!(
        Surface::new(
            vec![column("a", 0.4, 200.)],
            3,
            vec![
                patch(),
                Patch {
                    vertices: [2, 1, 0],
                    ..patch()
                }
            ]
        )
        .is_err()
    );
    assert!(Surface::new(vec![], 3, vec![patch()]).is_err());
    assert!(
        Surface::new(
            vec![column("a", 0.4, 200.), column("b", 0.4, 200.)],
            4,
            vec![
                patch(),
                Patch {
                    vertices: [1, 3, 2],
                    ..patch()
                }
            ]
        )
        .is_err()
    );
    assert_eq!(old.temperature_c(), &[vec![10.]]);
}

//! Independent network/enthalpy answers for declared coverage, not empirical garment validation.
use ftx_core::Vec3;
use ftx_drape::thermal::{
    Boundary, DryThermalModel,
    surface::{
        Patch, Surface,
        coverage::{Coverage, Layout, Portion, Region},
    },
};
use ftx_pattern::{material_catalog::Selection, materials::Catalog};
use serde_json::{Value, json};

fn column(r: f64, c: f64) -> DryThermalModel {
    let q = |v, u| json!({"value":v,"unit":u,"evidence":"estimate","source":"test","method":"synthetic known answer"});
    let s: Selection = serde_json::from_value(json!({"id":"test","allow_estimates":true,"record":{
        "id":"test","name":"Synthetic, not a commercial fabric","form":"woven",
        "properties":{"thermal_resistance":q(r,"m2*K/W"),"areal_density":q(0.2,"kg/m2"),"specific_heat":q(c/0.2,"J/(kg*K)")},
        "sources":{"test":{"reference":"test:coverage","accessed_on":"2026-10-14","note":"Synthetic inputs only"}}}})).unwrap();
    DryThermalModel::new(
        vec![Catalog::builtin().unwrap().resolve(&s).unwrap()],
        vec![],
        1,
    )
    .unwrap()
}
fn source() -> Surface {
    Surface::new(
        vec![column(0.1, 100.0), column(0.4, 200.0)],
        6,
        vec![
            Patch {
                vertices: [0, 1, 2],
                reference_area_m2: 0.5,
                column: 0,
            },
            Patch {
                vertices: [3, 4, 5],
                reference_area_m2: 0.5,
                column: 1,
            },
        ],
    )
    .unwrap()
}
fn x() -> Vec<Vec3> {
    vec![
        Vec3::ZERO,
        Vec3::new(1., 0., 0.),
        Vec3::new(0., 1., 0.),
        Vec3::new(0., 0., 0.01),
        Vec3::new(1., 0., 0.01),
        Vec3::new(0., 1., 0.01),
    ]
}
fn b() -> Boundary {
    Boundary {
        inside_c: 30.,
        outside_c: 0.,
        inside_resistance: 0.1,
        outside_resistance: 0.2,
    }
}
fn layout() -> Layout {
    Layout {
        schema: "ftx-thermal-coverage-layout/1".into(),
        mapping_basis: "Synthetic declared disjoint coverage".into(),
        body_vertex_count: 3,
        body_triangles: vec![[0, 1, 2]],
        cells_per_layer: 1,
        regions: vec![Region {
            id: "front".into(),
            body_triangle: 0,
            body_fraction: 1.,
            layers: vec![
                Portion {
                    layer: "lining".into(),
                    cloth_triangle: 0,
                    fraction: 1.,
                },
                Portion {
                    layer: "shell".into(),
                    cloth_triangle: 1,
                    fraction: 1.,
                },
            ],
            interface_resistance_m2_k_w: vec![0.0],
        }],
    }
}
fn near(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() < tol, "{a} != {b}");
}

#[test]
fn layers_are_series_connected_and_body_area_is_counted_once() {
    let s = source();
    let uncoupled = s.steady(&x(), &[b(); 2]).unwrap();
    let c = Coverage::new(s, layout()).unwrap();
    let r = c.steady(&x(), &x()[..3], &[b()]).unwrap();
    near(r.inside_heat_w, 0.5 * 30. / (0.1 + 0.1 + 0.4 + 0.2), 1e-10);
    near(r.inside_heat_w, r.outside_heat_w, 1e-12);
    assert_eq!(r.body_area_m2, 0.5);
    assert_eq!(r.covered_area_m2, 0.5);
    assert_eq!(r.bare_heat_w, 0.);
    assert_eq!(r.unassigned_cloth_reference_area_m2, 0.);
    assert!(uncoupled.inside_heat_w > 3. * r.inside_heat_w);
    assert_eq!(r.state.temperature_c()[0].len(), 2);
    assert!(r.state.temperature_c()[0][0] > r.state.temperature_c()[0][1]);
}

#[test]
fn partial_coverage_routes_exposed_area_through_declared_films() {
    let mut l = layout();
    l.regions[0].body_fraction = 0.5;
    for p in &mut l.regions[0].layers {
        p.fraction = 0.5;
    }
    let c = Coverage::new(source(), l).unwrap();
    let r = c.steady(&x(), &x()[..3], &[b()]).unwrap();
    near(r.bare_heat_w, 0.25 * 30. / 0.3, 1e-12);
    near(r.inside_heat_w, 0.25 * 30. / 0.8 + 0.25 * 30. / 0.3, 1e-10);
    assert_eq!(r.covered_area_m2, 0.25);
    assert_eq!(r.unassigned_cloth_reference_area_m2, 0.5);
    let advanced = c
        .advance(&c.initial(10.).unwrap(), &x(), &x()[..3], 5., &[b()])
        .unwrap();
    near(advanced.stored_change_j, 5. * advanced.net_heat_w, 1e-8);
    near(advanced.balance_error_j, 0., 1e-8);
}

#[test]
fn splitting_a_region_preserves_total_area_heat_and_reference_storage() {
    let whole = Coverage::new(source(), layout()).unwrap();
    let mut l = layout();
    l.regions[0].body_fraction = 0.25;
    for p in &mut l.regions[0].layers {
        p.fraction = 0.25;
    }
    let mut remainder = l.regions[0].clone();
    remainder.id = "remainder".into();
    remainder.body_fraction = 0.75;
    for p in &mut remainder.layers {
        p.fraction = 0.75;
    }
    l.regions.push(remainder);
    let split = Coverage::new(source(), l).unwrap();
    let a = whole
        .advance(&whole.initial(10.).unwrap(), &x(), &x()[..3], 5., &[b()])
        .unwrap();
    let b = split
        .advance(&split.initial(10.).unwrap(), &x(), &x()[..3], 5., &[b()])
        .unwrap();
    near(a.inside_heat_w, b.inside_heat_w, 1e-10);
    near(a.stored_change_j, b.stored_change_j, 1e-9);
    assert_eq!(a.body_area_m2, b.body_area_m2);
    assert_eq!(a.covered_area_m2, b.covered_area_m2);
    assert_eq!(a.state.temperature_c()[0], b.state.temperature_c()[0]);
    assert_eq!(a.state.temperature_c()[0], b.state.temperature_c()[1]);
}

#[test]
fn layer_current_areas_control_transport_and_reference_areas_control_storage() {
    let mut l = layout();
    l.regions[0].layers.remove(0);
    l.regions[0].interface_resistance_m2_k_w.clear();
    let c = Coverage::new(source(), l).unwrap();
    for (body_stretch, cloth_stretch) in [(1., 2.), (3., 0.5), (0.25, 4.)] {
        let mut cloth = x();
        cloth[4].x *= cloth_stretch;
        let mut body = x()[..3].to_vec();
        body[1].x *= body_stretch;
        let a_body = 0.5 * body_stretch;
        let a_cloth = 0.5 * cloth_stretch;
        // Independent W/K network: film paths use body area; sheet paths use sheet area.
        let gi = 1. / (0.1 / a_body + 0.2 / a_cloth);
        let go = 1. / (0.2 / a_body + 0.2 / a_cloth);
        let expected = (100. / 5. * 10. + gi * 30.) / (100. / 5. + gi + go);
        let r = c
            .advance(&c.initial(10.).unwrap(), &cloth, &body, 5., &[b()])
            .unwrap();
        near(r.state.temperature_c()[0][0], expected, 1e-10);
        near(r.stored_change_j, 100. * (expected - 10.), 1e-8);
        near(r.balance_error_j, 0., 1e-8);
    }
}

#[test]
fn source_sheet_can_cover_multiple_body_triangles_without_duplicating_mass() {
    let mut l = layout();
    l.body_vertex_count = 4;
    l.body_triangles = vec![[0, 1, 2], [1, 3, 2]];
    for p in &mut l.regions[0].layers {
        p.fraction = 0.5;
    }
    let mut second = l.regions[0].clone();
    second.id = "second".into();
    second.body_triangle = 1;
    l.regions.push(second);
    let c = Coverage::new(source(), l).unwrap();
    let body = [
        Vec3::ZERO,
        Vec3::new(0.5, 0., 0.),
        Vec3::new(0., 1., 0.),
        Vec3::new(0.5, 1., 0.),
    ];
    let boundaries = [
        b(),
        Boundary {
            inside_c: 20.,
            ..b()
        },
    ];
    let r = c.steady(&x(), &body, &boundaries).unwrap();
    near(r.inside_heat_w, 0.25 * 30. / 0.8 + 0.25 * 20. / 0.8, 1e-10);
    assert_eq!(r.covered_area_m2, 0.5);
    assert_eq!(r.unassigned_cloth_reference_area_m2, 0.);
    let r = c
        .advance(&c.initial(10.).unwrap(), &x(), &body, 5., &boundaries)
        .unwrap();
    let expected: f64 = r
        .state
        .temperature_c()
        .iter()
        .map(|t| 25. * (t[0] - 10.) + 50. * (t[1] - 10.))
        .sum();
    near(r.stored_change_j, expected, 1e-8);
    near(r.balance_error_j, 0., 1e-8);
}

#[test]
fn material_substitution_rebuilds_physics_and_refuses_old_state_or_tampered_snapshots() {
    let a = Coverage::new(source(), layout()).unwrap();
    let s = Surface::new(
        vec![column(0.1, 100.), column(0.8, 200.)],
        6,
        vec![
            Patch {
                vertices: [0, 1, 2],
                reference_area_m2: 0.5,
                column: 0,
            },
            Patch {
                vertices: [3, 4, 5],
                reference_area_m2: 0.5,
                column: 1,
            },
        ],
    )
    .unwrap();
    let changed = Coverage::new(s, layout()).unwrap();
    assert_ne!(a.snapshot()["sha256"], changed.snapshot()["sha256"]);
    assert!(
        changed
            .steady(&x(), &x()[..3], &[b()])
            .unwrap()
            .inside_heat_w
            < a.steady(&x(), &x()[..3], &[b()]).unwrap().inside_heat_w
    );
    assert!(
        changed
            .advance(&a.initial(10.).unwrap(), &x(), &x()[..3], 1., &[b()])
            .is_err()
    );
    let saved = serde_json::to_string(&a.snapshot()).unwrap();
    let restored = Coverage::from_snapshot(&serde_json::from_str(&saved).unwrap()).unwrap();
    assert_eq!(a.snapshot(), restored.snapshot());
    let r = restored
        .advance(&a.initial(10.).unwrap(), &x(), &x()[..3], 5., &[b()])
        .unwrap();
    let expected = a
        .advance(&a.initial(10.).unwrap(), &x(), &x()[..3], 5., &[b()])
        .unwrap();
    assert_eq!(r.state.temperature_c(), expected.state.temperature_c());
    let mut tampered = a.snapshot();
    tampered["layout"]["regions"][0]["layers"][0]["fraction"] = json!(0.5);
    assert!(Coverage::from_snapshot(&tampered).is_err());
}

#[test]
fn allocation_and_order_refusals_are_structural_not_clamped() {
    let changes: Vec<fn(&mut Layout)> = vec![
        |l| l.regions[0].body_fraction = 1.1,
        |l| l.regions[0].body_fraction = 0.,
        |l| l.regions[0].layers[0].fraction = f64::NAN,
        |l| l.regions[0].layers[0].fraction = 1.1,
        |l| l.regions[0].layers[0].cloth_triangle = 999,
        |l| l.regions[0].body_triangle = 999,
        |l| l.regions[0].layers[1].layer = "lining".into(),
        |l| l.regions[0].layers[1].cloth_triangle = 0,
        |l| l.regions[0].interface_resistance_m2_k_w.clear(),
        |l| l.regions[0].interface_resistance_m2_k_w[0] = -1.,
        |l| l.cells_per_layer = 4096,
        |l| l.regions.push(l.regions[0].clone()),
        |l| l.body_triangles.push([2, 1, 0]),
        |l| l.body_triangles[0] = [0, 0, 1],
        |l| l.mapping_basis.clear(),
        |l| l.schema = "typo".into(),
    ];
    for change in changes {
        let mut l = layout();
        change(&mut l);
        assert!(Coverage::new(source(), l).is_err());
    }
    let mut l = layout();
    l.regions[0].body_fraction = 0.5;
    // Two regions claiming all of the same cloth duplicate mass despite a valid body area sum.
    let mut r = l.regions[0].clone();
    r.id = "other".into();
    l.regions.push(r);
    assert!(
        Coverage::new(source(), l)
            .unwrap_err()
            .to_string()
            .contains("mass")
    );
    let mut l = layout();
    l.regions[0].body_fraction = 0.5;
    for p in &mut l.regions[0].layers {
        p.fraction = 0.5;
    }
    let mut r = l.regions[0].clone();
    r.id = "opposite-order".into();
    r.layers.reverse();
    l.regions.push(r);
    assert!(
        Coverage::new(source(), l)
            .unwrap_err()
            .to_string()
            .contains("cyclic")
    );
}

#[test]
fn invalid_poses_films_time_and_state_fail_without_mutating_input() {
    let c = Coverage::new(source(), layout()).unwrap();
    let old = c.initial(10.).unwrap();
    let mut collapsed = x();
    collapsed[1] = collapsed[0];
    assert!(c.advance(&old, &collapsed, &x()[..3], 1., &[b()]).is_err());
    assert!(c.advance(&old, &x(), &x()[..2], 1., &[b()]).is_err());
    assert!(c.advance(&old, &x(), &x()[..3], 1., &[]).is_err());
    assert!(c.advance(&old, &x(), &x()[..3], 0., &[b()]).is_err());
    assert!(
        c.steady(
            &x(),
            &x()[..3],
            &[Boundary {
                inside_c: f64::NAN,
                ..b()
            }]
        )
        .is_err()
    );
    assert_eq!(old.temperature_c(), &[vec![10., 10.]]);
    let mut l = layout();
    l.regions.clear();
    let bare = Coverage::new(source(), l).unwrap();
    let r = bare.steady(&x(), &x()[..3], &[b()]).unwrap();
    near(r.inside_heat_w, 50., 1e-12);
    assert_eq!(r.covered_area_m2, 0.);
    assert_eq!(r.unassigned_cloth_reference_area_m2, 1.);
    assert!(
        bare.steady(
            &x(),
            &x()[..3],
            &[Boundary {
                inside_resistance: 0.,
                outside_resistance: 0.,
                ..b()
            }]
        )
        .is_err()
    );
}

#[test]
fn temperature_dependent_enthalpy_survives_changing_body_and_cloth_area() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-variable-thermal.json"
    ))
    .unwrap();
    let selection: Selection = serde_json::from_value(fixture["layers"][0].clone()).unwrap();
    let variable = DryThermalModel::new(
        vec![Catalog::builtin().unwrap().resolve(&selection).unwrap()],
        vec![],
        1,
    )
    .unwrap();
    let s = Surface::new(
        vec![variable],
        3,
        vec![Patch {
            vertices: [0, 1, 2],
            reference_area_m2: 0.5,
            column: 0,
        }],
    )
    .unwrap();
    let mut l = layout();
    l.regions[0].layers.truncate(1);
    l.regions[0].interface_resistance_m2_k_w.clear();
    let c = Coverage::new(s, l).unwrap();
    let mut state = c.initial(10.).unwrap();
    let mut stored = 0.;
    let mut net = 0.;
    for i in 0..30 {
        let mut cloth = x()[..3].to_vec();
        cloth[1].x = if i % 2 == 0 { 2. } else { 0.5 };
        let mut body = x()[..3].to_vec();
        body[1].x = if i % 3 == 0 { 0.75 } else { 1.5 };
        let r = c.advance(&state, &cloth, &body, 1., &[b()]).unwrap();
        stored += r.stored_change_j;
        net += r.net_heat_w;
        state = r.state;
    }
    let t = state.temperature_c()[0][0];
    // Independently integrate Cp(T)=1000+20*T over the unchanged 0.1 kg reference material.
    let expected = 100. * (t - 10.) + (t * t - 100.);
    near(stored, expected, 1e-7);
    near(net, expected, 1e-6);
    assert!(c.initial(-1.).is_err());
    assert!(
        c.steady(
            &x()[..3],
            &x()[..3],
            &[Boundary {
                outside_c: -1.,
                ..b()
            }]
        )
        .is_err()
    );
}

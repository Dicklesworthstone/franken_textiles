//! Independent geometric/enthalpy answers for moving material, not empirical textile validation.
use ftx_core::Vec3;
use ftx_drape::thermal::{
    Boundary, DryThermalModel,
    surface::{
        Patch, Surface,
        coverage::motion::Frame,
        projection::{Request, Settings},
    },
};
use ftx_pattern::{material_catalog::Selection, materials::Catalog};
use serde_json::Value;

fn v(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3::new(x, y, z)
}
fn tri(z: f64) -> Vec<Vec3> {
    vec![v(0., 0., z), v(1., 0., z), v(0., 1., z)]
}
fn selection() -> Selection {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-variable-thermal.json"
    ))
    .unwrap();
    serde_json::from_value(fixture["layers"][0].clone()).unwrap()
}
fn source(selected: &Selection) -> Surface {
    Surface::new(
        vec![
            DryThermalModel::new(
                vec![Catalog::builtin().unwrap().resolve(selected).unwrap()],
                vec![],
                1,
            )
            .unwrap(),
        ],
        3,
        vec![Patch {
            vertices: [0, 1, 2],
            reference_area_m2: 0.5,
            column: 0,
        }],
    )
    .unwrap()
}
fn frame(
    source: Surface,
    cloth: Vec<Vec3>,
    body: Vec<Vec3>,
    faces: &[[usize; 3]],
    cells: usize,
) -> Frame {
    Frame::new(
        source,
        cloth,
        body,
        faces,
        Request {
            schema: "ftx-thermal-projection/1".into(),
            cloth_layers: vec!["shell".into()],
            settings: Settings {
                max_distance_m: 0.2,
                min_normal_cosine: 0.2,
                separation_tolerance_m: 1e-8,
                cells_per_layer: cells,
                interfaces: vec![],
            },
        },
    )
    .unwrap()
}
fn one(cells: usize) -> Frame {
    frame(
        source(&selection()),
        tri(0.04),
        tri(0.),
        &[[0, 1, 2]],
        cells,
    )
}
fn split(cells: usize) -> Frame {
    let mut body = tri(0.);
    body.push(v(0.5, 0., 0.));
    frame(
        source(&selection()),
        tri(0.04),
        body,
        &[[0, 3, 2], [3, 1, 2]],
        cells,
    )
}
fn boundary(t: f64) -> Boundary {
    Boundary {
        inside_c: t,
        outside_c: t,
        inside_resistance: 0.1,
        outside_resistance: 0.2,
    }
}
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 2e-9 * (1. + b.abs()), "{a} != {b}");
}
// Independent primitive of rho_A * Cp(T), rho_A=0.2 and Cp=1000+20*T.
fn h(t: f64) -> f64 {
    200. * t + 2. * t * t
}
fn mix(a: f64, b: f64, w: f64) -> f64 {
    let target = w * h(a) + (1. - w) * h(b);
    2. * target / (200. + (40_000. + 8. * target).sqrt())
}

#[test]
fn identity_preserves_every_temperature_without_advancing_time() {
    let f = split(3);
    let initial = f
        .steady(&[boundary(10.), boundary(30.)])
        .unwrap()
        .into_state();
    let transfer = f.remap(&f, &initial).unwrap();
    assert_eq!(transfer.state.temperature_c(), initial.temperature_c());
    assert_eq!(transfer.balance.residual_j, 0.);
    assert_eq!(transfer.balance.absolute_residual_j, 0.);
    near(transfer.balance.reference_area_m2, 0.5);
}

#[test]
fn coarsening_mixes_enthalpy_not_temperature_and_refinement_does_not_invent_gradients() {
    let old = split(1);
    let new = one(1);
    let initial = old
        .steady(&[boundary(10.), boundary(30.)])
        .unwrap()
        .into_state();
    let transfer = new.remap(&old, &initial).unwrap();
    let t = transfer.state.temperature_c()[0][0];
    near(t, mix(10., 30., 0.5));
    assert!((t - 20.).abs() > 0.5);
    // Half a square metre, each half initially at a different temperature.
    near(0.5 * h(t), 0.25 * h(10.) + 0.25 * h(30.));
    near(transfer.balance.before_j, 0.25 * (h(30.) - h(10.)));
    near(transfer.balance.after_j, transfer.balance.before_j);
    let refined = old.remap(&new, &transfer.state).unwrap();
    for region in refined.state.temperature_c() {
        near(region[0], t);
    }
    // Coarsening irreversibly mixes the unresolved gradient; remapping back is not an inverse.
    assert_ne!(refined.state.temperature_c(), initial.temperature_c());
}

#[test]
fn through_thickness_two_to_three_cells_conserves_integrated_heat() {
    let old = one(2);
    let new = one(3);
    let b = Boundary {
        inside_c: 40.,
        outside_c: 0.,
        ..boundary(0.)
    };
    let initial = old.steady(&[b]).unwrap().into_state();
    let before = &initial.temperature_c()[0];
    let transfer = new.remap(&old, &initial).unwrap();
    let after = &transfer.state.temperature_c()[0];
    near(after[0], before[0]);
    near(after[1], mix(before[0], before[1], 0.5));
    near(after[2], before[1]);
    near(
        after.iter().copied().map(h).sum::<f64>() / 3.,
        before.iter().copied().map(h).sum::<f64>() / 2.,
    );
    near(transfer.balance.residual_j, 0.);
    let step = new.advance(&transfer.state, 0.8, &[b]).unwrap();
    near(step.heat().balance_error_j, 0.);
    let actual = step.heat().state.temperature_c()[0]
        .iter()
        .zip(after)
        .map(|(&a, &b)| 0.5 / 3. * (h(a) - h(b)))
        .sum::<f64>();
    near(step.heat().stored_change_j, actual);
}

#[test]
fn changing_the_body_side_reverses_depth_but_does_not_swap_material_heat() {
    let old = one(3);
    let new = frame(source(&selection()), tri(0.04), tri(0.08), &[[0, 2, 1]], 3);
    let state = old
        .steady(&[Boundary {
            inside_c: 40.,
            outside_c: 0.,
            ..boundary(0.)
        }])
        .unwrap()
        .into_state();
    let transferred = new.remap(&old, &state).unwrap();
    let mut reversed = state.temperature_c()[0].clone();
    reversed.reverse();
    assert_eq!(transferred.state.temperature_c()[0], reversed);
    near(transferred.balance.residual_j, 0.);
}

#[test]
fn moving_cloth_keeps_material_heat_not_the_temperature_of_its_new_body_triangle() {
    // A flat boundary has a diagonal partition. Move the entire cloth from its cold side onto
    // the hot side; geometry changes but the material triangle and its reference mass do not.
    let body = vec![
        v(-1., -1., 0.),
        v(3., -1., 0.),
        v(3., 3., 0.),
        v(-1., 3., 0.),
    ];
    let faces = [[0, 1, 2], [0, 2, 3]];
    let old = frame(source(&selection()), tri(0.04), body.clone(), &faces, 1);
    let moved = tri(0.04).into_iter().map(|p| p + v(1.2, 0., 0.)).collect();
    let new = frame(source(&selection()), moved, body, &faces, 1);
    let state = old
        .steady(&[boundary(10.), boundary(30.)])
        .unwrap()
        .into_state();
    let transfer = new.remap(&old, &state).unwrap();
    near(transfer.balance.before_j, transfer.balance.after_j);
    assert_eq!(new.projection().layout.regions.len(), 1);
    near(transfer.state.temperature_c()[0][0], mix(10., 30., 0.5));
    assert!((transfer.state.temperature_c()[0][0] - 10.).abs() > 5.);
    // Physical cooling only happens when time is advanced after the instantaneous remap.
    let next = new
        .advance(&transfer.state, 2., &[boundary(10.), boundary(30.)])
        .unwrap();
    assert!(next.heat().stored_change_j < 0.);
    near(next.heat().balance_error_j, 0.);
}

#[test]
fn equal_area_disjoint_material_domains_refuse_instead_of_exchanging_unrelated_temperatures() {
    let old = frame(
        source(&selection()),
        tri(0.04),
        vec![v(0., 0., 0.), v(0.5, 0., 0.), v(0., 1., 0.)],
        &[[0, 1, 2]],
        1,
    );
    let new = frame(
        source(&selection()),
        tri(0.04),
        vec![v(0.5, 0., 0.), v(1., 0., 0.), v(0., 1., 0.)],
        &[[0, 1, 2]],
        1,
    );
    let state = old.initial(20.).unwrap();
    let err = new.remap(&old, &state).unwrap_err();
    assert!(err.to_string().contains("material domain changed"), "{err}");
    assert_eq!(state.temperature_c(), &[vec![20.]]);
}

#[test]
fn entering_and_leaving_cloth_both_refuse_atomically() {
    let old = one(1);
    let new = frame(
        source(&selection()),
        tri(0.04),
        vec![v(0., 0., 0.), v(0.5, 0., 0.), v(0., 1., 0.)],
        &[[0, 1, 2]],
        1,
    );
    assert!(new.remap(&old, &old.initial(20.).unwrap()).is_err());
    assert!(old.remap(&new, &new.initial(20.).unwrap()).is_err());
}

#[test]
fn source_or_frame_changes_cannot_silently_reuse_a_state() {
    let old = one(1);
    let state = old.initial(20.).unwrap();
    let mut material = selection();
    material
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("areal_density")
        .unwrap()
        .value = 0.3;
    let changed = frame(source(&material), tri(0.04), tri(0.), &[[0, 1, 2]], 1);
    assert!(changed.remap(&old, &state).is_err());
    let moved = frame(source(&selection()), tri(0.05), tri(0.), &[[0, 1, 2]], 1);
    // Same scalar coverage layout, different geometric receipt: explicit remap is still mandatory.
    assert!(moved.advance(&state, 1., &[boundary(20.)]).is_err());
    assert!(old.remap(&moved, &state).is_err());
    let valid = moved.remap(&old, &state).unwrap();
    assert!(moved.advance(&valid.state, 1., &[boundary(20.)]).is_ok());
}

#[test]
fn repeated_remaps_are_deterministic_and_keep_uniform_temperatures_exact() {
    let old = split(2);
    let new = one(5);
    let state = old.initial(17.25).unwrap();
    let a = new.remap(&old, &state).unwrap();
    let b = new.remap(&old, &state).unwrap();
    assert_eq!(
        serde_json::to_value(&a.balance).unwrap(),
        serde_json::to_value(&b.balance).unwrap()
    );
    assert_eq!(a.state.temperature_c(), b.state.temperature_c());
    assert!(
        a.state
            .temperature_c()
            .iter()
            .flatten()
            .all(|t| *t == 17.25)
    );
    let returned = old.remap(&new, &a.state).unwrap();
    assert_eq!(returned.state.temperature_c(), state.temperature_c());
}

#[test]
fn body_retessellation_and_cells_may_change_together_without_adding_energy() {
    let old = one(2);
    let new = split(3);
    let b = Boundary {
        inside_c: 40.,
        outside_c: 0.,
        ..boundary(0.)
    };
    let state = old.steady(&[b]).unwrap().into_state();
    let result = new.remap(&old, &state).unwrap();
    near(result.balance.absolute_residual_j, 0.);
    let hi = state.temperature_c()[0]
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    let lo = state.temperature_c()[0]
        .iter()
        .copied()
        .fold(f64::INFINITY, f64::min);
    assert!(
        result
            .state
            .temperature_c()
            .iter()
            .flatten()
            .all(|t| *t >= lo && *t <= hi)
    );
    assert_eq!(
        result.state.temperature_c()[0],
        result.state.temperature_c()[1]
    );
}

#[test]
fn different_materials_keep_their_own_enthalpy_laws_during_a_layered_remap() {
    use ftx_drape::thermal::surface::projection::Interface;
    let a = selection();
    let mut b = a.clone();
    let material = b.record.as_mut().unwrap();
    material.thermal_response.as_mut().unwrap().specific_heat = None;
    let mut cp = material.properties["areal_density"].clone();
    cp.value = 2000.;
    cp.unit = "J/(kg*K)".into();
    material.properties.insert("specific_heat".into(), cp);
    let db = Catalog::builtin().unwrap();
    let src = Surface::new(
        vec![
            DryThermalModel::new(vec![db.resolve(&a).unwrap()], vec![], 1).unwrap(),
            DryThermalModel::new(vec![db.resolve(&b).unwrap()], vec![], 1).unwrap(),
        ],
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
    let mut cloth = tri(0.04);
    cloth.extend(tri(0.08));
    let request = Request {
        schema: "ftx-thermal-projection/1".into(),
        cloth_layers: vec!["lining".into(), "shell".into()],
        settings: Settings {
            max_distance_m: 0.2,
            min_normal_cosine: 0.2,
            separation_tolerance_m: 1e-8,
            cells_per_layer: 1,
            interfaces: vec![Interface {
                inner: "lining".into(),
                outer: "shell".into(),
                resistance_m2_k_w: 0.05,
                basis: "synthetic".into(),
            }],
        },
    };
    let mut body = tri(0.);
    body.push(v(0.5, 0., 0.));
    let old = Frame::new(
        src.clone(),
        cloth.clone(),
        body,
        &[[0, 3, 2], [3, 1, 2]],
        request.clone(),
    )
    .unwrap();
    let new = Frame::new(src, cloth, tri(0.), &[[0, 1, 2]], request).unwrap();
    let state = old
        .steady(&[boundary(10.), boundary(30.)])
        .unwrap()
        .into_state();
    let transfer = new.remap(&old, &state).unwrap();
    near(transfer.state.temperature_c()[0][0], mix(10., 30., 0.5));
    near(transfer.state.temperature_c()[0][1], 20.);
    near(transfer.balance.absolute_residual_j, 0.);
    near(transfer.balance.reference_area_m2, 1.);
}

#[test]
fn nonuniform_identity_retains_cell_order_at_non_binary_depth_boundaries() {
    for n in [3, 7, 11, 31] {
        let f = one(n);
        let state = f
            .steady(&[Boundary {
                inside_c: 40.,
                outside_c: 0.,
                ..boundary(0.)
            }])
            .unwrap()
            .into_state();
        let result = f.remap(&f, &state).unwrap();
        for (&a, &b) in result.state.temperature_c()[0]
            .iter()
            .zip(&state.temperature_c()[0])
        {
            near(a, b);
        }
        near(result.balance.absolute_residual_j, 0.);
    }
}

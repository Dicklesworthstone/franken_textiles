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
fn fully_covered_overlap_roundoff_preserves_enthalpy_without_an_exposed_donor() {
    // One triangle reduced from the accepted-motion regression. Both projected cloth triangles
    // lie wholly inside the body face. Independent inverse projections round the old fraction
    // to 1 and the new fraction to 1 - 1.44e-15; their clipped overlap falls short by 2.22e-15.
    // This is not newly covered material and must not request nonexistent exposed history.
    let body = vec![
        v(0.1, 0., 0.52),
        v(0.092_387_953_251_128_68, 0.038_268_343_236_508_98, 0.52),
        v(0.092_387_953_251_128_68, 0.038_268_343_236_508_98, 1.04),
    ];
    let old = frame(
        source(&selection()),
        vec![
            v(
                0.094_521_360_135_866_87,
                0.036_449_699_618_585_98,
                0.540_656_514_817_741_2,
            ),
            v(
                0.101_304_215_849_790_41,
                0.006_830_905_207_137_199,
                0.540_676_849_856_315_4,
            ),
            v(
                0.094_494_702_124_663_52,
                0.036_435_531_136_497_15,
                0.520_766_480_695_692_4,
            ),
        ],
        body.clone(),
        &[[0, 1, 2]],
        2,
    );
    let new = frame(
        source(&selection()),
        vec![
            v(
                0.094_521_369_700_209_29,
                0.036_449_924_240_243_97,
                0.540_656_566_826_756_2,
            ),
            v(
                0.101_304_454_206_541_8,
                0.006_831_244_760_361_624_6,
                0.540_676_872_694_003_7,
            ),
            v(
                0.094_495_078_973_144_9,
                0.036_435_868_409_566_31,
                0.520_766_532_164_110_3,
            ),
        ],
        body,
        &[[0, 1, 2]],
        3,
    );
    assert!(!old.has_exposed_cloth());
    let state = old
        .steady(&[Boundary {
            inside_c: 40.,
            outside_c: 0.,
            ..boundary(0.)
        }])
        .unwrap()
        .into_state();
    assert!(state.exposed_temperature_c()[0].is_empty());
    let saved = old.checkpoint(&state).unwrap();
    let transfer = new.remap(&old, &state).unwrap();
    let before = &state.temperature_c()[0];
    let expected = [before[0], mix(before[0], before[1], 0.5), before[1]];
    for row in transfer
        .state
        .temperature_c()
        .iter()
        .chain(transfer.state.exposed_temperature_c())
    {
        for (&actual, &expected) in row.iter().zip(&expected) {
            near(actual, expected);
        }
    }
    near(
        source_enthalpy(&new, &transfer.state),
        source_enthalpy(&old, &state),
    );
    near(
        transfer.balance.residual_j,
        transfer.balance.after_j - transfer.balance.before_j,
    );
    assert!(transfer.balance.allocation_residual_m2 > 0.);
    assert!(transfer.balance.allocation_residual_m2 < 1e-12);
    near(transfer.balance.absolute_residual_j, 0.);
    assert_eq!(old.checkpoint(&state).unwrap(), saved);
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
fn equal_area_disjoint_covered_regions_exchange_through_retained_exposed_material_history() {
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
    let transfer = new.remap(&old, &state).unwrap();
    assert_eq!(transfer.state.temperature_c(), &[vec![20.]]);
    assert_eq!(transfer.state.exposed_temperature_c(), &[vec![20.]]);
    near(transfer.balance.absolute_residual_j, 0.);
    near(transfer.balance.reference_area_m2, 0.5);
    assert_eq!(state.temperature_c(), &[vec![20.]]);
}

#[test]
fn entering_and_leaving_body_coverage_preserves_complete_material_state_atomically() {
    let old = one(1);
    let new = frame(
        source(&selection()),
        tri(0.04),
        vec![v(0., 0., 0.), v(0.5, 0., 0.), v(0., 1., 0.)],
        &[[0, 1, 2]],
        1,
    );
    let left = new.remap(&old, &old.initial(20.).unwrap()).unwrap();
    assert_eq!(left.state.temperature_c(), &[vec![20.]]);
    assert_eq!(left.state.exposed_temperature_c(), &[vec![20.]]);
    near(left.balance.absolute_residual_j, 0.);
    let returned = old.remap(&new, &left.state).unwrap();
    assert_eq!(returned.state.temperature_c(), &[vec![20.]]);
    assert!(returned.state.exposed_temperature_c()[0].is_empty());
    near(returned.balance.absolute_residual_j, 0.);
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
                stagnant_gap: None,
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

    // Two source layers cover the SAME half-square-metre body patch. Each source keeps its
    // own energy and capacity; the body area must not be counted twice or choose one layer's Cp.
    let heat = new
        .advance_with_exchange_and_heat(
            &new.initial(10.).unwrap(),
            0.5,
            &[boundary(10.)],
            None,
            None,
            &[500., 100.],
        )
        .unwrap();
    let temperatures = &heat.heat().state.temperature_c()[0];
    let physical = 0.5 * (h(temperatures[0]) - h(10.) + 400. * (temperatures[1] - 10.));
    near(heat.heat().covered_area_m2, 0.5);
    near(heat.supplied_heat_j(), 600.);
    near(heat.internal_heat().unwrap().covered_heat_j, 600.);
    near(heat.internal_heat().unwrap().exposed_heat_j, 0.);
    near(physical, heat.stored_change_j());
    near(physical, 0.5 * heat.net_heat_w() + 600.);
    let direct = DryThermalModel::new(
        vec![db.resolve(&a).unwrap(), db.resolve(&b).unwrap()],
        vec![0.05],
        1,
    )
    .unwrap()
    .advance_with_heat(&[10., 10.], 0.5, boundary(10.), &[1000., 200.])
    .unwrap();
    for (&a, &b) in temperatures.iter().zip(&direct.temperature_c) {
        near(a, b);
    }
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

#[test]
fn saved_nonuniform_state_resumes_exactly_through_heat_steps_and_repartitioning() {
    let old = split(3);
    let state = old
        .steady(&[boundary(10.), boundary(30.)])
        .unwrap()
        .into_state();
    let checkpoint = old.checkpoint(&state).unwrap();
    let bytes = serde_json::to_string(&checkpoint).unwrap();
    let (restored, saved) = Frame::from_checkpoint(&serde_json::from_str(&bytes).unwrap()).unwrap();
    assert_eq!(restored.snapshot(), old.snapshot());
    assert_eq!(saved.temperature_c(), state.temperature_c());
    assert_eq!(restored.checkpoint(&saved).unwrap(), checkpoint);
    let films = [boundary(40.), boundary(20.)];
    let a = old.advance(&state, 0.3, &films).unwrap();
    let b = restored.advance(&saved, 0.3, &films).unwrap();
    assert_eq!(
        a.heat().state.temperature_c(),
        b.heat().state.temperature_c()
    );
    assert_eq!(a.heat().stored_change_j, b.heat().stored_change_j);
    let next = one(2);
    let a = next.remap(&old, &a.into_state()).unwrap();
    let b = next.remap(&restored, &b.into_state()).unwrap();
    assert_eq!(a.state.temperature_c(), b.state.temperature_c());
    assert_eq!(a.balance.residual_j, b.balance.residual_j);
}

#[test]
fn checkpoint_rejects_changed_polygons_materials_cells_and_uncharacterized_temperatures() {
    use serde_json::json;
    let frame = one(2);
    let state = frame.initial(20.).unwrap();
    let saved = frame.checkpoint(&state).unwrap();
    for change in [
        (|v: &mut Value| v["frame"]["cloth_positions_m"][0][0] = json!(1.)) as fn(&mut Value),
        |v| v["frame"]["request"]["settings"]["max_distance_m"] = json!(0.19),
        |v| v["frame"]["projection"]["layout"]["regions"][0]["body_fraction"] = json!(0.7),
        |v| v["frame"]["source"]["columns"][0]["layers"][0]["allow_estimates"] = json!(false),
        |v| v["temperature_c"][0] = json!([]),
        |v| v["temperature_c"] = json!([]),
        |v| v["temperature_c"][0][0] = json!(61.),
        |v| v["temperature_c"][0][0] = json!(21.),
        |v| v["sha256"] = json!("changed"),
        |v| v["extra"] = json!(true),
    ] {
        let mut changed = saved.clone();
        change(&mut changed);
        assert!(Frame::from_checkpoint(&changed).is_err());
    }
    assert_eq!(frame.checkpoint(&state).unwrap(), saved);
    assert!(split(2).checkpoint(&state).is_err());
}

#[test]
fn checkpoint_restoration_validates_temperatures_even_with_a_recomputed_digest() {
    use ftx_core::receipt::sha256_hex;
    use serde_json::json;
    let frame = one(1);
    let mut saved = frame.checkpoint(&frame.initial(20.).unwrap()).unwrap();
    saved["temperature_c"][0][0] = json!(-0.1);
    saved.as_object_mut().unwrap().remove("sha256");
    saved["sha256"] = json!(sha256_hex(&serde_json::to_vec(&saved).unwrap()));
    assert!(Frame::from_checkpoint(&saved).is_err());
}

#[test]
fn all_bare_frames_roundtrip_with_explicit_uncovered_cloth_temperature_history() {
    let frame = frame(
        source(&selection()),
        tri(0.04),
        vec![v(2., 0., 0.), v(3., 0., 0.), v(2., 1., 0.)],
        &[[0, 1, 2]],
        1,
    );
    assert_eq!(frame.cells(), 1);
    let state = frame.initial(20.).unwrap();
    let checkpoint = frame.checkpoint(&state).unwrap();
    let (restored, saved) = Frame::from_checkpoint(&checkpoint).unwrap();
    assert!(saved.temperature_c().is_empty());
    assert_eq!(saved.exposed_temperature_c(), &[vec![20.]]);
    let covered = one(1).remap(&restored, &saved).unwrap();
    assert_eq!(covered.state.temperature_c(), &[vec![20.]]);
    assert!(covered.state.exposed_temperature_c()[0].is_empty());
    near(covered.balance.absolute_residual_j, 0.);
}

#[test]
fn uncovered_cloth_heats_only_under_explicit_face_conditions_and_reenters_with_that_heat() {
    let covered = one(2);
    let bare = frame(
        source(&selection()),
        tri(0.04),
        vec![v(2., 0., 0.), v(3., 0., 0.), v(2., 1., 0.)],
        &[[0, 1, 2]],
        2,
    );
    let initial = covered.initial(10.).unwrap();
    let exposed = bare.remap(&covered, &initial).unwrap().state;
    assert_eq!(exposed.exposed_temperature_c(), &[vec![10., 10.]]);

    let adiabatic = bare.advance(&exposed, 1., &[boundary(10.)]).unwrap();
    assert_eq!(adiabatic.exposed_temperature_c(), &[vec![10., 10.]]);
    assert_eq!(adiabatic.exposed_stored_change_j(), 0.);

    let face = Boundary {
        inside_c: 40.,
        outside_c: 40.,
        inside_resistance: 0.05,
        outside_resistance: 0.05,
    };
    let heated = bare
        .advance_with_exposed(&exposed, 2., &[boundary(10.)], &[face])
        .unwrap();
    assert!(heated.exposed_inside_heat_w() > 0.);
    assert!(heated.exposed_stored_change_j() > 0.);
    assert!(heated.exposed_balance_error_j().abs() < 1e-8);
    assert!(heated.exposed_temperature_c()[0].iter().all(|&t| t > 10.));

    let returned = covered.remap(&bare, &heated.into_state()).unwrap();
    assert!(returned.state.temperature_c()[0].iter().all(|&t| t > 10.));
    assert!(returned.state.exposed_temperature_c()[0].is_empty());
    near(returned.balance.absolute_residual_j, 0.);
}

// Independent physical enthalpy for this one reference-half-square-metre variable-Cp sheet.
fn source_enthalpy(f: &Frame, state: &ftx_drape::thermal::surface::coverage::motion::State) -> f64 {
    let mut covered = 0.;
    let mut energy = 0.;
    for (region, values) in f
        .projection()
        .layout
        .regions
        .iter()
        .zip(state.temperature_c())
    {
        assert_eq!(region.layers.len(), 1);
        let fraction = region.layers[0].fraction;
        covered += fraction;
        energy += 0.5 * fraction * values.iter().copied().map(h).sum::<f64>() / values.len() as f64;
    }
    let exposed = &state.exposed_temperature_c()[0];
    if !exposed.is_empty() {
        energy += 0.5 * (1. - covered) * exposed.iter().copied().map(h).sum::<f64>()
            / exposed.len() as f64;
    }
    energy
}

#[test]
fn internal_heat_follows_reference_material_through_covered_exposed_and_recovered_states() {
    let mut current = one(3);
    let mut state = current.initial(10.).unwrap();
    let initial_enthalpy = source_enthalpy(&current, &state);
    let mut physical = 0.;
    let mut remapped = 0.;
    for offset in [0., 0.5, 2., 0.] {
        let next = frame(
            source(&selection()),
            tri(0.04),
            vec![v(offset, 0., 0.), v(offset + 1., 0., 0.), v(offset, 1., 0.)],
            &[[0, 1, 2]],
            3,
        );
        if next.sha256() != current.sha256() {
            let transfer = next.remap(&current, &state).unwrap();
            near(
                source_enthalpy(&next, &transfer.state) - source_enthalpy(&current, &state),
                transfer.balance.residual_j,
            );
            remapped += transfer.balance.residual_j;
            state = transfer.state;
        }
        current = next;
        let before = source_enthalpy(&current, &state);
        let saved = current.checkpoint(&state).unwrap();
        let step = current
            .advance_with_exchange_and_heat(&state, 0.25, &[boundary(10.)], None, None, &[500.])
            .unwrap();
        let source = step.internal_heat().unwrap();
        let fraction: f64 = current
            .projection()
            .layout
            .regions
            .iter()
            .map(|r| r.layers[0].fraction)
            .sum();
        near(source.covered_heat_j, 500. * fraction);
        near(source.exposed_heat_j, 500. * (1. - fraction));
        near(source.supplied_heat_j, 500.);
        near(source.allocation_residual_j, 0.);
        near(step.stored_change_j(), 0.25 * step.net_heat_w() + 500.);
        near(step.balance_error_j(), 0.);
        assert_eq!(current.checkpoint(&state).unwrap(), saved);
        let stored = step.stored_change_j();
        physical += stored;
        state = step.into_state();
        near(source_enthalpy(&current, &state) - before, stored);
        if fraction == 0. {
            // There is no explicit exposed reservoir. All received energy stays in source storage.
            near(stored, 500.);
            assert!(state.exposed_temperature_c()[0].iter().all(|&t| t > 10.));
        }
    }
    assert!(state.exposed_temperature_c()[0].is_empty());
    near(
        source_enthalpy(&current, &state) - initial_enthalpy,
        physical + remapped,
    );
}

#[test]
fn covered_internal_heat_matches_direct_column_with_exterior_exchange() {
    use ftx_drape::thermal::steady::exterior::Exterior;
    use ftx_drape::thermal::surface::coverage::ExteriorConditions;
    let f = one(2);
    let old = f.initial(10.).unwrap();
    let b = Boundary {
        outside_resistance: 0.,
        ..boundary(10.)
    };
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-exterior-exchange.json"
    ))
    .unwrap();
    let mut exterior: Exterior =
        serde_json::from_value(fixture["exterior_exchange"].clone()).unwrap();
    exterior.air_c = 10.;
    exterior.convection_w_m2_k = 5.;
    exterior.radiation = None;
    let step = f
        .advance_with_exchange_and_heat(
            &old,
            0.5,
            &[b],
            Some(ExteriorConditions {
                environments: std::slice::from_ref(&exterior),
                bare_boundaries: &[],
            }),
            None,
            &[500.],
        )
        .unwrap();
    let model = DryThermalModel::new(
        vec![Catalog::builtin().unwrap().resolve(&selection()).unwrap()],
        vec![],
        2,
    )
    .unwrap();
    let direct = model
        .advance_exterior_with_heat(&[10.; 2], 0.5, b, &exterior, &[500.; 2])
        .unwrap();
    for (a, b) in step.heat().state.temperature_c()[0]
        .iter()
        .zip(&direct.state.temperature_c)
    {
        near(*a, *b);
    }
    near(
        step.stored_change_j(),
        0.5 * direct.state.stored_change_j_m2,
    );
    near(step.inside_heat_w(), 0.5 * direct.state.inside_flux_w_m2);
    near(step.outside_heat_w(), 0.5 * direct.state.outside_flux_w_m2);
    near(
        step.exterior_convective_heat_w().unwrap(),
        step.outside_heat_w(),
    );
    near(step.balance_error_j(), 0.);
}

#[test]
fn internal_heat_refuses_bad_sources_without_mutation_and_zero_source_preserves_ordinary_solve() {
    let f = one(2);
    let state = f.initial(10.).unwrap();
    let saved = f.checkpoint(&state).unwrap();
    for energy in [vec![], vec![1., 2.], vec![-1.], vec![f64::NAN], vec![1e12]] {
        assert!(
            f.advance_with_exchange_and_heat(&state, 0.25, &[boundary(20.)], None, None, &energy)
                .is_err()
        );
        assert_eq!(f.checkpoint(&state).unwrap(), saved);
    }
    let ordinary = f.advance(&state, 0.25, &[boundary(20.)]).unwrap();
    let zero = f
        .advance_with_exchange_and_heat(&state, 0.25, &[boundary(20.)], None, None, &[0.])
        .unwrap();
    assert!(zero.internal_heat().is_none());
    assert_eq!(
        zero.heat().state.temperature_c(),
        ordinary.heat().state.temperature_c()
    );
    assert_eq!(zero.stored_change_j(), ordinary.stored_change_j());
    assert_eq!(zero.balance_error_j(), ordinary.balance_error_j());
}

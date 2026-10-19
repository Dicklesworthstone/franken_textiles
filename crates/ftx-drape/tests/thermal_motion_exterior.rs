//! Synthetic material/geometry energy answers for exterior heat during coverage changes.
use ftx_core::Vec3;
use ftx_drape::thermal::{
    Boundary, DryThermalModel,
    steady::exterior::Exterior,
    surface::{
        Patch, Surface,
        coverage::{
            ExteriorConditions,
            motion::{ExposedConditions, Frame, Step},
        },
        projection::{Request, Settings},
    },
};
use ftx_pattern::{material_catalog::Selection, materials::Catalog};
use serde_json::Value;

fn fixture() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-transient-exterior.json"
    ))
    .unwrap()
}

fn selection() -> Selection {
    serde_json::from_value(fixture()["layers"][0].clone()).unwrap()
}

fn environment() -> Exterior {
    serde_json::from_value(fixture()["exterior_exchange"].clone()).unwrap()
}

fn model(selected: &Selection, cells: usize) -> DryThermalModel {
    DryThermalModel::new(
        vec![Catalog::builtin().unwrap().resolve(selected).unwrap()],
        vec![],
        cells,
    )
    .unwrap()
}

fn tri(x: f64, z: f64) -> Vec<Vec3> {
    vec![
        Vec3::new(x, 0., z),
        Vec3::new(x + 1., 0., z),
        Vec3::new(x, 1., z),
    ]
}

fn source(selected: &Selection) -> Surface {
    Surface::new(
        vec![model(selected, 1)],
        3,
        vec![Patch {
            vertices: [0, 1, 2],
            reference_area_m2: 0.5,
            column: 0,
        }],
    )
    .unwrap()
}

fn mapped(
    source: Surface,
    cloth: Vec<Vec3>,
    body: Vec<Vec3>,
    cells: usize,
    cloth_layers: Vec<String>,
) -> Frame {
    Frame::new(
        source,
        cloth,
        body,
        &[[0, 1, 2]],
        Request {
            schema: "ftx-thermal-projection/1".into(),
            cloth_layers,
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

fn one(selected: &Selection, cloth: Vec<Vec3>, body: Vec<Vec3>, cells: usize) -> Frame {
    mapped(source(selected), cloth, body, cells, vec!["shell".into()])
}

fn film() -> Boundary {
    Boundary {
        inside_c: 35.,
        outside_c: 5.,
        inside_resistance: 0.1,
        outside_resistance: 0.2,
    }
}

fn exterior_boundary(inside: f64, environment: &Exterior) -> Boundary {
    Boundary {
        inside_c: inside,
        outside_c: environment.air_c,
        inside_resistance: 0.1,
        outside_resistance: 0.,
    }
}

fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 2e-8 * (1. + b.abs()), "{a} != {b}");
}

#[test]
fn covered_exposed_and_recovered_material_keeps_energy_and_checkpointed_weather() {
    let selected = selection();
    let full = one(&selected, tri(0., 0.04), tri(0., 0.), 4);
    let partial = one(
        &selected,
        tri(0., 0.04),
        vec![
            Vec3::new(0., 0., 0.),
            Vec3::new(0.5, 0., 0.),
            Vec3::new(0., 1., 0.),
        ],
        4,
    );
    let away = one(&selected, tri(0., 0.04), tri(4., 0.), 4);
    let cold = [environment()];
    let covered_boundary = [exterior_boundary(35., &cold[0])];
    let bare = [film()];
    let covered_conditions = ExteriorConditions {
        environments: &cold,
        bare_boundaries: &bare,
    };
    let mut warm = cold.clone();
    warm[0].air_c = 30.;
    warm[0].radiation.as_mut().unwrap().ambient_c = 45.;
    let exposed_boundary = [exterior_boundary(40., &warm[0])];
    let exposed_conditions = ExposedConditions {
        boundaries: &exposed_boundary,
        environments: Some(&warm),
    };

    let mut stored = 0.;
    let mut net = 0.;
    let mut outgoing = 0.;
    let mut exterior_energy = 0.;
    let mut bare_energy = 0.;
    let mut account = |step: &Step, seconds: f64| {
        stored += step.stored_change_j();
        net += seconds * step.net_heat_w();
        outgoing += seconds * step.outside_heat_w();
        exterior_energy += seconds
            * (step.exterior_convective_heat_w().unwrap()
                + step.exterior_radiative_heat_w().unwrap());
        bare_energy += seconds * step.heat().bare_heat_w;
        near(step.balance_error_j(), 0.);
        // Exterior acceptance is per current outer cloth area, not a fixed watt threshold.
        // Keep this boundary-equation check separate from analytic storage and remap checks.
        let covered_faces = step
            .heat()
            .exterior_exchange
            .iter()
            .flat_map(|e| e.regions.iter().map(|r| (r.outer_area_m2, &r.flux)));
        let exposed_faces = step
            .exposed_exterior_exchange()
            .into_iter()
            .flat_map(|e| e.triangles.iter().map(|r| (r.current_area_m2, &r.flux)));
        let tolerance = covered_faces
            .chain(exposed_faces)
            .map(|(area, flux)| {
                let outgoing = flux.convective_flux_w_m2
                    + flux.radiative_flux_w_m2
                    + flux.balance_residual_w_m2;
                1.01e-10
                    * area
                    * (1.
                        + outgoing.abs()
                        + flux.convective_flux_w_m2.abs()
                        + flux.radiative_flux_w_m2.abs())
            })
            .sum::<f64>();
        let residual = step.exterior_balance_residual_w().unwrap();
        assert!(
            residual.abs() <= tolerance,
            "exterior residual {residual} W exceeds area/flux-scaled tolerance {tolerance} W"
        );
    };

    let first = full
        .advance_with_exchange(
            &full.initial(10.).unwrap(),
            0.5,
            &covered_boundary,
            Some(covered_conditions),
            None,
        )
        .unwrap();
    let direct = model(&selected, 4)
        .advance_exterior(&[10.; 4], 0.5, covered_boundary[0], &cold[0])
        .unwrap();
    for (&a, &b) in first.heat().state.temperature_c()[0]
        .iter()
        .zip(&direct.state.temperature_c)
    {
        near(a, b);
    }
    account(&first, 0.5);
    let entered = partial.remap(&full, &first.into_state()).unwrap();
    near(entered.balance.absolute_residual_j, 0.);
    let middle = partial
        .advance_with_exchange(
            &entered.state,
            0.25,
            &covered_boundary,
            Some(covered_conditions),
            Some(exposed_conditions),
        )
        .unwrap();
    let covered_flux = middle.heat().exterior_exchange.as_ref().unwrap();
    let exposed_flux = middle.exposed_exterior_exchange().unwrap();
    assert_eq!(covered_flux.regions.len(), 1);
    assert_eq!(exposed_flux.triangles.len(), 1);
    assert_eq!(exposed_flux.triangles[0].cloth_triangle, 0);
    near(exposed_flux.triangles[0].current_area_m2, 0.25);
    near(
        middle.exterior_convective_heat_w().unwrap(),
        covered_flux.convective_heat_w + exposed_flux.convective_heat_w,
    );
    account(&middle, 0.25);
    let middle_state = middle.into_state();
    let checkpoint = partial.checkpoint(&middle_state).unwrap();
    let (restored, saved) = Frame::from_checkpoint(&checkpoint).unwrap();
    assert_eq!(restored.checkpoint(&saved).unwrap(), checkpoint);

    let left = away.remap(&partial, &middle_state).unwrap();
    let replay_left = away.remap(&restored, &saved).unwrap();
    assert_eq!(
        left.state.exposed_temperature_c(),
        replay_left.state.exposed_temperature_c()
    );
    for (i, &t) in left.state.exposed_temperature_c()[0].iter().enumerate() {
        near(
            t,
            (middle_state.temperature_c()[0][i] + middle_state.exposed_temperature_c()[0][i]) / 2.,
        );
    }
    near(left.balance.absolute_residual_j, 0.);
    let next = away
        .advance_with_exchange(
            &left.state,
            0.75,
            &covered_boundary,
            Some(covered_conditions),
            Some(exposed_conditions),
        )
        .unwrap();
    let replay = away
        .advance_with_exchange(
            &replay_left.state,
            0.75,
            &covered_boundary,
            Some(covered_conditions),
            Some(exposed_conditions),
        )
        .unwrap();
    assert_eq!(next.exposed_temperature_c(), replay.exposed_temperature_c());
    assert_eq!(next.stored_change_j(), replay.stored_change_j());
    assert_eq!(
        serde_json::to_value(next.exposed_exterior_exchange()).unwrap(),
        serde_json::to_value(replay.exposed_exterior_exchange()).unwrap()
    );
    let direct = model(&selected, 4)
        .advance_exterior(
            &left.state.exposed_temperature_c()[0],
            0.75,
            exposed_boundary[0],
            &warm[0],
        )
        .unwrap();
    assert_eq!(next.exposed_temperature_c()[0], direct.state.temperature_c);
    assert!(next.exterior_radiative_heat_w().unwrap() < 0.);
    near(next.heat().bare_heat_w, 50.);
    account(&next, 0.75);
    let returned = full.remap(&away, &next.into_state()).unwrap();
    near(returned.balance.absolute_residual_j, 0.);
    assert!(returned.state.exposed_temperature_c()[0].is_empty());

    // A new cold interval uses the stored warm state, without changing the material/frame hash.
    let final_step = full
        .advance_with_exchange(
            &returned.state,
            0.4,
            &covered_boundary,
            Some(covered_conditions),
            None,
        )
        .unwrap();
    account(&final_step, 0.4);
    let final_state = final_step.into_state();
    // Independent 0.5 m² * 0.2 kg/m² * 1000 J/(kg K), shared equally by four cells.
    let energy = 25.
        * final_state.temperature_c()[0]
            .iter()
            .map(|t| t - 10.)
            .sum::<f64>();
    near(stored, net);
    near(
        stored + entered.balance.residual_j + left.balance.residual_j + returned.balance.residual_j,
        energy,
    );
    near(outgoing, exterior_energy + bare_energy);
}

#[test]
fn exposed_stretch_changes_exchange_area_without_creating_heat_capacity() {
    let selected = selection();
    let mut cloth = tri(0., 0.04);
    cloth[1].x = 2.;
    let frame = one(&selected, cloth, tri(4., 0.), 4);
    let environment = [environment()];
    let outside = [exterior_boundary(35., &environment[0])];
    let initial = frame.initial(10.).unwrap();
    let step = frame
        .advance_with_exchange(
            &initial,
            0.5,
            &[film()],
            None,
            Some(ExposedConditions {
                boundaries: &outside,
                environments: Some(&environment),
            }),
        )
        .unwrap();
    let direct = model(&selected, 4)
        .advance_exterior(&[10.; 4], 1., outside[0], &environment[0])
        .unwrap();
    assert_eq!(step.exposed_temperature_c()[0], direct.state.temperature_c);
    let exchange = step.exposed_exterior_exchange().unwrap();
    assert_eq!(exchange.triangles.len(), 1);
    near(exchange.triangles[0].current_area_m2, 1.);
    near(
        step.exposed_outside_heat_w(),
        direct.state.outside_flux_w_m2,
    );
    near(
        step.exposed_stored_change_j(),
        0.5 * direct.state.stored_change_j_m2,
    );
    let energy = 25.
        * step.exposed_temperature_c()[0]
            .iter()
            .map(|t| t - 10.)
            .sum::<f64>();
    near(step.stored_change_j(), energy);
    near(
        step.stored_change_j(),
        0.5 * (step.exposed_inside_heat_w() - step.exposed_outside_heat_w()),
    );
    assert!(step.heat().exterior_exchange.is_none());
    near(
        step.exterior_radiative_heat_w().unwrap(),
        exchange.radiative_heat_w,
    );
}

#[test]
fn ordinary_films_and_adiabatic_exposure_keep_their_existing_results() {
    let selected = selection();
    let frame = one(&selected, tri(0., 0.04), tri(4., 0.), 2);
    let initial = frame.initial(10.).unwrap();
    let adiabatic = frame.advance(&initial, 0.5, &[film()]).unwrap();
    assert_eq!(adiabatic.exposed_temperature_c(), &[vec![10., 10.]]);
    assert_eq!(adiabatic.exposed_stored_change_j(), 0.);
    assert!(adiabatic.exposed_exterior_exchange().is_none());
    assert!(adiabatic.exterior_convective_heat_w().is_none());
    assert!(adiabatic.exterior_radiative_heat_w().is_none());
    assert!(adiabatic.exterior_balance_residual_w().is_none());

    let step = frame
        .advance_with_exposed(&initial, 0.5, &[film()], &[film()])
        .unwrap();
    let direct = model(&selected, 2).advance(&[10.; 2], 0.5, film()).unwrap();
    assert_eq!(step.exposed_temperature_c()[0], direct.temperature_c);
    near(
        step.exposed_stored_change_j(),
        0.5 * direct.stored_change_j_m2,
    );
    assert!(step.exposed_exterior_exchange().is_none());
    assert!(step.exterior_convective_heat_w().is_none());
    assert!(step.heat().exterior_exchange.is_none());
}

#[test]
fn only_active_exposed_sources_require_optics_and_keep_source_triangle_indices() {
    let selected = selection();
    let mut hidden = selected.clone();
    hidden
        .record
        .as_mut()
        .unwrap()
        .properties
        .remove("emissivity");
    let source = Surface::new(
        vec![model(&hidden, 1), model(&selected, 1)],
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
    let mut cloth = tri(0., 0.04);
    cloth.extend(tri(2., 0.04));
    let layers = vec!["lining".into(), "shell".into()];
    let covered = mapped(
        source.clone(),
        cloth.clone(),
        tri(0., 0.),
        2,
        layers.clone(),
    );
    let away = mapped(source, cloth, tri(4., 0.), 2, layers);
    let environment = [environment(), environment()];
    let boundaries = [
        exterior_boundary(35., &environment[0]),
        exterior_boundary(35., &environment[1]),
    ];
    let step = covered
        .advance_with_exchange(
            &covered.initial(10.).unwrap(),
            0.5,
            &[film()],
            None,
            Some(ExposedConditions {
                boundaries: &boundaries,
                environments: Some(&environment),
            }),
        )
        .unwrap();
    let exchange = step.exposed_exterior_exchange().unwrap();
    assert_eq!(exchange.triangles.len(), 1);
    assert_eq!(exchange.triangles[0].cloth_triangle, 1);
    let entered = away.remap(&covered, &step.into_state()).unwrap();
    // The formerly hidden material has no characterized emissivity once it becomes exposed.
    assert!(
        away.advance_with_exchange(
            &entered.state,
            0.5,
            &[film()],
            None,
            Some(ExposedConditions {
                boundaries: &boundaries,
                environments: Some(&environment),
            }),
        )
        .is_err()
    );
    let mut explicit_convection = environment;
    explicit_convection[0].radiation = None;
    let admitted = away
        .advance_with_exchange(
            &entered.state,
            0.5,
            &[film()],
            None,
            Some(ExposedConditions {
                boundaries: &boundaries,
                environments: Some(&explicit_convection),
            }),
        )
        .unwrap();
    let rows = &admitted.exposed_exterior_exchange().unwrap().triangles;
    assert_eq!(rows.len(), 2);
    assert_eq!([rows[0].cloth_triangle, rows[1].cloth_triangle], [0, 1]);
    assert!(!rows[0].flux.radiation_enabled);
    assert!(rows[1].flux.radiation_enabled);
}

#[test]
fn invalid_exterior_inputs_refuse_atomically_and_material_edits_require_new_states() {
    let selected = selection();
    let frame = one(&selected, tri(0., 0.04), tri(4., 0.), 2);
    let state = frame.initial(10.).unwrap();
    let saved = frame.checkpoint(&state).unwrap();
    let environment = [environment()];
    let boundary = [exterior_boundary(35., &environment[0])];
    for conditions in [
        ExposedConditions {
            boundaries: &[],
            environments: Some(&environment),
        },
        ExposedConditions {
            boundaries: &boundary,
            environments: Some(&[]),
        },
    ] {
        assert!(
            frame
                .advance_with_exchange(&state, 0.5, &[film()], None, Some(conditions))
                .is_err()
        );
    }
    for change in 0..4 {
        let mut bad_environment = environment.clone();
        let mut bad_boundary = boundary;
        match change {
            0 => bad_boundary[0].outside_resistance = 0.2,
            1 => bad_boundary[0].outside_c = 6.,
            2 => bad_environment[0].allow_estimates = false,
            _ => {
                bad_environment[0].radiation.as_mut().unwrap().exposed_face =
                    "unqualified-face".into();
            }
        }
        assert!(
            frame
                .advance_with_exchange(
                    &state,
                    0.5,
                    &[film()],
                    None,
                    Some(ExposedConditions {
                        boundaries: &bad_boundary,
                        environments: Some(&bad_environment),
                    }),
                )
                .is_err()
        );
    }
    assert_eq!(frame.checkpoint(&state).unwrap(), saved);
    let mut changed = selected;
    changed
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("emissivity")
        .unwrap()
        .value = 0.05;
    let changed = one(&changed, tri(0., 0.04), tri(4., 0.), 2);
    assert_ne!(changed.sha256(), frame.sha256());
    assert!(changed.remap(&frame, &state).is_err());
    assert!(
        changed
            .advance_with_exchange(
                &state,
                0.5,
                &[film()],
                None,
                Some(ExposedConditions {
                    boundaries: &boundary,
                    environments: Some(&environment),
                }),
            )
            .is_err()
    );
}

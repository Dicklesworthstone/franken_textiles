//! Explicit virtual-extensometer geometry and synthetic inverse/forward integration.
//! The invented observations verify software; no source acquisition operator is inferred.

use std::sync::OnceLock;

use ftx_drape::strip::{
    Fixture, Request, State, evaluate,
    fit::{Input, Report, calibrate},
    measurement::{self, Extensometer, Operator},
};
use ftx_pattern::material_calibration::Partition;

fn fixture(cells: [usize; 2]) -> Fixture {
    Fixture {
        gauge_length_m: 0.1,
        width_m: 0.06,
        warp_axis: [0.0, 1.0],
        cells,
    }
}

fn field(fixture: &Fixture, deform: impl Fn(f64, f64) -> [f64; 3]) -> State {
    let [nx, ny] = fixture.cells;
    let mut positions_m = Vec::new();
    for row in 0..=ny {
        for column in 0..=nx {
            positions_m.push(deform(
                fixture.width_m * column as f64 / nx as f64,
                fixture.gauge_length_m * row as f64 / ny as f64,
            ));
        }
    }
    State {
        extension_fraction: 0.0,
        force_n: 0.0,
        fixed_grip_force_n: 0.0,
        free_force_residual_n: 0.0,
        grip_balance_residual_n: 0.0,
        mid_width_m: 0.0,
        energy_j: 0.0,
        green_strain_range: [[0.0; 2]; 3],
        area_ratio_range: None,
        positions_m,
        newton_iterations: 0,
    }
}

fn close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-14, "{actual} != {expected}");
}

#[test]
fn affine_chords_and_signed_projections_are_distinct_observations() {
    let fixture = fixture([2, 4]);
    // Independent affine field: right-minus-left displacement is W*[0.8,0.6],
    // regardless of station, translation or shear caused by the axial coordinate.
    let state = field(&fixture, |x, y| {
        [2.0 + 0.8 * x + 0.3 * y, -1.0 + 0.6 * x + 1.2 * y, 0.0]
    });
    for station_fraction in [0.0, 0.17, 0.5, 1.0] {
        let chord = Operator::EdgeChord { station_fraction };
        close(
            measurement::evaluate(&chord, &fixture, &state).unwrap(),
            0.06,
        );
        for (axis, expected) in [
            ([1.0, 0.0], 0.048),
            ([-1.0, 0.0], -0.048),
            ([0.0, 1.0], 0.036),
        ] {
            let projected = Operator::EdgeProjection {
                station_fraction,
                axis,
            };
            close(
                measurement::evaluate(&projected, &fixture, &state).unwrap(),
                expected,
            );
        }
    }
}

#[test]
fn reference_station_interpolates_material_edges_and_survives_mesh_refinement() {
    // An independently specified field has width W*(1-0.7*s), so sampling a
    // nearest row or the midpoint would give a different answer at s=0.3.
    for cells in [[2, 2], [4, 4]] {
        let fixture = fixture(cells);
        let state = field(&fixture, |x, y| {
            [x * (1.0 - 0.7 * y / 0.1) + 0.2 * y, y, 0.0]
        });
        let operator = Operator::EdgeChord {
            station_fraction: 0.3,
        };
        close(
            measurement::evaluate(&operator, &fixture, &state).unwrap(),
            0.06 * 0.79,
        );
    }
}

#[test]
fn interior_material_gauges_follow_affine_fields_and_reference_points_across_meshes() {
    for cells in [[2, 2], [4, 8], [8, 4]] {
        let fixture = fixture(cells);
        let state = field(&fixture, |x, y| {
            [2.0 + 0.8 * x + 0.3 * y, -1.0 + 0.6 * x + 1.2 * y, 0.0]
        });
        // Independent affine answers, including axial/diagonal interior gauges and corners.
        for (start_fraction, end_fraction, dx, dy) in [
            ([0.2, 0.15], [0.8, 0.85], 0.0498_f64, 0.1056_f64),
            ([0.5, 0.2], [0.5, 0.8], 0.018, 0.072),
            ([0.0, 0.0], [1.0, 1.0], 0.078, 0.156),
            ([0.0, 1.0], [1.0, 0.0], 0.018, -0.084),
        ] {
            for (start, end, sign) in [
                (start_fraction, end_fraction, 1.0),
                (end_fraction, start_fraction, -1.0),
            ] {
                let chord = Operator::MaterialChord {
                    start_fraction: start,
                    end_fraction: end,
                };
                close(
                    measurement::evaluate(&chord, &fixture, &state).unwrap(),
                    dx.hypot(dy),
                );
                let projected = Operator::MaterialProjection {
                    start_fraction: start,
                    end_fraction: end,
                    axis: [0.6, 0.8],
                };
                close(
                    measurement::evaluate(&projected, &fixture, &state).unwrap(),
                    sign * (0.6 * dx + 0.8 * dy),
                );
                assert_eq!(
                    serde_json::from_value::<Operator>(serde_json::to_value(&projected).unwrap())
                        .unwrap(),
                    projected
                );
            }
        }
    }
}

#[test]
fn interior_markers_use_both_production_triangle_diagonals_instead_of_bilinear_sampling() {
    let fixture = fixture([2, 2]);
    let mut state = field(&fixture, |x, y| [x, y, 0.0]);
    state.positions_m[4][0] += 1.0;
    // The central vertex's piecewise-linear hat is min(u,v) in the lower-left cell,
    // min(1-u,v) in the lower-right, min(u,1-v) in the upper-left, and
    // 1-max(u,v) in the upper-right. Bilinear interpolation would use products.
    for (end_fraction, expected) in [
        ([0.375, 0.125], 0.2725),
        ([0.125, 0.375], 0.2575),
        ([0.25, 0.25], 0.515),
        ([0.625, 0.125], 0.2875),
        ([0.875, 0.375], 0.3025),
        ([0.875, 0.125], 0.3025),
        ([0.125, 0.625], 0.2575),
        ([0.375, 0.875], 0.2725),
        ([0.625, 0.875], 0.2875),
        ([0.875, 0.625], 0.3025),
        ([0.5, 0.25], 0.53),
        ([0.5, 0.5], 1.03),
        ([1.0, 0.75], 0.06),
        ([0.75, 1.0], 0.045),
        ([1.0, 1.0], 0.06),
    ] {
        let operator = Operator::MaterialProjection {
            start_fraction: [0.0, 0.0],
            end_fraction,
            axis: [1.0, 0.0],
        };
        close(
            measurement::evaluate(&operator, &fixture, &state).unwrap(),
            expected,
        );
    }
}

#[test]
fn interior_gauges_measure_free_motion_while_clamps_stay_fixed() {
    let fixture = fixture([2, 2]);
    let initial = field(&fixture, |x, y| [x, y, 0.0]);
    let mut contracted = initial.clone();
    contracted.positions_m[3][0] += 0.005;
    contracted.positions_m[5][0] -= 0.005;
    // These markers carry 0.3 of each opposite free-side vertex; their shared center
    // contribution cancels. Moving those vertices narrows the chord's x component
    // by 0.003 m without changing its 0.03 m axial component or either clamp.
    let chord = Operator::MaterialChord {
        start_fraction: [0.2, 0.35],
        end_fraction: [0.8, 0.65],
    };
    close(
        measurement::evaluate(&chord, &fixture, &initial).unwrap(),
        0.036_f64.hypot(0.03),
    );
    close(
        measurement::evaluate(&chord, &fixture, &contracted).unwrap(),
        0.033_f64.hypot(0.03),
    );
    let projected = Operator::MaterialProjection {
        start_fraction: [0.2, 0.35],
        end_fraction: [0.8, 0.65],
        axis: [-1.0, 0.0],
    };
    close(
        measurement::evaluate(&projected, &fixture, &initial).unwrap(),
        -0.036,
    );
    close(
        measurement::evaluate(&projected, &fixture, &contracted).unwrap(),
        -0.033,
    );
}

#[test]
fn material_edge_markers_retain_existing_station_answers_and_zero_projections() {
    let fixture = fixture([4, 6]);
    let state = field(&fixture, |x, y| [x * x + 0.2 * y, y * y + x * y, 0.0]);
    for station_fraction in [0.0, 0.17, 0.5, 0.91, 1.0] {
        for (edge, material) in [
            (
                Operator::EdgeChord { station_fraction },
                Operator::MaterialChord {
                    start_fraction: [0.0, station_fraction],
                    end_fraction: [1.0, station_fraction],
                },
            ),
            (
                Operator::EdgeProjection {
                    station_fraction,
                    axis: [-1.0, 0.0],
                },
                Operator::MaterialProjection {
                    start_fraction: [0.0, station_fraction],
                    end_fraction: [1.0, station_fraction],
                    axis: [-1.0, 0.0],
                },
            ),
        ] {
            assert_eq!(
                measurement::evaluate(&edge, &fixture, &state).unwrap(),
                measurement::evaluate(&material, &fixture, &state).unwrap()
            );
        }
    }
    let undeformed = field(&fixture, |x, y| [x, y, 0.0]);
    let zero_projection = Operator::MaterialProjection {
        start_fraction: [0.25, 0.5],
        end_fraction: [0.75, 0.5],
        axis: [0.0, 1.0],
    };
    assert_eq!(
        measurement::evaluate(&zero_projection, &fixture, &undeformed).unwrap(),
        0.0
    );
    let collapsed = field(&fixture, |_, _| [0.0; 3]);
    assert!(
        measurement::evaluate(
            &Operator::MaterialChord {
                start_fraction: [0.25, 0.5],
                end_fraction: [0.75, 0.5],
            },
            &fixture,
            &collapsed
        )
        .is_err()
    );
}

#[test]
fn malformed_operators_and_incompatible_states_refuse() {
    let fixture = fixture([2, 2]);
    let state = field(&fixture, |x, y| [x, y, 0.0]);
    for operator in [
        Operator::EdgeChord {
            station_fraction: -0.1,
        },
        Operator::EdgeChord {
            station_fraction: f64::NAN,
        },
        Operator::EdgeProjection {
            station_fraction: 1.1,
            axis: [1.0, 0.0],
        },
        Operator::EdgeProjection {
            station_fraction: 0.5,
            axis: [2.0, 0.0],
        },
        Operator::EdgeProjection {
            station_fraction: 0.5,
            axis: [0.0, 0.0],
        },
        Operator::MaterialChord {
            start_fraction: [-0.1, 0.2],
            end_fraction: [0.5, 0.8],
        },
        Operator::MaterialChord {
            start_fraction: [0.2, 0.2],
            end_fraction: [0.5, 1.1],
        },
        Operator::MaterialChord {
            start_fraction: [0.2, 0.2],
            end_fraction: [0.2, 0.2],
        },
        Operator::MaterialProjection {
            start_fraction: [0.2, f64::NAN],
            end_fraction: [0.5, 0.8],
            axis: [1.0, 0.0],
        },
        Operator::MaterialProjection {
            start_fraction: [0.2, 0.2],
            end_fraction: [f64::INFINITY, 0.8],
            axis: [1.0, 0.0],
        },
        Operator::MaterialProjection {
            start_fraction: [0.2, 0.2],
            end_fraction: [0.2, 0.2],
            axis: [1.0, 0.0],
        },
        Operator::MaterialProjection {
            start_fraction: [0.2, 0.2],
            end_fraction: [0.5, 0.8],
            axis: [0.0, 2.0],
        },
    ] {
        assert!(measurement::evaluate(&operator, &fixture, &state).is_err());
    }
    let operator = Operator::EdgeChord {
        station_fraction: 0.3,
    };
    let mut changed = state.clone();
    changed.positions_m.pop();
    assert!(measurement::evaluate(&operator, &fixture, &changed).is_err());
    let mut changed = state.clone();
    changed.positions_m[0][0] = f64::INFINITY;
    assert!(measurement::evaluate(&operator, &fixture, &changed).is_err());
    let mut changed = state;
    changed.positions_m[0][2] = 0.001;
    assert!(measurement::evaluate(&operator, &fixture, &changed).is_err());
}

fn input() -> Input {
    let mut input: Input = serde_json::from_str(include_str!(
        "../../../examples/strip-identification/synthetic-nonlinear-calibration.json"
    ))
    .unwrap();
    let forward: Request = serde_json::from_str(include_str!(
        "../../../examples/strip-identification/synthetic-nonlinear-forward.json"
    ))
    .unwrap();
    input.method = "Synthetic production strip observations with explicit quarter-gauge edge chords, signed transverse projections at three-quarter gauge and diagonal interior material-marker lengths/projections. No physical measurement or inferred source operator.".into();
    for experiment in &mut input.experiments {
        let report = evaluate(&Request {
            fixture: experiment.fixture.clone(),
            extensions: experiment
                .observations
                .iter()
                .map(|o| o.extension_fraction)
                .collect(),
            ..forward.clone()
        })
        .unwrap();
        for (observation, state) in experiment.observations.iter_mut().zip(&report.states) {
            observation.mid_width = None;
            observation.extensometers = [
                (
                    "quarter-chord",
                    Operator::EdgeChord {
                        station_fraction: 0.25,
                    },
                ),
                (
                    "three-quarter-negative-x",
                    Operator::EdgeProjection {
                        station_fraction: 0.75,
                        axis: [-1.0, 0.0],
                    },
                ),
                (
                    "interior-diagonal",
                    Operator::MaterialChord {
                        start_fraction: [0.2, 0.35],
                        end_fraction: [0.8, 0.65],
                    },
                ),
                (
                    "interior-negative-x",
                    Operator::MaterialProjection {
                        start_fraction: [0.2, 0.35],
                        end_fraction: [0.8, 0.65],
                        axis: [-1.0, 0.0],
                    },
                ),
            ]
            .into_iter()
            .map(|(id, operator)| Extensometer {
                value_m: measurement::evaluate(&operator, &experiment.fixture, state).unwrap(),
                id: id.into(),
                operator,
                scale_m: 0.001,
            })
            .collect();
        }
    }
    input
}

fn baseline() -> &'static Report {
    static REPORT: OnceLock<Report> = OnceLock::new();
    REPORT.get_or_init(|| calibrate(&input()).unwrap())
}

#[test]
fn explicit_operators_identify_the_law_and_replay_the_actual_published_field() {
    let report = baseline();
    assert!(
        report.accepted,
        "{:?}; parameters {:?}",
        report.reasons, report.parameters
    );
    assert!(report.domain_replay_verified);
    for (actual, known) in report.parameters.iter().zip([1.1, 0.9, 1.2, 40.0]) {
        assert!((actual - known).abs() < 1e-3, "{actual} != {known}");
    }
    assert_eq!(report.fit.components, 45);
    assert_eq!(report.check.as_ref().unwrap().components, 5);
    let selection = report.selection.clone().unwrap();
    let mut mesh_peak = 0.0_f64;
    for experiment in &report.input.experiments {
        let request = Request {
            schema: "ftx-strip-experiment/1".into(),
            material: selection.clone(),
            fixture: experiment.fixture.clone(),
            extensions: experiment
                .observations
                .iter()
                .map(|o| o.extension_fraction)
                .collect(),
            controls: report.input.controls.clone(),
        };
        let coarse = evaluate(&request).unwrap();
        let mut refined_request = request;
        refined_request
            .fixture
            .cells
            .iter_mut()
            .for_each(|n| *n *= 2);
        let refined = evaluate(&refined_request).unwrap();
        let replay = evaluate(&refined.input).unwrap();
        assert_eq!(
            serde_json::to_string(&refined).unwrap(),
            serde_json::to_string(&replay).unwrap()
        );
        for (((observation, coarse_state), state), prediction) in experiment
            .observations
            .iter()
            .zip(&coarse.states)
            .zip(&refined.states)
            .zip(
                report
                    .predictions
                    .iter()
                    .filter(|p| p.experiment == experiment.id),
            )
        {
            assert_eq!(
                serde_json::to_value(state).unwrap(),
                serde_json::to_value(&prediction.state).unwrap()
            );
            mesh_peak =
                mesh_peak.max((state.force_n - coarse_state.force_n).abs() / observation.scale_n);
            assert!(prediction.width_residual_m.is_none());
            for (channel, actual) in observation
                .extensometers
                .iter()
                .zip(&prediction.extensometers)
            {
                assert_eq!(actual.id, channel.id);
                assert_eq!(actual.operator, channel.operator);
                let value = measurement::evaluate(&channel.operator, &refined.input.fixture, state)
                    .unwrap();
                let coarse_value =
                    measurement::evaluate(&channel.operator, &coarse.input.fixture, coarse_state)
                        .unwrap();
                assert_eq!(actual.predicted_m, value);
                assert_eq!(actual.residual_m, value - channel.value_m);
                mesh_peak = mesh_peak.max((value - coarse_value).abs() / channel.scale_m);
                if matches!(
                    channel.operator,
                    Operator::EdgeProjection { .. } | Operator::MaterialProjection { .. }
                ) {
                    assert!(value < 0.0);
                }
            }
        }
    }
    assert_eq!(report.mesh_change_peak_scaled, mesh_peak);
}

#[test]
fn held_out_extensometers_cannot_choose_parameters_or_expand_support() {
    let expected = baseline();
    let mut changed = expected.input.clone();
    let check = changed
        .experiments
        .iter_mut()
        .find(|e| e.partition == Partition::Check)
        .unwrap();
    check.observations[0].extensometers[2].value_m += 0.1;
    let rejected = calibrate(&changed).unwrap();
    assert_eq!(rejected.parameters, expected.parameters);
    assert_eq!(
        rejected.fitted_green_strain_range,
        expected.fitted_green_strain_range
    );
    assert!(rejected.forward_solves < expected.forward_solves);
    assert!(!rejected.accepted && rejected.selection.is_none());
    assert!(rejected.check.unwrap().peak_scaled > 90.0);
    assert!(
        rejected
            .reasons
            .iter()
            .any(|reason| reason.contains("CHECK force/width"))
    );
}

#[test]
fn fit_extensometer_readings_enter_the_objective_and_parameter_sensitivities() {
    let expected = baseline();
    let mut changed = expected.input.clone();
    changed.experiments[0].observations[0].extensometers[2].value_m += 0.0001;
    let report = calibrate(&changed).unwrap();
    assert_ne!(report.parameters, expected.parameters);
    assert!(report.starts.iter().all(|start| start.fit_sse > 1e-6));
    assert!(expected.starts.iter().all(|start| start.fit_sse < 1e-12));
}

#[test]
fn invalid_channels_refuse_before_solving_and_legacy_json_omits_the_extension() {
    let legacy: Input = serde_json::from_str(include_str!(
        "../../../examples/strip-identification/synthetic-nonlinear-calibration.json"
    ))
    .unwrap();
    assert!(
        !serde_json::to_string(&legacy)
            .unwrap()
            .contains("extensometers")
    );
    let valid = input();
    for failure in 0..8 {
        let mut changed = valid.clone();
        let channels = &mut changed.experiments[0].observations[0].extensometers;
        match failure {
            0 => channels[0].id = channels[1].id.clone(),
            1 => channels[0].scale_m = 0.0,
            2 => channels[0].value_m = -0.01,
            3 => {
                channels[0].operator = Operator::EdgeChord {
                    station_fraction: 1.1,
                }
            }
            4 => channels.extend(vec![channels[0].clone(); 7]),
            5 => channels[2].value_m = 0.0,
            6 => {
                channels[2].operator = Operator::MaterialChord {
                    start_fraction: [0.25, 0.2],
                    end_fraction: [0.75, 1.1],
                }
            }
            _ => channels[2].value_m = -0.01,
        }
        assert!(calibrate(&changed).is_err());
    }
}

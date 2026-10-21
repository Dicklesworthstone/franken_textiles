//! Independent synthetic mechanics answers and source/domain admission for the clamped strip.
//! These chosen coefficients and test-authored receipts are not physical textile measurements.

use ftx_core::{RefusalCode, receipt::sha256_hex};
use ftx_drape::strip::{Controls, Fixture, Request, evaluate, solve};
use ftx_pattern::{material_catalog::Selection, materials::Form};
use serde_json::{Value, json};

const PARAMETERS: [f64; 4] = [800.0, 400.0, 0.0, 120.0];

fn controls() -> Controls {
    Controls {
        tolerance_m: 1e-9,
        force_tolerance_n: 1e-6,
        max_newton: 100,
    }
}

fn fixture() -> Fixture {
    Fixture {
        gauge_length_m: 0.1,
        width_m: 0.06,
        warp_axis: [0.0, 1.0],
        cells: [2, 4],
    }
}

fn quantity(value: f64, unit: &str) -> Value {
    json!({
        "value": value, "unit": unit, "source": "synthetic", "evidence": "estimate",
        "method": "Chosen synthetic constitutive input; not an empirical textile observation"
    })
}

fn selection() -> Selection {
    serde_json::from_value(json!({
        "id": "synthetic-strip", "allow_estimates": true,
        "record": {
            "id": "synthetic-strip", "name": "Synthetic orthotropic sheet", "form": "woven",
            "properties": {
                "warp_stiffness": quantity(PARAMETERS[0], "N/m"),
                "weft_stiffness": quantity(PARAMETERS[1], "N/m"),
                "poisson_warp_weft": quantity(PARAMETERS[2], "1"),
                "shear_stiffness": quantity(PARAMETERS[3], "N/m")
            },
            "sources": { "synthetic": {
                "reference": "test:strip-experiment",
                "accessed_on": "2026-10-21",
                "note": "Synthetic independent mechanics and admission test; no real specimen"
            }}
        }
    }))
    .unwrap()
}

fn request() -> Request {
    Request {
        schema: "ftx-strip-experiment/1".into(),
        material: selection(),
        fixture: fixture(),
        extensions: vec![0.0, 0.1],
        controls: controls(),
    }
}

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "actual {actual:e}, expected {expected:e}, tolerance {tolerance:e}"
    );
}

#[test]
fn uncoupled_uniform_strip_matches_green_strain_force_energy_and_reaction() {
    // With nu=0 an axis-aligned affine extension is the exact equilibrium, including at the grips.
    // Independently: E=(lambda²-1)/2, P=lambda*Y*E, force=W*P, energy=L*W*Y*E²/2.
    for (axis, modulus, component) in [
        ([0.0, 1.0], PARAMETERS[0], 0),
        ([1.0, 0.0], PARAMETERS[1], 1),
    ] {
        for cells in [[2, 2], [4, 4]] {
            let f = Fixture {
                warp_axis: axis,
                cells,
                ..fixture()
            };
            for extension in [0.0, 0.1, 0.3] {
                let state = solve(&f, PARAMETERS, extension, &controls()).unwrap();
                let lambda = 1.0 + extension;
                let strain = (lambda * lambda - 1.0) / 2.0;
                let force = f.width_m * lambda * modulus * strain;
                let energy = f.gauge_length_m * f.width_m * modulus * strain * strain / 2.0;
                close(state.force_n, force, 1e-10 * (1.0 + force));
                close(state.fixed_grip_force_n, -force, 1e-10 * (1.0 + force));
                close(state.energy_j, energy, 1e-12 * (1.0 + energy));
                close(state.mid_width_m, f.width_m, 1e-12);
                assert!(state.free_force_residual_n < 1e-10);
                assert!(state.grip_balance_residual_n < 1e-10);
                for (j, range) in state.green_strain_range.iter().enumerate() {
                    let expected = if j == component { strain } else { 0.0 };
                    close(range[0], expected, 1e-12);
                    close(range[1], expected, 1e-12);
                }
                for (i, position) in state.positions_m.iter().enumerate() {
                    close(
                        position[0],
                        f.width_m * (i % (cells[0] + 1)) as f64 / cells[0] as f64,
                        1e-12,
                    );
                    close(
                        position[1],
                        f.gauge_length_m * lambda * (i / (cells[0] + 1)) as f64 / cells[1] as f64,
                        1e-12,
                    );
                    assert_eq!(position[2], 0.0);
                }
            }
        }
    }
}

#[test]
fn clamped_coupled_strip_contracts_nonuniformly_and_reactions_obey_virtual_work() {
    let f = fixture();
    let parameters = [800.0, 400.0, 0.25, 120.0];
    let extension = 0.1;
    let state = solve(&f, parameters, extension, &controls()).unwrap();
    let [nx, ny] = f.cells;
    for row in [0, ny] {
        for column in 0..=nx {
            let p = state.positions_m[row * (nx + 1) + column];
            close(p[0], f.width_m * column as f64 / nx as f64, 1e-14);
            close(
                p[1],
                f.gauge_length_m * (1.0 + extension) * row as f64 / ny as f64,
                1e-14,
            );
            assert_eq!(p[2], 0.0);
        }
    }
    let mid = ny / 2 * (nx + 1);
    assert!(state.positions_m[mid][0] > 1e-5);
    assert!(state.positions_m[mid + nx][0] < f.width_m - 1e-5);
    assert!(state.mid_width_m < f.width_m - 1e-5);
    assert!(state.green_strain_range[1][0] < -1e-5);
    assert!(state.green_strain_range[0][1] - state.green_strain_range[0][0] > 1e-5);
    assert!(state.free_force_residual_n <= controls().force_tolerance_n);
    assert!(state.grip_balance_residual_n <= controls().force_tolerance_n);
    close(state.fixed_grip_force_n, -state.force_n, 1e-6);

    // The derivative is with respect to actual grip displacement, L*extension, not strain alone.
    let h = 1e-5;
    let before = solve(&f, parameters, extension - h, &controls()).unwrap();
    let after = solve(&f, parameters, extension + h, &controls()).unwrap();
    let energy_derivative = (after.energy_j - before.energy_j) / (2.0 * h * f.gauge_length_m);
    close(energy_derivative, state.force_n, 1e-5 * state.force_n.abs());
}

#[test]
fn forward_uses_only_admitted_membrane_data_and_freezes_authored_units_for_replay() {
    let mut input = request();
    let material = input.material.record.as_mut().unwrap();
    assert_eq!(material.properties.len(), 4);
    // No mass, thickness, bending or contact properties are supplied or inferred.
    for key in ["warp_stiffness", "weft_stiffness", "shear_stiffness"] {
        let q = material.properties.get_mut(key).unwrap();
        q.value /= 1000.0;
        q.unit = "kN/m".into();
    }
    let report = evaluate(&input).unwrap();
    close(report.states[1].force_n, 0.06 * 1.1 * 800.0 * 0.105, 1e-10);
    assert_eq!(report.schema, "ftx-strip-experiment-report/1");
    assert_eq!(
        report
            .input
            .material
            .record
            .as_ref()
            .unwrap()
            .properties
            .len(),
        4
    );
    assert_eq!(
        report.input.material.record.as_ref().unwrap().properties["warp_stiffness"].unit,
        "kN/m"
    );
    assert!(report.input.material.expected_sha256.is_some());
    assert_eq!(
        report.input_sha256,
        sha256_hex(&serde_json::to_vec(&report.input).unwrap())
    );
    let portable: Request =
        serde_json::from_slice(&serde_json::to_vec(&report.input).unwrap()).unwrap();
    let replay = evaluate(&portable).unwrap();
    assert_eq!(
        serde_json::to_value(&report).unwrap(),
        serde_json::to_value(replay).unwrap()
    );

    let mut stale = portable;
    stale
        .material
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("warp_stiffness")
        .unwrap()
        .value *= 2.0;
    let error = evaluate(&stale).unwrap_err();
    assert_eq!(error.code, RefusalCode::InvalidDocument);
    assert!(error.message.contains("expected_sha256"));
}

fn tabulated_selection() -> Selection {
    let mut selection = selection();
    let material = selection.record.as_mut().unwrap();
    material.properties.clear();
    let curve = |slope: f64| {
        json!({
            "points": [[-0.5, -0.5*slope], [0.0, 0.0], [1.0, slope]],
            "source": "synthetic", "evidence": "estimate",
            "method": "Synthetic linear second-Piola versus Green-strain law"
        })
    };
    material.membrane_response = Some(
        serde_json::from_value(json!({
            "model": "tabulated_green_strain/1", "interpolation": "piecewise_linear",
            "warp": curve(PARAMETERS[0]), "weft": curve(PARAMETERS[1]),
            "shear": curve(PARAMETERS[3]), "coupling": quantity(0.0, "N/m")
        }))
        .unwrap(),
    );
    selection
}

#[test]
fn tabulated_membrane_uses_the_same_force_law_and_characterized_domain() {
    let mut input = request();
    input.material = tabulated_selection();
    let report = evaluate(&input).unwrap();
    close(report.states[1].force_n, 0.06 * 1.1 * 800.0 * 0.105, 1e-10);
    close(
        report.states[1].energy_j,
        0.1 * 0.06 * 800.0 * 0.105_f64.powi(2) / 2.0,
        1e-12,
    );
    input.extensions = vec![1.0]; // E=1.5 exceeds this explicitly authored table's E=1 bound.
    let error = evaluate(&input).unwrap_err();
    assert_eq!(error.code, RefusalCode::OutOfRange);
    assert!(error.message.contains("strain"));
}

#[test]
fn neither_scalar_nor_tabulated_membranes_promote_fibers_yarns_or_fill_into_sheets() {
    for material in [selection(), tabulated_selection()] {
        for form in [Form::Fiber, Form::Yarn, Form::Fill] {
            let mut input = request();
            input.material = material.clone();
            input.material.record.as_mut().unwrap().form = form;
            let error = evaluate(&input).unwrap_err();
            assert_eq!(error.code, RefusalCode::InvalidDocument);
        }
    }
}

#[test]
fn missing_units_estimates_and_material_history_cannot_silently_fall_back() {
    let mut input = request();
    input.material.allow_estimates = false;
    assert!(evaluate(&input).unwrap_err().message.contains("estimate"));

    let mut input = request();
    input
        .material
        .record
        .as_mut()
        .unwrap()
        .properties
        .remove("shear_stiffness");
    assert!(
        evaluate(&input)
            .unwrap_err()
            .message
            .contains("shear_stiffness")
    );

    let mut input = request();
    input
        .material
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("warp_stiffness")
        .unwrap()
        .unit = "Pa".into();
    assert!(evaluate(&input).unwrap_err().message.contains("unit"));

    let mut input = request();
    input.material.record.as_mut().unwrap().structure.insert(
        "membrane_plasticity_model".into(),
        "unknown-history-selector".into(),
    );
    assert!(evaluate(&input).unwrap_err().message.contains("history"));

    let mut input = request();
    input.material.record.as_mut().unwrap().membrane_relaxation = Some(serde_json::from_value(json!({
        "model": "green_maxwell/1", "initialization": "stress_free_reference",
        "strain_range": [[-0.4, 1.0], [-0.4, 1.0], [-0.5, 1.0]],
        "source": "synthetic", "method": "Synthetic history refusal case",
        "branches": [{"id": "synthetic-branch", "stiffness_ratio": quantity(0.5, "1"), "relaxation_time": quantity(1.0, "s")}]
    })).unwrap());
    let error = evaluate(&input).unwrap_err();
    assert!(error.message.contains("duration"), "{error:?}");
}

#[test]
fn fitted_strip_domain_survives_frozen_replay_and_rejects_escape_or_stripped_bounds() {
    let mut input = request();
    let material = input.material.record.as_mut().unwrap();
    // Test-authored metadata exercises the consumer boundary; inverse-fit accuracy has its own tests.
    material.sources.get_mut("synthetic").unwrap().reference =
        "local:ftx-strip-calibration/synthetic-domain-admission-test".into();
    let ranges = serde_json::to_string(&[[-0.1, 0.2], [-0.1, 0.2], [-0.1, 0.1]]).unwrap();
    for q in material.properties.values_mut() {
        q.conditions
            .insert("fitted_green_strain_range".into(), ranges.clone());
    }
    let report = evaluate(&input).unwrap();
    let replay = evaluate(&report.input).unwrap();
    assert_eq!(report.input_sha256, replay.input_sha256);
    assert_eq!(
        serde_json::to_value(report.states).unwrap(),
        serde_json::to_value(replay.states).unwrap()
    );

    let mut outside = report.input.clone();
    outside.extensions = vec![0.3]; // E=0.345, outside the retained [−0.1, 0.2] FIT domain.
    let error = evaluate(&outside).unwrap_err();
    assert_eq!(error.code, RefusalCode::OutOfRange);
    assert!(error.message.contains("fitted ranges"));

    let mut stripped = input.clone();
    for q in stripped
        .material
        .record
        .as_mut()
        .unwrap()
        .properties
        .values_mut()
    {
        q.conditions.clear();
    }
    assert!(
        evaluate(&stripped)
            .unwrap_err()
            .message
            .contains("missing its Green-strain domain")
    );

    let mut inconsistent = input;
    inconsistent
        .material
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("weft_stiffness")
        .unwrap()
        .conditions
        .insert(
            "fitted_green_strain_range".into(),
            "[[-0.1,0.1],[-0.1,0.1],[-0.1,0.1]]".into(),
        );
    assert!(
        evaluate(&inconsistent)
            .unwrap_err()
            .message
            .contains("share one source and Green-strain domain")
    );
}

#[test]
fn malformed_fixture_controls_extensions_and_unstable_laws_are_refused() {
    let invalid_fixtures = [
        Fixture {
            width_m: 0.0,
            ..fixture()
        },
        Fixture {
            gauge_length_m: f64::INFINITY,
            ..fixture()
        },
        Fixture {
            warp_axis: [0.6, 0.6],
            ..fixture()
        },
        Fixture {
            cells: [3, 4],
            ..fixture()
        },
        Fixture {
            cells: [32, 32],
            ..fixture()
        },
        Fixture {
            cells: [usize::MAX, 4],
            ..fixture()
        },
    ];
    for f in invalid_fixtures {
        assert_eq!(
            solve(&f, PARAMETERS, 0.1, &controls()).unwrap_err().code,
            RefusalCode::InvalidDocument
        );
    }
    for extension in [-0.01, 2.01, f64::NAN] {
        assert_eq!(
            solve(&fixture(), PARAMETERS, extension, &controls())
                .unwrap_err()
                .code,
            RefusalCode::InvalidDocument
        );
    }
    for control in [
        Controls {
            max_newton: 0,
            ..controls()
        },
        Controls {
            force_tolerance_n: 0.0,
            ..controls()
        },
        Controls {
            tolerance_m: f64::NAN,
            ..controls()
        },
    ] {
        assert_eq!(
            solve(&fixture(), PARAMETERS, 0.1, &control)
                .unwrap_err()
                .code,
            RefusalCode::InvalidDocument
        );
    }
    for parameters in [[800.0, 400.0, 2.0, 120.0], [800.0, 400.0, 0.0, -1.0]] {
        assert_eq!(
            solve(&fixture(), parameters, 0.1, &controls())
                .unwrap_err()
                .code,
            RefusalCode::OutOfRange
        );
    }
    for extensions in [vec![], vec![0.0; 65]] {
        let mut input = request();
        input.extensions = extensions;
        assert_eq!(
            evaluate(&input).unwrap_err().code,
            RefusalCode::InvalidDocument
        );
    }
}

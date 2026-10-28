//! Independent RC and pressure/work answers; no measured textile or private wearer evidence.
use ftx_drape::thermal::{
    Boundary, DryThermalModel, adaptive,
    compression::{CompressedStack, Control, ControlledHold, Hold, Input, Setup, evaluate},
};
use ftx_pattern::{
    material_compression::{self, stack},
    materials::{CATALOG_SCHEMA, Catalog},
};

fn preparation() -> material_compression::Input {
    let mut p: material_compression::Input = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-compression.json"
    ))
    .unwrap();
    p.pressure_pa = 0.0;
    p
}

fn setup(n: usize, cells: usize) -> Setup {
    Setup {
        stack: stack::Input {
            schema: stack::SCHEMA.into(),
            gap_m: n as f64 * 0.020,
            reference_area_m2: 0.25,
            direction: stack::Direction::Loading,
            layers: (0..n)
                .map(|i| stack::Layer {
                    id: format!("layer-{i}"),
                    preparation: preparation(),
                })
                .collect(),
            allow_estimates: true,
        },
        cells_per_layer: cells,
        initial_temperature_c: vec![20.0; n * cells],
        thermal_evidence: "Synthetic constant heat capacities and transport; prescribed interfaces and reservoirs; mechanical heat conversion omitted for numerical verification".into(),
    }
}

fn hold(gap: f64, n: usize) -> Hold {
    Hold {
        seconds: 10.0,
        gap_m: gap,
        interface_resistance_m2_k_w: vec![0.0; n - 1],
        inside_c: 40.0,
        outside_c: 0.0,
        inside_resistance_m2_k_w: 0.0,
        outside_resistance_m2_k_w: 0.0,
    }
}

fn near(a: f64, b: f64, tolerance: f64) {
    assert!((a - b).abs() <= tolerance, "{a} != {b}");
}

fn fingerprint(s: &CompressedStack) -> serde_json::Value {
    serde_json::json!({"compression": s.compression(), "temperature":s.temperatures(), "last":s.last()})
}

fn controlled(control: Control, n: usize) -> ControlledHold {
    let thermal = hold(0.020 * n as f64, n);
    ControlledHold {
        seconds: thermal.seconds,
        control,
        interface_resistance_m2_k_w: thermal.interface_resistance_m2_k_w,
        inside_c: thermal.inside_c,
        outside_c: thermal.outside_c,
        inside_resistance_m2_k_w: thermal.inside_resistance_m2_k_w,
        outside_resistance_m2_k_w: thermal.outside_resistance_m2_k_w,
    }
}

#[test]
fn explicit_pressure_then_gap_preserve_the_same_mass_heat_and_branch_work_history() {
    let mut initial = setup(1, 1);
    initial.initial_temperature_c[0] = 0.0;
    let mut state = CompressedStack::new(&initial).unwrap();
    let first = state
        .advance_controlled(&controlled(Control::Pressure { pressure_pa: 250.0 }, 1))
        .unwrap();
    let t1 = ((2.0 / 0.375) * 40.0) / (10.0 + 4.0 / 0.375);
    near(first.temperature_c[0], t1, 1e-12);
    near(first.thickness_m, 0.015, 1e-14);
    near(first.external_work_j, 0.15625, 1e-12);
    near(first.reference_mass_kg, 0.025, 1e-14);
    assert_eq!(first.prescribed_pressure_pa, Some(250.0));
    assert_eq!(state.compression().prescribed_pressure_pa, Some(250.0));
    assert_eq!(state.compression().input.gap_m, 0.020);
    let second = state
        .advance_controlled(&controlled(Control::Gap { gap_m: 0.010 }, 1))
        .unwrap();
    let t2 = (10.0 * t1 + (2.0 / 0.25) * 40.0) / (10.0 + 4.0 / 0.25);
    near(second.temperature_c[0], t2, 1e-12);
    near(second.cumulative_external_work_j, 0.625, 1e-12);
    near(second.cumulative_stored_change_j, 25.0 * t2, 1e-10);
    near(second.cumulative_heat_balance_error_j, 0.0, 1e-10);
    assert_eq!(second.prescribed_pressure_pa, None);
    assert_eq!(state.compression().prescribed_pressure_pa, None);
    assert_eq!(second.elapsed_s, 20.0);
    let held = state
        .advance_controlled(&controlled(Control::Pressure { pressure_pa: 500.0 }, 1))
        .unwrap();
    assert_eq!(held.external_work_j, 0.0);
    assert_eq!(held.elapsed_s, 30.0);
    assert!(held.temperature_c[0] > second.temperature_c[0]);
}

#[test]
fn source_pressure_plateau_updates_heat_without_an_inferred_pressure_or_reset() {
    let mut initial = setup(1, 1);
    initial.initial_temperature_c[0] = 0.0;
    let p = &mut initial.stack.layers[0].preparation;
    let mut middle = p.samples[0].clone();
    middle.pressure_pa = 250.0;
    for q in middle.properties.values_mut() {
        q.conditions.insert("pressure_pa".into(), "250".into());
        q.bounds = None;
    }
    middle.properties.get_mut("thickness").unwrap().value = 15.0;
    middle
        .properties
        .get_mut("thermal_resistance")
        .unwrap()
        .value = 0.375;
    let end = p.samples[1].properties.get_mut("thickness").unwrap();
    end.value = 1.5; // This original row is in centimetres.
    end.bounds = None;
    p.samples.insert(1, middle);
    let mut state = CompressedStack::new(&initial).unwrap();
    let original = fingerprint(&state);
    assert!(state.advance(&hold(0.015, 1)).is_err());
    assert_eq!(fingerprint(&state), original);
    let first = state
        .advance_controlled(&controlled(Control::Pressure { pressure_pa: 375.0 }, 1))
        .unwrap();
    near(first.thickness_m, 0.015, 1e-14);
    near(first.external_work_j, 0.15625, 1e-12);
    let second = state
        .advance_controlled(&controlled(Control::Pressure { pressure_pa: 500.0 }, 1))
        .unwrap();
    near(second.thickness_m, first.thickness_m, 0.0);
    assert_eq!(second.external_work_j, 0.0);
    assert_ne!(first.layer_material_sha256, second.layer_material_sha256);
    assert!(second.temperature_c[0] > first.temperature_c[0]);
    near(second.cumulative_heat_balance_error_j, 0.0, 1e-10);
}

#[test]
fn pressure_failures_and_exhausted_adaptive_holds_roll_back_all_physical_state() {
    let mut initial = setup(1, 1);
    initial.initial_temperature_c[0] = 0.0;
    let mut state = CompressedStack::new(&initial).unwrap();
    state
        .advance_controlled(&controlled(Control::Pressure { pressure_pa: 250.0 }, 1))
        .unwrap();
    let accepted = fingerprint(&state);
    for pressure in [-1.0, 200.0, 501.0, f64::NAN, f64::INFINITY] {
        assert!(
            state
                .advance_controlled(&controlled(
                    Control::Pressure {
                        pressure_pa: pressure
                    },
                    1
                ))
                .is_err()
        );
        assert_eq!(fingerprint(&state), accepted);
    }
    let mut late = controlled(Control::Pressure { pressure_pa: 500.0 }, 1);
    late.outside_c = -300.0;
    assert!(state.advance_controlled(&late).is_err());
    assert_eq!(fingerprint(&state), accepted);
    late.outside_c = 0.0;
    let mut options = adaptive_options();
    options.max_cell_solves = 5;
    let mut budget = adaptive::Budget::new(options).unwrap();
    assert_eq!(
        state
            .advance_controlled_adaptive(&late, &mut budget)
            .unwrap_err()
            .code,
        ftx_core::RefusalCode::BudgetExceeded
    );
    assert_eq!(fingerprint(&state), accepted);
    assert_eq!(budget.used()[1], 5);
    let mut budget = adaptive::Budget::new(adaptive_options()).unwrap();
    let solved = state
        .advance_controlled_adaptive(&late, &mut budget)
        .unwrap();
    assert!(solved.integration.is_some());
    assert_eq!(solved.prescribed_pressure_pa, Some(500.0));
    near(solved.cumulative_heat_balance_error_j, 0.0, 1e-8);
}

fn adaptive_options() -> adaptive::Options {
    adaptive::Options {
        absolute_temperature_tolerance_k: 1e-6,
        absolute_heat_tolerance_j_m2: 1e-4,
        relative_tolerance: 0.0,
        min_step_s: 1e-5,
        max_step_s: 10.0,
        max_trials: 10_000,
        max_cell_solves: 262_144,
    }
}

#[test]
fn adaptive_history_matches_analytic_rc_heat_and_retains_the_original_final_material() {
    let mut initial = setup(1, 1);
    initial.initial_temperature_c[0] = 0.0;
    let mut input = Input {
        schema: "ftx-compressed-thermal-history/1".into(),
        setup: initial,
        holds: vec![hold(0.015, 1), hold(0.010, 1)],
        adaptive: None,
    };
    let legacy = evaluate(&input).unwrap();
    assert!(legacy.steps.iter().all(|s| s.integration.is_none()));
    assert!(
        !serde_json::to_string(&legacy)
            .unwrap()
            .contains("\"integration\"")
    );
    input.adaptive = Some(adaptive_options());
    let report = evaluate(&input).unwrap();
    assert_eq!(
        serde_json::to_value(&report.final_compression).unwrap(),
        serde_json::to_value(&legacy.final_compression).unwrap()
    );
    let (mut temperature, mut work) = (0.0_f64, 0);
    for (step, resistance) in report.steps.iter().zip([0.375_f64, 0.25]) {
        // Exact one-cell continuous RC solution, independent of either time integrator.
        let rate = 4.0 / (100.0 * resistance);
        let decay = (-rate * 10.0).exp();
        let temperature_integral = 200.0 + (temperature - 20.0) * (1.0 - decay) / rate;
        let entering = 0.25 * (2.0 / resistance) * (400.0 - temperature_integral);
        let leaving = 0.25 * (2.0 / resistance) * temperature_integral;
        temperature = 20.0 + (temperature - 20.0) * decay;
        near(step.temperature_c[0], temperature, 0.01);
        near(step.inside_heat_j, entering, 0.25);
        near(step.outside_heat_j, leaving, 0.25);
        near(step.cumulative_heat_balance_error_j, 0.0, 1e-8);
        near(step.reference_mass_kg, 0.025, 1e-14);
        let integration = step.integration.as_ref().unwrap();
        assert!(integration.max_accepted_error_ratio <= 1.0);
        assert!(integration.rejected_trials > 0);
        assert!(integration.accepted_substeps > 2);
        assert_eq!(integration.cell_solves, integration.trials * 3);
        assert_eq!(
            integration.cumulative_cell_solves,
            work + integration.cell_solves
        );
        work = integration.cumulative_cell_solves;
    }
    let first = &report.steps[0];
    assert!((first.inside_heat_j - 2.5 * first.inside_flux_w_m2).abs() > 0.1);
    assert!((first.temperature_c[0] - legacy.steps[0].temperature_c[0]).abs() > 1.0);
    near(report.steps[1].cumulative_external_work_j, 0.625, 1e-12);

    // A second hold cannot reset the actual attempted-work budget.
    input.adaptive.as_mut().unwrap().max_cell_solves =
        report.steps[0].integration.as_ref().unwrap().cell_solves;
    assert_eq!(
        evaluate(&input).unwrap_err().code,
        ftx_core::RefusalCode::BudgetExceeded
    );
    input.adaptive.as_mut().unwrap().max_cell_solves = 262_145;
    assert_eq!(
        evaluate(&input).unwrap_err().code,
        ftx_core::RefusalCode::BudgetExceeded
    );
}

#[test]
fn adaptive_failure_rolls_back_physics_without_refunding_partial_attempts() {
    let mut initial = setup(1, 1);
    initial.initial_temperature_c[0] = 0.0;
    let mut state = CompressedStack::new(&initial).unwrap();
    let original = fingerprint(&state);
    let mut options = adaptive_options();
    options.max_cell_solves = 5;
    let mut budget = adaptive::Budget::new(options).unwrap();
    let h = hold(0.015, 1);
    assert_eq!(
        state.advance_adaptive(&h, &mut budget).unwrap_err().code,
        ftx_core::RefusalCode::BudgetExceeded
    );
    assert_eq!(fingerprint(&state), original);
    assert_eq!(budget.used()[1], 5);
    assert!(state.advance_adaptive(&h, &mut budget).is_err());
    assert_eq!(fingerprint(&state), original);
    assert_eq!(budget.used()[1], 5);

    let mut options = adaptive_options();
    options.min_step_s = 10.0;
    let mut budget = adaptive::Budget::new(options).unwrap();
    assert_eq!(
        state.advance_adaptive(&h, &mut budget).unwrap_err().code,
        ftx_core::RefusalCode::NotConverged
    );
    assert_eq!(budget.used(), [1, 3]);
    assert_eq!(fingerprint(&state), original);
    let mut budget = adaptive::Budget::new(adaptive_options()).unwrap();
    assert!(state.advance_adaptive(&h, &mut budget).is_ok());
    assert_ne!(fingerprint(&state), original);
}

#[test]
fn compression_changes_resistance_without_resetting_or_creating_heat() {
    let mut initial = setup(1, 1);
    initial.initial_temperature_c[0] = 0.0;
    let mut compressed = CompressedStack::new(&initial).unwrap();
    let mut unloaded = compressed.clone();
    let first = compressed.advance(&hold(0.015, 1)).unwrap();
    let free = unloaded.advance(&hold(0.020, 1)).unwrap();
    // C=100 J/(m² K). R changes 0.5 -> 0.375; two half-cell conductances are 2/R.
    // (C/dt + 4/R) T_new = C/dt T_old + (2/R)(T_inside+T_outside).
    let expected = ((2.0 / 0.375) * 40.0) / (100.0 / 10.0 + 4.0 / 0.375);
    near(first.temperature_c[0], expected, 1e-12);
    assert!(first.temperature_c[0] > free.temperature_c[0]);
    near(first.pressure_pa, 250.0, 1e-10);
    near(first.normal_force_n, 62.5, 1e-10);
    near(first.reference_mass_kg, 0.025, 1e-14);
    near(first.external_work_j, 0.15625, 1e-12);
    near(first.stored_change_j, 25.0 * expected, 1e-11);
    near(first.heat_balance_error_j, 0.0, 1e-10);
    let second = compressed.advance(&hold(0.010, 1)).unwrap();
    let expected_second = (10.0 * expected + (2.0 / 0.25) * 40.0) / (10.0 + 4.0 / 0.25);
    near(second.temperature_c[0], expected_second, 1e-12);
    near(second.external_work_j, 0.46875, 1e-12);
    near(second.cumulative_external_work_j, 0.625, 1e-12);
    near(
        second.cumulative_stored_change_j,
        25.0 * expected_second,
        1e-10,
    );
    near(second.cumulative_heat_balance_error_j, 0.0, 1e-10);
    assert_ne!(first.layer_material_sha256, second.layer_material_sha256);
    // A hold at the characterized endpoint is legal; it adds heat, but no mechanical work.
    let third = compressed.advance(&hold(0.010, 1)).unwrap();
    near(third.external_work_j, 0.0, 0.0);
    assert!(third.temperature_c[0] > second.temperature_c[0]);
}

#[test]
fn squeeze_at_uniform_temperature_does_not_turn_external_work_into_heat() {
    let mut state = CompressedStack::new(&setup(2, 4)).unwrap();
    let mut input = hold(0.030, 2);
    input.inside_c = 20.0;
    input.outside_c = 20.0;
    let r = state.advance(&input).unwrap();
    assert_eq!(r.temperature_c, vec![20.0; 8]);
    assert_eq!(r.stored_change_j, 0.0);
    assert_eq!(r.inside_heat_j, 0.0);
    assert_eq!(r.outside_heat_j, 0.0);
    near(r.external_work_j, 0.3125, 1e-12);
    near(r.reference_mass_kg, 0.05, 1e-14);
}

#[test]
fn unequal_layer_storage_and_declared_interfaces_match_an_independent_two_cell_solve() {
    let mut initial = setup(2, 1);
    initial.initial_temperature_c = vec![0.0, 0.0];
    initial.stack.layers[1]
        .preparation
        .material
        .properties
        .get_mut("areal_density")
        .unwrap()
        .value = 300.0;
    let mut state = CompressedStack::new(&initial).unwrap();
    let mut h = hold(0.030, 2);
    h.interface_resistance_m2_k_w[0] = 0.125;
    h.inside_resistance_m2_k_w = 0.1;
    h.outside_resistance_m2_k_w = 0.2;
    let r = state.advance(&h).unwrap();
    let gi = 1.0 / (0.1 + 0.375 / 2.0);
    let go = 1.0 / (0.2 + 0.375 / 2.0);
    let gm = 1.0 / (0.375 + 0.125);
    let a = 100.0 / 10.0 + gi + gm;
    let d = 300.0 / 10.0 + go + gm;
    let b0 = gi * 40.0;
    let det = a * d - gm * gm;
    near(r.temperature_c[0], b0 * d / det, 1e-12);
    near(r.temperature_c[1], b0 * gm / det, 1e-12);
    near(r.reference_mass_kg, 0.1, 1e-14);
    near(
        r.stored_change_j,
        0.25 * (100.0 * r.temperature_c[0] + 300.0 * r.temperature_c[1]),
        1e-11,
    );
    // Returned selections are ordinary source-locked materials, not private thermal coefficients.
    let catalog = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: vec![],
    };
    let layers = state
        .compression()
        .layers
        .iter()
        .map(|l| {
            let selection = serde_json::from_str(&l.conditioned.selection_json).unwrap();
            catalog.resolve(&selection).unwrap()
        })
        .collect();
    let ordinary = DryThermalModel::new(layers, h.interface_resistance_m2_k_w.clone(), 1).unwrap();
    let direct = ordinary
        .advance(
            &[0.0, 0.0],
            10.0,
            Boundary {
                inside_c: 40.0,
                outside_c: 0.0,
                inside_resistance: 0.1,
                outside_resistance: 0.2,
            },
        )
        .unwrap();
    assert_eq!(r.temperature_c, direct.temperature_c);
    let locks: Vec<_> = ordinary.snapshot()["layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["sha256"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(r.layer_material_sha256, locks);
}

#[test]
fn failed_compression_or_late_thermal_step_rolls_back_every_field() {
    let mut state = CompressedStack::new(&setup(2, 2)).unwrap();
    state.advance(&hold(0.030, 2)).unwrap();
    let before = fingerprint(&state);
    let valid = hold(0.025, 2);
    for mutate in [
        |h: &mut Hold| h.gap_m = 0.050,     // wrong direction
        |h: &mut Hold| h.gap_m = 0.010,     // extrapolation
        |h: &mut Hold| h.inside_c = -300.0, // fails AFTER valid compression
        |h: &mut Hold| h.outside_c = f64::NAN,
        |h: &mut Hold| h.interface_resistance_m2_k_w.clear(),
        |h: &mut Hold| h.interface_resistance_m2_k_w[0] = -1.0,
        |h: &mut Hold| h.seconds = 0.0,
        |h: &mut Hold| h.seconds = f64::MIN_POSITIVE, // not representable after elapsed=10
        |h: &mut Hold| h.inside_c = f64::MAX,         // finite input, unrepresentable heat load
    ] {
        let mut bad = valid.clone();
        mutate(&mut bad);
        assert!(state.advance(&bad).is_err());
        assert_eq!(fingerprint(&state), before);
    }
    let mut untouched = state.clone();
    assert_eq!(
        serde_json::to_value(state.advance(&valid).unwrap()).unwrap(),
        serde_json::to_value(untouched.advance(&valid).unwrap()).unwrap()
    );
}

#[test]
fn preload_and_unloading_have_separate_signed_work_without_a_guessed_recovery_law() {
    let mut initial = setup(1, 1);
    initial.stack.gap_m = 0.015;
    initial.stack.layers[0].preparation.pressure_pa = 250.0;
    initial.stack.direction = stack::Direction::Unloading;
    // A synthetic unloading acquisition: same numbers, explicitly requalified branch throughout.
    let p = &mut initial.stack.layers[0].preparation;
    p.loading_branch = "unloading".into();
    for q in p
        .material
        .properties
        .values_mut()
        .chain(p.samples.iter_mut().flat_map(|s| s.properties.values_mut()))
    {
        q.conditions
            .insert("loading_branch".into(), "unloading".into());
    }
    for i in p.invariants.values_mut() {
        i.conditions
            .insert("loading_branch".into(), "unloading".into());
    }
    let mut state = CompressedStack::new(&initial).unwrap();
    let r = state.advance(&hold(0.020, 1)).unwrap();
    near(r.pressure_pa, 0.0, 0.0);
    near(r.external_work_j, -0.15625, 1e-12);
    let before = fingerprint(&state);
    assert!(state.advance(&hold(0.015, 1)).is_err());
    assert_eq!(fingerprint(&state), before);
}

#[test]
fn thermal_storage_needs_explicit_pressure_invariance_and_transport() {
    let initial = setup(1, 1);
    for key in ["areal_density", "specific_heat"] {
        let mut bad = initial.clone();
        bad.stack.layers[0].preparation.invariants.remove(key);
        assert!(CompressedStack::new(&bad).is_err());
        let mut bad = initial.clone();
        let p = &mut bad.stack.layers[0].preparation;
        for row in &mut p.samples {
            row.properties
                .insert(key.into(), p.material.properties[key].clone());
        }
        assert!(CompressedStack::new(&bad).is_err());
    }
    let mut bad = initial.clone();
    for row in &mut bad.stack.layers[0].preparation.samples {
        row.properties.remove("thermal_resistance");
    }
    // Geometry alone can still be characterized; this consumer additionally needs transport.
    assert!(stack::close(&bad.stack).is_ok());
    assert!(CompressedStack::new(&bad).is_err());
    let mut bad = initial.clone();
    bad.thermal_evidence.clear();
    assert!(CompressedStack::new(&bad).is_err());
    let mut bad = initial.clone();
    bad.stack.layers[0].preparation.allow_estimates = false;
    assert!(CompressedStack::new(&bad).is_err());
    let mut bad = initial;
    bad.initial_temperature_c[0] = f64::INFINITY;
    assert!(CompressedStack::new(&bad).is_err());
}

#[test]
fn history_is_deterministic_conservative_and_bounded_with_no_partial_success() {
    let mut input = Input {
        schema: "ftx-compressed-thermal-history/1".into(),
        adaptive: None,
        setup: setup(2, 3),
        holds: (0..80)
            .map(|i| {
                let mut h = hold(0.040 - i as f64 * 0.0002, 2);
                h.seconds = 0.2;
                h.inside_c = 25.0 + (i % 7) as f64;
                h.outside_c = 10.0 + (i % 11) as f64;
                h.interface_resistance_m2_k_w[0] = 0.01 + (i % 5) as f64 * 0.01;
                h
            })
            .collect(),
    };
    let original = serde_json::to_value(&input).unwrap();
    let report = evaluate(&input).unwrap();
    assert_eq!(serde_json::to_value(&input).unwrap(), original);
    for r in &report.steps {
        assert!(r.temperature_c.iter().all(|&t| (10.0..=31.0).contains(&t)));
        near(r.reference_mass_kg, 0.05, 1e-14);
        near(r.cumulative_heat_balance_error_j, 0.0, 1e-9);
    }
    assert_eq!(
        serde_json::to_value(&report).unwrap(),
        serde_json::to_value(evaluate(&input).unwrap()).unwrap()
    );
    let mut direct = input.setup.stack.clone();
    direct.gap_m = input.holds.last().unwrap().gap_m;
    near(
        report.steps.last().unwrap().cumulative_external_work_j,
        stack::close(&direct).unwrap().external_work_j,
        1e-12,
    );
    input.holds.last_mut().unwrap().inside_c = -300.0;
    assert!(evaluate(&input).is_err());
    input.holds = vec![hold(0.03, 2); 513];
    assert_eq!(
        evaluate(&input).unwrap_err().code,
        ftx_core::RefusalCode::BudgetExceeded
    );
    let mut json = original;
    json["setup"]["cells_per_layer"] = serde_json::json!(usize::MAX);
    let input: Input = serde_json::from_value(json).unwrap();
    assert!(evaluate(&input).is_err());
}

//! Synthetic acquired-path fixture with an independent hysteresis-area and one-cell RC answer.
//! No real textile data or assertion that loop work has been measured as generated heat.
use ftx_core::RefusalCode;
use ftx_drape::thermal::{
    Boundary, DryThermalModel, adaptive,
    compression::{
        CompressedStack, Control, ControlledHold, Hold, Setup,
        path::{self, BranchChange, Leg, TransitionEvidence},
    },
};
use ftx_pattern::{
    material_compression::{self, stack},
    materials::{CATALOG_SCHEMA, Catalog, Evidence},
};
use serde_json::{Value, json};

fn setup(n: usize, cells: usize) -> Setup {
    let mut p: material_compression::Input = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-compression.json"
    ))
    .unwrap();
    p.pressure_pa = 0.0;
    // Independent reference observations apply to both branches in THIS synthetic experiment.
    // An original value restricted to a different loading branch must not be silently transferred.
    for q in p.material.properties.values_mut() {
        q.conditions.remove("loading_branch");
    }
    Setup {
        stack: stack::Input {
            schema: stack::SCHEMA.into(), gap_m: 0.020 * n as f64,
            reference_area_m2: 0.25, direction: stack::Direction::Loading,
            layers: (0..n).map(|i| stack::Layer { id: format!("layer-{i}"), preparation: p.clone() }).collect(),
            allow_estimates: true,
        },
        cells_per_layer: cells, initial_temperature_c: vec![0.0; n*cells],
        thermal_evidence: "Synthetic constant storage across the declared branches; reservoirs and interfaces are prescribed; no work-to-heat conversion".into(),
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
fn change(initial: &Setup, unloading: bool) -> BranchChange {
    let mut entry = initial.stack.clone();
    let (from, to, pressure, thickness) = if unloading {
        ("loading", "unloading-after-500", 500.0, 0.010)
    } else {
        ("unloading-after-500", "loading", 0.0, 0.020)
    };
    entry.gap_m = thickness * entry.layers.len() as f64;
    entry.direction = if unloading {
        stack::Direction::Unloading
    } else {
        stack::Direction::Loading
    };
    let mut transitions = std::collections::BTreeMap::new();
    for layer in &mut entry.layers {
        let p = &mut layer.preparation;
        p.pressure_pa = pressure;
        p.loading_branch = to.into();
        if unloading {
            // Piecewise linear unloading: (0 Pa,20 mm), (250 Pa,12 mm), (500 Pa,10 mm).
            // Loading used (0 Pa,20 mm), (500 Pa,10 mm). The loop area is 0.75 J/m².
            let mut middle = p.samples[0].clone();
            middle.pressure_pa = 250.0;
            middle.properties.get_mut("thickness").unwrap().value = 12.0;
            middle.properties.get_mut("thickness").unwrap().bounds = None;
            middle
                .properties
                .get_mut("thermal_resistance")
                .unwrap()
                .value = 0.3;
            for q in middle.properties.values_mut() {
                q.conditions.insert("pressure_pa".into(), "250".into());
            }
            p.samples.insert(1, middle);
        }
        for sample in &mut p.samples {
            for q in sample.properties.values_mut() {
                q.conditions.insert("loading_branch".into(), to.into());
            }
        }
        for invariant in p.invariants.values_mut() {
            invariant
                .conditions
                .insert("loading_branch".into(), to.into());
        }
        transitions.insert(layer.id.clone(), TransitionEvidence {
            source: "synthetic".into(), evidence: Evidence::Estimate,
            method: "Explicit invented cyclic acquisition after the 500 Pa peak and the 0 Pa return; no minor-loop or recovery-time inference".into(),
            conditions: [
                ("specimen".into(), p.specimen.clone()), ("conditioning".into(), p.conditioning.clone()),
                ("from_loading_branch".into(), from.into()), ("to_loading_branch".into(), to.into()),
                ("pressure_pa".into(), pressure.to_string()),
            ].into(),
        });
    }
    BranchChange { entry, transitions }
}
fn input() -> path::Input {
    let setup = setup(1, 1);
    path::Input {
        schema: "ftx-compressed-thermal-history/2".into(),
        adaptive: None,
        legs: vec![
            Leg {
                branch_change: None,
                holds: vec![hold(0.010, 1)],
            },
            Leg {
                branch_change: Some(change(&setup, true)),
                holds: vec![hold(0.012, 1), hold(0.020, 1)],
            },
            Leg {
                branch_change: Some(change(&setup, false)),
                holds: vec![hold(0.015, 1)],
            },
        ],
        setup,
    }
}
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-9, "{a} != {b}");
}
fn fingerprint(s: &CompressedStack) -> Value {
    json!({"compression": s.compression(), "temperature":s.temperatures(), "last":s.last()})
}

fn controlled_input() -> path::ControlledInput {
    let mut value = serde_json::to_value(input()).unwrap();
    value["schema"] = json!("ftx-compressed-thermal-history/3");
    let mut pressures = [500.0, 250.0, 0.0, 250.0].into_iter();
    for leg in value["legs"].as_array_mut().unwrap() {
        for hold in leg["holds"].as_array_mut().unwrap() {
            hold.as_object_mut().unwrap().remove("gap_m");
            hold["control"] = json!({"kind":"pressure","pressure_pa":pressures.next().unwrap()});
        }
    }
    serde_json::from_value(value).unwrap()
}

#[test]
fn pressure_driven_source_cycles_keep_temperatures_and_independent_work_ledgers() {
    let legacy = path::evaluate(&input()).unwrap();
    let mut request = controlled_input();
    // Switching back to a clearance boundary does not change the active reloading branch.
    request.legs[2].holds[0].control = Control::Gap { gap_m: 0.015 };
    let result = path::evaluate_controlled(&request).unwrap();
    assert_eq!(result.schema, "ftx-compressed-thermal-history-report/3");
    assert_eq!(result.transitions.len(), 2);
    for (actual, expected) in result.steps.iter().zip(&legacy.steps) {
        near(actual.temperature_c[0], expected.temperature_c[0]);
        near(actual.external_work_j, expected.external_work_j);
        near(
            actual.cumulative_stored_change_j,
            expected.cumulative_stored_change_j,
        );
        near(actual.cumulative_heat_balance_error_j, 0.0);
        near(actual.reference_mass_kg, 0.025);
    }
    assert_eq!(result.steps[0].prescribed_pressure_pa, Some(500.0));
    assert_eq!(result.steps[1].prescribed_pressure_pa, Some(250.0));
    assert_eq!(result.steps[2].prescribed_pressure_pa, Some(0.0));
    assert_eq!(result.steps[3].prescribed_pressure_pa, None);
    near(result.steps[2].cumulative_external_work_j, 0.75 * 0.25);
    near(
        result.steps[3].cumulative_external_work_j,
        (0.75 + 0.625) * 0.25,
    );
    assert_eq!(result.steps[3].elapsed_s, 40.0);
    for transition in &result.transitions {
        assert_eq!(transition.result.entry_compression.external_work_j, 0.0);
    }
    let frozen = &result.final_compression.layers[0]
        .conditioned
        .selection_json;
    let material = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: vec![],
    }
    .resolve(&serde_json::from_str(frozen).unwrap())
    .unwrap();
    assert_eq!(result.steps[3].layer_material_sha256[0], material.sha256());
}

#[test]
fn controlled_branch_changes_cannot_leak_on_missing_evidence_or_late_thermal_failure() {
    let request = controlled_input();
    let mut state = CompressedStack::new(&request.setup).unwrap();
    state.advance_controlled(&request.legs[0].holds[0]).unwrap();
    let before = fingerprint(&state);
    let transition = request.legs[1].branch_change.as_ref().unwrap();
    let mut hold: ControlledHold = request.legs[1].holds[0].clone();
    let mut bad = transition.clone();
    bad.transitions.values_mut().next().unwrap().source = "absent".into();
    assert!(state.advance_controlled_on(&hold, &bad).is_err());
    assert_eq!(fingerprint(&state), before);
    hold.outside_c = -300.0;
    assert!(state.advance_controlled_on(&hold, transition).is_err());
    assert_eq!(fingerprint(&state), before);
    hold.outside_c = 0.0;
    let accepted = state.advance_controlled_on(&hold, transition).unwrap();
    near(accepted.step.thickness_m, 0.012);
    assert_eq!(accepted.step.prescribed_pressure_pa, Some(250.0));
    assert_eq!(accepted.step.elapsed_s, 20.0);
    near(accepted.step.cumulative_heat_balance_error_j, 0.0);
    // Reusing the just-consumed source transition at a different physical state must refuse.
    let last = fingerprint(&state);
    assert!(state.advance_controlled_on(&hold, transition).is_err());
    assert_eq!(fingerprint(&state), last);
}

#[test]
fn independently_qualified_pressure_join_can_enter_a_flat_unloading_interval() {
    let request = controlled_input();
    let mut state = CompressedStack::new(&request.setup).unwrap();
    state.advance_controlled(&request.legs[0].holds[0]).unwrap();
    let mut transition = request.legs[1].branch_change.clone().unwrap();
    for layer in &mut transition.entry.layers {
        let middle = &mut layer.preparation.samples[1];
        middle.properties.get_mut("thickness").unwrap().value = 10.0;
        middle
            .properties
            .get_mut("thermal_resistance")
            .unwrap()
            .value = 0.25;
    }
    // The source link already establishes the actual 500 Pa join. Its flat 250..500 Pa
    // thickness interval cannot determine that pressure by inverse clearance alone.
    assert!(
        stack::close(&transition.entry)
            .unwrap_err()
            .message
            .contains("nonunique")
    );
    let mut hold = request.legs[1].holds[0].clone();
    // The join is established by its known preload even when the target hold uses clearance.
    // On the new branch 14 mm uniquely gives 150 Pa; work from 500 Pa is -200*.004*.25 J.
    let mut gap_state = state.clone();
    hold.control = Control::Gap { gap_m: 0.014 };
    let gap = gap_state.advance_controlled_on(&hold, &transition).unwrap();
    assert_eq!(gap.entry_compression.prescribed_pressure_pa, Some(500.0));
    assert_eq!(gap.entry_compression.external_work_j, 0.0);
    assert_eq!(gap.step.prescribed_pressure_pa, None);
    near(gap.step.pressure_pa, 150.0);
    near(gap.step.thickness_m, 0.014);
    near(gap.step.external_work_j, -0.2);
    near(gap.step.cumulative_external_work_j, 0.425);
    near(gap.step.cumulative_heat_balance_error_j, 0.0);
    hold.control = Control::Pressure { pressure_pa: 375.0 };
    let result = state.advance_controlled_on(&hold, &transition).unwrap();
    assert_eq!(result.entry_compression.prescribed_pressure_pa, Some(500.0));
    assert_eq!(result.entry_compression.external_work_j, 0.0);
    assert_eq!(result.step.prescribed_pressure_pa, Some(375.0));
    near(result.step.thickness_m, 0.010);
    assert_eq!(result.step.external_work_j, 0.0);
    near(result.step.cumulative_external_work_j, 0.625);
    near(result.step.cumulative_heat_balance_error_j, 0.0);
    assert!(result.step.temperature_c[0] > request.setup.initial_temperature_c[0]);
}

#[test]
fn pressure_controlled_paths_keep_one_adaptive_budget_across_reversals() {
    let mut request = controlled_input();
    request.adaptive = Some(adaptive_options());
    let result = path::evaluate_controlled(&request).unwrap();
    let mut work = 0;
    for step in &result.steps {
        let integration = step.integration.as_ref().unwrap();
        work += integration.cell_solves;
        assert_eq!(integration.cumulative_cell_solves, work);
        near(step.cumulative_heat_balance_error_j, 0.0);
    }
    let first_work = result.steps[0].integration.as_ref().unwrap().cell_solves;
    request.adaptive.as_mut().unwrap().max_cell_solves = first_work;
    assert_eq!(
        path::evaluate_controlled(&request).unwrap_err().code,
        RefusalCode::BudgetExceeded
    );
}

fn adaptive_options() -> adaptive::Options {
    adaptive::Options {
        absolute_temperature_tolerance_k: 1e-6,
        absolute_heat_tolerance_j_m2: 1e-4,
        relative_tolerance: 0.0,
        min_step_s: 1e-5,
        max_step_s: 10.0,
        max_trials: 20_000,
        max_cell_solves: 262_144,
    }
}

#[test]
fn adaptive_cycles_keep_one_budget_across_reversals_without_resetting_heat_or_sources() {
    let mut request = input();
    let legacy = path::evaluate(&request).unwrap();
    request.adaptive = Some(adaptive_options());
    let report = path::evaluate(&request).unwrap();
    assert_eq!(report.transitions.len(), 2);
    assert_eq!(
        serde_json::to_value(&report.final_compression).unwrap(),
        serde_json::to_value(&legacy.final_compression).unwrap()
    );
    let (mut temperature, mut work) = (0.0_f64, 0);
    for ((step, old), resistance) in report
        .steps
        .iter()
        .zip(&legacy.steps)
        .zip([0.25_f64, 0.3, 0.5, 0.375])
    {
        temperature = 20.0 + (temperature - 20.0) * (-40.0 / (100.0 * resistance)).exp();
        assert!((step.temperature_c[0] - temperature).abs() < 0.01);
        assert_eq!(step.external_work_j, old.external_work_j);
        assert_eq!(step.layer_material_sha256, old.layer_material_sha256);
        assert!(step.cumulative_heat_balance_error_j.abs() < 1e-8);
        let integration = step.integration.as_ref().unwrap();
        assert_eq!(
            integration.cumulative_cell_solves,
            work + integration.cell_solves
        );
        assert!(integration.max_accepted_error_ratio <= 1.0);
        work = integration.cumulative_cell_solves;
    }
    for transition in &report.transitions {
        assert_eq!(
            serde_json::to_value(&transition.result.step).unwrap(),
            serde_json::to_value(&report.steps[transition.before_hold]).unwrap()
        );
    }
    request.adaptive.as_mut().unwrap().max_cell_solves =
        report.steps[0].integration.as_ref().unwrap().cell_solves;
    assert_eq!(
        path::evaluate(&request).unwrap_err().code,
        RefusalCode::BudgetExceeded
    );
}

#[test]
fn refused_adaptive_reversal_preserves_the_active_branch_but_charges_attempted_work() {
    let request = input();
    let report = {
        let mut warmup = request.clone();
        warmup.adaptive = Some(adaptive_options());
        path::evaluate(&warmup).unwrap()
    };
    let mut options = adaptive_options();
    options.max_cell_solves = report.steps[0].integration.as_ref().unwrap().cell_solves + 5;
    let mut budget = adaptive::Budget::new(options).unwrap();
    let mut state = CompressedStack::new(&request.setup).unwrap();
    state
        .advance_adaptive(&request.legs[0].holds[0], &mut budget)
        .unwrap();
    let before = fingerprint(&state);
    let used = budget.used()[1];
    let leg = &request.legs[1];
    let reversal = leg.branch_change.as_ref().unwrap();
    assert_eq!(
        state
            .advance_on_adaptive(&leg.holds[0], reversal, &mut budget)
            .unwrap_err()
            .code,
        RefusalCode::BudgetExceeded
    );
    assert_eq!(fingerprint(&state), before);
    assert_eq!(budget.used()[1], used + 5);
    assert!(
        state
            .advance_on_adaptive(&leg.holds[0], reversal, &mut budget)
            .is_err()
    );
    assert_eq!(fingerprint(&state), before);
    assert_eq!(budget.used()[1], used + 5);
}

#[test]
fn cyclic_branches_retain_temperature_and_match_independent_work_and_rc_answers() {
    let input = input();
    let original = serde_json::to_value(&input).unwrap();
    let report = path::evaluate(&input).unwrap();
    assert_eq!(
        report
            .transitions
            .iter()
            .map(|t| t.before_hold)
            .collect::<Vec<_>>(),
        vec![1, 3]
    );
    let mut temperature = 0.0;
    let mut work = 0.0;
    for (i, (step, (r, p, dw))) in report
        .steps
        .iter()
        .zip([
            (0.25, 500.0, 0.625),
            (0.3, 250.0, -0.1875),
            (0.5, 0.0, -0.25),
            (0.375, 250.0, 0.15625),
        ])
        .enumerate()
    {
        temperature = (10.0 * temperature + 2.0 / r * 40.0) / (10.0 + 4.0 / r);
        work += dw;
        near(step.temperature_c[0], temperature);
        near(step.pressure_pa, p);
        near(step.external_work_j, dw);
        near(step.cumulative_external_work_j, work);
        near(step.reference_mass_kg, 0.025);
        near(step.cumulative_stored_change_j, 25.0 * temperature);
        near(step.cumulative_heat_balance_error_j, 0.0);
        near(step.elapsed_s, (i + 1) as f64 * 10.0);
    }
    near(report.steps[2].cumulative_external_work_j, 0.1875);
    for transition in &report.transitions {
        assert_eq!(transition.result.entry_compression.external_work_j, 0.0);
        assert_eq!(
            transition.result.entry_compression.pressure_pa,
            transition.result.entry_compression.initial_pressure_pa
        );
        assert_eq!(
            transition.result.step.elapsed_s,
            report.steps[transition.before_hold].elapsed_s
        );
    }
    let material = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: vec![],
    }
    .resolve(&report.final_compression.layers[0].conditioned.selection)
    .unwrap();
    assert_eq!(report.steps[3].layer_material_sha256[0], material.sha256());
    near(material.thermal_resistance().unwrap(), 0.375);
    assert_eq!(original, serde_json::to_value(&input).unwrap());
    assert_eq!(
        serde_json::to_value(&report).unwrap(),
        serde_json::to_value(path::evaluate(&input).unwrap()).unwrap()
    );
}

#[test]
fn positive_loop_work_is_not_injected_as_heat_and_mass_cell_profiles_are_not_reset() {
    let mut input = input();
    input.setup.initial_temperature_c = vec![20.0];
    for leg in &mut input.legs {
        for h in &mut leg.holds {
            h.inside_c = 20.0;
            h.outside_c = 20.0;
        }
    }
    let report = path::evaluate(&input).unwrap();
    assert!(report.steps[2].cumulative_external_work_j > 0.0);
    for s in report.steps {
        near(s.temperature_c[0], 20.0);
        near(s.cumulative_stored_change_j, 0.0);
    }

    let mut initial = setup(2, 2);
    initial.initial_temperature_c = vec![5.0, 15.0, 25.0, 35.0];
    let mut state = CompressedStack::new(&initial).unwrap();
    state.advance(&hold(0.020, 2)).unwrap();
    let old_t = state.temperatures().to_vec();
    let mut h = hold(0.024, 2);
    h.interface_resistance_m2_k_w[0] = 0.2;
    let result = state.advance_on(&h, &change(&initial, true)).unwrap();
    let catalog = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: vec![],
    };
    let layers = state
        .compression()
        .layers
        .iter()
        .map(|l| catalog.resolve(&l.conditioned.selection).unwrap())
        .collect();
    let expected = DryThermalModel::new(layers, vec![0.2], 2)
        .unwrap()
        .advance(
            &old_t,
            10.0,
            Boundary {
                inside_c: 40.0,
                outside_c: 0.0,
                inside_resistance: 0.0,
                outside_resistance: 0.0,
            },
        )
        .unwrap();
    assert_eq!(result.step.temperature_c, expected.temperature_c);
    assert_ne!(result.step.temperature_c, initial.initial_temperature_c);
    near(result.step.reference_mass_kg, 0.05);
}

#[test]
fn discontinuous_individual_plies_cannot_hide_behind_the_same_total_gap() {
    let initial = setup(2, 1);
    let mut state = CompressedStack::new(&initial).unwrap();
    state.advance(&hold(0.020, 2)).unwrap();
    let before = fingerprint(&state);
    let mut bad = change(&initial, true);
    // At 500 Pa these still sum to 20 mm. Neither physical layer meets its preceding state.
    for (layer, thickness) in bad.entry.layers.iter_mut().zip([0.8, 1.2]) {
        let q = layer.preparation.samples[2]
            .properties
            .get_mut("thickness")
            .unwrap();
        q.value = thickness;
        q.bounds = None;
    }
    assert!(state.advance_on(&hold(0.024, 2), &bad).is_err());
    assert_eq!(fingerprint(&state), before);
}

#[test]
fn reversal_requires_the_actual_source_history_and_unchanged_identity() {
    let initial = setup(1, 1);
    let mut state = CompressedStack::new(&initial).unwrap();
    state.advance(&hold(0.010, 1)).unwrap();
    let before = fingerprint(&state);
    // Without an explicit reversal, the old one-direction behavior is unchanged.
    assert!(state.advance(&hold(0.012, 1)).is_err());
    for mutate in [
        |c: &mut BranchChange| c.transitions.clear(),
        |c: &mut BranchChange| c.transitions.get_mut("layer-0").unwrap().source = "unknown".into(),
        |c: &mut BranchChange| c.transitions.get_mut("layer-0").unwrap().method = " ".into(),
        |c: &mut BranchChange| {
            c.transitions
                .get_mut("layer-0")
                .unwrap()
                .conditions
                .insert("from_loading_branch".into(), "different-peak".into());
        },
        |c: &mut BranchChange| {
            c.transitions
                .get_mut("layer-0")
                .unwrap()
                .conditions
                .insert("pressure_pa".into(), "250".into());
        },
        |c: &mut BranchChange| c.entry.direction = stack::Direction::Loading,
        |c: &mut BranchChange| c.entry.reference_area_m2 = 0.5,
        |c: &mut BranchChange| c.entry.layers[0].id = "replacement".into(),
        |c: &mut BranchChange| c.entry.layers[0].preparation.material.id = "another-sheet".into(),
        |c: &mut BranchChange| c.entry.layers[0].preparation.specimen = "another-specimen".into(),
        |c: &mut BranchChange| c.entry.layers[0].preparation.conditioning = "wet".into(),
        |c: &mut BranchChange| c.entry.layers[0].preparation.pressure_pa = 250.0,
        |c: &mut BranchChange| c.entry.layers[0].preparation.allow_estimates = false,
        |c: &mut BranchChange| {
            c.entry.layers[0]
                .preparation
                .invariants
                .remove("specific_heat");
        },
        |c: &mut BranchChange| c.entry.gap_m = 0.011,
    ] {
        let mut changed = change(&initial, true);
        mutate(&mut changed);
        assert!(state.advance_on(&hold(0.012, 1), &changed).is_err());
        assert_eq!(fingerprint(&state), before);
    }
    state
        .advance_on(&hold(0.012, 1), &change(&initial, true))
        .unwrap();
    // A receipt qualifying unloading from 500 Pa cannot be replayed as a new transition at 250 Pa.
    assert!(
        state
            .advance_on(&hold(0.015, 1), &change(&initial, false))
            .is_err()
    );
}

#[test]
fn late_thermal_failure_rolls_back_branch_selection_and_all_ledgers_before_retry() {
    let initial = setup(1, 1);
    let mut state = CompressedStack::new(&initial).unwrap();
    state.advance(&hold(0.010, 1)).unwrap();
    let mut clean = state.clone();
    let before = fingerprint(&state);
    let mut invalid = hold(0.012, 1);
    invalid.outside_c = -300.0;
    assert!(state.advance_on(&invalid, &change(&initial, true)).is_err());
    assert_eq!(fingerprint(&state), before);
    // Repeating the current loading endpoint still works; failed unload never became active.
    let same = state.advance(&hold(0.010, 1)).unwrap();
    let expected = clean.advance(&hold(0.010, 1)).unwrap();
    assert_eq!(
        serde_json::to_value(same).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    let retry = state
        .advance_on(&hold(0.012, 1), &change(&initial, true))
        .unwrap();
    let correct = clean
        .advance_on(&hold(0.012, 1), &change(&initial, true))
        .unwrap();
    assert_eq!(
        serde_json::to_value(retry).unwrap(),
        serde_json::to_value(correct).unwrap()
    );
}

#[test]
fn whole_path_limits_apply_across_legs_and_a_late_bad_leg_publishes_nothing() {
    let mut request = input();
    request.legs[2].holds[0].outside_c = -300.0;
    assert!(path::evaluate(&request).is_err());
    for mutate in [
        |v: &mut path::Input| v.legs.clear(),
        |v: &mut path::Input| v.legs = vec![v.legs[0].clone(); 33],
        |v: &mut path::Input| v.legs[1].holds.clear(),
        |v: &mut path::Input| {
            v.legs[0].holds = vec![hold(0.020, 1); 256];
            v.legs[1].holds = vec![hold(0.020, 1); 257];
        },
        |v: &mut path::Input| {
            v.setup = setup(32, 64);
            v.legs[0].holds = vec![hold(0.640, 32); 129];
        },
    ] {
        let mut bad = input();
        mutate(&mut bad);
        assert_eq!(
            path::evaluate(&bad).unwrap_err().code,
            RefusalCode::BudgetExceeded
        );
    }
    let mut unknown = serde_json::to_value(input()).unwrap();
    unknown["legs"][1]["branch_change"]["heat_fraction"] = json!(0.9);
    assert!(serde_json::from_value::<path::Input>(unknown).is_err());
}

#[test]
fn splitting_one_branch_into_legs_preserves_legacy_results_and_links_change_path_identity() {
    let initial = setup(1, 1);
    let holds = vec![hold(0.015, 1), hold(0.010, 1)];
    let old = ftx_drape::thermal::compression::evaluate(&ftx_drape::thermal::compression::Input {
        schema: "ftx-compressed-thermal-history/1".into(),
        adaptive: None,
        setup: initial.clone(),
        holds: holds.clone(),
    })
    .unwrap();
    let segmented = path::evaluate(&path::Input {
        schema: "ftx-compressed-thermal-history/2".into(),
        adaptive: None,
        setup: initial,
        legs: holds
            .into_iter()
            .map(|h| Leg {
                branch_change: None,
                holds: vec![h],
            })
            .collect(),
    })
    .unwrap();
    assert!(segmented.transitions.is_empty());
    assert_eq!(
        serde_json::to_value(old.steps).unwrap(),
        serde_json::to_value(segmented.steps).unwrap()
    );
    assert_eq!(
        serde_json::to_value(old.final_compression).unwrap(),
        serde_json::to_value(segmented.final_compression).unwrap()
    );

    let mut request = input();
    let first = path::evaluate(&request).unwrap();
    request.legs[1]
        .branch_change
        .as_mut()
        .unwrap()
        .transitions
        .get_mut("layer-0")
        .unwrap()
        .method
        .push_str("; separately reviewed synthetic transition");
    let revised = path::evaluate(&request).unwrap();
    assert_ne!(first.input_sha256, revised.input_sha256);
    assert_eq!(
        serde_json::to_value(first.steps).unwrap(),
        serde_json::to_value(revised.steps).unwrap()
    );
    assert_ne!(
        serde_json::to_value(first.transitions).unwrap(),
        serde_json::to_value(revised.transitions).unwrap()
    );
}

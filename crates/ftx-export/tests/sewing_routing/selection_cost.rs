//! Complete fixed routes are priced from their actual qualified stitches and shared whole orders.

use super::{eligible_fixture, refresh_locks, route_selection, source};
use ftx_core::RefusalCode;
use ftx_export::{
    material_study,
    sewing::{SewOptions, SewPlan, program::SewProgram},
    sewing_selection::{Request, deadline::Status, select_json},
};
use ftx_pattern::{Pattern, material_catalog::Selection, materials::Catalog};
use serde_json::{Value, json};

fn request() -> (Pattern, Value) {
    let (pattern, mut input) = route_selection();
    let yarn = Catalog::builtin()
        .unwrap()
        .resolve(&Selection {
            id: "coats/epic-2994120-2026".into(),
            ..Selection::default()
        })
        .unwrap()
        .frozen_selection();
    input["objective"] = json!("minimize_thread_observed_order_cost");
    input["options"]["consumables"] = json!({"purchasing":{
        "quote_book":{"schema":"ftx-textile-quotes/1","quotes":[{
            "offer_id":"route-cost-yarn","material_id":yarn.id,"material_sha256":yarn.expected_sha256,
            "supplier":"synthetic-supplier","sku":"shared-yarn","price":10.0,"quantity":2.0,"unit":"linear_meter",
            "minimum_quantity":2.0,"order_increment":2.0,"price_breaks":[{"minimum_quantity":4.0,"price":7.0}],
            "currency":"USD","region":"test","quality":"new","terms":"synthetic merchandise only",
            "observed_on":"2026-10-27","available":true,"available_quantity":8.0,"maximum_continuous_quantity":4.0,
            "source":"test:route-cost-yarn","source_sha256":"ab".repeat(32)
        }]},
        "market":{"currency":"USD","region":"test","quality":"new","terms":"synthetic merchandise only",
            "as_of":"2026-10-27","max_age_days":2,"require_observed_quantity":true,"require_observed_continuous_length":true}
    }});
    for route_index in 0..2 {
        let mut options = input["options"].clone();
        if route_index == 1 {
            options["production"]["routing"] = input["route_candidates"][0]["routing"].clone();
        }
        for cell in options["production"]["routing"]["cells"]
            .as_array_mut()
            .unwrap()
        {
            cell["configuration"]["qualifications"][0]["thread_consumption"] = json!({
                "channels_complete":true,"evidence":"estimate","allow_estimates":true,
                "source":source("route-cost-thread"),"method":"Synthetic complete thread per actual specialist traversal",
                "channels":[{"id":"all-required-thread","material":yarn,
                    "metres_per_traversal":([0.01,0.02][route_index]),
                    "start_allowance_m":0.0,"end_allowance_m":0.0}]
            });
        }
        refresh_locks(&mut options);
        if route_index == 0 {
            input["options"] = options;
        } else {
            input["route_candidates"][0]["routing"] = options["production"]["routing"].clone();
        }
    }
    (pattern, input)
}

fn expensive_template(input: &mut Value) {
    for cell in input["options"]["production"]["routing"]["cells"]
        .as_array_mut()
        .unwrap()
    {
        cell["configuration"]["qualifications"][0]["thread_consumption"]["channels"][0]["metres_per_traversal"] =
            json!(0.04);
    }
    refresh_locks(&mut input["options"]);
}

#[test]
fn fixed_route_cost_uses_actual_overrides_and_needs_no_makespan_optimum() {
    let (pattern, mut raw) = request();
    let input: Request = serde_json::from_value(raw.clone()).unwrap();
    let cheap = input.select(&pattern).unwrap();
    assert_eq!(cheap.schema, "ftx-sewing-selection-report/6");
    assert_eq!(cheap.selected_route.as_deref(), Some("template"));
    assert_eq!(cheap.selected_thread_order_cost, Some(10.0));
    assert_eq!(cheap.objective_currency.as_deref(), Some("USD"));
    assert_eq!(
        cheap
            .selected_plan
            .as_ref()
            .unwrap()
            .parallel_makespan_seconds(),
        Some(23.1)
    );
    assert!(cheap.optimality_proven && cheap.unresolved_trials == 0);
    assert!(cheap.selected_objective_seconds.is_none());
    assert!(cheap.tie_break.contains("complete thread order cost"));
    assert!(!cheap.tie_break.contains("makespan"));
    assert_eq!(
        cheap
            .trials
            .iter()
            .map(|trial| trial.thread_order_cost)
            .collect::<Vec<_>>(),
        [Some(10.0), Some(14.0)]
    );
    assert!(cheap.trials.iter().all(|trial| {
        trial.objective_seconds.is_none() && trial.ordering_optimality_proven.is_none()
    }));

    expensive_template(&mut raw);
    let selected = serde_json::from_value::<Request>(raw.clone())
        .unwrap()
        .select(&pattern)
        .unwrap();
    assert_eq!(
        selected.selected_route.as_deref(),
        Some("available-specialists")
    );
    assert_eq!(selected.trials[0].thread_order_cost, Some(21.0));
    assert_eq!(selected.selected_thread_order_cost, Some(14.0));
    assert!(selected.optimality_proven && selected.unresolved_trials == 0);
    let plan = selected.selected_plan.as_ref().unwrap();
    assert_eq!(plan.parallel_optimality_proven(), Some(false));
    assert_eq!(plan.parallel_makespan_seconds(), Some(13.1));
    assert_eq!(plan.total_stitch_intervals(), 130);
    let consumption = plan.consumables.as_ref().unwrap();
    assert!((consumption.total_required_length_m.unwrap() - 2.6).abs() < 1e-12);
    assert_eq!(
        consumption
            .purchasing
            .as_ref()
            .unwrap()
            .least_observed_independent_order_total,
        selected.selected_thread_order_cost
    );
    assert_eq!(
        serde_json::to_value(plan).unwrap(),
        serde_json::to_value(SewPlan::build(&pattern, &plan.options).unwrap()).unwrap()
    );
    let program = SewProgram::build(&pattern, &plan.options).unwrap();
    assert_eq!(program.stitches.len(), 130);
    assert!(
        program
            .stitches
            .iter()
            .all(|stitch| { matches!(stitch.cell.as_deref(), Some("cell-a" | "cell-c")) })
    );
    let portable: Value =
        serde_json::from_str(&select_json(&pattern.raw().to_string(), &raw.to_string()).unwrap())
            .unwrap();
    assert_eq!(portable["report"], serde_json::to_value(&selected).unwrap());
    let replay: SewOptions =
        serde_json::from_str(portable["selected_options_json"].as_str().unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(SewProgram::build(&pattern, &replay).unwrap()).unwrap(),
        serde_json::to_value(program).unwrap()
    );
}

#[test]
fn fixed_route_cost_deadline_preserves_cheap_exclusions_and_unproved_misses() {
    let (pattern, mut raw) = request();
    raw["completion_deadline_milliseconds"] = json!(13100);
    let mut input: Request = serde_json::from_value(raw).unwrap();
    let result = input.select(&pattern).unwrap();
    assert_eq!(result.schema, "ftx-sewing-selection-report/7");
    assert_eq!(
        result.selected_route.as_deref(),
        Some("available-specialists")
    );
    assert_eq!(result.selected_thread_order_cost, Some(14.0));
    assert!(result.optimality_proven && result.unresolved_trials == 0);
    assert_eq!(result.trials[0].thread_order_cost, Some(10.0));
    assert_eq!(
        result.trials[0]
            .completion_deadline
            .as_ref()
            .unwrap()
            .status,
        Status::Excluded
    );
    let timing = result.trials[1].completion_deadline.as_ref().unwrap();
    assert_eq!(timing.status, Status::Feasible);
    assert_eq!(timing.selected_makespan_milliseconds, Some(13100));
    assert_eq!(timing.timing_optimality_proven, Some(false));

    input.max_plan_evaluations = 1;
    let bounded = input.select(&pattern).unwrap();
    assert!(bounded.selected_plan.is_none() && bounded.budget_exhausted);
    assert!(!bounded.search_complete && !bounded.no_feasible_listed_recipe);
    assert_eq!(bounded.trials[0].thread_order_cost, Some(10.0));

    input.max_plan_evaluations = 2;
    input.completion_deadline_milliseconds = Some(13099);
    let unresolved = input.select(&pattern).unwrap();
    assert!(unresolved.selected_plan.is_none() && !unresolved.no_feasible_listed_recipe);
    assert_eq!(unresolved.unresolved_trials, 1);
    assert_eq!(unresolved.trials[1].thread_order_cost, Some(14.0));
    assert_eq!(
        unresolved.trials[1]
            .completion_deadline
            .as_ref()
            .unwrap()
            .status,
        Status::Unresolved
    );

    input.completion_deadline_milliseconds = Some(0);
    let excluded = input.select(&pattern).unwrap();
    assert!(excluded.no_feasible_listed_recipe && excluded.unresolved_trials == 0);
}

#[test]
fn fixed_route_cost_shares_the_recipe_cap_and_preserves_canonical_ties() {
    let (pattern, mut raw) = request();
    expensive_template(&mut raw);
    let mut first = raw["route_candidates"][0].clone();
    first["id"] = json!("a-equal");
    let mut last = first.clone();
    last["id"] = json!("z-equal");
    raw["route_candidates"]
        .as_array_mut()
        .unwrap()
        .extend([last, first]);
    let pass = raw["options"]["passes"][1].clone();
    raw["alternatives"] = json!([{"operation":"reinforce","recipes":[{"id":"same-authored-pass",
        "needle":pass["needle"],"thread":pass["thread"],"max_stitch_mm":pass["max_stitch_mm"],
        "stitches_per_minute":pass["stitches_per_minute"]}]}]);
    raw["max_plan_evaluations"] = json!(8);
    let result = serde_json::from_value::<Request>(raw.clone())
        .unwrap()
        .select(&pattern)
        .unwrap();
    assert_eq!(result.candidate_combinations, 8);
    assert_eq!(result.trials.len(), 8);
    assert_eq!(result.selected_route.as_deref(), Some("a-equal"));
    assert_eq!(
        result.selected_choices.as_ref().unwrap()["reinforce"],
        "template"
    );
    assert_eq!(result.selected_thread_order_cost, Some(14.0));
    assert!(result.optimality_proven && result.search_complete);
    raw["route_candidates"].as_array_mut().unwrap().reverse();
    let reordered = serde_json::from_value::<Request>(raw.clone())
        .unwrap()
        .select(&pattern)
        .unwrap();
    assert_eq!(
        serde_json::to_value(&result.trials).unwrap(),
        serde_json::to_value(reordered.trials).unwrap()
    );
    raw["max_plan_evaluations"] = json!(3);
    let bounded = serde_json::from_value::<Request>(raw)
        .unwrap()
        .select(&pattern)
        .unwrap();
    assert!(bounded.budget_exhausted && !bounded.optimality_proven);
    assert_eq!(bounded.selected_route, result.selected_route);
    assert_eq!(
        serde_json::to_value(bounded.selected_plan).unwrap(),
        serde_json::to_value(result.selected_plan).unwrap()
    );
}

#[test]
fn fixed_route_cost_keeps_unknown_consumption_and_timing_unresolved() {
    let (pattern, raw) = request();
    for missing in ["consumption", "timing"] {
        let mut changed = raw.clone();
        if missing == "consumption" {
            changed["options"]["production"]["routing"]["cells"][0]["configuration"]["qualifications"][0]
                .as_object_mut()
                .unwrap()
                .remove("thread_consumption");
            refresh_locks(&mut changed["options"]);
        } else {
            changed["options"]["production"]["routing"]["cells"][0]["ordering"]["setup_times"] =
                Value::Null;
        }
        let result = serde_json::from_value::<Request>(changed)
            .unwrap()
            .select(&pattern)
            .unwrap();
        assert_eq!(
            result.selected_route.as_deref(),
            Some("available-specialists")
        );
        assert_eq!(result.selected_thread_order_cost, Some(14.0));
        assert!(result.search_complete && !result.optimality_proven);
        assert_eq!(result.unresolved_trials, 1);
        assert!(result.trials[0].thread_order_cost.is_none());
        assert!(result.trials[0].objective_unavailable.is_some());
        assert_eq!(result.trials[0].refusal.is_some(), missing == "timing");
    }
    let mut absent_orders = raw.clone();
    absent_orders["options"]["consumables"]["purchasing"]["quote_book"]["quotes"] = json!([]);
    let missing = serde_json::from_value::<Request>(absent_orders)
        .unwrap()
        .select(&pattern)
        .unwrap();
    assert!(missing.selected_plan.is_none() && !missing.no_feasible_listed_recipe);
    assert_eq!(missing.unresolved_trials, 2);

    let (_, eligible) = eligible_fixture(2);
    let mut unsupported = raw.clone();
    unsupported["route_candidates"][0]["routing"] = eligible["production"]["routing"].clone();
    let refusal = serde_json::from_value::<Request>(unsupported)
        .unwrap()
        .select(&pattern)
        .unwrap_err();
    assert!(refusal.message.contains("cannot claim their own minimum"));
    let mut unsupported = raw.clone();
    unsupported["objective"] = json!("minimize_feed_seconds");
    assert_eq!(
        serde_json::from_value::<Request>(unsupported)
            .unwrap()
            .select(&pattern)
            .unwrap_err()
            .code,
        RefusalCode::InvalidDocument
    );
    let mut missing_market = raw;
    missing_market["options"]["consumables"] = json!({});
    assert_eq!(
        serde_json::from_value::<Request>(missing_market)
            .unwrap()
            .select(&pattern)
            .unwrap_err()
            .code,
        RefusalCode::InvalidDocument
    );
}

#[test]
fn fixed_route_cost_study_reuses_the_winner_and_retains_the_entire_search_protocol() {
    let (pattern, mut selection) = request();
    expensive_template(&mut selection);
    let mut input = json!({"schema":"ftx-material-study/1",
        "variants":[{"key":"priced-routes","changes":{"schema":"ftx-material-bindings/1",
            "default":pattern.raw()["fabric"]["material"]}}],
        "sewing_selection":selection,
        "assembly_mass":{"schema":"ftx-assembly-mass/1","inventory_complete":true,"extras":[]},
        "comparison":{"basis":"nominal","required_consumers":["sewing","sewing_purchasing"],
            "objectives":[{"metric":"thread_observed_order_cost","direction":"minimize"}]}});
    let evaluate = |input: &Value| {
        serde_json::to_value(
            serde_json::from_value::<material_study::Request>(input.clone())
                .unwrap()
                .evaluate(&pattern)
                .unwrap(),
        )
        .unwrap()
    };
    let result = evaluate(&input);
    let row = &result["alternatives"][0];
    assert_eq!(row["sewing"]["status"], "available");
    assert_eq!(
        row["sewing_selection"]["result"]["selected_route"],
        "available-specialists"
    );
    assert_eq!(row["metrics"]["thread_observed_order_cost"], 14.0);
    assert_eq!(row["metrics"]["stitch_intervals"], 130);
    assert_eq!(result["comparison"]["frontier"], json!(["priced-routes"]));
    let bound =
        Pattern::from_json(row["binding"]["result"]["pattern_json"].as_str().unwrap()).unwrap();
    let direct = serde_json::from_value::<Request>(selection)
        .unwrap()
        .select(&bound)
        .unwrap();
    assert_eq!(
        row["sewing"]["result"],
        serde_json::to_value(direct.selected_plan).unwrap()
    );
    assert_eq!(
        row["sewing_selection"]["result"]["trials"],
        serde_json::to_value(direct.trials).unwrap()
    );
    let protocol = &result["comparison"]["alternatives"][0]["input_protocol_sha256"];
    assert!(protocol.is_string());
    for changed_field in ["candidate", "work"] {
        let mut changed = input.clone();
        if changed_field == "candidate" {
            // The unused original route still belongs to the declared cost-search domain.
            changed["sewing_selection"]["options"]["production"]["routing"]["source"]["note"] =
                json!("Different retained synthetic route observation");
        } else {
            changed["sewing_selection"]["max_plan_evaluations"] = json!(3);
        }
        let changed = evaluate(&changed);
        assert_eq!(changed["alternatives"][0]["metrics"], row["metrics"]);
        assert_ne!(
            changed["comparison"]["alternatives"][0]["input_protocol_sha256"],
            *protocol
        );
    }
    input["sewing_selection"]["completion_deadline_milliseconds"] = json!(0);
    let excluded = evaluate(&input);
    assert_eq!(excluded["alternatives"][0]["sewing"]["status"], "refused");
    assert_eq!(
        excluded["alternatives"][0]["sewing_selection"]["result"]["no_feasible_listed_recipe"],
        true
    );
    assert_eq!(
        excluded["alternatives"][0]["assembly_mass"]["status"],
        "available"
    );
    assert_eq!(
        excluded["alternatives"][0]["assembly_mass"],
        row["assembly_mass"]
    );
    assert!(excluded["alternatives"][0]["metrics"]["thread_observed_order_cost"].is_null());
    assert_eq!(excluded["comparison"]["frontier"], json!([]));
}

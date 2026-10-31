//! Complete routes and eligible-cell assignments use actual qualified stitches and whole orders.

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

fn eligible_cost(copies: usize) -> (Pattern, Value) {
    let (pattern, mut options) = eligible_fixture(copies);
    let (_, priced) = request();
    options["consumables"] = priced["options"]["consumables"].clone();
    let consumption =
        priced["options"]["production"]["routing"]["cells"][0]["configuration"]["qualifications"]
            [0]["thread_consumption"]
            .clone();
    let route = &mut options["production"]["routing"];
    route["objective"] = json!("minimize_thread_observed_order_cost");
    route["thread_order_search"] = json!({"max_order_evaluations":8});
    for cell in route["cells"].as_array_mut().unwrap() {
        let rate = if cell["id"] == "cell-c" { 0.2 } else { 0.01 };
        cell["configuration"]["qualifications"][0]["thread_consumption"] = consumption.clone();
        cell["configuration"]["qualifications"][0]["thread_consumption"]["channels"][0]["metres_per_traversal"] =
            json!(rate);
    }
    refresh_locks(&mut options);
    (pattern, options)
}

fn eligible_selection(options: &Value) -> Value {
    json!({"schema":"ftx-sewing-selection/6","options":options,
        "objective":"minimize_thread_observed_order_cost","max_plan_evaluations":2,
        "route_candidates":[{"id":"same-qualified-route","routing":options["production"]["routing"]}]})
}

fn eligible_plan(pattern: &Pattern, options: &Value) -> SewPlan {
    SewPlan::build(pattern, &serde_json::from_value(options.clone()).unwrap()).unwrap()
}

#[test]
fn eligible_cost_chooses_slower_cheap_assignments_with_an_inclusive_deadline() {
    let (pattern, mut raw) = eligible_cost(1);
    let cheap = eligible_plan(&pattern, &raw);
    let production = cheap.production.as_ref().unwrap();
    let route = production.routing.as_ref().unwrap();
    let search = route.thread_order_search.as_ref().unwrap();
    // Independent fixture arithmetic: a has 40 traversals, b has 20; 0.6m rounds to 2m/$10.
    // a: 100 load + 1000 setup + 4000 feed + 200 unload, 700 handoff, then b's 2800 = 8800.
    assert_eq!(
        (cheap.schema, production.schema, route.schema),
        (
            "ftx-sewing-plan/24",
            "ftx-sewing-production/12",
            "ftx-sewing-routing/7"
        )
    );
    assert_eq!(route.selected.makespan_milliseconds, 8800);
    assert_eq!(search.selected_order_cost, Some(10.0));
    assert_eq!(search.order_evaluations, 2);
    assert!(search.optimality_proven && route.search_complete && !route.optimality_proven);
    assert!(
        (cheap
            .consumables
            .as_ref()
            .unwrap()
            .total_required_length_m
            .unwrap()
            - 0.6)
            .abs()
            < 1e-12
    );
    assert_eq!(cheap.total_stitch_intervals(), 60);

    let mut fastest = raw.clone();
    fastest["production"]["routing"]["objective"] = json!("minimize_makespan");
    fastest["production"]["routing"]
        .as_object_mut()
        .unwrap()
        .remove("thread_order_search");
    let fast = eligible_plan(&pattern, &fastest);
    // c has 20 traversals at 0.2m, plus b's 0.2m: 4.2m rounds to 6m, 3 * $7 = $21.
    assert_eq!(fast.parallel_makespan_seconds(), Some(7.0));
    assert_eq!(
        fast.consumables
            .as_ref()
            .unwrap()
            .purchasing
            .as_ref()
            .unwrap()
            .selected_total(),
        Some(21.0)
    );
    assert_eq!(fast.schema, "ftx-sewing-plan/19");
    assert!(
        fast.production
            .as_ref()
            .unwrap()
            .routing
            .as_ref()
            .unwrap()
            .thread_order_search
            .is_none()
    );

    for (deadline, finish, cost) in [(8800, 8800, 10.0), (8799, 7000, 21.0), (7000, 7000, 21.0)] {
        raw["production"]["routing"]["thread_order_search"]["completion_deadline_milliseconds"] =
            json!(deadline);
        let admitted = eligible_plan(&pattern, &raw);
        let route = admitted
            .production
            .as_ref()
            .unwrap()
            .routing
            .as_ref()
            .unwrap();
        assert_eq!(route.selected.makespan_milliseconds, finish);
        assert_eq!(
            route
                .thread_order_search
                .as_ref()
                .unwrap()
                .selected_order_cost,
            Some(cost)
        );
        assert!(
            route
                .thread_order_search
                .as_ref()
                .unwrap()
                .optimality_proven
        );
    }
    raw["production"]["routing"]["thread_order_search"]["completion_deadline_milliseconds"] =
        json!(6999);
    assert_eq!(
        SewPlan::build(&pattern, &serde_json::from_value(raw).unwrap())
            .unwrap_err()
            .code,
        RefusalCode::NotConverged
    );
}

#[test]
fn eligible_cost_prices_mixed_physical_copies_and_replays_the_exact_program() {
    let (pattern, mut options) = eligible_cost(2);
    options["production"]["routing"]["cells"][2]["configuration"]["qualifications"][0]["thread_consumption"]
        ["channels"][0]["metres_per_traversal"] = json!(0.1);
    options["consumables"]["purchasing"]["market"]["split_orders"] =
        json!({"schema":"ftx-thread-order-allocation/1","max_search_nodes":3000});
    refresh_locks(&mut options);
    let plan = eligible_plan(&pattern, &options);
    let route = plan.production.as_ref().unwrap().routing.as_ref().unwrap();
    let joins: Vec<_> = route
        .selected
        .operations
        .iter()
        .filter(|operation| operation.operation_id == "join")
        .collect();
    assert_eq!(joins.len(), 2);
    assert!(joins.iter().any(|operation| operation.cell == "cell-a"));
    assert!(joins.iter().any(|operation| operation.cell == "cell-c"));
    assert_ne!(joins[0].copy, joins[1].copy);
    // Windows force one 40-traversal a copy and one 20-traversal c copy: .4 + 2 + .4 = 2.8m.
    let consumption = plan.consumables.as_ref().unwrap();
    assert!((consumption.total_required_length_m.unwrap() - 2.8).abs() < 1e-12);
    assert_eq!(
        route
            .thread_order_search
            .as_ref()
            .unwrap()
            .selected_order_cost,
        Some(14.0)
    );
    assert!(
        route
            .thread_order_search
            .as_ref()
            .unwrap()
            .optimality_proven
    );
    assert_eq!(
        route
            .thread_order_search
            .as_ref()
            .unwrap()
            .order_evaluations,
        2
    );
    let segments = consumption
        .purchasing
        .as_ref()
        .unwrap()
        .segments
        .as_ref()
        .unwrap();
    assert_eq!(segments.len(), 4);
    assert_eq!(
        segments.iter().filter(|segment| segment.copy == 0).count(),
        2
    );
    assert_eq!(
        segments.iter().filter(|segment| segment.copy == 1).count(),
        2
    );
    let program = SewProgram::build(&pattern, &plan.options).unwrap();
    assert_eq!(program.stitches.len(), 100);
    assert_eq!(
        serde_json::to_value(&plan).unwrap(),
        serde_json::to_value(&program.plan).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&plan).unwrap(),
        serde_json::to_value(SewPlan::build(&pattern, &plan.options).unwrap()).unwrap()
    );
}

#[test]
fn eligible_cost_keeps_nonmonotone_tiers_and_bounded_incumbents_without_false_proof() {
    let (pattern, mut raw) = eligible_cost(1);
    raw["consumables"]["purchasing"]["quote_book"]["quotes"][0]["price_breaks"] =
        json!([{"minimum_quantity":6.0,"price":1.0}]);
    let larger = eligible_plan(&pattern, &raw);
    let route = larger
        .production
        .as_ref()
        .unwrap()
        .routing
        .as_ref()
        .unwrap();
    // Complete six-metre order costs 3 * $1 = $3, less than the smaller two-metre $10 order.
    assert_eq!(
        route
            .thread_order_search
            .as_ref()
            .unwrap()
            .selected_order_cost,
        Some(3.0)
    );
    assert!(
        route
            .thread_order_search
            .as_ref()
            .unwrap()
            .optimality_proven
    );
    assert_eq!(route.selected.makespan_milliseconds, 7000);

    raw["production"]["routing"]["thread_order_search"]["max_order_evaluations"] = json!(1);
    let bounded = eligible_plan(&pattern, &raw);
    let route = bounded
        .production
        .as_ref()
        .unwrap()
        .routing
        .as_ref()
        .unwrap();
    let search = route.thread_order_search.as_ref().unwrap();
    assert_eq!(search.selected_order_cost, Some(10.0));
    assert_eq!(search.order_evaluations, 1);
    assert!(search.order_budget_exhausted && !search.optimality_proven && !route.search_complete);
    assert_eq!(route.selected.makespan_milliseconds, 8800);

    raw["production"]["routing"]["thread_order_search"]["max_order_evaluations"] = json!(8);
    raw["production"]["routing"]["max_search_nodes"] = json!(2);
    let bounded = eligible_plan(&pattern, &raw);
    let route = bounded
        .production
        .as_ref()
        .unwrap()
        .routing
        .as_ref()
        .unwrap();
    assert!(
        route.budget_exhausted
            && !route
                .thread_order_search
                .as_ref()
                .unwrap()
                .optimality_proven
    );
    assert_eq!(
        route
            .thread_order_search
            .as_ref()
            .unwrap()
            .selected_order_cost,
        Some(10.0)
    );
}

#[test]
fn eligible_cost_preserves_missing_orders_and_rejects_invalid_quotes_and_stale_qualifications() {
    let (pattern, options) = eligible_cost(1);
    let mut absent = options.clone();
    absent["consumables"]["purchasing"]["quote_book"]["quotes"] = json!([]);
    let plan = eligible_plan(&pattern, &absent);
    let search = plan
        .production
        .as_ref()
        .unwrap()
        .routing
        .as_ref()
        .unwrap()
        .thread_order_search
        .as_ref()
        .unwrap();
    assert_eq!(search.selected_order_cost, None);
    assert_eq!(search.unresolved_assignments, 2);
    assert!(!search.optimality_proven);
    let missing = serde_json::from_value::<Request>(eligible_selection(&absent))
        .unwrap()
        .select(&pattern)
        .unwrap();
    assert!(missing.selected_plan.is_none() && !missing.no_feasible_listed_recipe);
    assert_eq!(missing.unresolved_trials, 2);

    let mut zero = options.clone();
    zero["consumables"]["purchasing"]["quote_book"]["quotes"][0]["price"] = json!(0.0);
    zero["consumables"]["purchasing"]["quote_book"]["quotes"][0]["price_breaks"] = json!([]);
    // Cell assignment must preserve canonical quote admission; a zero-priced observation
    // cannot become a free supplier even though the report scorer preserves numeric zero.
    let refusal =
        SewPlan::build(&pattern, &serde_json::from_value(zero.clone()).unwrap()).unwrap_err();
    assert_eq!(refusal.code, RefusalCode::InvalidDocument);
    assert_eq!(refusal.message, "price must be finite and positive");
    let refusal = serde_json::from_value::<Request>(eligible_selection(&zero))
        .unwrap()
        .select(&pattern)
        .unwrap_err();
    assert_eq!(refusal.code, RefusalCode::InvalidDocument);
    assert_eq!(refusal.message, "price must be finite and positive");

    let mut stale = options;
    stale["production"]["routing"]["cells"][2]["configuration"]["qualifications"][0]["thread_consumption"]
        ["channels"][0]["metres_per_traversal"] = json!(0.001);
    assert!(SewPlan::build(&pattern, &serde_json::from_value(stale).unwrap()).is_err());
}

#[test]
fn eligible_cost_reserves_every_purchase_and_retains_outer_deadline_replay() {
    let (pattern, options) = eligible_cost(1);
    let mut raw = eligible_selection(&options);
    raw["completion_deadline_milliseconds"] = json!(8799);
    let result = serde_json::from_value::<Request>(raw.clone())
        .unwrap()
        .select(&pattern)
        .unwrap();
    assert_eq!(result.selected_thread_order_cost, Some(21.0));
    assert!(result.optimality_proven && result.unresolved_trials == 0);
    let plan = result.selected_plan.as_ref().unwrap();
    assert_eq!(
        plan.options
            .production
            .as_ref()
            .unwrap()
            .routing
            .as_ref()
            .unwrap()
            .thread_order_search
            .as_ref()
            .unwrap()
            .completion_deadline_milliseconds,
        Some(8799)
    );
    assert_eq!(plan.parallel_makespan_seconds(), Some(7.0));
    let portable: Value =
        serde_json::from_str(&select_json(&pattern.raw().to_string(), &raw.to_string()).unwrap())
            .unwrap();
    assert_eq!(portable["report"], serde_json::to_value(&result).unwrap());
    let replay: SewOptions =
        serde_json::from_str(portable["selected_options_json"].as_str().unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(SewPlan::build(&pattern, &replay).unwrap()).unwrap(),
        serde_json::to_value(plan).unwrap()
    );

    let mut heavy = options;
    heavy["production"]["routing"]["thread_order_search"]["max_order_evaluations"] = json!(64);
    heavy["consumables"]["purchasing"]["market"]["split_orders"] =
        json!({"schema":"ftx-thread-order-allocation/1","max_search_nodes":3000});
    let typed: SewOptions = serde_json::from_value(heavy.clone()).unwrap();
    // (one market scan + one yarn scan + one alias + 3000 allocation units) * (64 + replay).
    assert_eq!(
        ftx_export::sewing::consumables::purchasing_work_bound(&typed).unwrap(),
        195195
    );
    let outer: Request = serde_json::from_value(eligible_selection(&heavy)).unwrap();
    assert_eq!(
        outer.purchasing_work_bound().unwrap_err().code,
        RefusalCode::BudgetExceeded
    );
    let study = json!({"schema":"ftx-material-study/1",
        "variants":[{"key":"a","changes":{"schema":"ftx-material-bindings/1","default":pattern.raw()["fabric"]["material"]}},
                    {"key":"b","changes":{"schema":"ftx-material-bindings/1","default":pattern.raw()["fabric"]["material"]}}],
        "sewing":heavy});
    assert_eq!(
        serde_json::from_value::<material_study::Request>(study)
            .unwrap()
            .evaluate(&pattern)
            .unwrap_err()
            .code,
        RefusalCode::BudgetExceeded
    );
}

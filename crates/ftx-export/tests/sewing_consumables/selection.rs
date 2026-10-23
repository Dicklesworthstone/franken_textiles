//! Cost search uses complete production plans and canonical whole orders, including unknowns.

use super::{close, fixture, purchasing, quote};
use ftx_core::RefusalCode;
use ftx_export::sewing::SewPlan;
use ftx_export::sewing_selection::{Objective, Request, select_json};
use ftx_pattern::Pattern;
use serde_json::{Value, json};

fn request() -> (Pattern, Value) {
    let (pattern, mut options, yarn) = fixture();
    options["require_qualification"] = json!(true);
    let mut fast = options["qualifications"][0].clone();
    fast["id"] = json!("fast-qualified-recipe");
    fast["needle"] = json!("fast-needle");
    fast["thread"] = json!("fast-thread");
    fast["thread_consumption"]["channels"][0]["metres_per_traversal"] = json!(0.04);
    fast["thread_consumption"]["channels"][1]["metres_per_traversal"] = json!(0.08);
    options["qualifications"].as_array_mut().unwrap().push(fast);
    let mut offer = quote(&yarn, "thread-cone", "thread-a");
    offer["available_quantity"] = json!(8.0);
    offer["maximum_continuous_quantity"] = json!(3.0);
    purchasing(&mut options, vec![offer]);
    let alternatives = ["join", "reinforce"].map(|operation| {
        json!({"operation":operation,"recipes":[{
            "id":"faster","needle":"fast-needle","thread":"fast-thread",
            "max_stitch_mm":5.0,"stitches_per_minute":1000.0}]})
    });
    (
        Pattern::new(pattern),
        json!({"schema":"ftx-sewing-selection/3","objective":"minimize_thread_observed_order_cost",
            "options":options,"max_plan_evaluations":4,"alternatives":alternatives}),
    )
}

#[test]
fn complete_canonical_thread_orders_can_select_a_slower_qualified_recipe() {
    let (pattern, raw) = request();
    let mut request: Request = serde_json::from_value(raw).unwrap();
    let result = request.select(&pattern).unwrap();
    assert_eq!(result.schema, "ftx-sewing-selection-report/3");
    assert_eq!(result.objective_currency.as_deref(), Some("USD"));
    assert_eq!(result.candidate_combinations, 4);
    assert!(result.search_complete && result.optimality_proven);
    assert_eq!(result.unresolved_trials, 0);
    assert!(!result.no_feasible_listed_recipe && !result.budget_exhausted);
    assert_eq!(
        result
            .trials
            .iter()
            .map(|t| t.thread_order_cost)
            .collect::<Vec<_>>(),
        [Some(14.0), Some(21.0), Some(21.0), Some(28.0)]
    );
    assert!(result.trials.iter().all(|t| {
        t.refusal.is_none() && t.objective_unavailable.is_none() && t.objective_seconds.is_none()
    }));
    assert_eq!(
        result.selected_choices.as_ref().unwrap()["join"],
        "template"
    );
    assert_eq!(
        result.selected_choices.as_ref().unwrap()["reinforce"],
        "template"
    );
    close(result.selected_thread_order_cost.unwrap(), 14.0);
    let plan = result.selected_plan.as_ref().unwrap();
    close(plan.feed_seconds, 9.0);
    close(
        plan.consumables
            .as_ref()
            .unwrap()
            .total_required_length_m
            .unwrap(),
        3.7,
    );
    assert_eq!(plan.stitch_intervals, 90);
    assert_eq!(
        result.selected_thread_order_cost,
        plan.consumables
            .as_ref()
            .unwrap()
            .purchasing
            .as_ref()
            .unwrap()
            .least_observed_independent_order_total
    );
    let replay = SewPlan::build(&pattern, &plan.options).unwrap();
    assert_eq!(
        serde_json::to_value(plan).unwrap(),
        serde_json::to_value(replay).unwrap()
    );
    assert!(!plan.physical_seam_quality_validated && !plan.hardware_execution_authorized);

    request.schema = "ftx-sewing-selection/1".into();
    request.objective = Objective::MinimizeFeedSeconds;
    let timed = request.select(&pattern).unwrap();
    close(timed.selected_plan.as_ref().unwrap().feed_seconds, 3.0);
    close(
        timed
            .selected_plan
            .unwrap()
            .consumables
            .unwrap()
            .purchasing
            .unwrap()
            .least_observed_independent_order_total
            .unwrap(),
        28.0,
    );
}

#[test]
fn whole_order_price_threshold_can_make_the_largest_demand_cheapest() {
    let (pattern, mut raw) = request();
    raw["options"]["consumables"]["purchasing"]["quote_book"]["quotes"][0]["price_breaks"] =
        json!([{"minimum_quantity":4.0,"price":7.0},{"minimum_quantity":8.0,"price":1.0}]);
    let result = serde_json::from_value::<Request>(raw)
        .unwrap()
        .select(&pattern)
        .unwrap();
    assert!(result.optimality_proven);
    assert_eq!(
        result
            .trials
            .iter()
            .map(|t| t.thread_order_cost)
            .collect::<Vec<_>>(),
        [Some(14.0), Some(21.0), Some(21.0), Some(4.0)]
    );
    assert_eq!(result.selected_choices.as_ref().unwrap()["join"], "faster");
    assert_eq!(
        result.selected_choices.as_ref().unwrap()["reinforce"],
        "faster"
    );
    close(result.selected_thread_order_cost.unwrap(), 4.0);
    let plan = result.selected_plan.unwrap();
    close(
        plan.consumables
            .as_ref()
            .unwrap()
            .total_required_length_m
            .unwrap(),
        7.0,
    );
    let order = plan
        .consumables
        .as_ref()
        .unwrap()
        .purchasing
        .as_ref()
        .unwrap()
        .material_orders[0]
        .least_observed_order
        .as_ref()
        .unwrap();
    close(order.order_quantity, 8.0);
    close(
        order.applied_price_break.as_ref().unwrap().minimum_quantity,
        8.0,
    );
}

#[test]
fn incomplete_models_leave_real_plans_unscored_and_prevent_false_optima() {
    let (pattern, original) = request();
    for qualification in 0..2 {
        for partial in [false, true] {
            let mut raw = original.clone();
            if partial {
                raw["options"]["qualifications"][qualification]["thread_consumption"]["channels_complete"] =
                    json!(false);
            } else {
                raw["options"]["qualifications"][qualification]
                    .as_object_mut()
                    .unwrap()
                    .remove("thread_consumption");
            }
            let result = serde_json::from_value::<Request>(raw)
                .unwrap()
                .select(&pattern)
                .unwrap();
            assert!(result.search_complete && !result.budget_exhausted);
            assert!(!result.optimality_proven && !result.no_feasible_listed_recipe);
            assert_eq!(result.unresolved_trials, 3);
            assert!(
                result
                    .trials
                    .iter()
                    .all(|trial| trial.refusal.is_none() && trial.feed_seconds.is_some())
            );
            assert_eq!(
                result
                    .trials
                    .iter()
                    .filter(|trial| trial.thread_order_cost.is_none()
                        && trial.objective_unavailable.is_some())
                    .count(),
                3
            );
            close(
                result.selected_thread_order_cost.unwrap(),
                if qualification == 0 { 28.0 } else { 14.0 },
            );
            assert!(
                result
                    .selected_plan
                    .unwrap()
                    .consumables
                    .unwrap()
                    .complete_consumption_coverage
            );
        }
    }
}

#[test]
fn missing_expired_unlocked_and_wrong_currency_orders_never_become_zero_cost_winners() {
    let (pattern, original) = request();
    for mutation in 0..6 {
        let mut raw = original.clone();
        let purchasing = &mut raw["options"]["consumables"]["purchasing"];
        match mutation {
            0 => purchasing["quote_book"]["quotes"] = json!([]),
            1 => purchasing["quote_book"]["quotes"][0]["available"] = json!(false),
            2 => purchasing["quote_book"]["quotes"][0]["observed_on"] = json!("2026-10-01"),
            3 => {
                purchasing["quote_book"]["quotes"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("material_sha256");
            }
            4 => purchasing["quote_book"]["quotes"][0]["currency"] = json!("EUR"),
            _ => purchasing["quote_book"]["quotes"][0]["available_quantity"] = json!(0.0),
        }
        let result = serde_json::from_value::<Request>(raw)
            .unwrap()
            .select(&pattern)
            .unwrap();
        assert!(result.search_complete);
        assert_eq!(result.unresolved_trials, 4, "mutation {mutation}");
        assert!(!result.optimality_proven && !result.no_feasible_listed_recipe);
        assert!(result.selected_plan.is_none() && result.selected_thread_order_cost.is_none());
        assert_eq!(result.objective_currency.as_deref(), Some("USD"));
        assert!(result.trials.iter().all(|trial| trial.refusal.is_none()
            && trial.objective_unavailable.is_some()
            && trial.thread_order_cost.is_none()));
    }
}

#[test]
fn source_capacity_and_continuity_exclusions_keep_a_complete_incumbent_without_proof() {
    let (pattern, original) = request();
    for (field, limit) in [
        ("available_quantity", 4.0),
        ("maximum_continuous_quantity", 1.3),
    ] {
        let mut raw = original.clone();
        raw["options"]["consumables"]["purchasing"]["quote_book"]["quotes"][0][field] =
            json!(limit);
        let result = serde_json::from_value::<Request>(raw)
            .unwrap()
            .select(&pattern)
            .unwrap();
        close(result.selected_thread_order_cost.unwrap(), 14.0);
        assert_eq!(result.unresolved_trials, 3);
        assert!(result.search_complete && !result.optimality_proven);
    }
}

#[test]
fn explicit_unknown_stock_policy_is_preserved_independently_of_complete_price() {
    let (pattern, mut raw) = request();
    let purchasing = &mut raw["options"]["consumables"]["purchasing"];
    purchasing["market"]["require_observed_quantity"] = json!(false);
    purchasing["market"]["require_observed_continuous_length"] = json!(false);
    let offer = purchasing["quote_book"]["quotes"][0]
        .as_object_mut()
        .unwrap();
    offer.remove("available_quantity");
    offer.remove("maximum_continuous_quantity");
    let result = serde_json::from_value::<Request>(raw)
        .unwrap()
        .select(&pattern)
        .unwrap();
    assert!(result.optimality_proven);
    close(result.selected_thread_order_cost.unwrap(), 14.0);
    let price = result
        .selected_plan
        .unwrap()
        .consumables
        .unwrap()
        .purchasing
        .unwrap();
    assert!(price.complete_offer_coverage);
    assert!(
        !price.complete_observed_quantity_coverage
            && !price.complete_continuous_length_limit_coverage
    );
}

#[test]
fn finite_search_and_exact_cost_ties_retain_only_complete_deterministic_incumbents() {
    let (pattern, mut raw) = request();
    raw["max_plan_evaluations"] = json!(1);
    let result = serde_json::from_value::<Request>(raw.clone())
        .unwrap()
        .select(&pattern)
        .unwrap();
    assert!(result.budget_exhausted && !result.search_complete && !result.optimality_proven);
    assert_eq!(result.unresolved_trials, 0);
    close(result.selected_thread_order_cost.unwrap(), 14.0);
    raw["max_plan_evaluations"] = json!(4);
    raw["options"]["consumables"]["purchasing"]["quote_book"]["quotes"][0]["minimum_quantity"] =
        json!(8.0);
    raw["options"]["consumables"]["purchasing"]["quote_book"]["quotes"][0]["price"] = json!(7.0);
    raw["options"]["consumables"]["purchasing"]["quote_book"]["quotes"][0]["price_breaks"] =
        json!([]);
    let request: Request = serde_json::from_value(raw).unwrap();
    let result = request.select(&pattern).unwrap();
    assert!(result.optimality_proven);
    assert!(
        result
            .trials
            .iter()
            .all(|trial| trial.thread_order_cost == Some(28.0))
    );
    assert!(
        result
            .selected_choices
            .as_ref()
            .unwrap()
            .values()
            .all(|choice| choice == "template")
    );
    let mut reversed = request.clone();
    reversed.alternatives.reverse();
    let reordered = reversed.select(&pattern).unwrap();
    assert_eq!(
        serde_json::to_value(result.trials).unwrap(),
        serde_json::to_value(reordered.trials).unwrap()
    );
    assert_eq!(result.selected_choices, reordered.selected_choices);
}

#[test]
fn cost_requests_require_explicit_purchasing_and_canonical_market_before_recipe_enumeration() {
    let (pattern, original) = request();
    for mutation in 0..7 {
        let mut raw = original.clone();
        match mutation {
            0 => {
                raw["options"]
                    .as_object_mut()
                    .unwrap()
                    .remove("consumables");
            }
            1 => raw["options"]["consumables"] = json!({}),
            2 => raw["schema"] = json!("ftx-sewing-selection/1"),
            3 => raw["options"]["consumables"]["purchasing"]["market"]["currency"] = json!("usd"),
            4 => {
                raw["options"]["consumables"]["purchasing"]["market"]["as_of"] = json!("2026-02-30")
            }
            5 => {
                raw["options"]["consumables"]["purchasing"]["market"]["as_of_at"] =
                    json!("2026-10-23T12:00:00Z")
            }
            _ => {
                raw["options"]["consumables"]["purchasing"]["quote_book"]["quotes"][0]["price"] =
                    json!(-1.0)
            }
        }
        // Pricing syntax cannot be hidden by an otherwise unpriceable or unqualified template.
        raw["options"]["qualifications"] = json!([]);
        let refusal = serde_json::from_value::<Request>(raw)
            .unwrap()
            .select(&pattern)
            .unwrap_err();
        assert_eq!(
            refusal.code,
            RefusalCode::InvalidDocument,
            "mutation {mutation}: {refusal}"
        );
    }
}

#[test]
fn cost_work_bound_includes_the_canonical_common_market_preflight() {
    let (_, raw) = request();
    let mut request: Request = serde_json::from_value(raw).unwrap();
    assert_eq!(request.purchasing_work_bound().unwrap(), 13); // 4 * (2 quote visits + 1 alias) + 1 preflight.
    request.max_plan_evaluations = 17;
    let purchasing = request
        .options
        .consumables
        .as_mut()
        .unwrap()
        .purchasing
        .as_mut()
        .unwrap();
    let source = purchasing.quote_book.quotes[0].clone();
    purchasing.quote_book.quotes = (0..3846)
        .map(|index| {
            let mut quote = source.clone();
            quote.offer_id = format!("source-{index}");
            quote.sku = format!("sku-{index}");
            quote
        })
        .collect();
    assert_eq!(request.purchasing_work_bound().unwrap(), 199_992);
    let purchasing = request
        .options
        .consumables
        .as_mut()
        .unwrap()
        .purchasing
        .as_mut()
        .unwrap();
    let mut extra = source;
    extra.offer_id = "one-too-many".into();
    extra.sku = "one-too-many".into();
    purchasing.quote_book.quotes.push(extra);
    assert_eq!(
        request.purchasing_work_bound().unwrap_err().code,
        RefusalCode::BudgetExceeded
    );
}

#[test]
fn production_refusals_stay_separate_and_legacy_feed_wire_fields_remain_unchanged() {
    let (mut pattern, raw) = request();
    let mut request: Request = serde_json::from_value(raw).unwrap();
    let mut changed = pattern.raw().clone();
    changed["fabric"]["material"]["record"]["name"] = json!("Different material revision");
    pattern = Pattern::new(changed);
    let result = request.select(&pattern).unwrap();
    assert!(result.no_feasible_listed_recipe && result.selected_plan.is_none());
    assert_eq!(result.unresolved_trials, 0);
    assert!(result.trials.iter().all(|trial| trial.refusal.is_some()
        && trial.objective_unavailable.is_none()
        && trial.thread_order_cost.is_none()));

    request.schema = "ftx-sewing-selection/1".into();
    request.objective = Objective::MinimizeFeedSeconds;
    let report = serde_json::to_value(request.select(&pattern).unwrap()).unwrap();
    assert_eq!(report["schema"], "ftx-sewing-selection-report/1");
    assert!(report.get("selected_thread_order_cost").is_none());
    assert!(report.get("objective_currency").is_none());
    assert!(
        report["trials"]
            .as_array()
            .unwrap()
            .iter()
            .all(|trial| trial.get("thread_order_cost").is_none()
                && trial.get("objective_unavailable").is_none())
    );
}

#[test]
fn cost_selection_file_free_boundary_replays_its_exact_selected_options() {
    let (pattern, request) = request();
    let result: Value = serde_json::from_str(
        &select_json(
            &serde_json::to_string(pattern.raw()).unwrap(),
            &request.to_string(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(result["report"]["selected_thread_order_cost"], 14.0);
    assert_eq!(result["report"]["objective_currency"], "USD");
    let options = serde_json::from_str(result["selected_options_json"].as_str().unwrap()).unwrap();
    let replay = SewPlan::build(&pattern, &options).unwrap();
    assert_eq!(
        result["report"]["selected_plan"],
        serde_json::to_value(replay).unwrap()
    );
}

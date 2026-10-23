//! Synthetic source models establish software arithmetic, not measured thread use or seam quality.

use ftx_export::sewing::{SewOptions, SewPlan, consumables, program::SewProgram};
use ftx_pattern::Pattern;
use ftx_pattern::material_catalog::Selection;
use ftx_pattern::material_cost::QuoteBook;
use ftx_pattern::materials::{CATALOG_SCHEMA, Catalog};
use serde_json::{Value, json};

#[path = "sewing_consumables/material_study.rs"]
mod material_study;

fn yarn(id: &str) -> Selection {
    let selection = serde_json::from_value(json!({"id":id,"record":{
        "id":id,"name":"Synthetic source-locked thread","form":"yarn",
        "properties":{},"sources":{"fixture":{"reference":"test:thread-material",
            "accessed_on":"2026-10-23","note":"Synthetic software fixture, not a measured thread"}}
    }}))
    .unwrap();
    Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: Vec::new(),
    }
    .resolve(&selection)
    .unwrap()
    .frozen_selection()
}

fn fixture() -> (Value, Value, Selection) {
    let pattern: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-sewing-pattern.json"
    ))
    .unwrap();
    let mut options: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-sewing-options.json"
    ))
    .unwrap();
    options["machine"]["reverse"] =
        json!({"stitch_mm":[0.5,5.0],"feed_b_over_a":[0.5,2.0],"max_stitches_per_minute":1000.0});
    options["passes"][0]["id"] = json!("join");
    let mut reinforcement = options["passes"][0].clone();
    reinforcement["id"] = json!("reinforce");
    reinforcement["direction"] = json!("reverse");
    reinforcement["after"] = json!(["join"]);
    options["passes"][0]["backtack"] = json!({"start_stitches":2,"end_stitches":3});
    options["passes"]
        .as_array_mut()
        .unwrap()
        .push(reinforcement);
    let baseline = build(pattern.clone(), options.clone()).unwrap();
    let selected = yarn("synthetic-thread-a");
    options["qualifications"] = json!([{
        "id":"synthetic-seam-qualification","materials":baseline.seams[0].sides.iter().map(|s| &s.material_sha256).collect::<Vec<_>>(),
        "machine":options["machine"]["id"],"needle":options["passes"][0]["needle"],"thread":options["passes"][0]["thread"],
        "stitch_mm":[0.5,5.0],"feed_b_over_a":[0.5,2.0],"stack_mm":[0.01,3.0],
        "stitches_per_minute":[1.0,1000.0],"allow_reverse":true,
        "evidence":"estimate","allow_estimates":true,
        "source":{"reference":"test:seam-envelope","accessed_on":"2026-10-23","note":"Synthetic process envelope, not a physical qualification"},
        "method":"Synthetic operation-domain check",
        "thread_consumption":{
            "channels_complete":true,"evidence":"estimate","allow_estimates":true,
            "source":{"reference":"test:independent-thread-consumption","accessed_on":"2026-10-23","note":"Synthetic declared coefficients; independent of process-envelope evidence"},
            "method":"Invented effective per-traversal fixture with separately declared operation allowances",
            "conditions":{"scope":"software arithmetic only"},
            "channels":[
                {"id":"needle","material":selected,"metres_per_traversal":0.01,"start_allowance_m":0.1,"end_allowance_m":0.2},
                {"id":"bobbin","material":selected,"metres_per_traversal":0.02,"start_allowance_m":0.05,"end_allowance_m":0.15}
            ]
        }
    }]);
    options["consumables"] = json!({});
    (pattern, options, selected)
}

fn build(pattern: Value, options: Value) -> Result<SewPlan, ftx_core::Refusal> {
    SewPlan::build(
        &Pattern::new(pattern),
        &serde_json::from_value(options).unwrap(),
    )
}

fn quote(selection: &Selection, offer: &str, sku: &str) -> Value {
    json!({"offer_id":offer,"material_id":selection.id,"material_sha256":selection.expected_sha256,
        "supplier":"synthetic-supplier","sku":sku,"price":10.0,"quantity":2.0,"unit":"linear_meter",
        "minimum_quantity":2.0,"order_increment":2.0,
        "price_breaks":[{"minimum_quantity":4.0,"price":7.0}],
        "currency":"USD","region":"test","quality":"new","terms":"synthetic merchandise only",
        "observed_on":"2026-10-23","available":true,"available_quantity":4.0,
        "maximum_continuous_quantity":2.0,"source":"test:thread-offer","source_sha256":"ab".repeat(32)})
}

fn purchasing(options: &mut Value, quotes: Vec<Value>) {
    options["consumables"]["purchasing"] = json!({
        "quote_book":{"schema":"ftx-textile-quotes/1","quotes":quotes},
        "market":{"currency":"USD","region":"test","quality":"new","terms":"synthetic merchandise only",
            "as_of":"2026-10-23","max_age_days":2,"require_observed_quantity":true,
            "require_observed_continuous_length":true}
    });
}

fn close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-12, "{actual} != {expected}");
}

#[test]
fn actual_repeated_traversals_and_per_operation_allowances_aggregate_before_canonical_pricing() {
    let (pattern, mut options, selected) = fixture();
    purchasing(&mut options, vec![quote(&selected, "cone-a", "thread-a")]);
    let typed: SewOptions = serde_json::from_value(options.clone()).unwrap();
    let program = SewProgram::build(&Pattern::new(pattern), &typed).unwrap();
    assert_eq!(program.stitches.len(), 90); // 40 + 2*(2+3) + 40 reverse reinforcement intervals.
    let plan = &program.plan;
    assert_eq!(plan.schema, "ftx-sewing-plan/11");
    let report = plan.consumables.as_ref().unwrap();
    assert!(report.complete_consumption_coverage);
    assert_eq!(report.operation_channels.len(), 4);
    assert_eq!(
        report
            .operation_channels
            .iter()
            .map(|c| c.stitch_intervals)
            .collect::<Vec<_>>(),
        [50, 50, 40, 40]
    );
    let join_needle = report
        .operation_channels
        .iter()
        .find(|c| c.operation_id == "join" && c.channel_id == "needle")
        .unwrap();
    close(join_needle.traversal_length_m, 0.5);
    close(join_needle.required_length_m, 0.8);
    close(report.total_required_length_m.unwrap(), 3.7);
    assert_eq!(report.material_requirements.len(), 1);
    close(report.material_requirements[0].required_length_m, 3.7);
    close(
        report.material_requirements[0].required_continuous_length_m,
        1.2,
    );
    let price = report.purchasing.as_ref().unwrap();
    assert!(
        price.complete_offer_coverage
            && price.complete_observed_quantity_coverage
            && price.complete_continuous_length_limit_coverage
    );
    let order = price.material_orders[0]
        .least_observed_order
        .as_ref()
        .unwrap();
    close(order.order_quantity, 4.0);
    close(order.order_cost, 14.0);
    close(price.least_observed_independent_order_total.unwrap(), 14.0);
    assert_eq!(
        order.applied_price_break.as_ref().unwrap().minimum_quantity,
        4.0
    );
    let book: QuoteBook =
        serde_json::from_value(options["consumables"]["purchasing"]["quote_book"].clone()).unwrap();
    let material = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: Vec::new(),
    }
    .resolve(&selected)
    .unwrap();
    let canonical = book
        .evaluate(&material, &price.material_orders[0].sample.scope)
        .unwrap();
    assert_eq!(
        serde_json::to_value(canonical).unwrap(),
        serde_json::to_value(&price.material_orders[0].sample).unwrap()
    );
    assert_eq!(
        report.purchasing_work_bound,
        consumables::purchasing_work_bound(&typed).unwrap()
    );
    assert!(!plan.physical_seam_quality_validated && !plan.hardware_execution_authorized);
}

#[test]
fn missing_or_partial_consumption_retains_diagnostics_but_never_prices_a_prefix() {
    let (pattern, options, selected) = fixture();
    for mutation in 0..4 {
        let mut options = options.clone();
        purchasing(&mut options, vec![quote(&selected, "cone-a", "thread-a")]);
        match mutation {
            0 => options["qualifications"][0]
                .as_object_mut()
                .unwrap()
                .remove("thread_consumption"),
            1 => {
                options["qualifications"][0]["thread_consumption"]["channels_complete"] =
                    json!(false);
                None
            }
            2 => {
                options["passes"][1]["thread"] = json!("unqualified-thread-setup");
                None
            }
            _ => {
                options["qualifications"] = json!([]);
                None
            }
        };
        let plan = build(pattern.clone(), options).unwrap();
        let report = plan.consumables.unwrap();
        assert!(!report.complete_consumption_coverage);
        assert!(!report.missing_operations.is_empty());
        assert!(
            report.total_required_length_m.is_none() && report.material_requirements.is_empty()
        );
        assert_eq!(report.operation_channels.len(), [0, 4, 2, 0][mutation]);
        let price = report.purchasing.unwrap();
        assert!(!price.complete_offer_coverage);
        assert!(
            price.material_orders.is_empty()
                && price.least_observed_independent_order_total.is_none()
        );
        assert_eq!(price.quote_observations_evaluated, 0);
    }
}

#[test]
fn consumption_has_independent_source_and_estimate_consent_and_exact_yarn_identity() {
    let (pattern, options, _) = fixture();
    for mutation in 0..10 {
        let mut changed = options.clone();
        let model = &mut changed["qualifications"][0]["thread_consumption"];
        match mutation {
            0 => model["allow_estimates"] = json!(false),
            1 => model["source"]["reference"] = json!(""),
            2 => model["method"] = json!(""),
            3 => model["channels"][0]["material"]
                .as_object_mut()
                .unwrap()
                .remove("expected_sha256")
                .map(|_| ())
                .unwrap(),
            4 => model["channels"][0]["material"]
                .as_object_mut()
                .unwrap()
                .remove("record")
                .map(|_| ())
                .unwrap(),
            5 => {
                model["channels"][0]["material"]["record"]["name"] = json!("Changed yarn revision")
            }
            6 => model["channels"][0]["metres_per_traversal"] = json!(0),
            7 => model["channels"][0]["start_allowance_m"] = json!(-0.1),
            8 => model["channels"][1]["id"] = json!("needle"),
            _ => changed["qualifications"][0]["allow_estimates"] = json!(false),
        }
        assert!(
            build(pattern.clone(), changed).is_err(),
            "mutation {mutation} admitted"
        );
    }
    let mut sheet = options.clone();
    let mut selection = yarn("not-a-yarn");
    selection.expected_sha256 = None;
    selection.record.as_mut().unwrap().form = ftx_pattern::materials::Form::Sheet;
    let frozen = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: Vec::new(),
    }
    .resolve(&selection)
    .unwrap()
    .frozen_selection();
    sheet["qualifications"][0]["thread_consumption"]["channels"][0]["material"] = json!(frozen);
    assert!(
        build(pattern.clone(), sheet)
            .unwrap_err()
            .message
            .contains("yarn record")
    );
    let mut overflow = options;
    overflow["qualifications"][0]["thread_consumption"]["channels"][0]["metres_per_traversal"] =
        json!(f64::MAX);
    assert!(
        build(pattern, overflow)
            .unwrap_err()
            .message
            .contains("representable")
    );
}

#[test]
fn supplier_capacity_continuity_latest_depletion_and_source_locks_apply_to_whole_thread_orders() {
    let (pattern, options, selected) = fixture();
    for mutation in 0..6 {
        let mut options = options.clone();
        let mut latest = quote(&selected, "cone-a", "thread-a");
        let mut previous = latest.clone();
        previous["observed_on"] = json!("2026-10-22");
        previous["price"] = json!(1.0);
        match mutation {
            0 => latest["available_quantity"] = json!(3.9),
            1 => latest["maximum_continuous_quantity"] = json!(1.1),
            2 => latest["available"] = json!(false),
            3 => {
                latest.as_object_mut().unwrap().remove("material_sha256");
            }
            4 => latest["material_sha256"] = json!("cd".repeat(32)),
            _ => {
                latest.as_object_mut().unwrap().remove("available_quantity");
            }
        }
        purchasing(&mut options, vec![previous, latest]);
        let result = build(pattern.clone(), options)
            .unwrap()
            .consumables
            .unwrap()
            .purchasing
            .unwrap();
        assert!(
            !result.complete_offer_coverage,
            "mutation {mutation} admitted"
        );
        assert!(result.least_observed_independent_order_total.is_none());
        assert!(result.material_orders[0].least_observed_order.is_none());
        if mutation == 3 {
            assert_eq!(
                result.material_orders[0].excluded_unlocked_offer_ids,
                ["cone-a"]
            );
        }
    }
}

#[test]
fn different_yarns_never_merge_by_setup_id_and_missing_prices_never_supply_a_total() {
    let (pattern, mut options, a) = fixture();
    let b = yarn("synthetic-thread-b");
    options["qualifications"][0]["thread_consumption"]["channels"][1]["material"] = json!(b);
    purchasing(&mut options, vec![quote(&a, "cone-a", "thread-a")]);
    let first = build(pattern.clone(), options.clone())
        .unwrap()
        .consumables
        .unwrap();
    assert_eq!(first.material_requirements.len(), 2);
    assert!(
        first
            .purchasing
            .unwrap()
            .least_observed_independent_order_total
            .is_none()
    );
    options["consumables"]["purchasing"]["quote_book"]["quotes"]
        .as_array_mut()
        .unwrap()
        .push(quote(&b, "cone-b", "thread-b"));
    let complete = build(pattern, options)
        .unwrap()
        .consumables
        .unwrap()
        .purchasing
        .unwrap();
    assert_eq!(complete.material_orders.len(), 2);
    close(
        complete.least_observed_independent_order_total.unwrap(),
        24.0,
    );
}

#[test]
fn transitive_inventory_aliases_through_an_unused_offer_cannot_duplicate_capacity() {
    let (pattern, mut options, a) = fixture();
    let b = yarn("synthetic-thread-b");
    let c = yarn("unused-bridging-thread");
    options["qualifications"][0]["thread_consumption"]["channels"][1]["material"] = json!(b);
    let mut qa = quote(&a, "cone-a", "thread-a");
    let mut qb = quote(&b, "cone-b", "thread-b");
    let mut bridge = quote(&c, "bridge", "alias-left");
    qa["price_breaks"][0]["sku"] = json!("alias-left");
    qb["price_breaks"][0]["sku"] = json!("alias-right");
    bridge["price_breaks"][0]["sku"] = json!("alias-right");
    purchasing(&mut options, vec![qa, qb, bridge]);
    let refused = build(pattern.clone(), options.clone())
        .unwrap()
        .consumables
        .unwrap()
        .purchasing
        .unwrap();
    assert!(refused.least_observed_independent_order_total.is_none());
    assert!(
        refused
            .material_orders
            .iter()
            .all(|o| o.excluded_shared_stock_offer_ids.len() == 1)
    );
    let mut other_a = quote(&a, "other-a", "separate-a");
    let mut other_b = quote(&b, "other-b", "separate-b");
    other_a["supplier"] = json!("independent-supplier");
    other_b["supplier"] = json!("independent-supplier");
    options["consumables"]["purchasing"]["quote_book"]["quotes"]
        .as_array_mut()
        .unwrap()
        .extend([other_a, other_b]);
    let complete = build(pattern, options)
        .unwrap()
        .consumables
        .unwrap()
        .purchasing
        .unwrap();
    close(
        complete.least_observed_independent_order_total.unwrap(),
        24.0,
    );
    assert!(complete.material_orders.iter().all(|o| {
        o.least_observed_order
            .as_ref()
            .unwrap()
            .quote
            .offer_id
            .starts_with("other-")
    }));
}

#[test]
fn inventory_aliases_use_current_market_observations_without_future_or_superseded_bridges() {
    for intraday in [false, true] {
        let (pattern, mut options, a) = fixture();
        let b = yarn("synthetic-thread-b");
        let c = yarn("unused-temporal-bridge");
        options["qualifications"][0]["thread_consumption"]["channels"][1]["material"] = json!(b);
        let mut qa = quote(&a, "cone-a", "thread-a");
        let mut qb = quote(&b, "cone-b", "thread-b");
        let mut old = quote(&c, "bridge", "alias-left");
        qa["price_breaks"][0]["sku"] = json!("alias-left");
        qb["price_breaks"][0]["sku"] = json!("alias-right");
        old["price_breaks"][0]["sku"] = json!("alias-right");
        let mut current = old.clone();
        current["price_breaks"][0]
            .as_object_mut()
            .unwrap()
            .remove("sku");
        let mut later = old.clone();
        let dates = if intraday {
            ["2026-10-23"; 3]
        } else {
            ["2026-10-22", "2026-10-23", "2026-10-24"]
        };
        for q in [&mut qa, &mut qb, &mut old] {
            q["observed_on"] = json!(dates[0]);
            if intraday {
                q["observed_at"] = json!("2026-10-23T08:00:00Z");
            }
        }
        current["observed_on"] = json!(dates[1]);
        current["source_sha256"] = json!("cd".repeat(32));
        later["observed_on"] = json!(dates[2]);
        later["source_sha256"] = json!("ef".repeat(32));
        if intraday {
            current["observed_at"] = json!("2026-10-23T09:00:00Z");
            later["observed_at"] = json!("2026-10-23T10:00:00Z");
        }
        purchasing(&mut options, vec![qa, qb, later, old, current]);
        if intraday {
            options["consumables"]["purchasing"]["quote_book"]["schema"] =
                json!("ftx-textile-quotes/2");
        }
        for (index, at) in [
            "2026-10-23T08:00:00Z",
            "2026-10-23T09:00:00Z",
            "2026-10-23T10:00:00Z",
        ]
        .into_iter()
        .enumerate()
        {
            options["consumables"]["purchasing"]["market"]["as_of"] = json!(dates[index]);
            if intraday {
                options["consumables"]["purchasing"]["market"]["as_of_at"] = json!(at);
            }
            let report = build(pattern.clone(), options.clone())
                .unwrap()
                .consumables
                .unwrap();
            let price = report.purchasing.unwrap();
            assert!(
                price.quote_observations_evaluated + price.stock_aliases_examined
                    <= report.purchasing_work_bound
            );
            if index == 1 {
                close(price.least_observed_independent_order_total.unwrap(), 24.0);
                assert!(
                    price
                        .material_orders
                        .iter()
                        .all(|o| o.excluded_shared_stock_offer_ids.is_empty())
                );
            } else {
                assert!(price.least_observed_independent_order_total.is_none());
                assert!(
                    price
                        .material_orders
                        .iter()
                        .all(|o| o.excluded_shared_stock_offer_ids.len() == 1)
                );
            }
        }
    }
}

#[test]
fn recipe_selection_reserves_thread_purchasing_for_its_whole_configured_trial_budget() {
    let (pattern, mut options, selected) = fixture();
    options["require_qualification"] = json!(true);
    purchasing(
        &mut options,
        vec![quote(&selected, "cone-a", "thread-a"); 1100],
    );
    let mut request: ftx_export::sewing_selection::Request = serde_json::from_value(json!({
        "schema":"ftx-sewing-selection/1", "objective":"minimize_feed_seconds",
        "max_plan_evaluations":64,
        "alternatives":[{"operation":"join","recipes":[{"id":"same-source-recipe",
            "needle":options["passes"][0]["needle"], "thread":options["passes"][0]["thread"],
            "max_stitch_mm":options["passes"][0]["max_stitch_mm"],
            "stitches_per_minute":options["passes"][0]["stitches_per_minute"]}]}],
        "options":options
    }))
    .unwrap();
    let refusal = request.select(&Pattern::new(pattern.clone())).unwrap_err();
    assert_eq!(refusal.code, ftx_core::RefusalCode::BudgetExceeded);
    assert!(refusal.message.contains("recipe/thread purchasing"));
    request.max_plan_evaluations = 1;
    let selected = request.select(&Pattern::new(pattern)).unwrap();
    assert_eq!(selected.trials.len(), 1);
    close(
        selected
            .selected_plan
            .unwrap()
            .consumables
            .unwrap()
            .purchasing
            .unwrap()
            .least_observed_independent_order_total
            .unwrap(),
        14.0,
    );
}

#[test]
fn source_changes_recompute_consumption_and_omitted_request_preserves_legacy_output() {
    let (mut pattern, options, _) = fixture();
    let original = build(pattern.clone(), options.clone()).unwrap();
    let mut faster = options.clone();
    faster["qualifications"][0]["thread_consumption"]["channels"][0]["metres_per_traversal"] =
        json!(0.03);
    let changed = build(pattern.clone(), faster).unwrap();
    close(
        changed
            .consumables
            .unwrap()
            .total_required_length_m
            .unwrap()
            - original
                .consumables
                .unwrap()
                .total_required_length_m
                .unwrap(),
        1.8,
    );
    pattern["fabric"]["material"]["record"]["name"] = json!("Different source textile revision");
    let stale = build(pattern, options).unwrap().consumables.unwrap();
    assert!(!stale.complete_consumption_coverage && stale.total_required_length_m.is_none());
    let base_pattern: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-sewing-pattern.json"
    ))
    .unwrap();
    let base_options: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-sewing-options.json"
    ))
    .unwrap();
    let legacy = build(base_pattern, base_options).unwrap();
    assert_eq!(legacy.schema, "ftx-sewing-plan/1");
    assert!(
        serde_json::to_value(legacy)
            .unwrap()
            .get("consumables")
            .is_none()
    );
}

#[test]
fn preflight_bounds_thread_choices_quote_work_and_unsupported_supplier_splitting() {
    let (_, options, selected) = fixture();
    let mut too_many_channels = options.clone();
    let channel =
        too_many_channels["qualifications"][0]["thread_consumption"]["channels"][0].clone();
    too_many_channels["qualifications"][0]["thread_consumption"]["channels"] =
        json!(vec![channel; 9]);
    assert!(
        consumables::purchasing_work_bound(&serde_json::from_value(too_many_channels).unwrap())
            .is_err()
    );
    let mut too_many_quotes = options.clone();
    purchasing(
        &mut too_many_quotes,
        vec![quote(&selected, "one", "one"); 4097],
    );
    assert!(
        consumables::purchasing_work_bound(&serde_json::from_value(too_many_quotes).unwrap())
            .is_err()
    );
    let mut splitting = options.clone();
    purchasing(&mut splitting, vec![]);
    splitting["consumables"]["purchasing"]["market"]["split_orders"] =
        json!({"schema":"ftx-cut-order-allocation/1","max_search_nodes":100});
    assert!(
        consumables::purchasing_work_bound(&serde_json::from_value(splitting).unwrap())
            .unwrap_err()
            .message
            .contains("split orders")
    );
    let mut materials = options;
    let qualification = materials["qualifications"][0].clone();
    materials["qualifications"] = json!((0..33).map(|i| {
        let mut q = qualification.clone();
        q["id"] = json!(format!("qualification-{i}"));
        q["thread_consumption"]["channels"] = json!([{
            "id":"needle","material":yarn(&format!("thread-{i}")),"metres_per_traversal":0.01,
            "start_allowance_m":0.0,"end_allowance_m":0.0
        }]); q
    }).collect::<Vec<_>>());
    assert!(
        consumables::purchasing_work_bound(&serde_json::from_value(materials).unwrap()).is_err()
    );
}

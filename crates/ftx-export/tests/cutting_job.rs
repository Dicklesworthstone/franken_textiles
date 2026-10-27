//! Mixed physical-stock allocation and complete-order answers; all observations are synthetic.
use std::collections::BTreeSet;

use ftx_core::{RefusalCode, Vec2};
use ftx_export::{
    Ctx, ExportOptions, Source,
    cutter::cutter_ring,
    cutting::{CutPlan, job, procurement, stock},
    material_study,
};
use ftx_pattern::{Pattern, material_catalog::Selection, textile_import};
use serde_json::{Value, json};

const PATTERN: &str = include_str!("../../../examples/mixed-cutting/pattern.json");
const STUDY: &str = include_str!("../../../examples/mixed-cutting/study.json");
const SPLIT_STUDY: &str = include_str!("../../../examples/mixed-cutting/split-orders-study.json");

fn inputs() -> (Pattern, job::Request) {
    let study: material_study::Request = serde_json::from_str(STUDY).unwrap();
    (
        Pattern::from_json(PATTERN).unwrap(),
        study.cutting_job.unwrap(),
    )
}
fn value(x: &impl serde::Serialize) -> Value {
    serde_json::to_value(x).unwrap()
}
fn near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-11, "{actual} != {expected}");
}
fn priced(pattern: &Pattern, request: &job::Request) -> job::Request {
    let plan = request.build(pattern).unwrap();
    let finite = plan.finite_stock.as_ref().unwrap();
    let identities = procurement::finite::identities(finite).unwrap();
    let mut quotes = Vec::new();
    let mut bindings = Vec::new();
    for identity in &identities {
        let offer = format!("item/{}", identity.stock_id);
        quotes.push(json!({"offer_id":offer,"material_id":"mixed-example-hide","material_sha256":identity.material_sha256,
            "supplier":"Synthetic example","sku":identity.stock_id,"price":7.0,
            "quantity":identity.gross_area_m2,"unit":"square_meter","minimum_quantity":identity.gross_area_m2,
            "order_increment":identity.gross_area_m2,"available_quantity":identity.gross_area_m2,
            "currency":"USD","region":"test","quality":"synthetic","terms":"no-shipping",
            "observed_on":"2026-10-22","available":true,"source":"test:mixed-stock-item-quote","source_sha256":"1".repeat(64)}));
        bindings.push(json!({"stock_id":identity.stock_id,"stock_sha256":identity.stock_sha256,"offer_ids":[offer]}));
    }
    let roll = &plan.roll.as_ref().unwrap().rolls[0];
    quotes.push(json!({"offer_id":"roll/lining","material_id":roll.material.material().id,"material_sha256":roll.material.sha256(),
        "supplier":"Synthetic example","sku":"lining","price":10.0,"quantity":1.0,
        "unit":"linear_meter","minimum_quantity":0.1,"order_increment":0.1,"available_quantity":2.0,
        "currency":"USD","region":"test","quality":"synthetic","terms":"no-shipping",
        "observed_on":"2026-10-22","available":true,"source":"test:mixed-roll-quote","source_sha256":"2".repeat(64)}));
    let mut result = request.clone();
    result.purchasing = Some(
        serde_json::from_value(json!({
        "quotes":{"schema":"ftx-textile-quotes/1","quotes":quotes},
        "market":{"currency":"USD","region":"test","quality":"synthetic","terms":"no-shipping",
            "as_of":"2026-10-22","max_age_days":0,"require_observed_quantity":true},
        "finite_bindings":bindings}))
        .unwrap(),
    );
    result
}

#[test]
fn lower_width_roll_planning_preserves_finite_inventory_and_exact_mixed_job_replay() {
    let (pattern, mut request) = inputs();
    let mut raw = pattern.raw().clone();
    raw["pieces"][1]["fabric"]["material"]["record"]["properties"]["width"]["bounds"] =
        json!([0.03, 0.11]);
    let pattern = Pattern::new(raw.clone());
    request.roll.as_mut().unwrap().maximum_marker_length_m = Some(0.029);
    let mut request = priced(&pattern, &request);
    let nominal = request.build(&pattern).unwrap();
    request.roll.as_mut().unwrap().width_basis =
        ftx_export::cutting::WidthBasis::ReportedLowerBound;
    let bounded = request.build(&pattern).unwrap();
    assert_eq!(pattern.raw(), &raw);
    assert_eq!(bounded.inventory, nominal.inventory);
    assert!(bounded.complete_inventory);
    assert_eq!(value(&bounded.finite_stock), value(&nominal.finite_stock));
    assert_eq!(nominal.roll.as_ref().unwrap().rolls.len(), 1);
    let rolls = &bounded.roll.as_ref().unwrap().rolls;
    assert_eq!(rolls.len(), 2);
    for marker in rolls {
        near(marker.width_m, 0.1);
        near(marker.layout_width.as_ref().unwrap().width_m, 0.03);
        near(marker.length_m, 0.029);
        assert_eq!(marker.placements.len(), 1);
        assert!(marker.placements[0].bounds_mm[2] <= 28.0);
    }
    near(bounded.external_cut_area_m2, nominal.external_cut_area_m2);
    near(bounded.stock_area_m2 - nominal.stock_area_m2, 0.0029);
    let pricing = bounded.purchasing.as_ref().unwrap();
    assert!(pricing.complete_offer_coverage);
    assert_eq!(
        value(&pricing.finite_stock),
        value(&nominal.purchasing.as_ref().unwrap().finite_stock)
    );
    let supplied = request.purchasing.as_ref().unwrap();
    assert_eq!(
        value(&pricing.roll)["result"],
        procurement::evaluate(
            bounded.roll.as_ref().unwrap(),
            &supplied.quotes,
            &supplied.market
        )
        .unwrap()
    );
    let replay: job::Request = serde_json::from_str(&bounded.replay_request_json).unwrap();
    assert_eq!(
        replay.roll.as_ref().unwrap().width_basis,
        ftx_export::cutting::WidthBasis::ReportedLowerBound
    );
    assert_eq!(
        value(&bounded),
        value(
            &replay
                .build(&Pattern::from_json(&bounded.pattern_json).unwrap())
                .unwrap()
        )
    );
}

#[test]
fn mixed_job_allocates_original_mirrors_fold_lining_and_copies_exactly_once() {
    let (pattern, request) = inputs();
    let original = pattern.raw().clone();
    assert!(CutPlan::build(&pattern, request.roll.as_ref().unwrap()).is_err());
    assert!(
        request
            .finite_stock
            .as_ref()
            .unwrap()
            .build(&pattern)
            .is_err()
    );
    let plan = request.build(&pattern).unwrap();
    assert_eq!(pattern.raw(), &original);
    assert!(plan.complete_inventory);
    assert!(!plan.hardware_execution_authorized);
    assert_eq!(plan.inventory.len(), 6);
    assert_eq!(
        plan.inventory
            .iter()
            .map(|i| (&i.piece, &i.instance, i.copy))
            .collect::<BTreeSet<_>>()
            .len(),
        6
    );
    assert_eq!(
        plan.inventory.iter().filter(|i| i.piece == "shell").count(),
        4
    );
    assert_eq!(
        plan.inventory
            .iter()
            .filter(|i| i.piece == "lining")
            .count(),
        2
    );
    near(plan.external_cut_area_m2, 0.003);
    near(plan.stock_area_m2, 0.0129);
    near(plan.stock_mass_kg.unwrap(), 0.00558);
    let roll = plan.roll.as_ref().unwrap();
    let finite = plan.finite_stock.as_ref().unwrap();
    assert_eq!(roll.pattern_sha256, plan.pattern_sha256);
    assert_eq!(value(finite)["pattern_sha256"], plan.pattern_sha256);
    assert_eq!(roll.source_piece_ids.as_ref().unwrap(), &["lining"]);
    assert_eq!(value(finite)["source_piece_ids"], json!(["shell"]));
    assert_eq!(
        value(&plan.finite_stock_identities),
        value(&procurement::finite::identities(finite).unwrap())
    );
    let mut reordered = original.clone();
    reordered["pieces"].as_array_mut().unwrap().reverse();
    let reordered = request.build(&Pattern::new(reordered)).unwrap();
    assert_eq!(reordered.inventory, plan.inventory);
    near(reordered.external_cut_area_m2, plan.external_cut_area_m2);
    let ctx = Ctx::new(&pattern, ExportOptions::default()).unwrap();
    let placements = roll
        .rolls
        .iter()
        .flat_map(|r| &r.placements)
        .chain(finite.markers().iter().flat_map(|m| &m.placements));
    for placement in placements {
        let instance = pattern
            .instances
            .iter()
            .find(|i| i.id == placement.instance)
            .unwrap();
        let geom = ctx.geom(Source::Instance(instance), 6.0).unwrap();
        let map = |p: Vec2| {
            Vec2::new(
                placement.transform[0][0] * p.x
                    + placement.transform[0][1] * p.y
                    + placement.transform[0][2],
                placement.transform[1][0] * p.x
                    + placement.transform[1][1] * p.y
                    + placement.transform[1][2],
            )
        };
        for (a, b) in cutter_ring(&geom).unwrap().iter().zip(&placement.cut_mm) {
            near(map(*a).dist(*b), 0.0);
        }
        for ((a, closed), (b, actual_closed)) in
            geom.internal_cut.iter().zip(&placement.internal_cut_mm)
        {
            assert_eq!(closed, actual_closed);
            for (a, b) in a.iter().zip(b) {
                near(map(*a).dist(*b), 0.0);
            }
        }
        for ((a, d), (b, actual_d)) in geom.drills.iter().zip(&placement.drills_mm) {
            near(map(*a).dist(*b), 0.0);
            assert_eq!(d, actual_d);
        }
    }
    let replay_pattern = Pattern::from_json(&plan.pattern_json).unwrap();
    let replay: job::Request = serde_json::from_str(&plan.replay_request_json).unwrap();
    assert_eq!(value(&plan), value(&replay.build(&replay_pattern).unwrap()));
}

#[test]
fn missing_duplicate_unknown_routes_and_mismatched_jobs_refuse_before_partial_output() {
    let (pattern, request) = inputs();
    for case in 0..8 {
        let mut bad = request.clone();
        match case {
            0 => {
                bad.routes.pop();
            }
            1 => bad.routes.push(bad.routes[0].clone()),
            2 => bad.routes[0].piece = "not-a-source-piece".into(),
            3 => bad.roll.as_mut().unwrap().copies = 1,
            4 => bad.roll.as_mut().unwrap().seam_allowance_mm = Some(1.0),
            5 => bad.finite_stock = None,
            6 => {
                bad.finite_stock
                    .as_mut()
                    .unwrap()
                    .options
                    .piece_zones
                    .insert("lining".into(), "outside-scope".into());
            }
            _ => bad.routes[0].stock = job::StockKind::Roll,
        }
        assert!(bad.build(&pattern).is_err(), "case {case} must refuse");
    }
    let mut too_many = request;
    too_many.roll.as_mut().unwrap().copies = 1400;
    too_many.finite_stock.as_mut().unwrap().options.copies = 1400;
    assert_eq!(
        too_many.build(&pattern).unwrap_err().code,
        RefusalCode::BudgetExceeded
    );
}

#[test]
fn complete_same_market_job_cost_counts_each_roll_order_and_used_hide_once() {
    let (pattern, request) = inputs();
    let request = priced(&pattern, &request);
    let plan = request.build(&pattern).unwrap();
    let pricing = plan.purchasing.as_ref().unwrap();
    assert!(pricing.complete_offer_coverage && pricing.complete_observed_quantity_coverage);
    near(pricing.least_observed_independent_order_total.unwrap(), 8.0);
    near(pricing.selected_observed_order_total.unwrap(), 8.0);
    assert!(pricing.order_allocation_optimality_proven);
    assert!(!pricing.execution_authorized);
    let expected_roll = procurement::evaluate(
        plan.roll.as_ref().unwrap(),
        &request.purchasing.as_ref().unwrap().quotes,
        &request.purchasing.as_ref().unwrap().market,
    )
    .unwrap();
    assert_eq!(value(&pricing.roll)["result"], expected_roll);
    let finite_request: procurement::finite::Request = serde_json::from_value(json!({
        "schema":"ftx-finite-stock-pricing/1","quotes":request.purchasing.as_ref().unwrap().quotes,
        "market":request.purchasing.as_ref().unwrap().market,"bindings":request.purchasing.as_ref().unwrap().finite_bindings})).unwrap();
    assert_eq!(
        value(&pricing.finite_stock)["result"],
        value(
            &procurement::finite::evaluate(plan.finite_stock.as_ref().unwrap(), &finite_request)
                .unwrap()
        )
    );
    let replay: job::Request = serde_json::from_str(&plan.replay_request_json).unwrap();
    assert_eq!(
        value(&plan),
        value(
            &replay
                .build(&Pattern::from_json(&plan.pattern_json).unwrap())
                .unwrap()
        )
    );
}

#[test]
fn split_suppliers_keep_complete_job_incumbents_separate_from_proved_minimum_cost() {
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let study: material_study::Request = serde_json::from_str(SPLIT_STUDY).unwrap();
    let mut request = study.cutting_job.unwrap();
    let exhaustive = request.build(&pattern).unwrap();
    assert_eq!(exhaustive.inventory.len(), 15);
    assert_eq!(exhaustive.roll.as_ref().unwrap().rolls.len(), 2);
    let optimum = exhaustive.purchasing.as_ref().unwrap();
    assert_eq!(optimum.schema, "ftx-cutting-job-procurement/1");
    assert!(optimum.complete_offer_coverage && optimum.complete_observed_quantity_coverage);
    near(optimum.selected_observed_order_total.unwrap(), 7.09);
    near(
        optimum.least_observed_independent_order_total.unwrap(),
        7.09,
    );
    assert!(optimum.order_allocation_optimality_proven);
    let roll = value(&optimum.roll);
    let allocation = &roll["result"]["material_orders"][0]["allocation"];
    let orders = allocation["selected_orders"].as_array().unwrap();
    assert_eq!(orders.len(), 2);
    assert_eq!(orders[0]["offer"]["quote"]["offer_id"], "roll/a");
    assert_eq!(orders[1]["offer"]["quote"]["offer_id"], "roll/b");
    for order in orders {
        near(order["offer"]["order_quantity"].as_f64().unwrap(), 0.03);
        assert_eq!(order["roll_indices"].as_array().unwrap().len(), 1);
        assert_eq!(order["offer"]["quantity_sufficiency_observed"], true);
    }
    assert_ne!(orders[0]["roll_indices"], orders[1]["roll_indices"]);
    assert_eq!(
        allocation["whole_order_sample"]["excluded_offers"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    request
        .purchasing
        .as_mut()
        .unwrap()
        .market
        .split_orders
        .as_mut()
        .unwrap()
        .max_search_nodes = 1;
    let bounded = request.build(&pattern).unwrap();
    assert_eq!(bounded.inventory, exhaustive.inventory);
    assert_eq!(value(&bounded.roll), value(&exhaustive.roll));
    assert_eq!(
        value(&bounded.finite_stock),
        value(&exhaustive.finite_stock)
    );
    let selected = bounded.purchasing.as_ref().unwrap();
    assert!(selected.complete_offer_coverage && selected.complete_observed_quantity_coverage);
    near(selected.selected_observed_order_total.unwrap(), 8.0);
    assert!(selected.least_observed_independent_order_total.is_none());
    assert!(!selected.order_allocation_optimality_proven);
    assert_eq!(value(&selected.roll)["result"]["budget_exhausted"], true);
    assert_eq!(
        value(&selected.roll)["result"]["material_orders"][0]["allocation"]["selected_orders"][0]["roll_indices"],
        json!([0, 1])
    );
    let replay: job::Request = serde_json::from_str(&bounded.replay_request_json).unwrap();
    assert_eq!(
        value(&bounded),
        value(
            &replay
                .build(&Pattern::from_json(&bounded.pattern_json).unwrap())
                .unwrap()
        )
    );

    // Either missing physical-hide evidence or missing roll offers invalidates the whole-job
    // cost. A valid counterpart remains inspectable, never relabeled as the complete purchase.
    for finite_missing in [true, false] {
        let mut missing = request.clone();
        let purchasing = missing.purchasing.as_mut().unwrap();
        if finite_missing {
            purchasing.finite_bindings.clear();
        } else {
            purchasing
                .quotes
                .quotes
                .retain(|q| q.offer_id.starts_with("item/"));
        }
        let result = missing.build(&pattern).unwrap();
        assert_eq!(result.inventory, bounded.inventory);
        let price = result.purchasing.as_ref().unwrap();
        assert!(!price.complete_offer_coverage && !price.complete_observed_quantity_coverage);
        assert!(price.selected_observed_order_total.is_none());
        assert!(price.least_observed_independent_order_total.is_none());
        assert!(!price.order_allocation_optimality_proven);
        if finite_missing {
            assert_eq!(
                value(&price.roll)["result"]["selected_observed_order_total"],
                1.0
            );
        } else {
            assert_eq!(
                value(&price.finite_stock)["result"]["least_observed_independent_order_total"],
                7.0
            );
        }
    }
}

#[test]
fn studies_can_compare_complete_split_purchases_without_claiming_unproved_least_cost() {
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let mut request: Value = serde_json::from_str(SPLIT_STUDY).unwrap();
    request["cutting_job"]["purchasing"]["market"]["split_orders"]["max_search_nodes"] = json!(1);
    let evaluate = |request: &Value| {
        value(
            &serde_json::from_value::<material_study::Request>(request.clone())
                .unwrap()
                .evaluate(&pattern)
                .unwrap(),
        )
    };
    let feasible = evaluate(&request);
    assert_eq!(
        feasible["alternatives"][0]["metrics"]["cutting_job_selected_order_cost"],
        8.0
    );
    assert!(feasible["alternatives"][0]["metrics"]["cutting_job_observed_order_cost"].is_null());
    assert_eq!(
        feasible["comparison"]["frontier"],
        json!(["mixed-shell-and-lining"])
    );
    assert_eq!(
        feasible["comparison"]["alternatives"][0]["readings"]["cutting_job_selected_order_cost"]["value"],
        8.0
    );
    request["comparison"]["objectives"][0]["metric"] = json!("cutting_job_observed_order_cost");
    let unproved = evaluate(&request);
    assert_eq!(unproved["alternatives"], feasible["alternatives"]);
    assert_eq!(unproved["comparison"]["frontier"], json!([]));

    request["cutting_job"]["purchasing"]["market"]["split_orders"]["max_search_nodes"] =
        json!(10000);
    let proved = evaluate(&request);
    assert_eq!(
        proved["comparison"]["frontier"],
        json!(["mixed-shell-and-lining"])
    );
    near(
        proved["alternatives"][0]["metrics"]["cutting_job_observed_order_cost"]
            .as_f64()
            .unwrap(),
        7.09,
    );
    assert_eq!(
        proved["alternatives"][0]["metrics"]["cutting_job_observed_order_cost"],
        proved["alternatives"][0]["metrics"]["cutting_job_selected_order_cost"]
    );
    request["comparison"]["objectives"][0]["metric"] = json!("cutting_job_selected_order_cost");
    request["cutting_job"]["purchasing"]["finite_bindings"] = json!([]);
    let incomplete = evaluate(&request);
    assert_eq!(incomplete["comparison"]["frontier"], json!([]));
    assert!(incomplete["alternatives"][0]["metrics"]["cutting_job_selected_order_cost"].is_null());
    assert_eq!(
        incomplete["alternatives"][0]["cutting_job"]["status"],
        "available"
    );
    assert_eq!(
        incomplete["alternatives"][0]["cutting_job"]["result"]["purchasing"]["roll"]["result"]["complete_offer_coverage"],
        true
    );
}

#[test]
fn bounded_split_order_incumbent_remains_a_complete_job_and_an_explicit_study_objective() {
    let (pattern, request) = inputs();
    let mut request = priced(&pattern, &request);
    let mut other = request
        .purchasing
        .as_ref()
        .unwrap()
        .quotes
        .quotes
        .last()
        .unwrap()
        .clone();
    other.offer_id = "roll/alternative".into();
    other.supplier = "Another synthetic supplier".into();
    other.price *= 2.;
    request
        .purchasing
        .as_mut()
        .unwrap()
        .quotes
        .quotes
        .push(other);
    request.purchasing.as_mut().unwrap().market.split_orders = Some(
        serde_json::from_value(json!({"schema":"ftx-cut-order-allocation/1",
            "max_search_nodes":1}))
        .unwrap(),
    );
    let plan = request.build(&pattern).unwrap();
    let pricing = plan.purchasing.as_ref().unwrap();
    assert!(pricing.complete_offer_coverage && pricing.complete_observed_quantity_coverage);
    near(pricing.selected_observed_order_total.unwrap(), 8.0);
    assert!(pricing.least_observed_independent_order_total.is_none());
    assert!(!pricing.order_allocation_optimality_proven);
    assert_eq!(value(&pricing.roll)["result"]["budget_exhausted"], true);
    let mut study: Value = serde_json::from_str(STUDY).unwrap();
    study["cutting_job"] = value(&request);
    for (metric, eligible) in [
        ("cutting_job_selected_order_cost", true),
        ("cutting_job_observed_order_cost", false),
    ] {
        study["comparison"] = json!({"basis":"nominal",
            "required_consumers":["cutting_job_purchasing"],
            "objectives":[{"metric":metric,"direction":"minimize"}]});
        let evaluated = value(
            &serde_json::from_value::<material_study::Request>(study.clone())
                .unwrap()
                .evaluate(&pattern)
                .unwrap(),
        );
        let candidate = &evaluated["comparison"]["alternatives"][0];
        assert_eq!(candidate["eligible"], eligible, "{metric}: {candidate}");
        assert_eq!(
            evaluated["alternatives"][0]["metrics"]["cutting_job_selected_order_cost"],
            8.0
        );
        if eligible {
            assert_eq!(candidate["readings"][metric]["value"], 8.0);
            assert_eq!(candidate["readings"][metric]["unit"], "USD");
        }
    }
    request.purchasing.as_mut().unwrap().quotes.quotes[0].available = false;
    let failed = request.build(&pattern).unwrap();
    assert!(failed.complete_inventory);
    assert!(!failed.purchasing.as_ref().unwrap().complete_offer_coverage);
    assert!(
        failed
            .purchasing
            .as_ref()
            .unwrap()
            .selected_observed_order_total
            .is_none()
    );
}

#[test]
fn missing_stale_and_unavailable_whole_item_offers_preserve_geometry_without_partial_cost() {
    let (pattern, request) = inputs();
    let request = priced(&pattern, &request);
    let baseline = request.build(&pattern).unwrap();
    for case in 0..4 {
        let mut bad = request.clone();
        let p = bad.purchasing.as_mut().unwrap();
        match case {
            0 => p.finite_bindings.clear(),
            1 => p.finite_bindings[0].stock_sha256 = "0".repeat(64),
            2 => p.quotes.quotes[0].available = false,
            _ => p.quotes.quotes[0].material_sha256 = Some("0".repeat(64)),
        }
        let plan = bad.build(&pattern).unwrap();
        assert_eq!(value(&plan.roll), value(&baseline.roll));
        assert_eq!(value(&plan.finite_stock), value(&baseline.finite_stock));
        let p = plan.purchasing.as_ref().unwrap();
        assert!(!p.complete_offer_coverage);
        assert!(p.least_observed_independent_order_total.is_none());
        near(
            value(&p.roll)["result"]["least_observed_independent_order_total"]
                .as_f64()
                .unwrap(),
            1.0,
        );
    }
}

#[test]
fn same_physical_inventory_remains_source_locked_when_material_studies_change() {
    let (pattern, request) = inputs();
    let request = priced(&pattern, &request);
    let mut study: Value = serde_json::from_str(STUDY).unwrap();
    study["cutting_job"] = value(&request);
    study["comparison"] = json!({"basis":"nominal","required_consumers":["cutting_job","cutting_job_purchasing"],
        "objectives":[{"metric":"cutting_job_observed_order_cost","direction":"minimize"}],"constraints":[]});
    let mut changed = pattern.raw()["fabric"]["material"].clone();
    changed["id"] = json!("new-hide-lot");
    changed["record"]["id"] = json!("new-hide-lot");
    study["variants"].as_array_mut().unwrap().push(json!({"key":"unavailable-lot","changes":{"schema":"ftx-material-bindings/1","default":changed}}));
    let request: material_study::Request = serde_json::from_value(study).unwrap();
    let report = value(&request.evaluate(&pattern).unwrap());
    let rows = report["alternatives"].as_array().unwrap();
    assert_eq!(rows[0]["cutting_job"]["status"], "available");
    assert_eq!(rows[0]["metrics"]["cutting_job_observed_order_cost"], 8.0);
    assert_eq!(rows[1]["binding"]["status"], "available");
    assert_eq!(rows[1]["cutting_job"]["status"], "refused");
    assert!(rows[1]["metrics"]["cutting_job_observed_order_cost"].is_null());
    assert_eq!(
        report["comparison"]["frontier"],
        json!(["mixed-shell-and-lining"])
    );
    assert_eq!(report["geometry_changed"], false);
    assert_eq!(report["physical_validation_claim"], false);
    let mut overlapping = value(&request);
    overlapping["cutting"] = value(request.cutting_job.as_ref().unwrap().roll.as_ref().unwrap());
    assert!(
        serde_json::from_value::<material_study::Request>(overlapping)
            .unwrap()
            .evaluate(&pattern)
            .is_err()
    );
}

#[test]
fn explicit_cost_search_reuses_complete_job_quotes_and_retains_the_selected_inventory() {
    let (pattern, mut request) = inputs();
    let mut second = request.finite_stock.as_ref().unwrap().stocks[0].clone();
    second.id = "hide-002".into();
    request.finite_stock.as_mut().unwrap().stocks.push(second);
    let mut request = priced(&pattern, &request);
    request.purchasing.as_mut().unwrap().quotes.quotes[1].price = 3.0;
    let stock = request.finite_stock.as_mut().unwrap();
    stock.options.backtrack_limit = Some(500);
    stock.options.work_limit = 2000000;
    stock.options.optimize = Some(stock::search::Objective::MinimizePurchaseCost);
    let plan = request.build(&pattern).unwrap();
    let finite = plan.finite_stock.as_ref().unwrap();
    assert!(finite.request().pricing.is_some());
    near(
        finite
            .purchasing()
            .unwrap()
            .least_observed_independent_order_total
            .unwrap(),
        3.0,
    );
    near(
        plan.purchasing
            .as_ref()
            .unwrap()
            .least_observed_independent_order_total
            .unwrap(),
        4.0,
    );
    assert!(finite.markers()[0].placements.is_empty());
    assert_eq!(finite.markers()[1].placements.len(), 4);
}

#[test]
fn roll_only_scope_retains_complete_job_semantics_and_missing_mass_is_unknown() {
    let (pattern, mut request) = inputs();
    let mut raw = pattern.raw().clone();
    let mut lining = raw["pieces"][1]["fabric"]["material"].clone();
    lining["record"]["properties"]
        .as_object_mut()
        .unwrap()
        .remove("areal_density");
    raw["fabric"]["material"] = lining.clone();
    raw["pieces"][1]["fabric"]["material"] = lining;
    let pattern = Pattern::new(raw);
    for route in &mut request.routes {
        route.stock = job::StockKind::Roll;
    }
    request.finite_stock = None;
    let plan = request.build(&pattern).unwrap();
    assert_eq!(plan.inventory.len(), 6);
    assert!(plan.stock_mass_kg.is_none());
    assert!(plan.finite_stock.is_none());
    let material: Selection =
        serde_json::from_value(pattern.raw()["fabric"]["material"].clone()).unwrap();
    let resolved = textile_import::catalog()
        .unwrap()
        .resolve(&material)
        .unwrap();
    assert!(
        plan.inventory
            .iter()
            .all(|i| i.material_sha256 == resolved.sha256())
    );
}

#[test]
fn a_bound_physical_remnant_offer_cannot_also_supply_the_roll_route() {
    let (pattern, mut request) = inputs();
    let mut raw = pattern.raw().clone();
    let selection = raw["pieces"][1]["fabric"]["material"].clone();
    raw["fabric"]["material"] = selection.clone();
    request.finite_stock.as_mut().unwrap().stocks[0].material =
        serde_json::from_value(selection).unwrap();
    let pattern = Pattern::new(raw);
    let mut request = priced(&pattern, &request);
    let quotes = &mut request.purchasing.as_mut().unwrap().quotes.quotes;
    quotes[0].material_id = "mixed-example-lining".into();
    quotes[0].price = 0.1;
    let mut alias = quotes[0].clone();
    alias.offer_id = "item/hide-alias".into();
    alias.price = 0.05;
    quotes.push(alias);
    let plan = request.build(&pattern).unwrap();
    let pricing = plan.purchasing.as_ref().unwrap();
    near(pricing.least_observed_independent_order_total.unwrap(), 1.1);
    assert_eq!(
        value(&pricing.roll)["result"]["roll_prices"][0]["least_observed_order"]["quote"]["offer_id"],
        "roll/lining"
    );
    assert_eq!(
        value(&pricing.finite_stock)["result"]["stocks"][0]["least_observed_offer_id"],
        "item/hide-001"
    );
    assert_eq!(
        pricing.roll_excluded_physical_stock_offer_ids,
        ["item/hide-001", "item/hide-alias"]
    );

    // With no independent roll source, keep the finite item and geometry but not a whole-job cost.
    request
        .purchasing
        .as_mut()
        .unwrap()
        .quotes
        .quotes
        .retain(|quote| quote.offer_id != "roll/lining");
    let missing = request.build(&pattern).unwrap();
    assert!(missing.complete_inventory);
    assert_eq!(missing.inventory, plan.inventory);
    let price = missing.purchasing.as_ref().unwrap();
    assert!(!price.complete_offer_coverage && !price.complete_observed_quantity_coverage);
    assert!(price.least_observed_independent_order_total.is_none());
    assert!(price.selected_observed_order_total.is_none());
    assert!(!price.order_allocation_optimality_proven);
    assert_eq!(value(&price.roll)["status"], "available");
    assert_eq!(
        value(&price.finite_stock)["result"]["stocks"],
        value(&pricing.finite_stock)["result"]["stocks"]
    );

    let mut study: Value = serde_json::from_str(STUDY).unwrap();
    study["cutting_job"] = value(&request);
    study["comparison"] = json!({"basis":"nominal",
        "required_consumers":["cutting_job_purchasing"],
        "objectives":[{"metric":"cutting_job_selected_order_cost","direction":"minimize"}]});
    let study: material_study::Request = serde_json::from_value(study).unwrap();
    let report = value(&study.evaluate(&pattern).unwrap());
    assert_eq!(
        report["alternatives"][0]["cutting_job"]["status"],
        "available"
    );
    assert!(report["alternatives"][0]["metrics"]["cutting_job_selected_order_cost"].is_null());
    assert_eq!(report["comparison"]["frontier"], json!([]));
}

#[test]
fn mixed_job_physical_stock_exclusion_follows_current_transitive_aliases() {
    let (pattern, request) = inputs();
    let mut request = priced(&pattern, &request);
    let original = request.build(&pattern).unwrap();
    let quotes = &mut request.purchasing.as_mut().unwrap().quotes.quotes;
    let mut bridge = quotes[0].clone();
    bridge.offer_id = "unused-bridge-a".into();
    bridge.material_id = "unneeded-product".into();
    bridge.material_sha256 = None;
    bridge.available = false;
    bridge.price_breaks = serde_json::from_value(json!([{
        "minimum_quantity":bridge.minimum_quantity * 2.0,
        "price":bridge.price,"sku":"middle-stock"
    }]))
    .unwrap();
    quotes.push(bridge.clone());
    bridge.offer_id = "unused-bridge-b".into();
    bridge.sku = "middle-stock".into();
    bridge.price_breaks[0].sku = Some("lining".into());
    quotes.push(bridge.clone());
    for split in [false, true] {
        let mut request = request.clone();
        if split {
            request.purchasing.as_mut().unwrap().market.split_orders = Some(
                serde_json::from_value(json!({"schema":"ftx-cut-order-allocation/1",
                    "max_search_nodes":10000}))
                .unwrap(),
            );
        }
        let blocked = request.build(&pattern).unwrap();
        assert!(blocked.complete_inventory);
        assert_eq!(blocked.inventory, original.inventory);
        let price = blocked.purchasing.as_ref().unwrap();
        assert!(!price.complete_offer_coverage && !price.complete_observed_quantity_coverage);
        assert!(price.least_observed_independent_order_total.is_none());
        assert!(price.selected_observed_order_total.is_none());
        assert_eq!(
            value(&price.roll)["result"]["complete_offer_coverage"],
            false
        );
        assert_eq!(
            value(&price.finite_stock)["result"]["complete_offer_coverage"],
            true
        );
        assert_eq!(
            price.roll_excluded_physical_stock_offer_ids,
            [
                "item/hide-001",
                "roll/lining",
                "unused-bridge-a",
                "unused-bridge-b"
            ]
        );

        // The latest source disconnects the bridge. Its superseded alias cannot reappear.
        let mut disconnected = bridge.clone();
        disconnected.observed_on = "2026-10-23".into();
        disconnected.price_breaks.clear();
        request
            .purchasing
            .as_mut()
            .unwrap()
            .quotes
            .quotes
            .push(disconnected);
        // Future observations do not change the earlier declared market's stock identity.
        let earlier = request.build(&pattern).unwrap();
        let earlier_price = earlier.purchasing.as_ref().unwrap();
        assert_eq!(value(&earlier_price.roll), value(&price.roll));
        assert_eq!(
            earlier_price.roll_excluded_physical_stock_offer_ids,
            price.roll_excluded_physical_stock_offer_ids
        );

        request.purchasing.as_mut().unwrap().market.as_of = "2026-10-23".into();
        request.purchasing.as_mut().unwrap().market.max_age_days = 1;
        let complete = request.build(&pattern).unwrap();
        let price = complete.purchasing.as_ref().unwrap();
        assert!(price.complete_offer_coverage && price.complete_observed_quantity_coverage);
        near(price.selected_observed_order_total.unwrap(), 8.0);
        near(price.least_observed_independent_order_total.unwrap(), 8.0);
        assert!(
            !price
                .roll_excluded_physical_stock_offer_ids
                .contains(&"roll/lining".into())
        );
    }
}

fn material_routing_inputs() -> (Pattern, job::Request, [Value; 2]) {
    let (pattern, request) = inputs();
    let mut request = value(&priced(&pattern, &request));
    let db = textile_import::catalog().unwrap();
    let hide: Selection =
        serde_json::from_value(pattern.raw()["fabric"]["material"].clone()).unwrap();
    let hide_hash = db.resolve(&hide).unwrap().sha256().to_owned();
    let mut choices = vec![json!({"material_sha256":hide_hash,"stock":"finite_stock"})];
    let mut roll_hashes = Vec::new();
    let materials = ["woven-linen", "performance-knit"].map(|name| {
        let mut selection = pattern.raw()["pieces"][1]["fabric"]["material"].clone();
        let id = format!("synthetic-{name}");
        selection["id"] = json!(id);
        selection["record"]["id"] = json!(id);
        selection["record"]["name"] = json!(format!("Synthetic {name} routing test, not measured"));
        if name == "performance-knit" {
            selection["record"]["form"] = json!("knit");
            selection["record"]["properties"]["width"]["value"] = json!(0.07);
        }
        let resolved = db
            .resolve(&serde_json::from_value(selection.clone()).unwrap())
            .unwrap();
        let hash = resolved.sha256().to_owned();
        roll_hashes.push(hash.clone());
        choices.push(json!({"material_sha256":hash,"stock":"roll"}));
        let mut quote = request["purchasing"]["quotes"]["quotes"][1].clone();
        quote["offer_id"] = json!(format!("roll/{name}"));
        quote["sku"] = json!(name);
        quote["material_id"] = json!(id);
        quote["material_sha256"] = json!(hash);
        quote["price"] = json!(if name == "performance-knit" { 6.0 } else { 4.0 });
        request["purchasing"]["quotes"]["quotes"]
            .as_array_mut()
            .unwrap()
            .push(quote);
        selection
    });
    request["schema"] = json!("ftx-cutting-job/2");
    request["routes"] = json!([{"piece":"lining","stock":"roll"}]);
    request["material_routes"] = json!([{"piece":"shell","choices":choices}]);
    request["roll"]["nesting"] =
        json!({"passes":1,"work_limit":100000,"grain_reversal_materials":roll_hashes});
    request["finite_stock"]["options"]["grain_reversal_materials"] = json!([hide_hash]);
    (pattern, serde_json::from_value(request).unwrap(), materials)
}

#[test]
fn exact_material_choices_reroute_the_same_shell_and_price_only_engaged_stock() {
    let (pattern, request, materials) = material_routing_inputs();
    let original = pattern.raw().clone();
    let mut study: Value = serde_json::from_str(STUDY).unwrap();
    study["cutting_job"] = value(&request);
    study["variants"] = json!([
        {"key":"hide","changes":{"schema":"ftx-material-bindings/1"}},
        {"key":"linen","changes":{"schema":"ftx-material-bindings/1","default":materials[0]}},
        {"key":"knit","changes":{"schema":"ftx-material-bindings/1","default":materials[1]}}
    ]);
    // This explicit synthetic study compares unlike width/finite-stock acquisition protocols.
    study["comparison"] = json!({"basis":"nominal","allow_mixed_protocols":true,"required_consumers":["cutting_job","cutting_job_purchasing"],
        "objectives":[{"metric":"cutting_job_observed_order_cost","direction":"minimize"}],"constraints":[]});
    let study: material_study::Request = serde_json::from_value(study).unwrap();
    let report = value(&study.evaluate(&pattern).unwrap());
    assert_eq!(pattern.raw(), &original);
    assert_eq!(report["geometry_changed"], false);
    for alternative in report["alternatives"].as_array().unwrap() {
        assert_eq!(alternative["cutting_job"]["status"], "available");
        let plan = &alternative["cutting_job"]["result"];
        assert_eq!(plan["schema"], "ftx-cutting-job-plan/2");
        assert_eq!(plan["inventory"].as_array().unwrap().len(), 6);
        assert_eq!(
            plan["purchasing"]["complete_observed_quantity_coverage"],
            true
        );
        let shell = plan["resolved_routes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["piece"] == "shell")
            .unwrap();
        let is_hide = alternative["key"] == "hide";
        assert_eq!(
            shell["stock"],
            if is_hide { "finite_stock" } else { "roll" }
        );
        assert_eq!(plan["finite_stock"].is_null(), !is_hide);
        assert_eq!(
            plan["purchasing"]["finite_stock"]["status"],
            if is_hide {
                "available"
            } else {
                "not_requested"
            }
        );
        near(
            alternative["metrics"]["cutting_job_observed_order_cost"]
                .as_f64()
                .unwrap(),
            match alternative["key"].as_str().unwrap() {
                "hide" => 8.0,
                "linen" => 1.4,
                "knit" => 1.6,
                _ => unreachable!(),
            },
        );
        let active_permissions =
            plan["roll"]["options"]["nesting"]["grain_reversal_materials"].as_array();
        assert_eq!(
            active_permissions.map_or(0, Vec::len),
            usize::from(!is_hide)
        );
        if let Some(permissions) = active_permissions {
            assert_eq!(permissions[0], shell["material_sha256"]);
        }
        assert_eq!(
            plan["request"]["roll"]["nesting"]["grain_reversal_materials"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        let replay: job::Request =
            serde_json::from_str(plan["replay_request_json"].as_str().unwrap()).unwrap();
        let frozen_pattern = Pattern::from_json(plan["pattern_json"].as_str().unwrap()).unwrap();
        assert_eq!(value(&replay.build(&frozen_pattern).unwrap()), *plan);
        if !is_hide {
            // The complete job scopes its explicit pool; the standalone roll planner stays strict.
            assert!(CutPlan::build(&frozen_pattern, request.roll.as_ref().unwrap()).is_err());
            let direct = procurement::evaluate(
                &replay.build(&frozen_pattern).unwrap().roll.unwrap(),
                &request.purchasing.as_ref().unwrap().quotes,
                &request.purchasing.as_ref().unwrap().market,
            )
            .unwrap();
            assert_eq!(plan["purchasing"]["roll"]["result"], direct);
        }
    }
    assert_eq!(report["comparison"]["frontier"], json!(["linen"]));
}

#[test]
fn routing_never_reuses_a_changed_same_id_snapshot_or_ambiguous_choice() {
    let (pattern, request, _) = material_routing_inputs();
    let mut changed = pattern.raw().clone();
    changed["fabric"]["material"]["record"]["properties"]["areal_density"]["value"] = json!(0.51);
    let refusal = request.build(&Pattern::new(changed)).unwrap_err();
    assert!(
        refusal
            .message
            .contains("no cutting route declares material snapshot")
    );
    for case in 0..11 {
        let mut bad = value(&request);
        match case {
            0 => {
                let duplicate = bad["material_routes"][0]["choices"][0].clone();
                bad["material_routes"][0]["choices"]
                    .as_array_mut()
                    .unwrap()
                    .push(duplicate);
            }
            1 => bad["routes"]
                .as_array_mut()
                .unwrap()
                .push(json!({"piece":"shell","stock":"roll"})),
            2 => bad["material_routes"][0]["piece"] = json!("unknown"),
            3 => bad["material_routes"][0]["choices"][0]["material_sha256"] = json!("0".repeat(64)),
            4 => bad["schema"] = json!("ftx-cutting-job/1"),
            5 => bad["roll"] = Value::Null,
            6 => bad["roll"]["nesting"]["grain_reversal_materials"] = json!(["0".repeat(64)]),
            7 => {
                bad["finite_stock"]["options"]["piece_zones"] =
                    json!({"lining":"wrong-stock-route"})
            }
            8 => {
                bad["material_routes"][0]["choices"][1]["material_sha256"] =
                    json!("ABCDEF".repeat(10) + "ABCD")
            }
            9 => bad["finite_stock"]["options"]["zone_margin_mm"] = json!(-1.0),
            _ => bad["finite_stock"]["options"]["zone_margin_mm"] = json!(1.0),
        }
        assert!(
            serde_json::from_value::<job::Request>(bad)
                .unwrap()
                .build(&pattern)
                .is_err(),
            "case {case}"
        );
    }
}

#[test]
fn material_routing_keeps_dormant_planner_copy_allowance_and_inventory_locks() {
    let (pattern, request, materials) = material_routing_inputs();
    let mut raw = pattern.raw().clone();
    raw["fabric"]["material"] = materials[0].clone();
    let pattern = Pattern::new(raw);
    for case in 0..4 {
        let mut bad = request.clone();
        let stock = bad.finite_stock.as_mut().unwrap();
        match case {
            0 => stock.options.copies = 1,
            1 => stock.options.seam_allowance_mm = Some(1.0),
            2 => {
                let mut selection = value(&stock.stocks[0].material);
                selection["expected_sha256"] = json!("0".repeat(64));
                stock.stocks[0].material = serde_json::from_value(selection).unwrap();
            }
            _ => stock.stocks.push(stock.stocks[0].clone()),
        }
        assert!(bad.build(&pattern).is_err(), "case {case}");
    }
    let (legacy_pattern, legacy) = inputs();
    let plan = value(&legacy.build(&legacy_pattern).unwrap());
    assert_eq!(plan["schema"], "ftx-cutting-job-plan/1");
    assert!(plan.get("resolved_routes").is_none());
    assert!(plan["request"].get("material_routes").is_none());
}

#[test]
fn material_routes_can_leave_roll_options_dormant_without_omitting_lining() {
    let (pattern, request, _) = material_routing_inputs();
    let db = textile_import::catalog().unwrap();
    let mut raw = pattern.raw().clone();
    let hide = raw["fabric"]["material"].clone();
    let hide_hash = db
        .resolve(&serde_json::from_value(hide.clone()).unwrap())
        .unwrap()
        .sha256()
        .to_owned();
    let lining_hash = db
        .resolve(&serde_json::from_value(raw["pieces"][1]["fabric"]["material"].clone()).unwrap())
        .unwrap()
        .sha256()
        .to_owned();
    raw["pieces"][1]["fabric"]["material"] = hide;
    let mut request = value(&request);
    request["routes"] = json!([]);
    request["material_routes"]
        .as_array_mut()
        .unwrap()
        .push(json!({"piece":"lining","choices":[
            {"material_sha256":hide_hash,"stock":"finite_stock"},
            {"material_sha256":lining_hash,"stock":"roll"}
        ]}));
    let request: job::Request = serde_json::from_value(request).unwrap();
    let plan = request.build(&Pattern::new(raw)).unwrap();
    assert!(plan.roll.is_none());
    assert_eq!(plan.inventory.len(), 6);
    assert!(
        plan.inventory
            .iter()
            .all(|i| i.stock == job::StockKind::FiniteStock)
    );
    assert_eq!(
        value(&plan.purchasing.as_ref().unwrap().roll)["status"],
        "not_requested"
    );
    near(
        plan.purchasing
            .unwrap()
            .least_observed_independent_order_total
            .unwrap(),
        7.0,
    );
}

#[test]
fn dormant_finite_inventory_retains_canonical_geometry_budgets() {
    let (pattern, request, materials) = material_routing_inputs();
    let mut raw = pattern.raw().clone();
    raw["fabric"]["material"] = materials[0].clone();
    let pattern = Pattern::new(raw);
    assert!(request.build(&pattern).unwrap().finite_stock.is_none());
    let polygon = vec![
        Vec2::new(1.0, 1.0),
        Vec2::new(2.0, 1.0),
        Vec2::new(1.0, 2.0),
    ];
    for case in 0..4 {
        let mut bad = request.clone();
        let stock = &mut bad.finite_stock.as_mut().unwrap().stocks[0];
        let zone = stock::zones::Zone {
            id: "synthetic-budget-zone".into(),
            outline_mm: polygon.clone(),
            geometry_source: stock.geometry_source.clone(),
        };
        match case {
            0 => {
                stock.outline_mm = vec![polygon[0]; 8192];
                stock.defects_mm = vec![vec![polygon[0]; 8192]];
                stock.occupied_mm = vec![vec![polygon[0]; 8192]];
                stock.zones = vec![stock::zones::Zone {
                    outline_mm: vec![polygon[0]; 8193],
                    ..zone
                }];
            }
            1 => stock.defects_mm = vec![polygon.clone(); 257],
            2 => stock.occupied_mm = vec![polygon.clone(); 4097],
            _ => stock.zones = vec![zone; 257],
        }
        assert_eq!(
            bad.build(&pattern).unwrap_err().code,
            RefusalCode::BudgetExceeded,
            "case {case} must refuse even when every selected route is roll stock"
        );
    }
}

#[test]
fn dormant_finite_pricing_retains_canonical_binding_and_work_limits() {
    let (pattern, request, materials) = material_routing_inputs();
    let mut raw = pattern.raw().clone();
    raw["fabric"]["material"] = materials[0].clone();
    let pattern = Pattern::new(raw);
    for case in 0..6 {
        let mut bad = request.clone();
        let pricing = bad.purchasing.as_mut().unwrap();
        match case {
            0 => pricing.finite_bindings = vec![pricing.finite_bindings[0].clone(); 129],
            1 => pricing
                .finite_bindings
                .push(pricing.finite_bindings[0].clone()),
            2 => pricing.finite_bindings[0].stock_id = "unknown-stock".into(),
            3 => pricing.finite_bindings[0].offer_ids = vec!["item/hide-001".into(); 4097],
            4 => pricing.quotes.quotes = vec![pricing.quotes.quotes[0].clone(); 16385],
            _ => pricing.finite_bindings[0].offer_ids = vec!["unknown-offer".into()],
        }
        let plan = bad.build(&pattern).unwrap();
        assert!(plan.complete_inventory);
        assert!(plan.finite_stock.is_none());
        assert_eq!(plan.inventory.len(), 6);
        let price = plan.purchasing.unwrap();
        assert!(!price.complete_offer_coverage);
        assert!(price.least_observed_independent_order_total.is_none());
        let refusal = price.aggregation_refusal.unwrap();
        assert_eq!(
            refusal.code,
            if matches!(case, 0 | 3 | 4) {
                RefusalCode::BudgetExceeded
            } else {
                RefusalCode::InvalidDocument
            }
        );
    }
}

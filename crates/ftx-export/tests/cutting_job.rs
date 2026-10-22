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
}

//! Synthetic independent whole-item price answers, not live offers or physical cutting qualification.
use ftx_export::cutting::{
    procurement::finite,
    stock::{Request, search::Objective},
};
use ftx_pattern::Pattern;
use serde_json::{Value, json};

const PATTERN: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-stock-pattern.json");
const STOCK: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-stock-optimization.json");

fn setup() -> (Pattern, Request) {
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let request: Request = serde_json::from_str(STOCK).unwrap();
    (pattern, request)
}
fn prices(pattern: &Pattern, r: &mut Request, amounts: [f64; 3]) {
    // The independent input geometry is qualified through the ordinary identity API. Prices are
    // authored whole-item observations; their numerical answers do not come from the cost search.
    r.options.optimize = None;
    r.pricing = None;
    let plan = r.build(pattern).unwrap();
    let ids = finite::identities(&plan).unwrap();
    let mut quotes = Vec::new();
    let mut bindings = Vec::new();
    for (identity, amount) in ids.iter().zip(amounts) {
        let offer = format!("synthetic/{}", identity.stock_id);
        quotes.push(json!({"offer_id":offer, "material_id":"example-hide",
            "material_sha256":identity.material_sha256, "supplier":"test", "sku":identity.stock_id,
            "price":amount, "unit":"square_meter", "quantity":identity.gross_area_m2,
            "minimum_quantity":identity.gross_area_m2, "order_increment":identity.gross_area_m2,
            "available_quantity":identity.gross_area_m2, "available":true, "currency":"USD",
            "region":"test", "quality":"test", "terms":"whole-item merchandise only",
            "observed_on":"2026-10-20", "source":"test:synthetic-not-live", "source_sha256":"a".repeat(64)}));
        bindings.push(
            json!({"stock_id":identity.stock_id, "stock_sha256":identity.stock_sha256,
            "offer_ids":[offer]}),
        );
    }
    r.pricing = Some(
        serde_json::from_value(json!({"schema":"ftx-finite-stock-pricing/1",
        "quotes":{"schema":"ftx-textile-quotes/1", "quotes":quotes}, "bindings":bindings,
        "market":{"currency":"USD", "region":"test", "quality":"test",
        "terms":"whole-item merchandise only", "as_of":"2026-10-20", "max_age_days":1,
        "require_observed_quantity":true}}))
        .unwrap(),
    );
    r.options.optimize = Some(Objective::MinimizePurchaseCost);
}
fn solve(pattern: &Pattern, request: &Request) -> Value {
    serde_json::to_value(request.build(pattern).unwrap()).unwrap()
}
fn cost(v: &Value) -> f64 {
    v["optimization"]["selected"]["purchase_cost"]
        .as_f64()
        .unwrap()
}
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-12, "{a} != {b}");
}

#[test]
fn price_objective_can_disagree_with_both_area_and_count_and_charges_each_hide_once() {
    let (p, mut r) = setup();
    for (amounts, expected, count) in [([100., 3., 4.], 7., 2), ([5., 10., 10.], 5., 1)] {
        prices(&p, &mut r, amounts);
        let before = serde_json::to_value(&r).unwrap();
        let plan = r.build(&p).unwrap();
        let v = serde_json::to_value(&plan).unwrap();
        assert_eq!(cost(&v), expected);
        assert_eq!(v["used_stock_count"], count);
        assert_eq!(v["purchasing"]["currency"], "USD");
        assert_eq!(
            v["purchasing"]["least_observed_independent_order_total"],
            expected
        );
        assert_eq!(v["purchasing"]["complete_observed_quantity_coverage"], true);
        assert_eq!(v["optimization"]["lattice_optimal"], true);
        assert_eq!(v["optimality_claim"], false);
        assert_eq!(v["hardware_execution_authorized"], false);
        let rep = finite::evaluate(&plan, r.pricing.as_ref().unwrap()).unwrap();
        assert_eq!(rep.least_observed_independent_order_total, Some(expected));
        assert!(rep.complete_offer_coverage);
        assert_eq!(v, solve(&p, plan.request()));
        assert_eq!(serde_json::to_value(&r).unwrap(), before);
        assert_eq!(
            plan.residual_request()
                .unwrap()
                .stocks
                .iter()
                .map(|s| s.occupied_mm.len())
                .sum::<usize>(),
            2
        );
    }
}

#[test]
fn tiny_inventory_matches_independent_enumerated_whole_item_costs() {
    let (p, mut r) = setup();
    // Either the large sheet holds both cutters, or two small sheets hold one each. Mixed choices
    // cannot beat the positive price of the large sheet alone. No other complete subset exists.
    for a in [2., 7., 13.] {
        for b in [1., 4., 9.] {
            for c in [1., 3., 8.] {
                prices(&p, &mut r, [a, b, c]);
                let v = solve(&p, &r);
                assert_eq!(cost(&v), a.min(b + c), "prices {a}/{b}/{c}");
                assert_eq!(v["optimization"]["lattice_optimal"], true);
            }
        }
    }
}

#[test]
fn unpriced_unavailable_unlocked_or_understocked_first_fit_is_not_a_zero_cost_winner() {
    let (p, mut r) = setup();
    for case in 0..6 {
        prices(&p, &mut r, [1., 3., 4.]);
        let pricing = r.pricing.as_mut().unwrap();
        match case {
            0 => {
                pricing.bindings.remove(0);
            }
            1 => pricing.quotes.quotes[0].available = false,
            2 => pricing.quotes.quotes[0].material_sha256 = None,
            3 => pricing.bindings[0].stock_sha256 = "0".repeat(64),
            4 => pricing.quotes.quotes[0].available_quantity = Some(0.0),
            _ => pricing.quotes.quotes[0].available_quantity = None,
        }
        let v = solve(&p, &r);
        assert_eq!(cost(&v), 7., "case {case}");
        let large = &v["purchasing"]["stocks"][0];
        assert_eq!(large["identity"]["used"], false);
        assert!(large["least_observed_order_cost"].is_null());
        assert!(
            !large["binding_refusal"].is_null()
                || !large["source_exclusions"].as_array().unwrap().is_empty()
                || !large["stock_exclusions"].as_array().unwrap().is_empty()
        );
    }
    prices(&p, &mut r, [1., 3., 4.]);
    let pricing = r.pricing.as_mut().unwrap();
    pricing.market.require_observed_quantity = false;
    pricing.quotes.quotes[0].available_quantity = None;
    let v = solve(&p, &r);
    assert_eq!(cost(&v), 1.);
    assert_eq!(
        v["purchasing"]["complete_observed_quantity_coverage"],
        false
    );
}

#[test]
fn latest_excluded_observation_never_revives_cheaper_old_offer_during_search() {
    let (p, mut r) = setup();
    prices(&p, &mut r, [1., 3., 4.]);
    let pricing = r.pricing.as_mut().unwrap();
    pricing.quotes.quotes[0].observed_on = "2026-10-19".into();
    let mut latest = pricing.quotes.quotes[0].clone();
    latest.observed_on = "2026-10-20".into();
    latest.material_sha256 = None;
    pricing.quotes.quotes.push(latest);
    assert_eq!(cost(&solve(&p, &r)), 7.);
}

#[test]
fn bounded_search_retains_complete_priced_incumbent_and_never_stale_trial_cuts() {
    let (p, mut r) = setup();
    prices(&p, &mut r, [100., 3., 4.]);
    r.options.backtrack_limit = Some(1);
    let plan = r.build(&p).unwrap();
    let v = serde_json::to_value(&plan).unwrap();
    near(cost(&v), 100.);
    assert_eq!(v["optimization"]["budget_exhausted"], true);
    assert_eq!(v["optimization"]["lattice_optimal"], false);
    assert_eq!(
        plan.residual_request().unwrap().stocks[0].occupied_mm.len(),
        2
    );
    assert!(
        plan.residual_request().unwrap().stocks[1]
            .occupied_mm
            .is_empty()
    );
    r.options.work_limit = 1;
    assert!(r.build(&p).is_err());
}

#[test]
fn prices_cannot_override_named_regions_or_physical_material_identity() {
    let (p, mut r) = setup();
    r.stocks[0].zones = vec![
        serde_json::from_value(json!({"id":"prime",
        "outline_mm":[[1,1],[69,1],[69,29],[1,29]], "geometry_source":{
        "reference":"test:prime", "accessed_on":"2026-10-20", "note":"Synthetic region"}}))
        .unwrap(),
    ];
    r.options.piece_zones.insert("front".into(), "prime".into());
    prices(&p, &mut r, [100., 1., 1.]);
    let v = solve(&p, &r);
    near(cost(&v), 100.);
    assert_eq!(v["used_stock_count"], 1);
    // Even an apparent cheap exact-stock lock cannot override the pattern's material revision.
    r.stocks[0].material.record.as_mut().unwrap().name = "Different revision".into();
    assert!(r.build(&p).is_err());
}

#[test]
fn residual_cost_search_requires_new_bindings_for_consumed_stock() {
    let (p, mut r) = setup();
    prices(&p, &mut r, [100., 3., 4.]);
    let first = r.build(&p).unwrap();
    let residual = first.residual_request().unwrap();
    let second = solve(&p, &residual);
    near(cost(&second), 100.); // the unchanged, unused large sheet still has a valid offer
    for row in second["purchasing"]["stocks"]
        .as_array()
        .unwrap()
        .iter()
        .skip(1)
    {
        assert!(
            row["binding_refusal"]["message"]
                .as_str()
                .unwrap()
                .contains("snapshot changed")
        );
    }
}

#[test]
fn missing_policy_all_unknown_prices_and_unrequested_cost_inputs_refuse() {
    let (p, mut r) = setup();
    r.options.optimize = Some(Objective::MinimizePurchaseCost);
    assert!(
        r.build(&p)
            .unwrap_err()
            .message
            .contains("requires pricing")
    );
    prices(&p, &mut r, [100., 3., 4.]);
    r.options.optimize = Some(Objective::MinimizeGrossArea);
    assert!(r.build(&p).is_err());
    r.options.optimize = Some(Objective::MinimizePurchaseCost);
    r.options.backtrack_limit = None;
    assert!(r.build(&p).is_err());
    r.options.backtrack_limit = Some(100);
    r.pricing.as_mut().unwrap().bindings.clear();
    assert!(
        r.build(&p)
            .unwrap_err()
            .message
            .contains("unknown prices are not zero")
    );
    let (_, plain) = setup();
    let v = solve(&p, &plain);
    assert!(v.get("purchasing").is_none());
    assert!(v["request"].get("pricing").is_none());
    assert!(v["optimization"]["selected"].get("purchase_cost").is_none());
}

//! Synthetic whole-item purchasing, not observed supplier prices or a checkout guarantee.
use ftx_export::cutting::{
    procurement::finite::{self, Request},
    stock,
};
use ftx_pattern::Pattern;
use serde_json::{Value, json};

const PATTERN: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-stock-pattern.json");
const STOCK: &str = include_str!("../../../tests/fixtures/materials/synthetic-stock-request.json");

fn plan(two_used: bool) -> stock::Plan {
    let mut request: Value = serde_json::from_str(STOCK).unwrap();
    if two_used {
        request["stocks"][0]["outline_mm"] = json!([[0, 0], [26, 0], [26, 26], [0, 26]]);
        request["stocks"][0]["defects_mm"] = json!([]);
    }
    let mut other = request["stocks"][0].clone();
    other["id"] = json!("hide-002");
    request["stocks"].as_array_mut().unwrap().push(other);
    serde_json::from_value::<stock::Request>(request)
        .unwrap()
        .build(&Pattern::from_json(PATTERN).unwrap())
        .unwrap()
}
fn pricing(plan: &stock::Plan) -> Request {
    let ids = finite::identities(plan).unwrap();
    let mut quotes = Vec::new();
    let mut bindings = Vec::new();
    for id in ids.into_iter().filter(|i| i.used) {
        let offer_id = format!("supplier/{}", id.stock_id);
        quotes.push(json!({"offer_id":offer_id,"material_id":"example-hide", "material_sha256":id.material_sha256,
            "supplier":"synthetic", "sku":id.stock_id, "price":17.0+id.stock_index as f64,
            "quantity":id.gross_area_m2,"unit":"square_meter","minimum_quantity":id.gross_area_m2,
            "order_increment":id.gross_area_m2,"available_quantity":id.gross_area_m2,"available":true,
            "currency":"USD","region":"test","quality":"test","terms":"merchandise only",
            "observed_on":"2026-10-20","source":"test:whole-item-price-not-live","source_sha256":"a".repeat(64)}));
        bindings.push(
            json!({"stock_id":id.stock_id,"stock_sha256":id.stock_sha256,"offer_ids":[offer_id]}),
        );
    }
    serde_json::from_value(json!({"schema":"ftx-finite-stock-pricing/1",
        "quotes":{"schema":"ftx-textile-quotes/1","quotes":quotes}, "bindings":bindings,
        "market":{"currency":"USD","region":"test","quality":"test","terms":"merchandise only",
        "as_of":"2026-10-20","max_age_days":2,"require_observed_quantity":true}}))
    .unwrap()
}
fn total(plan: &stock::Plan, request: &Request) -> Option<f64> {
    finite::evaluate(plan, request)
        .unwrap()
        .least_observed_independent_order_total
}

#[test]
fn exact_items_are_charged_once_and_unused_inventory_needs_no_price() {
    for two_used in [false, true] {
        let p = plan(two_used);
        let r = pricing(&p);
        let before = serde_json::to_value(&r).unwrap();
        let result = finite::evaluate(&p, &r).unwrap();
        assert_eq!(
            result.stocks.iter().filter(|s| s.identity.used).count(),
            if two_used { 2 } else { 1 }
        );
        assert_eq!(
            result.least_observed_independent_order_total,
            Some(if two_used { 35.0 } else { 17.0 })
        );
        assert!(result.complete_offer_coverage && result.complete_observed_quantity_coverage);
        assert!(!result.execution_authorized);
        assert_eq!(serde_json::to_value(&r).unwrap(), before);
        assert_eq!(
            serde_json::to_value(finite::evaluate(&p, &result.request).unwrap()).unwrap(),
            serde_json::to_value(result).unwrap()
        );
    }
}

#[test]
fn missing_binding_price_or_observed_quantity_never_becomes_free_stock() {
    let p = plan(true);
    let mut r = pricing(&p);
    r.bindings.pop();
    let out = finite::evaluate(&p, &r).unwrap();
    assert!(out.least_observed_independent_order_total.is_none());
    assert!(out.stocks[1].binding_refusal.is_some());
    assert_eq!(out.stocks[0].least_observed_order_cost, Some(17.0));
    let mut r = pricing(&p);
    r.quotes.quotes[1].available_quantity = None;
    assert_eq!(total(&p, &r), None);
    r.market.require_observed_quantity = false;
    let out = finite::evaluate(&p, &r).unwrap();
    assert_eq!(out.least_observed_independent_order_total, Some(35.0));
    assert!(!out.complete_observed_quantity_coverage);
    r.bindings.clear();
    r.quotes.quotes.clear();
    assert_eq!(total(&p, &r), None);
}

#[test]
fn complete_stock_area_is_not_usable_area_cut_area_or_a_linear_quote() {
    let p = plan(false);
    for case in 0..7 {
        let mut r = pricing(&p);
        let q = &mut r.quotes.quotes[0];
        match case {
            0 => q.quantity = p.markers()[0].defect_free_area_m2,
            1 => q.minimum_quantity = p.markers()[0].external_cut_area_m2,
            2 => q.order_increment /= 2.0,
            3 => q.available_quantity = Some(q.quantity * 2.0),
            4 => q.unit = ftx_pattern::material_cost::SaleUnit::LinearYard,
            5 => q.unit = ftx_pattern::material_cost::SaleUnit::Kilogram,
            _ => q.material_sha256 = None,
        }
        let out = finite::evaluate(&p, &r).unwrap();
        assert!(!out.complete_offer_coverage, "case {case}");
        assert_eq!(
            out.stocks[0].stock_exclusions.len()
                + out.stocks[0].source_exclusions.len()
                + out.stocks[0].source_refusals.len(),
            1
        );
    }
}

#[test]
fn changed_material_geometry_and_consumed_regions_invalidate_old_offer_bindings() {
    let p = plan(false);
    let r = pricing(&p);
    let pattern = Pattern::from_json(PATTERN).unwrap();
    // Same physical ID and equal gross area, but different source geometry.
    let mut changed = p.request().clone();
    changed.stocks[0].geometry_source.note.push_str(" revised");
    assert_eq!(total(&changed.build(&pattern).unwrap(), &r), None);
    let residual = p.residual_request().unwrap().build(&pattern).unwrap();
    assert_eq!(total(&residual, &r), None);
    let mut wrong_material = r.clone();
    wrong_material.quotes.quotes[0].material_sha256 = Some("0".repeat(64));
    let out = finite::evaluate(&p, &wrong_material).unwrap();
    assert_eq!(out.stocks[0].source_exclusions.len(), 1);
    assert!(out.least_observed_independent_order_total.is_none());
}

#[test]
fn latest_unavailable_expired_incompatible_or_unlocked_observation_does_not_revive_old_price() {
    let p = plan(false);
    for case in 0..6 {
        let mut r = pricing(&p);
        r.market.as_of = "2026-10-22".into();
        let mut latest = r.quotes.quotes[0].clone();
        latest.observed_on = "2026-10-21".into();
        match case {
            0 => latest.available = false,
            1 => latest.valid_through = Some("2026-10-21".into()),
            2 => latest.unit = ftx_pattern::material_cost::SaleUnit::LinearMeter,
            3 => latest.material_sha256 = None,
            4 => latest.available_quantity = Some(0.0),
            _ => latest.minimum_quantity *= 2.0,
        }
        r.quotes.quotes.push(latest);
        let out = finite::evaluate(&p, &r).unwrap();
        assert_eq!(out.stocks[0].offers.len(), 0, "case {case}");
        assert_eq!(
            out.stocks[0].stock_exclusions.len()
                + out.stocks[0].source_exclusions.len()
                + out.stocks[0].source_refusals.len(),
            1
        );
        assert_eq!(out.least_observed_independent_order_total, None);
    }
}

#[test]
fn intraday_scope_and_expiry_use_the_canonical_integer_clock() {
    let p = plan(false);
    let mut r = pricing(&p);
    r.quotes.schema = "ftx-textile-quotes/2".into();
    r.quotes.quotes[0].observed_at = Some("2026-10-20T10:00:00.000000001Z".into());
    let mut later = r.quotes.quotes[0].clone();
    later.observed_at = Some("2026-10-20T10:00:00.000000002Z".into());
    later.valid_through_at = later.observed_at.clone();
    later.price = 29.0;
    r.quotes.quotes.push(later);
    for (stamp, price) in [
        ("000000001", Some(17.0)),
        ("000000002", Some(29.0)),
        ("000000003", None),
    ] {
        r.market.as_of_at = Some(format!("2026-10-20T10:00:00.{stamp}Z"));
        let actual = total(&p, &r);
        assert_eq!(actual.is_some(), price.is_some());
        if let (Some(actual), Some(price)) = (actual, price) {
            assert!((actual - price).abs() < 1e-12);
        }
    }
    r.market.as_of_at = Some("2026-10-20T24:00:00Z".into());
    assert!(finite::evaluate(&p, &r).is_err());
}

#[test]
fn cheapest_bound_offer_is_deterministic_and_market_mismatches_remain_unknown() {
    let p = plan(false);
    let mut r = pricing(&p);
    let mut other = r.quotes.quotes[0].clone();
    other.offer_id = "a/other-source".into();
    other.price = 12.0;
    r.bindings[0].offer_ids.push(other.offer_id.clone());
    r.quotes.quotes.push(other);
    assert_eq!(total(&p, &r), Some(12.0));
    r.quotes.quotes[0].price = 12.0;
    assert_eq!(
        finite::evaluate(&p, &r).unwrap().stocks[0]
            .least_observed_offer_id
            .as_deref(),
        Some("a/other-source")
    );
    r.quotes.quotes.reverse();
    r.bindings[0].offer_ids.reverse();
    assert_eq!(
        finite::evaluate(&p, &r).unwrap().stocks[0]
            .least_observed_offer_id
            .as_deref(),
        Some("a/other-source")
    );
    r.market.terms = "delivered, tax included".into();
    assert_eq!(total(&p, &r), None);
}

#[test]
fn one_bad_history_does_not_erase_an_independent_whole_item_offer() {
    let p = plan(false);
    let mut r = pricing(&p);
    let mut good = r.quotes.quotes[0].clone();
    good.offer_id = "independent/whole-hide".into();
    good.price = 11.0;
    r.bindings[0].offer_ids.push(good.offer_id.clone());
    let mut revised = r.quotes.quotes[0].clone();
    r.quotes.quotes[0].observed_on = "2026-10-19".into();
    revised.unit = ftx_pattern::material_cost::SaleUnit::LinearMeter;
    r.quotes.quotes.extend([revised, good]);
    let result = finite::evaluate(&p, &r).unwrap();
    assert_eq!(result.stocks[0].offers.len(), 1);
    assert_eq!(result.stocks[0].source_refusals.len(), 1);
    assert!((result.least_observed_independent_order_total.unwrap() - 11.0).abs() < 1e-12);
}

#[test]
fn unknown_and_duplicate_identity_links_invalid_histories_and_budgets_refuse() {
    let p = plan(true);
    for case in 0..7 {
        let mut r = pricing(&p);
        match case {
            0 => r.bindings[1].offer_ids = r.bindings[0].offer_ids.clone(),
            1 => r.bindings[1].stock_id = r.bindings[0].stock_id.clone(),
            2 => r.bindings[0].stock_id = "not inventory".into(),
            3 => r.bindings[0].offer_ids = vec!["not an offer".into()],
            4 => r.bindings[0].stock_sha256 = "invalid".into(),
            5 => {
                let mut conflict = r.quotes.quotes[0].clone();
                conflict.price = 1.0;
                r.quotes.quotes.push(conflict);
            }
            _ => r.quotes.quotes = vec![r.quotes.quotes[0].clone(); 16385],
        }
        assert!(finite::evaluate(&p, &r).is_err(), "case {case}");
    }
}

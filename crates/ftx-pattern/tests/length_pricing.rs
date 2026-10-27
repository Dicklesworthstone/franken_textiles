//! Linear procurement known answers. Synthetic offers remain separate from the explicitly retained
//! source-observed thread quote used by the final regression.
use ftx_pattern::material_catalog::{ResolvedMaterial, Selection};
use ftx_pattern::material_cost::{Basis, ExclusionReason, Quote, QuoteBook, SaleUnit, Scope};
use ftx_pattern::materials::Catalog;
use serde_json::json;

fn material() -> ResolvedMaterial {
    let selection: Selection = serde_json::from_value(json!({"id":"synthetic-cord", "record":{
        "id":"synthetic-cord", "name":"Synthetic cord without width or density", "form":"yarn",
        "properties":{},"sources":{}}}))
    .unwrap();
    Catalog {
        schema: "ftx-textile-catalog/1".into(),
        materials: vec![],
    }
    .resolve(&selection)
    .unwrap()
}
fn quote() -> Quote {
    serde_json::from_value(json!({
        "offer_id":"synthetic-yard-offer", "material_id":"synthetic-cord",
        "material_sha256":material().sha256(), "supplier":"synthetic-supplier", "sku":"cord",
        "price":4.0,"quantity":0.5,"unit":"linear_yard","minimum_quantity":1.0,
        "order_increment":0.5,"currency":"USD","region":"test","quality":"synthetic",
        "terms":"merchandise only; excludes shipping and tax", "observed_on":"2026-10-22",
        "available":true,"available_quantity":5.0,"maximum_continuous_quantity":4.0,
        "source":"test:invented-quote","source_sha256":"0".repeat(64)
    }))
    .unwrap()
}
fn scope() -> Scope {
    serde_json::from_value(
        json!({"currency":"USD","region":"test","quality":"synthetic",
        "terms":"merchandise only; excludes shipping and tax","as_of":"2026-10-22",
        "max_age_days":0,"requirement":2.0,"basis":"length",
        "require_observed_quantity":true,"continuous_length_m":2.0,
        "require_observed_continuous_length":true}),
    )
    .unwrap()
}
fn book(quotes: Vec<Quote>) -> QuoteBook {
    QuoteBook {
        schema: "ftx-textile-quotes/1".into(),
        quotes,
    }
}
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-12, "{a} != {b}");
}

#[test]
fn yards_and_metres_price_exact_linear_stock_without_width_or_sheet_mechanics() {
    let m = material();
    assert!(m.mechanics().is_err());
    let result = book(vec![quote()]).evaluate(&m, &scope()).unwrap();
    let offer = &result.offers[0];
    near(offer.rate, 8.0 / 0.9144); // USD/m, not USD/m²
    near(offer.order_quantity, 2.5); // yards after HALF-yard increment rounding
    near(offer.acquired, 2.286); // metres
    near(offer.order_cost, 20.0);
    assert!(offer.quantity_sufficiency_observed);
    assert_eq!(offer.within_observed_continuous_limit, Some(true));
    assert!(!result.population_claim);
    let mut metre = quote();
    metre.unit = SaleUnit::LinearMeter;
    metre.price = 3.0;
    metre.quantity = 1.0;
    let result = book(vec![metre]).evaluate(&m, &scope()).unwrap();
    near(result.offers[0].rate, 3.0);
    near(result.offers[0].acquired, 2.0);
    near(result.offers[0].order_cost, 6.0);
}

#[test]
fn stock_and_continuity_apply_to_different_quantities_in_the_offer_unit() {
    let mut q = quote();
    q.available_quantity = Some(2.0); // enough nominal pieces is not enough rounded yards
    let r = book(vec![q.clone()])
        .evaluate(&material(), &scope())
        .unwrap();
    assert_eq!(
        r.excluded_offers[0].reason,
        ExclusionReason::InsufficientQuantity
    );
    assert_eq!(r.excluded_offers[0].required_order_quantity, Some(2.5));
    q.available_quantity = Some(100.0);
    q.maximum_continuous_quantity = Some(2.0); // 2 yards is less than the requested 2 metres
    let r = book(vec![q.clone()])
        .evaluate(&material(), &scope())
        .unwrap();
    assert_eq!(
        r.excluded_offers[0].reason,
        ExclusionReason::InsufficientContinuousLength
    );
    q.maximum_continuous_quantity = None;
    let r = book(vec![q.clone()])
        .evaluate(&material(), &scope())
        .unwrap();
    assert_eq!(
        r.excluded_offers[0].reason,
        ExclusionReason::UnknownContinuousLength
    );
    let mut permissive = scope();
    permissive.require_observed_continuous_length = false;
    let r = book(vec![q]).evaluate(&material(), &permissive).unwrap();
    assert_eq!(r.offers[0].within_observed_continuous_limit, None);
}

#[test]
fn missing_yarn_density_and_area_quotes_never_create_a_length_conversion() {
    let mut request = scope();
    request.continuous_length_m = None;
    request.require_observed_continuous_length = false;
    for (unit, reason) in [
        (
            SaleUnit::Kilogram,
            ExclusionReason::YarnLengthMassConversionUnavailable,
        ),
        (SaleUnit::SquareMeter, ExclusionReason::IncompatibleBasis),
    ] {
        let mut q = quote();
        q.unit = unit;
        q.maximum_continuous_quantity = None;
        let out = book(vec![q]).evaluate(&material(), &request).unwrap();
        assert!(out.offers.is_empty());
        assert!(out.rate_p10_p50_p90.is_none());
        assert_eq!(out.excluded_offers.len(), 1);
        assert_eq!(out.excluded_offers[0].reason, reason);
        assert_eq!(out.excluded_offers[0].required_order_quantity, None);
    }
    request.basis = Basis::Area;
    assert!(book(vec![quote()]).evaluate(&material(), &request).is_err());
    request.basis = Basis::Mass;
    request.continuous_length_m = Some(1.0);
    assert!(book(vec![quote()]).evaluate(&material(), &request).is_err());
}

#[test]
fn exact_snapshot_and_latest_observation_rules_are_not_weakened_by_length_basis() {
    let old = quote();
    let mut latest = old.clone();
    latest.observed_on = "2026-10-23".into();
    latest.available = false;
    let mut request = scope();
    request.as_of = latest.observed_on.clone();
    request.max_age_days = 10;
    let r = book(vec![old.clone(), latest])
        .evaluate(&material(), &request)
        .unwrap();
    assert!(r.offers.is_empty());
    assert_eq!(r.excluded_offers[0].reason, ExclusionReason::Unavailable);
    let mut selection = material().frozen_selection();
    selection.record.as_mut().unwrap().name = "Different cord lot".into();
    selection.expected_sha256 = None;
    let m = Catalog {
        schema: "ftx-textile-catalog/1".into(),
        materials: vec![],
    }
    .resolve(&selection)
    .unwrap();
    let r = book(vec![old]).evaluate(&m, &scope()).unwrap();
    assert!(r.offers.is_empty());
    assert_eq!(
        r.excluded_offers[0].reason,
        ExclusionReason::MaterialSnapshotMismatch
    );
}

#[test]
fn requested_uninterrupted_length_cannot_exceed_the_total_order_or_hide_bad_numbers() {
    for length in [2.1, 0.0, -1.0, f64::INFINITY, f64::NAN] {
        let mut request = scope();
        request.continuous_length_m = Some(length);
        assert!(book(vec![quote()]).evaluate(&material(), &request).is_err());
    }
    let mut request = scope();
    request.continuous_length_m = None;
    assert!(book(vec![quote()]).evaluate(&material(), &request).is_err());
    let q = quote();
    let r = book(vec![q]).evaluate(&material(), &scope()).unwrap();
    let roundtrip: Scope = serde_json::from_value(serde_json::to_value(&r.scope).unwrap()).unwrap();
    assert_eq!(roundtrip.basis, Basis::Length);
    assert_eq!(roundtrip.requirement, 2.0);
}

#[test]
fn observed_white_thread_tiers_price_whole_cones_and_preserve_unknown_inventory() {
    let quotes: QuoteBook = serde_json::from_str(include_str!(
        "../../../docs/examples/textile-thread-tier-quotes-2026-10-23.json"
    ))
    .unwrap();
    assert_eq!(quotes.quotes.len(), 1);
    let q = &quotes.quotes[0];
    let material = Catalog::builtin()
        .unwrap()
        .resolve(&Selection {
            id: q.material_id.clone(),
            ..Selection::default()
        })
        .unwrap();
    assert_eq!(q.material_sha256.as_deref(), Some(material.sha256()));
    assert_eq!(q.sku, "THR4800-5000WHT");
    assert_eq!(q.price_breaks[0].sku.as_deref(), Some("THR4800-5000WHT-t"));
    assert!(q.available_quantity.is_none());
    assert!(material.mechanics().is_err());
    let mut request = Scope {
        currency: q.currency.clone(),
        region: q.region.clone(),
        quality: q.quality.clone(),
        terms: q.terms.clone(),
        as_of: q.observed_on.clone(),
        as_of_at: q.observed_at.clone(),
        max_age_days: 0,
        requirement: 20_000.0,
        basis: Basis::Length,
        require_observed_quantity: false,
        continuous_length_m: None,
        require_observed_continuous_length: false,
    };
    let base = quotes.evaluate(&material, &request).unwrap();
    near(base.offers[0].order_quantity, 20_000.0);
    near(base.offers[0].order_cost, 199.96);
    assert!(base.offers[0].applied_price_break.is_none());
    request.requirement = 20_001.0;
    let bulk = quotes.evaluate(&material, &request).unwrap();
    near(bulk.offers[0].order_quantity, 25_000.0);
    near(bulk.offers[0].order_cost, 237.45);
    assert_eq!(
        bulk.offers[0].applied_price_break,
        Some(q.price_breaks[0].clone())
    );
    assert!(!bulk.population_claim);
    request.require_observed_quantity = true;
    let unknown = quotes.evaluate(&material, &request).unwrap();
    assert!(unknown.offers.is_empty());
    assert_eq!(
        unknown.excluded_offers[0].reason,
        ExclusionReason::UnknownQuantity
    );
}

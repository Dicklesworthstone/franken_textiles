//! Portable browser-boundary equivalence with the production stock and price engines.
use ftx_export::cutting::{procurement::finite, stock};
use ftx_pattern::Pattern;
use ftx_wasm::stock_pricing::{stock_identities, stock_price};
use serde_json::{Value, json};
const PATTERN: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-stock-pattern.json");
const STOCK: &str = include_str!("../../../tests/fixtures/materials/synthetic-stock-request.json");
const PRICE: &str = include_str!("../../../tests/fixtures/materials/synthetic-stock-pricing.json");
#[test]
fn identity_and_price_json_match_native_and_keep_unknown_required_costs_explicit() {
    let stock: stock::Request = serde_json::from_str(STOCK).unwrap();
    let plan = stock.build(&Pattern::from_json(PATTERN).unwrap()).unwrap();
    let price: finite::Request = serde_json::from_str(PRICE).unwrap();
    assert_eq!(
        stock_price(PATTERN, STOCK, PRICE).unwrap(),
        serde_json::to_string(&finite::evaluate(&plan, &price).unwrap()).unwrap()
    );
    assert_eq!(
        stock_identities(PATTERN, STOCK).unwrap(),
        serde_json::to_string(&finite::identities(&plan).unwrap()).unwrap()
    );
    let mut missing: Value = serde_json::from_str(PRICE).unwrap();
    missing["bindings"] = json!([]);
    let out: Value =
        serde_json::from_str(&stock_price(PATTERN, STOCK, &missing.to_string()).unwrap()).unwrap();
    assert_eq!(out["complete_offer_coverage"], false);
    assert!(out["least_observed_independent_order_total"].is_null());
    assert_eq!(out["stocks"][0]["identity"]["stock_id"], "hide-001");
}
#[test]
fn malformed_and_oversized_source_inputs_are_not_partial_success() {
    assert!(stock_price(PATTERN, STOCK, "{}").is_err());
    assert!(stock_identities("invalid", STOCK).is_err());
    assert_eq!(
        stock_price(PATTERN, STOCK, &" ".repeat(64 * 1024 * 1024))
            .unwrap_err()
            .code,
        ftx_core::RefusalCode::BudgetExceeded
    );
    let mut bad: Value = serde_json::from_str(STOCK).unwrap();
    bad["stocks"] = json!([]);
    assert!(stock_price(PATTERN, &bad.to_string(), PRICE).is_err());
}

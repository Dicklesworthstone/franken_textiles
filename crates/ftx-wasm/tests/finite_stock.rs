//! Native executions of the file-free browser boundary, not browser-runtime/hardware validation.
use ftx_core::RefusalCode;
use ftx_export::cutting::{preview::stock_svg, stock::Request};
use ftx_pattern::Pattern;
use ftx_wasm::manufacturing;
use serde_json::{Value, json};
const PATTERN: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-stock-pattern.json");
const STOCK: &str = include_str!("../../../tests/fixtures/materials/synthetic-stock-request.json");
#[test]
fn file_free_stock_and_preview_match_native_and_replay_frozen_inputs() {
    let p = Pattern::from_json(PATTERN).unwrap();
    let r: Request = serde_json::from_str(STOCK).unwrap();
    let native = r.build(&p).unwrap();
    let output = manufacturing::stock_plan(PATTERN, STOCK).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&output).unwrap(),
        serde_json::to_value(&native).unwrap()
    );
    assert_eq!(
        manufacturing::stock_svg(PATTERN, STOCK, 0).unwrap(),
        stock_svg(&native, 0).unwrap()
    );
    let frozen = serde_json::to_string(native.request()).unwrap();
    assert_eq!(output, manufacturing::stock_plan(PATTERN, &frozen).unwrap());
    let mut tampered: Value = serde_json::from_str(&frozen).unwrap();
    tampered["stocks"][0]["material"]["record"]["name"] = json!("New specimen");
    assert!(manufacturing::stock_plan(PATTERN, &tampered.to_string()).is_err());
}
#[test]
fn preview_has_true_mm_frame_escaped_labels_and_distinct_noncutting_stock_defects() {
    let mut p: Value = serde_json::from_str(PATTERN).unwrap();
    p["pieces"][0]["id"] = json!("panel<\"&");
    let mut r: Value = serde_json::from_str(STOCK).unwrap();
    r["stocks"][0]["id"] = json!("<script>alert(1)</script>");
    r["stocks"][0]["grain_mm"] = json!([[0, 0], [10, 0]]);
    let svg = manufacturing::stock_svg(&p.to_string(), &r.to_string(), 0).unwrap();
    assert!(svg.contains("width=\"100mm\" height=\"100mm\""));
    assert!(svg.contains("viewBox=\"-100 -100 100 100\""));
    assert!(!svg.contains("<script>"));
    assert!(svg.contains("&lt;script&gt;"));
    assert!(svg.contains("panel&lt;&quot;&amp;"));
    assert!(svg.contains("id=\"stock-boundary\""));
    assert!(svg.contains("id=\"stock-defects\""));
    assert!(svg.contains("NOT cut paths"));
    assert!(svg.contains("class=\"cut-exterior\""));
    assert!(!svg.contains("href="));
    assert!(manufacturing::stock_svg(PATTERN, STOCK, 1).is_err());
}
#[test]
fn budgets_precede_parsing_and_geometry_work_exhaustion_publishes_nothing() {
    let huge = " ".repeat(64 * 1024 * 1024);
    assert_eq!(
        manufacturing::stock_plan(PATTERN, &huge).unwrap_err().code,
        RefusalCode::BudgetExceeded
    );
    assert!(manufacturing::stock_plan("bad", STOCK).is_err());
    let mut r: Value = serde_json::from_str(STOCK).unwrap();
    r["options"]["work_limit"] = json!(1);
    assert_eq!(
        manufacturing::stock_svg(PATTERN, &r.to_string(), 0)
            .unwrap_err()
            .code,
        RefusalCode::BudgetExceeded
    );
}

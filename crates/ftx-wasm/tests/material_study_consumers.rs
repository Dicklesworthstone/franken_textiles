//! Native execution of the file-free browser study API, using real finite-stock/roll planners.
use ftx_export::{cutting::stock, material_study::Request};
use ftx_pattern::Pattern;
use ftx_wasm::materials;
use serde_json::{Value, json};

const PATTERN: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-stock-pattern.json");
const STOCK: &str = include_str!("../../../tests/fixtures/materials/synthetic-stock-request.json");

#[test]
fn file_free_stock_study_preserves_production_plans_and_independent_source_refusals() {
    let source: Value = serde_json::from_str(PATTERN).unwrap();
    let hide = source["fabric"]["material"].clone();
    let mut roll = hide.clone();
    roll["record"]["form"] = json!("woven");
    roll["record"]["properties"]["width"] = json!({"value":0.1,"unit":"m",
        "evidence":"estimate","source":"test","method":"Synthetic roll width"});
    let input = json!({"schema":"ftx-material-study/1","variants":[
        {"key":"hide","changes":{"schema":"ftx-material-bindings/1","default":hide}},
        {"key":"roll","changes":{"schema":"ftx-material-bindings/1","default":roll}}],
        "finite_stock":serde_json::from_str::<Value>(STOCK).unwrap(),
        "cutting":{"copies":1,"gap_mm":2.0,"edge_margin_mm":2.0,"end_margin_mm":2.0},
        "comparison":{"basis":"nominal","required_consumers":["finite_stock"],
            "objectives":[{"metric":"used_stock_mass_kg","direction":"minimize"}]}});
    let typed: Request = serde_json::from_value(input.clone()).unwrap();
    let native = typed.evaluate(&Pattern::new(source)).unwrap();
    let text = materials::study(PATTERN, &input.to_string()).unwrap();
    let result: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(result, serde_json::to_value(native).unwrap());
    assert_eq!(text, materials::study(PATTERN, &input.to_string()).unwrap());
    let rows = result["alternatives"].as_array().unwrap();
    let stock_pattern = Pattern::from_json(
        rows[0]["binding"]["result"]["pattern_json"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let inventory: stock::Request = serde_json::from_str(STOCK).unwrap();
    assert_eq!(
        rows[0]["finite_stock"]["result"],
        serde_json::to_value(inventory.build(&stock_pattern).unwrap()).unwrap()
    );
    assert_eq!(rows[0]["cutting"]["status"], "refused");
    assert_eq!(rows[1]["finite_stock"]["status"], "refused");
    assert_eq!(rows[1]["cutting"]["status"], "available");
    assert_eq!(result["comparison"]["frontier"], json!(["hide"]));
    assert_eq!(rows[0]["metrics"]["used_stock_count"], 1);
    assert!(
        (rows[0]["metrics"]["finite_stock_external_cut_area_m2"]
            .as_f64()
            .unwrap()
            - 0.0007)
            .abs()
            < 1e-14
    );

    let mut replay = input;
    replay["finite_stock"] = rows[0]["finite_stock"]["result"]["request"].clone();
    for (variant, row) in replay["variants"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .zip(rows)
    {
        variant["changes"] = row["frozen_changes"].clone();
    }
    let again: Value =
        serde_json::from_str(&materials::study(PATTERN, &replay.to_string()).unwrap()).unwrap();
    assert_eq!(again["alternatives"], result["alternatives"]);
    // Editing an inventory snapshot does not authorize substituting its locked material.
    replay["finite_stock"]["stocks"][0]["material"]["record"]["name"] = json!("Changed lot");
    let refused: Value =
        serde_json::from_str(&materials::study(PATTERN, &replay.to_string()).unwrap()).unwrap();
    assert!(
        refused["alternatives"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["finite_stock"]["status"] == "refused")
    );
    assert_eq!(
        refused["alternatives"][1]["cutting"],
        result["alternatives"][1]["cutting"]
    );
    assert_eq!(refused["comparison"]["frontier"], json!([]));
}

//! One bounded production search supplies every browser stock artifact; no browser-rendering claim.
use ftx_core::RefusalCode;
use ftx_export::cutting::{preview::stock_svg, stock::Request};
use ftx_pattern::Pattern;
use ftx_wasm::{manufacturing, materials};
use serde_json::{Value, json};

const PATTERN: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-stock-pattern.json");
const STOCK: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-stock-optimization.json");
const COST_STOCK: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-stock-cost.json");

#[test]
fn priced_stock_job_retains_native_prices_and_study_recomputes_the_same_layout() {
    let p = Pattern::from_json(PATTERN).unwrap();
    let r: Request = serde_json::from_str(COST_STOCK).unwrap();
    let plan = r.build(&p).unwrap();
    let job: Value =
        serde_json::from_str(&manufacturing::stock_job(PATTERN, COST_STOCK).unwrap()).unwrap();
    assert_eq!(
        job["plan_json"].as_str().unwrap(),
        serde_json::to_string(&plan).unwrap()
    );
    assert_eq!(
        job["purchasing_json"].as_str().unwrap(),
        serde_json::to_string(plan.purchasing().unwrap()).unwrap()
    );
    assert_eq!(
        job["residual_json"].as_str().unwrap(),
        serde_json::to_string(&plan.residual_request().unwrap()).unwrap()
    );
    assert_eq!(
        plan.purchasing()
            .unwrap()
            .least_observed_independent_order_total,
        Some(7.)
    );
    let study = json!({"schema":"ftx-material-study/1", "variants":[{"key":"current",
        "changes":{"schema":"ftx-material-bindings/1"}}], "finite_stock":r});
    let report: Value =
        serde_json::from_str(&materials::study(PATTERN, &study.to_string()).unwrap()).unwrap();
    let stock = &report["alternatives"][0]["finite_stock"];
    assert_eq!(stock["status"], "available");
    assert_eq!(
        stock["result"]["purchasing"]["least_observed_independent_order_total"],
        7.
    );
    assert_eq!(stock["result"]["used_stock_count"], 2);
    assert_eq!(stock["result"]["optimization"]["lattice_optimal"], true);
    let plain: Value =
        serde_json::from_str(&manufacturing::stock_job(PATTERN, STOCK).unwrap()).unwrap();
    assert!(plain.get("purchasing_json").is_none());
}

#[test]
fn one_job_retains_exact_plan_residual_preview_and_budget_limited_incumbent() {
    let pattern = Pattern::from_json(PATTERN).unwrap();
    for (objective, limit, count) in [
        ("minimize_gross_area", 10000, 2),
        ("minimize_stock_count", 10000, 1),
        ("minimize_gross_area", 1, 1),
    ] {
        let mut request: Value = serde_json::from_str(STOCK).unwrap();
        request["options"]["optimize"] = json!(objective);
        request["options"]["backtrack_limit"] = json!(limit);
        let input = request.to_string();
        let direct: Request = serde_json::from_str(&input).unwrap();
        let direct = direct.build(&pattern).unwrap();
        let actual = manufacturing::stock_job(PATTERN, &input).unwrap();
        let job: Value = serde_json::from_str(&actual).unwrap();
        assert_eq!(job["schema"], "ftx-stock-job/1");
        let plan = job["plan_json"].as_str().unwrap();
        let residual = job["residual_json"].as_str().unwrap();
        assert_eq!(plan, serde_json::to_string(&direct).unwrap());
        assert_eq!(
            residual,
            serde_json::to_string(&direct.residual_request().unwrap()).unwrap()
        );
        let index = direct
            .markers()
            .iter()
            .position(|m| !m.placements.is_empty())
            .unwrap();
        assert_eq!(job["stock_index"], index);
        assert_eq!(job["preview_svg"], stock_svg(&direct, index).unwrap());
        assert!(job["preview_refusal"].is_null());
        let report: Value = serde_json::from_str(plan).unwrap();
        assert_eq!(report["used_stock_count"], count);
        assert_eq!(report["optimization"]["budget_exhausted"], limit == 1);
        assert_eq!(report["optimization"]["lattice_optimal"], limit != 1);
        assert_eq!(report["optimality_claim"], false);
        // Frozen selections and source number spellings stay exactly as emitted by Rust.
        assert_eq!(
            actual,
            manufacturing::stock_job(PATTERN, &serde_json::to_string(direct.request()).unwrap())
                .unwrap()
        );
        let next: Request = serde_json::from_str(residual).unwrap();
        assert_eq!(
            next.stocks
                .iter()
                .map(|s| s.occupied_mm.len())
                .sum::<usize>(),
            2
        );
        assert_eq!(
            next.stocks[index].occupied_mm.len(),
            direct.markers()[index].placements.len()
        );
    }
}

#[test]
fn incomplete_or_changed_source_jobs_publish_no_artifacts() {
    let huge = " ".repeat(64 * 1024 * 1024);
    assert_eq!(
        manufacturing::stock_job(PATTERN, &huge).unwrap_err().code,
        RefusalCode::BudgetExceeded
    );
    let mut r: Value = serde_json::from_str(STOCK).unwrap();
    r["options"]["work_limit"] = json!(1);
    assert_eq!(
        manufacturing::stock_job(PATTERN, &r.to_string())
            .unwrap_err()
            .code,
        RefusalCode::BudgetExceeded
    );
    let job: Value =
        serde_json::from_str(&manufacturing::stock_job(PATTERN, STOCK).unwrap()).unwrap();
    let mut request: Request =
        serde_json::from_str(job["residual_json"].as_str().unwrap()).unwrap();
    request.stocks[0].material.record.as_mut().unwrap().name = "Changed lot".into();
    assert!(manufacturing::stock_job(PATTERN, &serde_json::to_string(&request).unwrap()).is_err());
    assert!(manufacturing::stock_job("bad", STOCK).is_err());
}

#[test]
fn study_metrics_use_selected_stock_not_first_fit_and_keep_budget_status_explicit() {
    let mut stock: Value = serde_json::from_str(STOCK).unwrap();
    for (limit, expected) in [(10000, 0.00125), (1, 0.0021)] {
        stock["options"]["backtrack_limit"] = json!(limit);
        let request = json!({"schema":"ftx-material-study/1", "variants":[{"key":"current",
            "changes":{"schema":"ftx-material-bindings/1"}}], "finite_stock":stock,
            "comparison":{"basis":"nominal","objectives":[{"metric":"used_stock_gross_area_m2",
                "direction":"minimize"}],"required_consumers":["finite_stock"]}});
        let report: Value =
            serde_json::from_str(&materials::study(PATTERN, &request.to_string()).unwrap())
                .unwrap();
        let row = &report["alternatives"][0];
        assert_eq!(row["finite_stock"]["status"], "available");
        assert!(
            (row["metrics"]["used_stock_gross_area_m2"].as_f64().unwrap() - expected).abs() < 1e-14
        );
        assert_eq!(
            row["finite_stock"]["result"]["optimization"]["budget_exhausted"],
            limit == 1
        );
        assert_eq!(report["comparison"]["frontier"], json!(["current"]));
        // Eligibility asserts a checked feasible layout, not proof of optimizer completion.
        assert_eq!(row["binding"]["status"], "available");
    }
}

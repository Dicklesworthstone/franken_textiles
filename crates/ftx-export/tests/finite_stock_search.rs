//! Known feasible synthetic inventories missed by first-fit. No physical/optimal-yield claim.
use ftx_core::RefusalCode;
use ftx_export::cutting::stock::Request;
use ftx_pattern::Pattern;
use serde_json::{Value, json};

const PATTERN: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-stock-pattern.json");
const STOCK: &str = include_str!("../../../tests/fixtures/materials/synthetic-stock-request.json");
fn rect(w: f64, h: f64) -> Value {
    json!([[0, 0], [w, 0], [w, h], [0, h]])
}
fn inputs(sizes: [[f64; 2]; 2], outlines: Vec<Value>) -> (Pattern, Request) {
    let mut p: Value = serde_json::from_str(PATTERN).unwrap();
    for (part, [w, h]) in p["pieces"].as_array_mut().unwrap().iter_mut().zip(sizes) {
        part["vertices"] = json!({"a":[0,0],"b":[w,0],"c":[w,h],"d":[0,h]});
    }
    let mut r: Value = serde_json::from_str(STOCK).unwrap();
    let mut stock = r["stocks"][0].clone();
    stock["defects_mm"] = json!([]);
    r["stocks"] = Value::Array(
        outlines
            .into_iter()
            .enumerate()
            .map(|(i, outline)| {
                let mut s = stock.clone();
                s["id"] = json!(format!("stock-{i}"));
                s["outline_mm"] = outline;
                s
            })
            .collect(),
    );
    (Pattern::new(p), serde_json::from_value(r).unwrap())
}
fn constrained_l() -> (Pattern, Request) {
    inputs(
        [[40., 10.], [15., 20.]],
        vec![json!([
            [0, 0],
            [65, 0],
            [65, 15],
            [25, 15],
            [25, 25],
            [0, 25]
        ])],
    )
}
#[test]
fn later_piece_can_force_an_earlier_cut_to_move_on_the_same_hide() {
    let (p, mut r) = constrained_l();
    assert!(r.build(&p).is_err());
    r.options.backtrack_limit = Some(100);
    let original = serde_json::to_value(&r).unwrap();
    let plan = r.build(&p).unwrap();
    let m = &plan.markers()[0];
    let a = m.placements.iter().find(|c| c.piece == "front").unwrap();
    let b = m.placements.iter().find(|c| c.piece == "pocket").unwrap();
    assert!(a.bounds_mm[0] >= b.bounds_mm[2] + r.options.gap_mm);
    assert!(b.bounds_mm[2] <= 23. && b.bounds_mm[3] <= 23.);
    assert!((m.external_cut_area_m2 - 0.0007).abs() < 1e-15);
    let report = serde_json::to_value(&plan).unwrap();
    assert!(report["backtracks_used"].as_u64().unwrap() > 0);
    assert!(report["work_used"].as_u64().unwrap() <= r.options.work_limit as u64);
    assert_eq!(
        report,
        serde_json::to_value(plan.request().build(&p).unwrap()).unwrap()
    );
    assert_eq!(serde_json::to_value(&r).unwrap(), original);
    assert_eq!(
        plan.residual_request().unwrap().stocks[0].occupied_mm.len(),
        2
    );
    assert_eq!(report["optimality_claim"], false);
}
#[test]
fn earlier_cut_can_change_physical_stock_without_leaving_ghost_allocations() {
    let (p, mut r) = inputs(
        [[20., 20.], [30., 10.]],
        vec![rect(35., 25.), rect(25., 25.)],
    );
    r.options.step_mm = 10.;
    assert!(r.build(&p).is_err());
    r.options.backtrack_limit = Some(20);
    let plan = r.build(&p).unwrap();
    assert_eq!(plan.markers()[0].placements.len(), 1);
    assert_eq!(plan.markers()[1].placements.len(), 1);
    assert_eq!(plan.markers()[0].placements[0].piece, "pocket");
    assert_eq!(plan.markers()[1].placements[0].piece, "front");
    assert!((plan.markers()[0].external_cut_area_m2 - 0.0003).abs() < 1e-15);
    assert!((plan.markers()[1].external_cut_area_m2 - 0.0004).abs() < 1e-15);
    let residual = plan.residual_request().unwrap();
    assert!(residual.stocks.iter().all(|s| s.occupied_mm.len() == 1));
}
#[test]
fn search_and_shared_work_limits_refuse_atomically_and_never_reset_on_undo() {
    let (p, mut r) = constrained_l();
    for limit in [0, 100001] {
        r.options.backtrack_limit = Some(limit);
        assert_eq!(r.build(&p).unwrap_err().code, RefusalCode::InvalidDocument);
    }
    r.options.backtrack_limit = Some(1);
    let before = serde_json::to_value(&r).unwrap();
    let e = r.build(&p).unwrap_err();
    assert_eq!(e.code, RefusalCode::BudgetExceeded);
    assert!(e.message.contains("backtrack limit"));
    assert_eq!(serde_json::to_value(&r).unwrap(), before);
    r.options.backtrack_limit = Some(100);
    let complete = serde_json::to_value(r.build(&p).unwrap()).unwrap();
    r.options.work_limit = complete["work_used"].as_u64().unwrap() as usize - 1;
    assert_eq!(r.build(&p).unwrap_err().code, RefusalCode::BudgetExceeded);
}
#[test]
fn no_fit_and_missing_region_or_material_never_become_partial_success() {
    let (p, mut r) = inputs([[20., 20.], [30., 10.]], vec![rect(25., 25.)]);
    r.options.step_mm = 10.;
    r.options.backtrack_limit = Some(20);
    assert!(r.build(&p).unwrap_err().message.contains("not a proof"));
    r.options
        .piece_zones
        .insert("front".into(), "missing".into());
    assert!(r.build(&p).unwrap_err().message.contains("required zone"));
    r.options.piece_zones.clear();
    r.stocks[0].material.record.as_mut().unwrap().name = "Different physical material".into();
    assert!(
        r.build(&p)
            .unwrap_err()
            .message
            .contains("no stock matches")
    );
}
#[test]
fn default_first_fit_and_an_unneeded_search_have_the_same_complete_geometry() {
    let (p, mut r) = inputs([[20., 20.], [15., 10.]], vec![rect(100., 100.)]);
    let baseline = r.build(&p).unwrap();
    let v = serde_json::to_value(&baseline).unwrap();
    assert!(v.get("backtracks_used").is_none());
    assert!(v["request"]["options"].get("backtrack_limit").is_none());
    r.options.backtrack_limit = Some(20);
    let searched = r.build(&p).unwrap();
    assert_eq!(
        serde_json::to_value(baseline.markers()).unwrap(),
        serde_json::to_value(searched.markers()).unwrap()
    );
    assert_eq!(
        serde_json::to_value(searched).unwrap()["backtracks_used"],
        0
    );
}

#[test]
fn overlapping_required_regions_remain_binding_during_reconsideration() {
    let (p, mut r) = constrained_l();
    let region = |id: &str, points: Value| {
        serde_json::from_value(json!({
            "id":id,"outline_mm":points,"geometry_source":{"reference":"test:search-zone",
            "accessed_on":"2026-10-20","note":"Synthetic admissible region, not measured quality"}
        }))
        .unwrap()
    };
    r.stocks[0].zones = vec![
        region("wide", json!([[1, 1], [64, 1], [64, 14], [1, 14]])),
        region("tall", json!([[1, 1], [24, 1], [24, 24], [1, 24]])),
    ];
    r.options.piece_zones.insert("front".into(), "wide".into());
    r.options.piece_zones.insert("pocket".into(), "tall".into());
    assert!(r.build(&p).is_err());
    r.options.backtrack_limit = Some(100);
    let plan = r.build(&p).unwrap();
    assert!(
        serde_json::to_value(&plan).unwrap()["backtracks_used"]
            .as_u64()
            .unwrap()
            > 0
    );
    for c in &plan.markers()[0].placements {
        if c.piece == "front" {
            assert!(c.bounds_mm[3] < 14.);
        } else {
            assert!(c.bounds_mm[2] < 24. && c.bounds_mm[3] < 24.);
        }
    }
    let next = plan.residual_request().unwrap();
    assert_eq!(next.options.piece_zones, r.options.piece_zones);
    assert_eq!(next.options.backtrack_limit, r.options.backtrack_limit);
    assert_eq!(next.stocks[0].occupied_mm.len(), 2);
}

#[test]
fn search_never_infers_grain_reversal_from_a_region_name() {
    let mut raw: Value = serde_json::from_str(PATTERN).unwrap();
    raw["pieces"].as_array_mut().unwrap().truncate(1);
    raw["pieces"][0]["vertices"] = json!({"a":[0,0],"b":[60,0],"c":[0,60]});
    raw["pieces"][0]["edges"] = json!([
        {"id":"bottom","from":"a","to":"b"},
        {"id":"diagonal","from":"b","to":"c"},
        {"id":"left","from":"c","to":"a"}]);
    let p = Pattern::new(raw);
    let (_, mut r) = inputs([[20., 20.], [15., 10.]], vec![rect(80., 80.)]);
    r.options.step_mm = 5.;
    r.options.backtrack_limit = Some(20);
    r.stocks[0].zones = vec![
        serde_json::from_value(json!({"id":"prime",
        "outline_mm":[[3,73],[73,3],[73,73]],"geometry_source":{
        "reference":"test:reverse-region","accessed_on":"2026-10-20","note":"Synthetic"}}))
        .unwrap(),
    ];
    r.options.piece_zones.insert("front".into(), "prime".into());
    assert!(r.build(&p).is_err());
    let material = ftx_pattern::textile_import::resolve_fabric(&p.raw()["fabric"])
        .unwrap()
        .unwrap();
    r.options
        .grain_reversal_materials
        .push(material.sha256().into());
    let plan = r.build(&p).unwrap();
    assert!(plan.markers()[0].placements[0].grain_reversed);
    assert_eq!(plan.markers()[0].placements[0].copy, 0);
}

#[test]
fn manufacturing_studies_share_the_same_search_and_keep_failure_local() {
    let (p, mut stock) = constrained_l();
    let evaluate = |stock: &Request| {
        let r: ftx_export::material_study::Request = serde_json::from_value(json!({
            "schema":"ftx-material-study/1", "variants":[{"key":"original", "changes":{
                "schema":"ftx-material-bindings/1"}}], "finite_stock":stock
        }))
        .unwrap();
        serde_json::to_value(r.evaluate(&p).unwrap()).unwrap()
    };
    assert_eq!(
        evaluate(&stock)["alternatives"][0]["finite_stock"]["status"],
        "refused"
    );
    stock.options.backtrack_limit = Some(100);
    let report = evaluate(&stock);
    let row = &report["alternatives"][0];
    assert_eq!(row["finite_stock"]["status"], "available");
    assert_eq!(
        row["finite_stock"]["result"]["markers"],
        serde_json::to_value(stock.build(&p).unwrap()).unwrap()["markers"]
    );
    assert_eq!(report, evaluate(&stock));
    stock.options.backtrack_limit = Some(1);
    let failed = evaluate(&stock);
    assert_eq!(
        failed["alternatives"][0]["finite_stock"]["status"],
        "refused"
    );
    assert_eq!(failed["alternatives"][0]["binding"]["status"], "available");
}

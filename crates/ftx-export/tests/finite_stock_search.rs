//! Known feasible synthetic inventories missed by first-fit. No physical/optimal-yield claim.
use ftx_core::RefusalCode;
use ftx_export::cutting::stock::Request;
use ftx_export::cutting::stock::search::Objective;
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

fn optimizing_inventory() -> (Pattern, Request) {
    let (p, mut r) = inputs(
        [[20., 20.], [20., 20.]],
        vec![rect(25., 25.), rect(25., 25.), rect(70., 30.)],
    );
    r.options.step_mm = 20.;
    r.options.backtrack_limit = Some(10_000);
    (p, r)
}
fn value(plan: &ftx_export::cutting::stock::Plan) -> Value {
    serde_json::to_value(plan).unwrap()
}
fn check_complete(plan: &ftx_export::cutting::stock::Plan) {
    let markers = plan.markers();
    let mut ids: Vec<_> = markers
        .iter()
        .flat_map(|m| &m.placements)
        .map(|p| (p.instance.as_str(), p.copy))
        .collect();
    ids.sort();
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1]);
    let next = plan.residual_request().unwrap();
    assert_eq!(
        next.stocks
            .iter()
            .map(|s| s.occupied_mm.len())
            .sum::<usize>(),
        2
    );
    for (s, m) in next.stocks.iter().zip(markers) {
        assert_eq!(s.occupied_mm.len(), m.placements.len());
        assert!((m.external_cut_area_m2 - m.placements.len() as f64 * 0.0004).abs() < 1e-14);
    }
}

#[test]
fn optimization_reduces_stock_count_without_a_hidden_area_objective() {
    let (p, mut r) = optimizing_inventory();
    let first = r.build(&p).unwrap();
    assert_eq!(value(&first)["used_stock_count"], 2);
    r.options.optimize = Some(Objective::MinimizeStockCount);
    let original = serde_json::to_value(&r).unwrap();
    let plan = r.build(&p).unwrap();
    let v = value(&plan);
    assert_eq!(v["used_stock_count"], 1);
    assert_eq!(plan.markers()[2].placements.len(), 2);
    let o = &v["optimization"];
    assert_eq!(o["baseline"]["stock_count"], 2);
    assert_eq!(o["selected"]["stock_count"], 1);
    assert_eq!(o["improvements"], 1);
    assert_eq!(o["termination"], "lattice_exhausted");
    assert_eq!(o["lattice_optimal"], true);
    assert_eq!(o["budget_exhausted"], false);
    assert!(o["selected"]["gross_area_m2"].as_f64() > o["baseline"]["gross_area_m2"].as_f64());
    assert_eq!(v["optimality_claim"], false); // no continuous-placement proof
    assert_eq!(v, value(&plan.request().build(&p).unwrap()));
    assert_eq!(original, serde_json::to_value(&r).unwrap());
    check_complete(&plan);
}

#[test]
fn area_objective_can_prefer_two_small_remnants_over_one_large_hide() {
    let (p, mut r) = optimizing_inventory();
    r.stocks.rotate_right(1); // one big sheet is now first-fit's complete choice
    r.options.optimize = Some(Objective::MinimizeGrossArea);
    let plan = r.build(&p).unwrap();
    let v = value(&plan);
    assert_eq!(v["optimization"]["baseline"]["stock_count"], 1);
    assert_eq!(v["optimization"]["selected"]["stock_count"], 2);
    assert!((v["used_gross_area_m2"].as_f64().unwrap() - 0.00125).abs() < 1e-15);
    assert_eq!(v["optimization"]["lattice_optimal"], true);
    assert!(plan.markers()[0].placements.is_empty());
    check_complete(&plan);
}

#[test]
fn optimization_preserves_complete_incumbents_at_both_work_and_undo_limits() {
    let (p, mut r) = optimizing_inventory();
    let baseline = r.build(&p).unwrap();
    let used = value(&baseline)["work_used"].as_u64().unwrap() as usize;
    r.options.optimize = Some(Objective::MinimizeStockCount);
    // At the first complete layout, before and after the charged backup copy. Both stop safely.
    for extra in [0, 1, 16, 20] {
        r.options.work_limit = used + extra;
        let plan = r.build(&p).unwrap();
        let v = value(&plan);
        assert_eq!(v["optimization"]["termination"], "work_limit");
        assert_eq!(v["optimization"]["lattice_optimal"], false);
        assert_eq!(v["work_used"], r.options.work_limit);
        assert_eq!(
            serde_json::to_value(plan.markers()).unwrap(),
            serde_json::to_value(baseline.markers()).unwrap()
        );
        check_complete(&plan);
    }
    r.options.work_limit = 2_000_000;
    let complete = value(&r.build(&p).unwrap());
    let undos = complete["backtracks_used"].as_u64().unwrap() as usize;
    let mut retained_improvement = false;
    for limit in 1..undos {
        r.options.backtrack_limit = Some(limit);
        let plan = r.build(&p).unwrap();
        let v = value(&plan);
        assert_eq!(v["optimization"]["termination"], "backtrack_limit");
        assert_eq!(v["optimization"]["lattice_optimal"], false);
        assert_eq!(v["backtracks_used"], limit);
        retained_improvement |= v["used_stock_count"] == 1;
        check_complete(&plan);
    }
    assert!(
        retained_improvement,
        "an improved complete layout must survive incomplete proof"
    );
    r.options.work_limit = used - 1;
    assert_eq!(r.build(&p).unwrap_err().code, RefusalCode::BudgetExceeded);
}

#[test]
fn optimization_never_promotes_partial_inventory_or_unknown_policies() {
    let (p, mut r) = constrained_l();
    r.options.optimize = Some(Objective::MinimizeStockCount);
    assert!(r.build(&p).unwrap_err().message.contains("backtrack_limit"));
    r.options.backtrack_limit = Some(1);
    assert_eq!(r.build(&p).unwrap_err().code, RefusalCode::BudgetExceeded);
    let mut raw = serde_json::to_value(&r).unwrap();
    raw["options"]["optimize"] = json!("guess_cheapest_supplier");
    assert!(serde_json::from_value::<Request>(raw).is_err());
    let (p, mut r) = optimizing_inventory();
    r.stocks.truncate(1); // not enough matching physical stock even under optimization
    r.options.optimize = Some(Objective::MinimizeGrossArea);
    assert!(r.build(&p).unwrap_err().message.contains("not a proof"));
}

#[test]
fn optional_optimizer_retains_legacy_identity_and_first_equal_layout() {
    let (p, mut r) = optimizing_inventory();
    r.stocks.rotate_right(1);
    let first = r.build(&p).unwrap();
    let before = value(&first);
    assert!(before.get("optimization").is_none());
    assert!(before["request"]["options"].get("optimize").is_none());
    r.options.optimize = Some(Objective::MinimizeStockCount);
    let plan = r.build(&p).unwrap();
    assert_eq!(
        serde_json::to_value(plan.markers()).unwrap(),
        serde_json::to_value(first.markers()).unwrap()
    );
    assert_eq!(value(&plan)["optimization"]["improvements"], 0);
}

#[test]
fn cheaper_inventory_cannot_bypass_region_material_defect_or_prior_cut_constraints() {
    for exclusion in ["region", "material", "defect", "prior"] {
        let (p, mut r) = optimizing_inventory();
        r.stocks.rotate_right(1);
        r.options.optimize = Some(Objective::MinimizeGrossArea);
        match exclusion {
            "region" => {
                r.stocks[0].zones = vec![serde_json::from_value(json!({"id":"prime", "outline_mm":[[1,1],[69,1],[69,29],[1,29]],
                    "geometry_source":{"reference":"test:prime","accessed_on":"2026-10-20","note":"Synthetic"}})).unwrap()];
                r.options.piece_zones.insert("front".into(), "prime".into());
            }
            "material" => {
                for s in &mut r.stocks[1..] {
                    s.material.record.as_mut().unwrap().name = "Different lot".into();
                }
            }
            _ => {
                for s in &mut r.stocks[1..] {
                    let obstacle = vec![
                        ftx_core::Vec2::new(10., 10.),
                        ftx_core::Vec2::new(14., 10.),
                        ftx_core::Vec2::new(14., 14.),
                        ftx_core::Vec2::new(10., 14.),
                    ];
                    if exclusion == "defect" {
                        s.defects_mm.push(obstacle);
                    } else {
                        s.occupied_mm.push(obstacle);
                        s.occupied_basis = Some("test:prior cut".into());
                    }
                }
            }
        }
        let plan = r.build(&p).unwrap();
        assert_eq!(plan.markers()[0].placements.len(), 2, "{exclusion}");
        assert_eq!(value(&plan)["optimization"]["improvements"], 0);
        assert_eq!(value(&plan)["optimization"]["lattice_optimal"], true);
    }
}

// Independent exhaustive answer for two unrotated rectangles. Enumerate physical item/position
// combinations directly; no production collision, allocation, objective or bound code is reused.
fn rectangular_oracle(sizes: [[f64; 2]; 2], stock_sizes: &[[f64; 2]]) -> (usize, f64) {
    let candidates = |size: [f64; 2]| {
        let mut all = Vec::new();
        for (stock, [w, h]) in stock_sizes.iter().copied().enumerate() {
            let guard = 128. * f64::EPSILON * (1. + w.max(h));
            let lo = 2. + guard;
            let xs = w - 2. - guard - size[0];
            let ys = h - 2. - guard - size[1];
            if xs < lo || ys < lo {
                continue;
            }
            let axis = |hi: f64| {
                let n = ((hi - lo) / 20.).ceil() as usize;
                (0..=n)
                    .map(|i| if i == n { hi } else { lo + i as f64 * 20. })
                    .collect::<Vec<_>>()
            };
            for y in axis(ys) {
                for x in axis(xs) {
                    all.push((stock, [x, y, x + size[0], y + size[1]]));
                }
            }
        }
        all
    };
    let mut count = usize::MAX;
    let mut area = f64::INFINITY;
    for (a, ab) in candidates(sizes[0]) {
        for (b, bb) in candidates(sizes[1]) {
            if a == b
                && !(ab[2] + 2. <= bb[0]
                    || bb[2] + 2. <= ab[0]
                    || ab[3] + 2. <= bb[1]
                    || bb[3] + 2. <= ab[1])
            {
                continue;
            }
            count = count.min(if a == b { 1 } else { 2 });
            let m2 = stock_sizes
                .iter()
                .enumerate()
                .filter(|(i, _)| *i == a || *i == b)
                .map(|(_, s)| s[0] * s[1] * 1e-6)
                .sum::<f64>();
            area = area.min(m2);
        }
    }
    (count, area)
}

#[test]
fn bounded_objectives_match_independent_exhaustive_rectangle_answers() {
    for sizes in [
        [[20., 20.], [20., 20.]],
        [[20., 10.], [15., 20.]],
        [[30., 10.], [10., 10.]],
    ] {
        for stock_sizes in [
            vec![[25., 25.], [25., 25.], [70., 30.]],
            vec![[70., 30.], [25., 25.], [25., 25.]],
            vec![[36., 36.], [45., 25.]],
            vec![[20., 35.], [40., 20.], [55., 45.]],
        ] {
            let expected = rectangular_oracle(sizes, &stock_sizes);
            for objective in [Objective::MinimizeStockCount, Objective::MinimizeGrossArea] {
                let (p, mut r) = inputs(
                    sizes,
                    stock_sizes.iter().map(|s| rect(s[0], s[1])).collect(),
                );
                r.options.step_mm = 20.;
                r.options.backtrack_limit = Some(10000);
                r.options.optimize = Some(objective);
                let plan = r.build(&p).unwrap();
                let v = value(&plan);
                assert_eq!(v["optimization"]["lattice_optimal"], true, "{v}");
                match objective {
                    Objective::MinimizeStockCount => assert_eq!(v["used_stock_count"], expected.0),
                    Objective::MinimizeGrossArea => assert!(
                        (v["used_gross_area_m2"].as_f64().unwrap() - expected.1).abs() < 1e-14
                    ),
                }
            }
        }
    }
}

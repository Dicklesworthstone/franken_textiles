//! Synthetic geometry/stock known answers; no measured fabric or machine-process claims.
use ftx_core::{RefusalCode, Vec2};
use ftx_export::{
    Ctx, ExportOptions, Source,
    cutter::cutter_ring,
    cutting::{CutOptions, CutPlan},
};
use ftx_pattern::{Pattern, textile_import::resolve_fabric};
use serde_json::{Value, json};

fn fabric(id: &str, width: f64) -> Value {
    json!({"material":{"id":id,"allow_estimates":true,"record":{
        "id":id,"name":"Artificial stock","form":"woven","properties":{
            "width":{"value":width,"unit":"m","source":"t","evidence":"estimate","method":"Synthetic test"},
            "areal_density":{"value":0.2,"unit":"kg/m2","source":"t","evidence":"estimate","method":"Synthetic test"}
        },"sources":{"t":{"reference":"test:stock","accessed_on":"2026-10-14","note":"Invented values for software tests"}}
    }}})
}
fn piece(id: &str, w: f64, h: f64) -> Value {
    json!({"id":id,"cut":{"qty":1},"grain":[[0,0],[0,10]],
        "vertices":{"a":[0,0],"b":[w,0],"c":[w,h],"d":[0,h]},
        "edges":[{"id":"bottom","from":"a","to":"b"},{"id":"right","from":"b","to":"c"},
            {"id":"top","from":"c","to":"d"},{"id":"left","from":"d","to":"a"}]})
}
fn pattern(pieces: Vec<Value>) -> Pattern {
    Pattern::new(
        json!({"schema":"fabric-pattern/1","units":"mm","fabric":fabric("shell",0.1),
        "meta":{"sa_mm":0},"pieces":pieces,"seams":[]}),
    )
}
fn options() -> CutOptions {
    CutOptions {
        copies: 1,
        gap_mm: 5.,
        edge_margin_mm: 5.,
        end_margin_mm: 10.,
        seam_allowance_mm: None,
        nesting: None,
    }
}
fn check_marker(plan: &CutPlan) {
    for roll in &plan.rolls {
        for a in &roll.placements {
            let [x0, y0, x1, y1] = a.bounds_mm;
            assert!(
                x0 >= plan.options.edge_margin_mm - 1e-10
                    && y0 >= plan.options.end_margin_mm - 1e-10
            );
            assert!(x1 <= roll.width_m * 1000. - plan.options.edge_margin_mm + 1e-10);
            assert!(y1 <= roll.length_m * 1000. - plan.options.end_margin_mm + 1e-10);
            let d = a.grain_mm[1] - a.grain_mm[0];
            assert!(d.x.abs() < 1e-9 && d.y > 0.);
        }
        for (i, a) in roll.placements.iter().enumerate() {
            for b in &roll.placements[i + 1..] {
                let g = plan.options.gap_mm;
                assert!(
                    a.bounds_mm[2] + g <= b.bounds_mm[0] + 1e-10
                        || b.bounds_mm[2] + g <= a.bounds_mm[0] + 1e-10
                        || a.bounds_mm[3] + g <= b.bounds_mm[1] + 1e-10
                        || b.bounds_mm[3] + g <= a.bounds_mm[1] + 1e-10
                );
            }
        }
    }
}

#[test]
fn roll_requirements_count_real_cut_instances_not_simulated_vertices_or_net_area() {
    let mut a = piece("front", 40., 100.);
    a["cut"]["qty"] = json!(2);
    a["sim"] = json!(false);
    a["sim_scale"] = json!([9., 9.]);
    let p = pattern(vec![a, piece("pocket", 40., 50.)]);
    let before = p.raw().clone();
    let plan = CutPlan::build(&p, &options()).unwrap();
    assert_eq!(p.raw(), &before);
    assert_eq!(plan.rolls.len(), 1);
    let roll = &plan.rolls[0];
    assert_eq!(roll.placements.len(), 3);
    assert!((roll.length_m - 0.175).abs() < 1e-14); // 100 + 5 + 50 + 20 mm.
    assert!((roll.external_cut_area_m2 - 0.010).abs() < 1e-14);
    assert!((roll.stock_area_m2 - 0.0175).abs() < 1e-14);
    assert!((roll.stock_mass_kg.unwrap() - 0.0035).abs() < 1e-14);
    assert!(roll.material.mechanics().is_err()); // Purchasing does not invent mechanics to proceed.
    check_marker(&plan);
}

#[test]
fn first_fit_shelves_reuse_space_instead_of_only_filling_the_last_shelf() {
    let p = pattern(vec![
        piece("tall", 45., 100.),
        piece("wide", 60., 90.),
        piece("small", 40., 80.),
    ]);
    let plan = CutPlan::build(&p, &options()).unwrap();
    assert_eq!(plan.rolls[0].length_m, 0.215);
    let placements = &plan.rolls[0].placements;
    let a = placements.iter().find(|p| p.instance == "tall").unwrap();
    let b = placements.iter().find(|p| p.instance == "small").unwrap();
    assert_eq!(a.bounds_mm[1], b.bounds_mm[1]);
    check_marker(&plan);
}

#[test]
fn per_piece_overrides_share_snapshot_identity_with_the_material_resolver() {
    let mut cuff = piece("cuff", 40., 60.);
    cuff["fabric"] = fabric("knit", 0.12);
    cuff["material"] = json!("informational label, not a SKU");
    let p = pattern(vec![piece("shell", 50., 100.), cuff]);
    let plan = CutPlan::build(&p, &options()).unwrap();
    assert_eq!(plan.rolls.len(), 2);
    for roll in &plan.rolls {
        let piece = &p.raw()["pieces"][if roll.material.material().id == "knit" {
            1
        } else {
            0
        }];
        let f = piece.get("fabric").unwrap_or(&p.raw()["fabric"]);
        let expected = resolve_fabric(f).unwrap().unwrap();
        assert_eq!(roll.material.sha256(), expected.sha256());
    }
    assert!(plan.rolls.iter().all(|r| r.placements.len() == 1));
    check_marker(&plan);
}

#[test]
fn changing_width_changes_layout_and_purchase_area_without_changing_cut_shapes() {
    let p = pattern(vec![piece("a", 40., 100.), piece("b", 40., 100.)]);
    let a = CutPlan::build(&p, &options()).unwrap();
    let mut raw = p.raw().clone();
    raw["fabric"] = fabric("shell", 0.075);
    let b = CutPlan::build(&Pattern::new(raw), &options()).unwrap();
    assert_eq!(a.rolls[0].length_m, 0.12);
    assert_eq!(b.rolls[0].length_m, 0.225);
    assert_eq!(
        a.rolls[0].external_cut_area_m2,
        b.rolls[0].external_cut_area_m2
    );
    assert_ne!(a.rolls[0].material.sha256(), b.rolls[0].material.sha256());
    assert!(b.rolls[0].stock_area_m2 > a.rolls[0].stock_area_m2);
    check_marker(&b);
}

#[test]
fn unfolded_pieces_mirror_pairs_copies_and_notch_tabs_match_export_geometry() {
    let mut fold = piece("fold", 30., 80.);
    fold["cut"]["on_fold"] = json!("left");
    let mut pair = piece("pair", 20., 50.);
    pair["cut"]["qty"] = json!(2);
    pair["notches"] = json!([{"id":"n","edge":"right","s":0.5}]);
    let p = pattern(vec![fold, pair]);
    let mut o = options();
    o.copies = 2;
    o.seam_allowance_mm = Some(2.);
    let plan = CutPlan::build(&p, &o).unwrap();
    assert_eq!(plan.rolls[0].placements.len(), 6);
    let ctx = Ctx::new(
        &p,
        ExportOptions {
            sa: Some(2.),
            ..ExportOptions::default()
        },
    )
    .unwrap();
    for placed in &plan.rolls[0].placements {
        let instance = p.instance(&placed.instance).unwrap();
        let g = ctx.geom(Source::Instance(instance), 6.).unwrap();
        let ring = cutter_ring(&g).unwrap();
        let t = placed.transform;
        let transformed: Vec<_> = ring
            .iter()
            .map(|v| {
                Vec2::new(
                    t[0][0] * v.x + t[0][1] * v.y + t[0][2],
                    t[1][0] * v.x + t[1][1] * v.y + t[1][2],
                )
            })
            .collect();
        assert_eq!(transformed.len(), placed.cut_mm.len());
        for (a, b) in transformed.iter().zip(&placed.cut_mm) {
            assert!((*a - *b).norm() < 1e-10);
        }
    }
    check_marker(&plan);
}

#[test]
fn grain_polarity_and_canonical_grain_are_preserved() {
    let mut down = piece("down", 40., 80.);
    down["grain"] = json!({"p0":[0,10],"p1":[0,0]});
    let plan = CutPlan::build(&pattern(vec![down]), &options()).unwrap();
    assert!(plan.rolls[0].placements[0].transform[0][0] < -0.99);
    check_marker(&plan);
}

#[test]
fn output_is_deterministic_and_source_piece_order_does_not_change_layout() {
    let a = piece("a", 35., 90.);
    let b = piece("b", 30., 80.);
    let p = pattern(vec![a.clone(), b.clone()]);
    let plan = CutPlan::build(&p, &options()).unwrap();
    assert_eq!(
        serde_json::to_value(&plan).unwrap(),
        serde_json::to_value(CutPlan::build(&p, &options()).unwrap()).unwrap()
    );
    let reversed = CutPlan::build(&pattern(vec![b, a]), &options()).unwrap();
    assert_eq!(
        serde_json::to_value(plan.rolls).unwrap(),
        serde_json::to_value(reversed.rolls).unwrap()
    );
}

#[test]
fn invalid_incomplete_ambiguous_unscaled_and_unsupported_inputs_refuse() {
    let base = pattern(vec![piece("p", 40., 80.)]);
    for mutation in 0..11 {
        let mut v = base.raw().clone();
        match mutation {
            0 => {
                v["meta"]["scale_status"] = json!("UNSCALED");
            }
            1 => {
                v["pieces"][0].as_object_mut().unwrap().remove("grain");
            }
            2 => {
                v["pieces"][0]["grain"] = json!([[0, 0], [0, 0]]);
            }
            3 => {
                v["fabric"]["material"]["record"]["properties"]
                    .as_object_mut()
                    .unwrap()
                    .remove("width");
            }
            4 => {
                v["fabric"]["material"]["allow_estimates"] = json!(false);
            }
            5 => {
                v["fabric"]["material"]["record"]["form"] = json!("hide");
            }
            6 => {
                v["pieces"][0]["fabric"] = json!({"areal_g_m2":200});
            }
            7 => {
                v["pieces"][0]["material_id"] = json!("unsupported");
            }
            8 => {
                v["fabric"]["material"]["record"]["properties"]["width"]["value"] = json!(0.01);
            }
            9 => {
                v["pieces"][0]["cut"]["qty"] = json!(3);
            }
            _ => {
                v["fabric"]["layers"] = json!([]);
            }
        }
        assert!(
            CutPlan::build(&Pattern::new(v), &options()).is_err(),
            "case {mutation}"
        );
    }
    let mut o = options();
    o.copies = 0;
    assert!(CutPlan::build(&base, &o).is_err());
    o = options();
    o.gap_mm = f64::NAN;
    assert!(CutPlan::build(&base, &o).is_err());
    o = options();
    o.copies = 4096;
    let p = pattern(vec![piece("a", 40., 80.), piece("b", 40., 80.)]);
    assert_eq!(
        CutPlan::build(&p, &o).unwrap_err().code,
        RefusalCode::BudgetExceeded
    );
}

#[test]
fn missing_density_is_unknown_not_zero_and_does_not_block_cutting() {
    let mut raw = pattern(vec![piece("p", 40., 80.)]).raw().clone();
    raw["fabric"]["material"]["record"]["properties"]
        .as_object_mut()
        .unwrap()
        .remove("areal_density");
    let plan = CutPlan::build(&Pattern::new(raw), &options()).unwrap();
    assert!(plan.rolls[0].stock_mass_kg.is_none());
    assert!(plan.rolls[0].mass_unavailable.is_some());
}

fn concave_pair() -> Pattern {
    let mut a = piece("elbow", 80.0, 100.0);
    a["vertices"] = json!({"a":[0,0],"b":[80,0],"c":[80,20],"d":[20,20],"e":[20,100],"f":[0,100]});
    a["edges"] = json!([
        {"id":"bottom","from":"a","to":"b"},{"id":"right","from":"b","to":"c"},
        {"id":"inner-horizontal","from":"c","to":"d"},{"id":"inner-vertical","from":"d","to":"e"},
        {"id":"top","from":"e","to":"f"},{"id":"left","from":"f","to":"a"}]);
    pattern(vec![a, piece("pocket", 40.0, 50.0)])
}

fn triangular_pair() -> Pattern {
    let mut a = piece("a", 80., 100.);
    a["vertices"] = json!({"a":[0,0],"b":[80,0],"c":[0,100]});
    a["edges"] = json!([
        {"id":"bottom","from":"a","to":"b"},
        {"id":"diagonal","from":"b","to":"c"},
        {"id":"left","from":"c","to":"a"}]);
    a["internal"] = json!([{"kind":"cut","pts":[[5,8],[10,8]]}]);
    a["drills"] = json!([{"p":[8,12],"d":2}]);
    let mut b = a.clone();
    b["id"] = json!("b");
    pattern(vec![a, b])
}

#[test]
fn permitted_half_turn_nests_complementary_pieces_and_reduces_actual_purchase_cost() {
    use ftx_export::cutting::procurement::{Market, evaluate};
    use ftx_pattern::material_cost::QuoteBook;
    let p = triangular_pair();
    let before = p.raw().clone();
    let mut o = options();
    o.nesting = Some(Default::default());
    let one_way = CutPlan::build(&p, &o).unwrap();
    assert!(
        one_way.rolls[0]
            .placements
            .iter()
            .all(|p| !p.grain_reversed)
    );
    o.nesting.as_mut().unwrap().grain_reversal_materials =
        vec![one_way.rolls[0].material.sha256().to_owned()];
    let nested = CutPlan::build(&p, &o).unwrap();
    assert_eq!(p.raw(), &before);
    assert_eq!(
        serde_json::to_value(&nested).unwrap(),
        serde_json::to_value(CutPlan::build(&p, &o).unwrap()).unwrap()
    );
    let a = &one_way.rolls[0];
    let b = &nested.rolls[0];
    assert!((b.length_m - 0.120).abs() < 1e-12);
    assert!(a.length_m - b.length_m > 0.08);
    assert_eq!(a.external_cut_area_m2, b.external_cut_area_m2);
    assert!(b.stock_area_m2 < a.stock_area_m2 && b.stock_mass_kg < a.stock_mass_kg);
    assert!(b.utilization > a.utilization);
    assert_eq!(
        b.nesting.as_ref().unwrap().algorithm,
        "bounded_polygon_compaction/2"
    );
    assert!(!b.nesting.as_ref().unwrap().budget_exhausted);
    let lower = b.placements.iter().find(|p| !p.grain_reversed).unwrap();
    let upper = b.placements.iter().find(|p| p.grain_reversed).unwrap();
    // Independent analytic separation between the triangles' parallel diagonal edges.
    let normal = Vec2::new(1. / 80., 1. / 100.);
    let bottom = lower
        .cut_mm
        .iter()
        .map(|p| p.dot(normal))
        .fold(f64::NEG_INFINITY, f64::max);
    let top = upper
        .cut_mm
        .iter()
        .map(|p| p.dot(normal))
        .fold(f64::INFINITY, f64::min);
    assert!((top - bottom) / normal.norm() >= o.gap_mm - 1e-10);
    let ctx = Ctx::new(&p, ExportOptions::default()).unwrap();
    for placed in &b.placements {
        let g = ctx
            .geom(Source::Instance(p.instance(&placed.instance).unwrap()), 6.)
            .unwrap();
        let t = placed.transform;
        assert!((t[0][0] * t[1][1] - t[0][1] * t[1][0] - 1.).abs() < 1e-12);
        let map = |v: Vec2| {
            Vec2::new(
                t[0][0] * v.x + t[0][1] * v.y + t[0][2],
                t[1][0] * v.x + t[1][1] * v.y + t[1][2],
            )
        };
        for (source, actual) in cutter_ring(&g).unwrap().iter().zip(&placed.cut_mm) {
            assert!((map(*source) - *actual).norm() < 1e-10);
        }
        for ((path, closed), (actual, actual_closed)) in
            g.internal_cut.iter().zip(&placed.internal_cut_mm)
        {
            assert_eq!(closed, actual_closed);
            for (source, actual) in path.iter().zip(actual) {
                assert!((map(*source) - *actual).norm() < 1e-10);
            }
        }
        for ((point, diameter), (actual, d)) in g.drills.iter().zip(&placed.drills_mm) {
            assert!((map(*point) - *actual).norm() < 1e-10);
            assert_eq!(diameter, d);
        }
        let d = placed.grain_mm[1] - placed.grain_mm[0];
        assert!(d.x.abs() < 1e-10 && (d.y < 0.) == placed.grain_reversed);
    }
    let mut raw = p.raw().clone();
    raw["pieces"].as_array_mut().unwrap().reverse();
    assert_eq!(
        serde_json::to_value(&nested.rolls).unwrap(),
        serde_json::to_value(CutPlan::build(&Pattern::new(raw), &o).unwrap().rolls).unwrap()
    );
    let book: QuoteBook = serde_json::from_value(json!({"schema":"ftx-textile-quotes/1","quotes":[{
        "offer_id":"synthetic/shell", "material_id":"shell", "supplier":"Synthetic supplier", "sku":"shell",
        "price":10., "quantity":1., "unit":"linear_meter", "minimum_quantity":0.05, "order_increment":0.05,
        "currency":"USD", "region":"US", "quality":"test", "terms":"ex-tax-shipping",
        "observed_on":"2026-10-18", "available":true, "source":"test:grain-reversal", "source_sha256":"0".repeat(64)
    }]})).unwrap();
    let market: Market =
        serde_json::from_value(json!({"currency":"USD","region":"US","quality":"test",
        "terms":"ex-tax-shipping","as_of":"2026-10-18","max_age_days":3}))
        .unwrap();
    let full_price = evaluate(&one_way, &book, &market).unwrap();
    let nested_price = evaluate(&nested, &book, &market).unwrap();
    assert_eq!(full_price["least_observed_independent_order_total"], 2.5);
    assert!(
        (nested_price["least_observed_independent_order_total"]
            .as_f64()
            .unwrap()
            - 1.5)
            .abs()
            < 1e-12
    );
    let svg = ftx_export::cutting::preview::svg(&nested, 0).unwrap();
    assert!(svg.contains("data-grain-reversed=\"true\""));
}

#[test]
fn reversal_is_material_scoped_refuses_stale_permissions_and_preserves_budget_fallback() {
    let mut raw = triangular_pair().raw().clone();
    let mut other = raw["pieces"].as_array().unwrap().clone();
    for piece in &mut other {
        piece["id"] = json!(format!("other-{}", piece["id"].as_str().unwrap()));
        piece["fabric"] = fabric("directional", 0.1);
    }
    raw["pieces"].as_array_mut().unwrap().extend(other);
    let p = Pattern::new(raw.clone());
    let baseline = CutPlan::build(&p, &options()).unwrap();
    let material = baseline
        .rolls
        .iter()
        .find(|r| r.material.material().id == "shell")
        .unwrap()
        .material
        .sha256()
        .to_owned();
    let mut o = options();
    o.nesting = Some(Default::default());
    let search = o.nesting.as_mut().unwrap();
    search.grain_reversal_materials = vec![material];
    search.work_limit = 1;
    let limited = CutPlan::build(&p, &o).unwrap();
    for (a, b) in baseline.rolls.iter().zip(&limited.rolls) {
        assert_eq!(
            serde_json::to_value(&a.placements).unwrap(),
            serde_json::to_value(&b.placements).unwrap()
        );
        assert!(b.nesting.as_ref().unwrap().budget_exhausted);
    }
    o.nesting.as_mut().unwrap().work_limit = 1_000_000;
    let nested = CutPlan::build(&p, &o).unwrap();
    for roll in &nested.rolls {
        assert_eq!(
            roll.placements.iter().any(|p| p.grain_reversed),
            roll.material.material().id == "shell"
        );
    }
    raw["fabric"]["material"]["record"]["name"] = json!("New material finish");
    let err = CutPlan::build(&Pattern::new(raw), &o).unwrap_err();
    assert!(err.message.contains("grain_reversal_materials"));
    let search = o.nesting.as_mut().unwrap();
    search
        .grain_reversal_materials
        .push(search.grain_reversal_materials[0].clone());
    assert!(
        CutPlan::build(&p, &o)
            .unwrap_err()
            .message
            .contains("unique exact")
    );
}

#[test]
fn polygon_nesting_recovers_concave_space_and_updates_real_stock_requirements() {
    let p = concave_pair();
    let before = p.raw().clone();
    let shelf = CutPlan::build(&p, &options()).unwrap();
    let mut o = options();
    o.nesting = Some(Default::default());
    let nested = CutPlan::build(&p, &o).unwrap();
    let a = &shelf.rolls[0];
    let b = &nested.rolls[0];
    assert!((a.length_m - 0.175).abs() < 1e-12);
    assert!((b.length_m - 0.120).abs() < 1e-12);
    assert_eq!(a.external_cut_area_m2, b.external_cut_area_m2);
    assert!(b.stock_area_m2 < a.stock_area_m2 && b.stock_mass_kg < a.stock_mass_kg);
    assert!(b.utilization > a.utilization);
    assert_eq!(p.raw(), &before);
    let search = b.nesting.as_ref().unwrap();
    assert!(search.saved_length_m > 0.054 && search.accepted_moves > 0);
    assert!(!search.optimality_claim && !search.budget_exhausted);
    // Independent known geometry: the L occupies its bottom 20 mm and leftmost 20 mm.
    // The rectangle is inside the empty corner, outside BOTH occupied rectangles by the full gap.
    let elbow = b.placements.iter().find(|p| p.instance == "elbow").unwrap();
    let pocket = b
        .placements
        .iter()
        .find(|p| p.instance == "pocket")
        .unwrap();
    assert!(pocket.bounds_mm[0] >= elbow.bounds_mm[0] + 20.0 + o.gap_mm);
    assert!(pocket.bounds_mm[1] >= elbow.bounds_mm[1] + 20.0 + o.gap_mm);
    assert!(pocket.bounds_mm[2] < elbow.bounds_mm[2] && pocket.bounds_mm[3] < elbow.bounds_mm[3]);
    for placement in &b.placements {
        let d = placement.grain_mm[1] - placement.grain_mm[0];
        assert!(d.x.abs() < 1e-10 && d.y > 0.0);
    }
}

#[test]
fn exhausted_nesting_retains_every_shelf_piece_and_never_publishes_a_trial_move() {
    use ftx_export::cutting::nesting::NestingOptions;
    let p = concave_pair();
    let baseline = CutPlan::build(&p, &options()).unwrap();
    let mut o = options();
    o.nesting = Some(NestingOptions {
        passes: 8,
        work_limit: 1,
        ..NestingOptions::default()
    });
    let result = CutPlan::build(&p, &o).unwrap();
    assert_eq!(
        serde_json::to_value(&baseline.rolls[0].placements).unwrap(),
        serde_json::to_value(&result.rolls[0].placements).unwrap()
    );
    assert_eq!(baseline.rolls[0].length_m, result.rolls[0].length_m);
    let report = result.rolls[0].nesting.as_ref().unwrap();
    assert!(report.budget_exhausted);
    assert_eq!((report.work_used, report.accepted_moves), (1, 0));
    assert_eq!(report.saved_length_m, 0.0);
}

#[test]
fn nesting_is_deterministic_source_order_independent_and_transform_preserving() {
    let p = concave_pair();
    let mut o = options();
    o.nesting = Some(Default::default());
    let a = CutPlan::build(&p, &o).unwrap();
    let again = CutPlan::build(&p, &o).unwrap();
    assert_eq!(
        serde_json::to_value(&a).unwrap(),
        serde_json::to_value(again).unwrap()
    );
    let mut raw = p.raw().clone();
    raw["pieces"].as_array_mut().unwrap().reverse();
    let reordered = CutPlan::build(&Pattern::new(raw), &o).unwrap();
    assert_eq!(
        serde_json::to_value(&a.rolls).unwrap(),
        serde_json::to_value(reordered.rolls).unwrap()
    );
    let ctx = Ctx::new(&p, ExportOptions::default()).unwrap();
    for placed in &a.rolls[0].placements {
        let geom = ctx
            .geom(Source::Instance(p.instance(&placed.instance).unwrap()), 6.0)
            .unwrap();
        let original = cutter_ring(&geom).unwrap();
        assert_eq!(original.len(), placed.cut_mm.len());
        for (old, new) in original.iter().zip(&placed.cut_mm) {
            let t = placed.transform;
            let expected = Vec2::new(
                t[0][0] * old.x + t[0][1] * old.y + t[0][2],
                t[1][0] * old.x + t[1][1] * old.y + t[1][2],
            );
            assert!((expected - *new).norm() < 1e-10);
        }
    }
}

#[test]
fn nesting_policy_validation_and_rectangular_baseline_are_preserved() {
    let p = pattern(vec![piece("a", 40.0, 100.0), piece("b", 40.0, 80.0)]);
    let mut o = options();
    let baseline = CutPlan::build(&p, &o).unwrap();
    assert!(baseline.rolls[0].nesting.is_none());
    assert!(
        serde_json::to_value(&baseline).unwrap()["options"]
            .get("nesting")
            .is_none()
    );
    o.nesting = Some(Default::default());
    let b = CutPlan::build(&p, &o).unwrap();
    assert!(b.rolls[0].length_m <= baseline.rolls[0].length_m + 1e-14);
    for (passes, work_limit) in [(0, 100), (9, 100), (1, 0), (1, 20_000_001)] {
        o.nesting = Some(ftx_export::cutting::nesting::NestingOptions {
            passes,
            work_limit,
            ..Default::default()
        });
        assert!(CutPlan::build(&p, &o).is_err());
    }
    assert!(
        serde_json::from_value::<CutOptions>(json!({"copies":1,"gap_mm":1,
        "edge_margin_mm":1,"end_margin_mm":1,"nesting":{"flip_grain":true}}))
        .is_err()
    );
}

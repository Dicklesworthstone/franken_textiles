//! Synthetic geometry/stock known answers; no measured fabric or machine-process claims.
use ftx_core::{RefusalCode, Vec2};
use ftx_export::{
    Ctx, ExportOptions, Source,
    cutter::cutter_ring,
    cutting::{CutOptions, CutPlan, WidthBasis},
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
        width_basis: Default::default(),
        end_margin_mm: 10.,
        seam_allowance_mm: None,
        maximum_marker_length_m: None,
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
            let layout_width = roll
                .layout_width
                .as_ref()
                .map_or(roll.width_m, |w| w.width_m);
            assert!(x1 <= layout_width * 1000. - plan.options.edge_margin_mm + 1e-10);
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
fn source_lower_width_replans_complete_geometry_and_preserves_nominal_purchase_basis() {
    let p = pattern(vec![
        piece("front", 40.0, 100.0),
        piece("back", 40.0, 100.0),
    ]);
    let mut raw = p.raw().clone();
    let width = &mut raw["fabric"]["material"]["record"]["properties"]["width"];
    width["value"] = json!(100.0);
    width["unit"] = json!("mm");
    width["bounds"] = json!([80.0, 110.0]);
    let p = Pattern::new(raw.clone());
    let mut opts = options();
    let nominal = CutPlan::build(&p, &opts).unwrap();
    assert_eq!(nominal.rolls[0].length_m, 0.12);
    assert!(
        serde_json::to_value(&opts)
            .unwrap()
            .get("width_basis")
            .is_none()
    );
    assert!(
        serde_json::to_value(&nominal.rolls[0])
            .unwrap()
            .get("layout_width")
            .is_none()
    );
    let mut explicit = serde_json::to_value(&opts).unwrap();
    explicit["width_basis"] = json!("nominal");
    let explicit = CutPlan::build(&p, &serde_json::from_value(explicit).unwrap()).unwrap();
    assert_eq!(
        serde_json::to_string(&nominal).unwrap(),
        serde_json::to_string(&explicit).unwrap()
    );

    opts.width_basis = WidthBasis::ReportedLowerBound;
    let bounded = CutPlan::build(&p, &opts).unwrap();
    let roll = &bounded.rolls[0];
    assert_eq!(bounded.schema, nominal.schema);
    assert_eq!(roll.width_m, 0.1);
    assert_eq!(roll.layout_width.as_ref().unwrap().width_m, 0.08);
    assert_eq!(
        roll.layout_width.as_ref().unwrap().reported_bounds_m,
        [0.08, 0.11]
    );
    assert_eq!(roll.length_m, 0.225);
    assert!((roll.stock_area_m2 - 0.0225).abs() < 1e-14);
    assert!((roll.stock_mass_kg.unwrap() - 0.0045).abs() < 1e-14);
    assert_eq!(
        roll.external_cut_area_m2,
        nominal.rolls[0].external_cut_area_m2
    );
    assert_eq!(roll.material.sha256(), nominal.rolls[0].material.sha256());
    assert_eq!(p.raw(), &raw);
    check_marker(&bounded);
    let svg = ftx_export::cutting::preview::svg(&bounded, 0).unwrap();
    assert!(svg.contains("id=\"stock\" width=\"100\""));
    assert!(svg.contains("id=\"layout-width\" width=\"80\""));
    assert!(svg.contains("Reported source width range: 80 to 110 mm"));
    assert!(
        !ftx_export::cutting::preview::svg(&nominal, 0)
            .unwrap()
            .contains("layout-width")
    );

    for work_limit in [1, 100_000] {
        opts.nesting = Some(ftx_export::cutting::nesting::NestingOptions {
            work_limit,
            ..Default::default()
        });
        let nested = CutPlan::build(&p, &opts).unwrap();
        check_marker(&nested);
        assert_eq!(nested.rolls[0].length_m, bounded.rolls[0].length_m);
        assert_eq!(nested.rolls[0].placements.len(), 2);
        assert_eq!(
            nested.rolls[0].nesting.as_ref().unwrap().budget_exhausted,
            work_limit == 1
        );
    }
    opts.nesting = None;
    opts.maximum_marker_length_m = Some(0.12);
    let partitioned = CutPlan::build(&p, &opts).unwrap();
    assert_eq!(partitioned.rolls.len(), 2);
    assert!(
        partitioned
            .rolls
            .iter()
            .all(|r| r.length_m == 0.12 && r.placements.len() == 1)
    );
    assert_eq!(
        partitioned
            .rolls
            .iter()
            .map(|r| r.external_cut_area_m2)
            .sum::<f64>(),
        roll.external_cut_area_m2
    );
    check_marker(&partitioned);
}

#[test]
fn lower_width_requires_admitted_positive_source_bounds_and_refuses_oversized_cutters() {
    let p = pattern(vec![piece("front", 75.0, 100.0)]);
    let mut opts = options();
    assert!(CutPlan::build(&p, &opts).is_ok());
    opts.width_basis = WidthBasis::ReportedLowerBound;
    assert!(
        CutPlan::build(&p, &opts)
            .unwrap_err()
            .message
            .contains("source-reported width bounds")
    );
    for bounds in [
        json!([0.0, 0.11]),
        json!([-0.01, 0.11]),
        json!([0.11, 0.12]),
        json!([0.08, 0.09]),
    ] {
        let mut raw = p.raw().clone();
        raw["fabric"]["material"]["record"]["properties"]["width"]["bounds"] = bounds;
        assert!(CutPlan::build(&Pattern::new(raw), &opts).is_err());
    }
    let mut raw = p.raw().clone();
    raw["fabric"]["material"]["record"]["properties"]["width"]["bounds"] = json!([0.08, 0.11]);
    let error = CutPlan::build(&Pattern::new(raw.clone()), &opts).unwrap_err();
    assert!(error.message.contains("usable width 70"));
    raw["fabric"]["material"]["allow_estimates"] = json!(false);
    assert!(
        CutPlan::build(&Pattern::new(raw.clone()), &opts)
            .unwrap_err()
            .message
            .contains("estimate")
    );
    raw["fabric"]["material"]["allow_estimates"] = json!(true);
    raw["fabric"]["material"]["record"]["properties"]["width"]["bounds"] = json!([0.1, 0.1]);
    let exact_pattern = Pattern::new(raw);
    let exact = CutPlan::build(&exact_pattern, &opts).unwrap();
    assert_eq!(
        exact.rolls[0].layout_width.as_ref().unwrap().width_m,
        exact.rolls[0].width_m
    );
    opts.edge_margin_mm = 50.0;
    assert!(
        CutPlan::build(&exact_pattern, &opts)
            .unwrap_err()
            .message
            .contains("usable width")
    );
}

#[test]
fn lower_width_marker_requirements_reach_whole_and_split_supplier_orders() {
    use ftx_export::cutting::procurement::evaluate;
    let mut raw = pattern(vec![
        piece("front", 40.0, 100.0),
        piece("back", 40.0, 100.0),
    ])
    .raw()
    .clone();
    raw["fabric"]["material"]["record"]["properties"]["width"]["bounds"] = json!([0.08, 0.11]);
    let p = Pattern::new(raw);
    let mut opts = options();
    opts.maximum_marker_length_m = Some(0.12);
    let nominal = CutPlan::build(&p, &opts).unwrap();
    opts.width_basis = WidthBasis::ReportedLowerBound;
    let bounded = CutPlan::build(&p, &opts).unwrap();
    let (_, mut quotes, mut market) = split_order_fixture();
    for quote in &mut quotes {
        quote["material_sha256"] = json!(bounded.rolls[0].material.sha256());
    }
    let book = split_quote_book(quotes.clone());
    let nominal_price = evaluate(&nominal, &book, &market).unwrap();
    let bounded_price = evaluate(&bounded, &book, &market).unwrap();
    assert!(
        (nominal_price["selected_observed_order_total"]
            .as_f64()
            .unwrap()
            - 0.15)
            .abs()
            < 1e-12
    );
    assert!(
        (bounded_price["selected_observed_order_total"]
            .as_f64()
            .unwrap()
            - 0.45)
            .abs()
            < 1e-12
    );
    assert_eq!(bounded_price["complete_offer_coverage"], true);
    assert_eq!(bounded_price["order_allocation_optimality_proven"], true);
    assert_eq!(
        bounded_price["material_orders"][0]["required_length_m"],
        0.24
    );
    assert_eq!(
        bounded_price["material_orders"][0]["required_stock_area_m2"],
        0.024
    );
    assert_eq!(
        bounded_price["material_orders"][0]["allocation"]["selected_orders"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    market.split_orders = None;
    assert_eq!(
        evaluate(&bounded, &book, &market).unwrap()["complete_offer_coverage"],
        false
    );
    quotes.truncate(1);
    quotes[0]["available_quantity"] = json!(0.25);
    quotes[0]["price"] = json!(10.0);
    let book = split_quote_book(quotes);
    let whole = evaluate(&bounded, &book, &market).unwrap();
    assert_eq!(
        whole["material_orders"][0]["least_observed_order"]["order_quantity"],
        0.25
    );
    assert_eq!(whole["least_observed_independent_order_total"], 2.5);
    // Replacing nominal purchase width by the narrower layout width would under-order this job.
    assert_eq!(
        whole["material_orders"][0]["sample"]["scope"]["requirement"],
        0.024
    );
    assert_eq!(whole["cut_plan"], serde_json::to_value(&bounded).unwrap());
}

#[test]
fn reported_lower_width_changes_the_actual_material_frontier_and_retains_independent_mass() {
    use ftx_export::material_study::Request;
    let p = pattern(vec![
        piece("front", 40.0, 100.0),
        piece("back", 40.0, 100.0),
    ]);
    let mut variants = Vec::new();
    let mut quotes = Vec::new();
    for (key, lower, price) in [
        ("variable-width", Some(0.08), 1.0),
        ("stable-width", Some(0.095), 1.5),
        ("missing-range", None, 5.0),
    ] {
        let mut selection = fabric(key, 0.1)["material"].clone();
        if let Some(lower) = lower {
            selection["record"]["properties"]["width"]["bounds"] = json!([lower, 0.11]);
        }
        variants.push(
            json!({"key":key,"changes":{"schema":"ftx-material-bindings/1","default":selection}}),
        );
        quotes.push(json!({"offer_id":key,"material_id":key,"supplier":"Synthetic supplier","sku":key,
            "price":price,"quantity":1.0,"unit":"linear_meter","minimum_quantity":0.01,"order_increment":0.01,
            "currency":"USD","region":"US","quality":"test","terms":"ex-tax-shipping",
            "observed_on":"2026-10-18","available":true,"source":"test:reported-width-study","source_sha256":"0".repeat(64)}));
    }
    let mut opts = options();
    opts.maximum_marker_length_m = Some(0.12);
    let mut request = json!({"schema":"ftx-material-study/1","variants":variants,"cutting":opts,
        "assembly_mass":{"schema":"ftx-assembly-mass/1","inventory_complete":true,"extras":[]},
        "purchasing":{"quotes":{"schema":"ftx-textile-quotes/1","quotes":quotes},
            "market":{"currency":"USD","region":"US","quality":"test","terms":"ex-tax-shipping","as_of":"2026-10-18","max_age_days":0}},
        "comparison":{"basis":"nominal","required_consumers":["purchasing"],
            "objectives":[{"metric":"observed_order_cost","direction":"minimize"}],"constraints":[]}});
    let nominal: Request = serde_json::from_value(request.clone()).unwrap();
    let nominal = serde_json::to_value(nominal.evaluate(&p).unwrap()).unwrap();
    assert_eq!(nominal["comparison"]["frontier"], json!(["variable-width"]));
    request["cutting"]["width_basis"] = json!("reported_lower_bound");
    let request: Request = serde_json::from_value(request).unwrap();
    let bounded = serde_json::to_value(request.evaluate(&p).unwrap()).unwrap();
    assert_eq!(bounded["comparison"]["frontier"], json!(["stable-width"]));
    for row in bounded["alternatives"].as_array().unwrap() {
        let key = row["key"].as_str().unwrap();
        assert_eq!(row["binding"]["status"], "available");
        assert_eq!(row["assembly_mass"]["status"], "available");
        assert!((row["metrics"]["assembly_mass_kg"].as_f64().unwrap() - 0.0016).abs() < 1e-14);
        let decision = bounded["comparison"]["alternatives"]
            .as_array()
            .unwrap()
            .iter()
            .find(|candidate| candidate["key"] == key)
            .unwrap();
        if key == "missing-range" {
            assert_eq!(row["cutting"]["status"], "refused");
            assert_eq!(row["purchasing"]["status"], "refused");
            assert!(row["metrics"]["least_observed_independent_order_total"].is_null());
            assert_eq!(decision["eligible"], false);
            continue;
        }
        let actual_pattern = Pattern::new(row["binding"]["result"]["pattern"].clone());
        let direct = CutPlan::build(&actual_pattern, request.cutting.as_ref().unwrap()).unwrap();
        assert_eq!(
            row["cutting"]["result"],
            serde_json::to_value(&direct).unwrap()
        );
        let purchasing = request.purchasing.as_ref().unwrap();
        let priced = ftx_export::cutting::procurement::evaluate(
            &direct,
            &purchasing.quotes,
            &purchasing.market,
        )
        .unwrap();
        assert_eq!(row["purchasing"]["result"], priced);
        assert_eq!(
            decision["readings"]["observed_order_cost"]["value"],
            priced["least_observed_independent_order_total"]
        );
        let expected = if key == "variable-width" { 0.24 } else { 0.18 };
        assert!(
            (priced["least_observed_independent_order_total"]
                .as_f64()
                .unwrap()
                - expected)
                .abs()
                < 1e-12
        );
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
fn bounded_markers_keep_whole_copies_and_include_each_markers_end_margins() {
    let p = pattern(vec![piece("front", 40.0, 100.0)]);
    let mut o = options();
    o.copies = 3;
    let unbounded = CutPlan::build(&p, &o).unwrap();
    assert_eq!(unbounded.schema, "ftx-roll-cut-plan/1");
    assert!(
        serde_json::to_value(&o)
            .unwrap()
            .get("maximum_marker_length_m")
            .is_none()
    );
    assert_eq!(unbounded.rolls.len(), 1);
    assert_eq!(unbounded.rolls[0].length_m, 0.225);

    o.maximum_marker_length_m = Some(0.12);
    let bounded = CutPlan::build(&p, &o).unwrap();
    assert_eq!(bounded.schema, "ftx-roll-cut-plan/2");
    assert_eq!(bounded.rolls.len(), 2);
    assert_eq!(bounded.rolls[0].placements.len(), 2);
    assert_eq!(bounded.rolls[1].placements.len(), 1);
    assert!(bounded.rolls.iter().all(|r| r.length_m == 0.12));
    let copies: std::collections::BTreeSet<_> = bounded
        .rolls
        .iter()
        .flat_map(|r| r.placements.iter().map(|p| (&p.instance, p.copy)))
        .collect();
    assert_eq!(copies.len(), 3);
    assert!(
        copies
            .iter()
            .all(|(instance, _)| instance.as_str() == "front")
    );
    let area: f64 = bounded.rolls.iter().map(|r| r.stock_area_m2).sum();
    assert!((area - 0.024).abs() < 1e-14); // The second marker needs its own end margins.
    let cut_area: f64 = bounded.rolls.iter().map(|r| r.external_cut_area_m2).sum();
    assert_eq!(cut_area, unbounded.rolls[0].external_cut_area_m2);
    assert_eq!(
        serde_json::to_value(&bounded).unwrap(),
        serde_json::to_value(CutPlan::build(&p, &o).unwrap()).unwrap()
    );
    check_marker(&bounded);
}

#[test]
fn bounded_markers_preserve_unfolded_mirrored_notched_and_internal_cut_geometry() {
    let mut fold = piece("fold", 30., 80.);
    fold["cut"]["on_fold"] = json!("left");
    let mut pair = piece("pair", 20., 50.);
    pair["cut"]["qty"] = json!(2);
    pair["notches"] = json!([{"id":"n","edge":"right","s":0.5}]);
    pair["internal"] = json!([{"kind":"cut","pts":[[5,8],[10,8]]}]);
    pair["drills"] = json!([{"p":[8,12],"d":2}]);
    let p = pattern(vec![fold, pair]);
    let before = p.raw().clone();
    let mut o = options();
    o.copies = 2;
    o.seam_allowance_mm = Some(2.);
    o.maximum_marker_length_m = Some(0.11);
    let plan = CutPlan::build(&p, &o).unwrap();
    assert!(plan.rolls.len() > 1);
    let mut seen = std::collections::BTreeSet::new();
    let ctx = Ctx::new(
        &p,
        ExportOptions {
            sa: Some(2.),
            ..ExportOptions::default()
        },
    )
    .unwrap();
    for roll in &plan.rolls {
        assert!(roll.length_m <= 0.11);
        for placed in &roll.placements {
            assert!(seen.insert((placed.instance.clone(), placed.copy)));
            let g = ctx
                .geom(Source::Instance(p.instance(&placed.instance).unwrap()), 6.)
                .unwrap();
            let t = placed.transform;
            let map = |v: Vec2| {
                Vec2::new(
                    t[0][0] * v.x + t[0][1] * v.y + t[0][2],
                    t[1][0] * v.x + t[1][1] * v.y + t[1][2],
                )
            };
            let ring = cutter_ring(&g).unwrap();
            assert_eq!(ring.len(), placed.cut_mm.len());
            for (source, actual) in ring.iter().zip(&placed.cut_mm) {
                assert!((map(*source) - *actual).norm() < 1e-10);
            }
            assert_eq!(g.internal_cut.len(), placed.internal_cut_mm.len());
            for ((source, closed), (actual, actual_closed)) in
                g.internal_cut.iter().zip(&placed.internal_cut_mm)
            {
                assert_eq!(closed, actual_closed);
                for (source, actual) in source.iter().zip(actual) {
                    assert!((map(*source) - *actual).norm() < 1e-10);
                }
            }
            assert_eq!(g.drills.len(), placed.drills_mm.len());
            for ((source, diameter), (actual, actual_diameter)) in
                g.drills.iter().zip(&placed.drills_mm)
            {
                assert_eq!(diameter, actual_diameter);
                assert!((map(*source) - *actual).norm() < 1e-10);
            }
        }
    }
    assert_eq!(seen.len(), p.instances.len() * o.copies);
    assert_eq!(p.raw(), &before);
    check_marker(&plan);
}

#[test]
fn marker_length_policy_refuses_oversized_whole_pieces_and_invalid_limits() {
    let p = pattern(vec![piece("front", 40.0, 100.0)]);
    for limit in [0.0, -1.0, f64::NAN, f64::INFINITY, 0.02, 0.119] {
        let mut o = options();
        o.maximum_marker_length_m = Some(limit);
        assert!(CutPlan::build(&p, &o).is_err(), "limit {limit:?}");
    }
    let mut o = options();
    o.maximum_marker_length_m = Some(0.12);
    assert!(CutPlan::build(&p, &o).is_ok());
    // A narrow but too-long piece is not made feasible by splitting its area into two cuts.
    assert!(CutPlan::build(&pattern(vec![piece("long", 1.0, 101.0)]), &o).is_err());
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
fn bounded_marker_compaction_retains_every_assigned_piece_and_budget_fallback() {
    let p = triangular_pair();
    let mut o = options();
    o.copies = 2;
    o.maximum_marker_length_m = Some(0.23);
    let baseline = CutPlan::build(&p, &o).unwrap();
    assert_eq!(baseline.rolls.len(), 2);
    o.nesting = Some(Default::default());
    o.nesting.as_mut().unwrap().grain_reversal_materials =
        vec![baseline.rolls[0].material.sha256().to_owned()];
    let compacted = CutPlan::build(&p, &o).unwrap();
    assert_eq!(compacted.rolls.len(), baseline.rolls.len());
    for (a, b) in baseline.rolls.iter().zip(&compacted.rolls) {
        assert_eq!(
            a.placements
                .iter()
                .map(|p| (&p.instance, p.copy))
                .collect::<Vec<_>>(),
            b.placements
                .iter()
                .map(|p| (&p.instance, p.copy))
                .collect::<Vec<_>>()
        );
        assert!((b.length_m - 0.12).abs() < 1e-12);
        assert!(b.length_m < a.length_m && b.length_m <= 0.23);
        assert!(!b.nesting.as_ref().unwrap().optimality_claim);
        assert_eq!(a.external_cut_area_m2, b.external_cut_area_m2);
    }
    o.nesting.as_mut().unwrap().work_limit = 1;
    let limited = CutPlan::build(&p, &o).unwrap();
    for (a, b) in baseline.rolls.iter().zip(&limited.rolls) {
        assert_eq!(a.length_m, b.length_m);
        assert_eq!(
            serde_json::to_value(&a.placements).unwrap(),
            serde_json::to_value(&b.placements).unwrap()
        );
        assert!(b.nesting.as_ref().unwrap().budget_exhausted);
    }
}

#[test]
fn multiple_markers_share_one_rounded_order_and_cannot_reuse_finite_stock() {
    use ftx_export::cutting::procurement::{Market, evaluate};
    use ftx_pattern::material_cost::QuoteBook;
    let p = pattern(vec![piece("front", 40.0, 100.0)]);
    let mut o = options();
    o.copies = 3;
    o.maximum_marker_length_m = Some(0.12);
    let plan = CutPlan::build(&p, &o).unwrap();
    let quote = json!({"offer_id":"short-stock", "material_id":"shell", "supplier":"Synthetic supplier", "sku":"shell",
        "price":1.0,"quantity":1.0,"unit":"linear_meter","minimum_quantity":0.2,"order_increment":0.05,
        "currency":"USD","region":"US","quality":"test","terms":"ex-tax-shipping",
        "observed_on":"2026-10-18","available":true,"available_quantity":0.2,
        "maximum_continuous_quantity":0.12,"source":"test:bounded-marker-order","source_sha256":"0".repeat(64)});
    let mut enough = quote.clone();
    enough["offer_id"] = json!("whole-order-stock");
    enough["sku"] = json!("whole-order-stock");
    enough["price"] = json!(10.0);
    enough["available_quantity"] = json!(0.25);
    let market: Market =
        serde_json::from_value(json!({"currency":"USD","region":"US","quality":"test",
        "terms":"ex-tax-shipping","as_of":"2026-10-18","max_age_days":3,
        "require_observed_quantity":true,"require_observed_continuous_length":true}))
        .unwrap();
    let book: QuoteBook = serde_json::from_value(
        json!({"schema":"ftx-textile-quotes/1","quotes":[quote.clone(),enough.clone()]}),
    )
    .unwrap();
    let priced = evaluate(&plan, &book, &market).unwrap();
    assert_eq!(priced["schema"], "ftx-cut-procurement/2");
    assert_eq!(priced["material_orders"].as_array().unwrap().len(), 1);
    let order = &priced["material_orders"][0];
    assert_eq!(order["roll_indices"], json!([0, 1]));
    assert_eq!(order["marker_lengths_m"], json!([0.12, 0.12]));
    assert_eq!(order["required_length_m"], 0.24);
    assert_eq!(order["required_continuous_length_m"], 0.12);
    assert_eq!(order["sample"]["scope"]["requirement"], 0.024);
    assert_eq!(
        order["least_observed_order"]["quote"]["offer_id"],
        "whole-order-stock"
    );
    assert_eq!(order["least_observed_order"]["order_quantity"], 0.25);
    assert_eq!(priced["least_observed_independent_order_total"], 2.5);
    assert_eq!(
        order["sample"]["excluded_offers"][0]["reason"],
        "insufficient_quantity"
    );
    assert_eq!(
        order["sample"]["excluded_offers"][0]["required_order_quantity"],
        0.25
    );
    assert_eq!(priced["complete_observed_quantity_coverage"], true);
    assert_eq!(priced["complete_continuous_length_limit_coverage"], true);
    for (index, row) in priced["roll_prices"].as_array().unwrap().iter().enumerate() {
        assert_eq!(row["roll_index"], index);
        assert_eq!(row["material_order_index"], 0);
        assert!(row.get("least_observed_order").is_none());
        assert!(row.get("sample").is_none());
    }
    let depleted: QuoteBook =
        serde_json::from_value(json!({"schema":"ftx-textile-quotes/1","quotes":[quote]})).unwrap();
    let failed = evaluate(&plan, &depleted, &market).unwrap();
    assert_eq!(failed["complete_offer_coverage"], false);
    assert!(failed["least_observed_independent_order_total"].is_null());

    // Adequate aggregate inventory never relaxes the actual longest-marker constraint.
    enough["maximum_continuous_quantity"] = json!(0.119);
    enough["available_quantity"] = json!(100.0);
    let short: QuoteBook =
        serde_json::from_value(json!({"schema":"ftx-textile-quotes/1","quotes":[enough]})).unwrap();
    let refused = evaluate(&plan, &short, &market).unwrap();
    assert_eq!(
        refused["material_orders"][0]["sample"]["excluded_offers"][0]["reason"],
        "insufficient_continuous_length"
    );
    assert_eq!(refused["complete_offer_coverage"], false);
}

#[test]
fn complete_marker_order_earns_one_source_price_break_after_combined_rounding() {
    use ftx_export::cutting::procurement::{Market, evaluate};
    use ftx_pattern::material_cost::QuoteBook;
    let p = pattern(vec![piece("front", 40.0, 100.0)]);
    let mut o = options();
    o.copies = 3;
    o.maximum_marker_length_m = Some(0.12);
    let plan = CutPlan::build(&p, &o).unwrap();
    let mut source = json!({"offer_id":"tiered-stock", "material_id":"shell", "supplier":"Synthetic supplier", "sku":"shell",
        "price":10.0,"quantity":1.0,"unit":"linear_meter","minimum_quantity":0.2,"order_increment":0.05,
        "price_breaks":[{"minimum_quantity":0.25,"price":8.0}],
        "currency":"USD","region":"US","quality":"test","terms":"ex-tax-shipping",
        "observed_on":"2026-10-18","available":true,"available_quantity":0.25,
        "maximum_continuous_quantity":0.12,"source":"test:tiered-marker-order","source_sha256":"0".repeat(64)});
    let market: Market =
        serde_json::from_value(json!({"currency":"USD","region":"US","quality":"test",
        "terms":"ex-tax-shipping","as_of":"2026-10-18","max_age_days":3,
        "require_observed_quantity":true,"require_observed_continuous_length":true}))
        .unwrap();
    let prices = |source: &Value| {
        let book: QuoteBook =
            serde_json::from_value(json!({"schema":"ftx-textile-quotes/1","quotes":[source]}))
                .unwrap();
        evaluate(&plan, &book, &market).unwrap()
    };
    let result = prices(&source);
    let order = &result["material_orders"][0]["least_observed_order"];
    assert_eq!(order["order_quantity"], 0.25); // Two 0.12-m markers share the rounded order.
    assert_eq!(order["applied_price_break"]["minimum_quantity"], 0.25);
    assert_eq!(order["order_cost"], 2.0);
    assert_eq!(result["least_observed_independent_order_total"], 2.0);
    source["available_quantity"] = json!(0.24);
    let missing = prices(&source);
    assert_eq!(missing["complete_offer_coverage"], false);
    assert!(missing["least_observed_independent_order_total"].is_null());
    source["available_quantity"] = json!(0.25);
    source["maximum_continuous_quantity"] = json!(0.119);
    assert_eq!(prices(&source)["complete_offer_coverage"], false);
}

fn split_order_fixture() -> (
    CutPlan,
    Vec<Value>,
    ftx_export::cutting::procurement::Market,
) {
    let p = pattern(vec![piece("front", 40.0, 100.0)]);
    let mut o = options();
    o.copies = 3;
    o.maximum_marker_length_m = Some(0.12);
    let plan = CutPlan::build(&p, &o).unwrap();
    assert_eq!(plan.rolls.len(), 2);
    let quotes = ["a", "b"].map(|id| json!({
        "offer_id":id,"material_id":"shell","material_sha256":plan.rolls[0].material.sha256(),
        "supplier":format!("synthetic-{id}"),"sku":"shell","price":if id == "a" {1.0} else {2.0},
        "quantity":1.0,"unit":"linear_meter","minimum_quantity":0.1,"order_increment":0.05,
        "currency":"USD","region":"US","quality":"test","terms":"ex-tax-shipping",
        "observed_on":"2026-10-18","available":true,"available_quantity":0.15,
        "maximum_continuous_quantity":0.12,"source":"test:split-orders","source_sha256":"0".repeat(64)
    })).to_vec();
    let market = serde_json::from_value(json!({"currency":"USD","region":"US","quality":"test",
        "terms":"ex-tax-shipping","as_of":"2026-10-18","max_age_days":3,
        "require_observed_quantity":true,"require_observed_continuous_length":true,
        "split_orders":{"schema":"ftx-cut-order-allocation/1","max_search_nodes":10000}}))
    .unwrap();
    (plan, quotes, market)
}

fn split_quote_book(quotes: Vec<Value>) -> ftx_pattern::material_cost::QuoteBook {
    serde_json::from_value(json!({"schema":"ftx-textile-quotes/1","quotes":quotes})).unwrap()
}

#[test]
fn whole_markers_can_use_multiple_finite_suppliers_with_canonical_order_rounding() {
    use ftx_export::cutting::procurement::evaluate;
    use ftx_pattern::material_cost::Scope;
    let (plan, quotes, mut market) = split_order_fixture();
    let book = split_quote_book(quotes);
    let priced = evaluate(&plan, &book, &market).unwrap();
    assert_eq!(priced["schema"], "ftx-cut-procurement/3");
    assert_eq!(priced["complete_offer_coverage"], true);
    assert_eq!(priced["complete_observed_quantity_coverage"], true);
    assert_eq!(priced["complete_continuous_length_limit_coverage"], true);
    assert_eq!(priced["order_allocation_optimality_proven"], true);
    assert!((priced["selected_observed_order_total"].as_f64().unwrap() - 0.45).abs() < 1e-12);
    assert_eq!(
        priced["selected_observed_order_total"],
        priced["least_observed_independent_order_total"]
    );
    let allocation = &priced["material_orders"][0]["allocation"];
    assert_eq!(allocation["whole_order_sample"]["offers"], json!([]));
    let orders = allocation["selected_orders"].as_array().unwrap();
    assert_eq!(orders.len(), 2);
    let mut markers = Vec::new();
    for order in orders {
        let scope: Scope = serde_json::from_value(order["scope"].clone()).unwrap();
        let canonical = book.evaluate(&plan.rolls[0].material, &scope).unwrap();
        let selected = canonical
            .offers
            .iter()
            .find(|offer| order["offer"]["quote"]["offer_id"] == offer.quote.offer_id)
            .unwrap();
        assert_eq!(order["offer"], serde_json::to_value(selected).unwrap());
        assert!((selected.order_quantity - 0.15).abs() < 1e-12);
        assert_eq!(order["required_continuous_length_m"], 0.12);
        markers.extend(
            order["roll_indices"]
                .as_array()
                .unwrap()
                .iter()
                .map(|i| i.as_u64().unwrap()),
        );
    }
    markers.sort();
    assert_eq!(markers, [0, 1]);
    // Opt-in does not change the old whole-order consumer or silently split geometry for it.
    market.split_orders = None;
    let original = evaluate(&plan, &book, &market).unwrap();
    assert_eq!(original["schema"], "ftx-cut-procurement/2");
    assert_eq!(original["complete_offer_coverage"], false);
    assert!(original["least_observed_independent_order_total"].is_null());
    assert_eq!(priced["cut_plan"], original["cut_plan"]);
}

#[test]
fn bounded_supplier_search_keeps_only_complete_incumbents_and_proves_cost_when_exhaustive() {
    use ftx_export::cutting::procurement::evaluate;
    let (plan, mut quotes, mut market) = split_order_fixture();
    let mut full = quotes[0].clone();
    full["offer_id"] = json!("whole");
    full["supplier"] = json!("synthetic-whole");
    full["price"] = json!(10.0);
    full["available_quantity"] = json!(1.0);
    quotes.push(full);
    let book = split_quote_book(quotes);
    market.split_orders.as_mut().unwrap().max_search_nodes = 1;
    let limited = evaluate(&plan, &book, &market).unwrap();
    assert_eq!(limited["complete_offer_coverage"], true);
    assert_eq!(limited["selected_observed_order_total"], 2.5);
    assert!(limited["least_observed_independent_order_total"].is_null());
    assert_eq!(limited["order_allocation_optimality_proven"], false);
    assert_eq!(limited["budget_exhausted"], true);
    assert_eq!(limited["quote_evaluations"], 1);
    assert_eq!(limited["search_nodes"], 0);
    let orders = limited["material_orders"][0]["allocation"]["selected_orders"]
        .as_array()
        .unwrap();
    assert_eq!(orders.len(), 1);
    assert_eq!(orders[0]["roll_indices"], json!([0, 1]));
    market.split_orders.as_mut().unwrap().max_search_nodes = 10000;
    let optimum = evaluate(&plan, &book, &market).unwrap();
    assert!((optimum["selected_observed_order_total"].as_f64().unwrap() - 0.45).abs() < 1e-12);
    assert_eq!(optimum["order_allocation_optimality_proven"], true);
    assert_eq!(optimum["budget_exhausted"], false);
    // A budget without an incumbent cannot report a subtotal as a feasible cheap order.
    let (plan, quotes, mut market) = split_order_fixture();
    market.split_orders.as_mut().unwrap().max_search_nodes = 1;
    let unknown = evaluate(&plan, &split_quote_book(quotes), &market).unwrap();
    assert_eq!(unknown["complete_offer_coverage"], false);
    assert!(unknown["selected_observed_order_total"].is_null());
    assert_eq!(
        unknown["material_orders"][0]["allocation"]["no_feasible_listed_allocation"],
        false
    );
}

#[test]
fn assigned_markers_share_one_supplier_minimum_increment_and_capacity() {
    use ftx_export::cutting::procurement::evaluate;
    let (_, mut quotes, market) = split_order_fixture();
    let mut o = options();
    o.copies = 5;
    o.maximum_marker_length_m = Some(0.12);
    let plan = CutPlan::build(&pattern(vec![piece("front", 40.0, 100.0)]), &o).unwrap();
    assert_eq!(plan.rolls.len(), 3);
    quotes[0]["minimum_quantity"] = json!(0.2);
    quotes[0]["available_quantity"] = json!(0.25);
    quotes[1]["price"] = json!(4.0);
    quotes[1]["available_quantity"] = json!(0.2);
    let mut whole = quotes[1].clone();
    whole["offer_id"] = json!("whole");
    whole["supplier"] = json!("synthetic-whole");
    whole["price"] = json!(3.0);
    whole["available_quantity"] = json!(0.4);
    quotes.push(whole);
    let priced = evaluate(&plan, &split_quote_book(quotes), &market).unwrap();
    assert_eq!(priced["order_allocation_optimality_proven"], true);
    // The whole-capacity offer may supply a subset too: 0.25 m at 1/m plus 0.15 m at 3/m.
    // Treating it as a whole-job-only fallback would miss this cheaper feasible allocation.
    assert!(
        (priced["selected_observed_order_total"].as_f64().unwrap() - 0.70).abs() < 1e-12,
        "{priced:#}"
    );
    let orders = priced["material_orders"][0]["allocation"]["selected_orders"]
        .as_array()
        .unwrap();
    assert_eq!(orders.len(), 2);
    assert_eq!(orders[0]["offer"]["quote"]["offer_id"], "a");
    assert_eq!(orders[0]["roll_indices"].as_array().unwrap().len(), 2);
    assert_eq!(orders[0]["offer"]["order_quantity"], 0.25);
    assert_eq!(orders[1]["roll_indices"].as_array().unwrap().len(), 1);
    assert_eq!(orders[1]["offer"]["quote"]["offer_id"], "whole");
    assert!((orders[1]["offer"]["order_quantity"].as_f64().unwrap() - 0.15).abs() < 1e-12);
}

#[test]
fn supplier_allocation_keeps_expensive_prefixes_that_reach_cheaper_all_unit_tiers() {
    use ftx_export::cutting::procurement::evaluate;
    let (_, mut quotes, market) = split_order_fixture();
    let mut o = options();
    o.copies = 5;
    o.maximum_marker_length_m = Some(0.12);
    let plan = CutPlan::build(&pattern(vec![piece("front", 40.0, 100.0)]), &o).unwrap();
    assert_eq!(plan.rolls.len(), 3);
    quotes[0]["price"] = json!(100.0);
    quotes[0]["available_quantity"] = json!(0.25);
    quotes[0]["price_breaks"] = json!([{"minimum_quantity":0.25,"price":1.0,"sku":"bulk-a"}]);
    quotes[1]["price"] = json!(4.0);
    quotes[1]["available_quantity"] = json!(1.0);
    let book = split_quote_book(quotes);
    let mut bounded = market.clone();
    bounded.split_orders.as_mut().unwrap().max_search_nodes = 1;
    let baseline = evaluate(&plan, &book, &bounded).unwrap();
    assert!((baseline["selected_observed_order_total"].as_f64().unwrap() - 1.6).abs() < 1e-12);
    let priced = evaluate(&plan, &book, &market).unwrap();
    // A single marker at a's base rate costs 15, above the 1.6 incumbent. Two markers reach
    // its all-unit tier and cost only 0.25; b supplies the final 0.15-m rounded order for 0.6.
    assert!(
        (priced["selected_observed_order_total"].as_f64().unwrap() - 0.85).abs() < 1e-12,
        "{priced:#}"
    );
    assert_eq!(priced["order_allocation_optimality_proven"], true);
    let orders = priced["material_orders"][0]["allocation"]["selected_orders"]
        .as_array()
        .unwrap();
    assert_eq!(orders.len(), 2);
    assert_eq!(orders[0]["offer"]["quote"]["offer_id"], "a");
    assert_eq!(orders[0]["offer"]["order_quantity"], 0.25);
    assert_eq!(orders[0]["offer"]["applied_price_break"]["sku"], "bulk-a");
}

#[test]
fn supplier_tier_sku_aliases_cannot_create_additional_independent_stock() {
    use ftx_export::cutting::procurement::evaluate;
    for shared_tier_only in [false, true] {
        let (plan, mut quotes, market) = split_order_fixture();
        quotes[0]["price_breaks"] =
            json!([{"minimum_quantity":0.25,"price":1.0,"sku":"shared-stock"}]);
        quotes[1]["supplier"] = quotes[0]["supplier"].clone();
        quotes[1]["sku"] = json!(if shared_tier_only {
            "other-base"
        } else {
            "shared-stock"
        });
        if shared_tier_only {
            quotes[1]["price_breaks"] =
                json!([{"minimum_quantity":0.25,"price":1.0,"sku":"shared-stock"}]);
        }
        let priced = evaluate(&plan, &split_quote_book(quotes), &market).unwrap();
        assert_eq!(priced["complete_offer_coverage"], false);
        assert!(priced["selected_observed_order_total"].is_null());
        assert_eq!(
            priced["material_orders"][0]["allocation"]["no_feasible_listed_allocation"],
            true
        );
    }
}

#[test]
fn unselected_supplier_quotes_keep_transitive_stock_aliases_connected() {
    use ftx_export::cutting::procurement::evaluate;
    let (plan, mut quotes, market) = split_order_fixture();
    for (quote, (base, alias)) in quotes.iter_mut().zip([("x", "y"), ("z", "w")]) {
        quote["supplier"] = json!("shared source");
        quote["sku"] = json!(base);
        quote["price_breaks"] = json!([{"minimum_quantity":0.25,"price":1.0,"sku":alias}]);
    }
    let mut bridge = quotes[0].clone();
    bridge["offer_id"] = json!("bridge");
    bridge["sku"] = json!("y");
    bridge["price_breaks"] = json!([{"minimum_quantity":0.25,"price":1.0,"sku":"z"}]);
    bridge["available_quantity"] = json!(0.0);
    quotes.push(bridge);
    let priced = evaluate(&plan, &split_quote_book(quotes), &market).unwrap();
    // The unusable bridge still establishes y=z. Buying x/y and z/w cannot create two stocks.
    assert_eq!(priced["complete_offer_coverage"], false);
    assert!(priced["selected_observed_order_total"].is_null());
    assert_eq!(
        priced["material_orders"][0]["allocation"]["no_feasible_listed_allocation"],
        true
    );
}

#[test]
fn stock_alias_components_include_unselected_bridges_in_other_material_groups() {
    use ftx_export::cutting::procurement::evaluate;
    let (_, base_quotes, market) = split_order_fixture();
    let mut lining = piece("lining", 40.0, 100.0);
    lining["fabric"] = fabric("lining", 0.1);
    let mut trim = piece("trim", 40.0, 100.0);
    trim["fabric"] = fabric("trim", 0.1);
    let plan = CutPlan::build(
        &pattern(vec![piece("front", 40.0, 100.0), lining, trim]),
        &options(),
    )
    .unwrap();
    assert_eq!(plan.rolls.len(), 3);
    let mut quotes = Vec::new();
    for (id, base, alias) in [
        ("shell", "x", "y"),
        ("lining", "y", "z"),
        ("trim", "z", "w"),
    ] {
        let material = &plan
            .rolls
            .iter()
            .find(|r| r.material.material().id == id)
            .unwrap()
            .material;
        let mut quote = base_quotes[0].clone();
        quote["offer_id"] = json!(id);
        quote["material_id"] = json!(id);
        quote["material_sha256"] = json!(material.sha256());
        quote["supplier"] = json!("shared source");
        quote["sku"] = json!(base);
        quote["price_breaks"] = json!([{"minimum_quantity":0.25,"price":1.0,"sku":alias}]);
        if id == "lining" {
            let mut independent = quote.clone();
            independent["offer_id"] = json!("lining-independent");
            independent["supplier"] = json!("independent source");
            independent["sku"] = json!("lining-independent");
            independent["price_breaks"] = json!([]);
            quotes.push(independent);
            quote["available_quantity"] = json!(0.0);
        }
        quotes.push(quote);
    }
    let refusal = evaluate(&plan, &split_quote_book(quotes.clone()), &market).unwrap_err();
    assert!(
        refusal.message.contains("stock alias component"),
        "{refusal:?}"
    );
    // A genuinely different current identity disconnects the stock, so independent orders work.
    quotes
        .iter_mut()
        .find(|q| q["offer_id"] == "lining")
        .unwrap()["price_breaks"] = json!([]);
    let independent = evaluate(&plan, &split_quote_book(quotes), &market).unwrap();
    assert_eq!(independent["complete_offer_coverage"], true);
    assert_eq!(independent["order_allocation_optimality_proven"], true);
}

#[test]
fn supplier_tiers_never_multiply_stock_and_unlocked_or_short_offers_cannot_fill_markers() {
    use ftx_export::cutting::procurement::evaluate;
    let (plan, mut quotes, market) = split_order_fixture();
    quotes[1]["supplier"] = quotes[0]["supplier"].clone();
    let tiers = evaluate(&plan, &split_quote_book(quotes), &market).unwrap();
    assert_eq!(tiers["complete_offer_coverage"], false);
    assert_eq!(
        tiers["material_orders"][0]["allocation"]["no_feasible_listed_allocation"],
        true
    );
    let (_, mut quotes, _) = split_order_fixture();
    quotes[0]["material_sha256"] = Value::Null;
    let unlocked = evaluate(&plan, &split_quote_book(quotes), &market).unwrap();
    assert_eq!(unlocked["complete_offer_coverage"], false);
    assert_eq!(
        unlocked["material_orders"][0]["allocation"]["excluded_unlocked_offer_ids"],
        json!(["a"])
    );
    let (_, mut quotes, _) = split_order_fixture();
    for quote in &mut quotes {
        quote["available_quantity"] = json!(100.0);
        quote["maximum_continuous_quantity"] = json!(0.119);
    }
    let short = evaluate(&plan, &split_quote_book(quotes), &market).unwrap();
    assert_eq!(short["complete_offer_coverage"], false);
    assert!(short["selected_observed_order_total"].is_null());
}

#[test]
fn split_orders_keep_latest_source_state_and_charge_one_shared_budget() {
    use ftx_export::cutting::procurement::evaluate;
    let (plan, mut quotes, mut market) = split_order_fixture();
    let mut later = quotes[0].clone();
    later["observed_on"] = json!("2026-10-19");
    later["available_quantity"] = json!(0.0);
    quotes.push(later);
    market.as_of = "2026-10-19".into();
    let depleted = evaluate(&plan, &split_quote_book(quotes), &market).unwrap();
    assert_eq!(depleted["complete_offer_coverage"], false);
    assert!(depleted["selected_observed_order_total"].is_null());
    let mut lining = piece("lining", 40.0, 100.0);
    lining["fabric"] = fabric("lining", 0.1);
    let mixed = CutPlan::build(
        &pattern(vec![piece("front", 40.0, 100.0), lining]),
        &options(),
    )
    .unwrap();
    let (_, quotes, mut market) = split_order_fixture();
    market.split_orders.as_mut().unwrap().max_search_nodes = 1;
    let bounded = evaluate(&mixed, &split_quote_book(quotes), &market).unwrap();
    assert_eq!(bounded["quote_evaluations"], 1);
    assert_eq!(bounded["search_nodes"], 0);
    assert_eq!(bounded["budget_exhausted"], true);
    assert_eq!(bounded["complete_offer_coverage"], false);
    assert!(bounded["selected_observed_order_total"].is_null());
    market.split_orders.as_mut().unwrap().max_search_nodes = 0;
    assert!(evaluate(&mixed, &split_quote_book(vec![]), &market).is_err());
}

#[test]
fn grouped_orders_keep_materials_separate_and_missing_coverage_makes_total_unknown() {
    use ftx_export::cutting::procurement::{Market, evaluate};
    use ftx_pattern::material_cost::QuoteBook;
    let mut lining = piece("lining", 40.0, 100.0);
    lining["fabric"] = fabric("lining", 0.1);
    let p = pattern(vec![piece("front", 40.0, 100.0), lining]);
    let mut o = options();
    o.copies = 3;
    o.maximum_marker_length_m = Some(0.12);
    let plan = CutPlan::build(&p, &o).unwrap();
    assert_eq!(plan.rolls.len(), 4);
    let book: QuoteBook = serde_json::from_value(json!({"schema":"ftx-textile-quotes/1","quotes":[{
        "offer_id":"shell", "material_id":"shell", "material_sha256":plan.rolls.iter().find(|r| r.material.material().id=="shell").unwrap().material.sha256(),
        "supplier":"Synthetic supplier", "sku":"shell","price":10.0,"quantity":1.0,"unit":"linear_meter","minimum_quantity":0.2,"order_increment":0.05,
        "currency":"USD","region":"US","quality":"test","terms":"ex-tax-shipping",
        "observed_on":"2026-10-18","available":true,"available_quantity":0.25,
        "maximum_continuous_quantity":0.12,"source":"test:bounded-materials","source_sha256":"0".repeat(64)
    }]})).unwrap();
    let market: Market =
        serde_json::from_value(json!({"currency":"USD","region":"US","quality":"test",
        "terms":"ex-tax-shipping","as_of":"2026-10-18","max_age_days":3}))
        .unwrap();
    let priced = evaluate(&plan, &book, &market).unwrap();
    let orders = priced["material_orders"].as_array().unwrap();
    assert_eq!(orders.len(), 2);
    assert_eq!(
        orders
            .iter()
            .filter(|o| !o["least_observed_order"].is_null())
            .count(),
        1
    );
    assert!(
        orders
            .iter()
            .all(|o| o["roll_indices"].as_array().unwrap().len() == 2)
    );
    assert_ne!(orders[0]["material_sha256"], orders[1]["material_sha256"]);
    assert!(!priced["complete_offer_coverage"].as_bool().unwrap());
    assert!(priced["least_observed_independent_order_total"].is_null());
    // Public marker mutation cannot conceal a negative requirement in the aggregated sum.
    let mut malformed = plan;
    malformed.rolls[0].stock_area_m2 = -0.01;
    assert!(evaluate(&malformed, &book, &market).is_err());
}

#[test]
fn procurement_qualifies_the_actual_marker_length_without_splitting_or_hiding_unknowns() {
    use ftx_export::cutting::procurement::{Market, evaluate};
    use ftx_pattern::material_cost::QuoteBook;
    let p = pattern(vec![piece("front", 40.0, 100.0)]);
    let plan = CutPlan::build(&p, &options()).unwrap();
    assert_eq!(plan.rolls[0].length_m, 0.12); // Includes both 10 mm end margins.
    let source = json!({"offer_id":"short", "material_id":"shell", "supplier":"Synthetic supplier", "sku":"short",
        "price":1.0,"quantity":1.0,"unit":"linear_meter","minimum_quantity":0.05,"order_increment":0.05,
        "currency":"USD","region":"US","quality":"test","terms":"ex-tax-shipping",
        "observed_on":"2026-10-18","available":true,"available_quantity":1000.0,
        "maximum_continuous_quantity":0.1,"source":"test:continuous-cut","source_sha256":"0".repeat(64)});
    let mut quotes = vec![source.clone()];
    let market: Market =
        serde_json::from_value(json!({"currency":"USD","region":"US","quality":"test",
        "terms":"ex-tax-shipping","as_of":"2026-10-18","max_age_days":3}))
        .unwrap();
    let evaluate_quotes = |quotes: &[Value], market: &Market| {
        let book: QuoteBook =
            serde_json::from_value(json!({"schema":"ftx-textile-quotes/1","quotes":quotes}))
                .unwrap();
        evaluate(&plan, &book, market).unwrap()
    };
    let blocked = evaluate_quotes(&quotes, &market);
    assert_eq!(blocked["complete_offer_coverage"], false);
    assert!(blocked["least_observed_independent_order_total"].is_null());
    assert_eq!(
        blocked["roll_prices"][0]["sample"]["excluded_offers"][0]["reason"],
        "insufficient_continuous_length"
    );
    assert_eq!(
        blocked["roll_prices"][0]["sample"]["excluded_offers"][0]["required_continuous_length_m"],
        0.12
    );
    assert_eq!(blocked["cut_plan"], serde_json::to_value(&plan).unwrap());

    let mut unknown = source.clone();
    unknown["offer_id"] = json!("unknown");
    unknown["sku"] = json!("unknown");
    unknown
        .as_object_mut()
        .unwrap()
        .remove("maximum_continuous_quantity");
    quotes.push(unknown);
    let unqualified = evaluate_quotes(&quotes, &market);
    assert_eq!(unqualified["complete_offer_coverage"], true);
    assert_eq!(unqualified["complete_observed_quantity_coverage"], true);
    assert_eq!(
        unqualified["complete_continuous_length_limit_coverage"],
        false
    );
    assert_eq!(
        unqualified["roll_prices"][0]["continuity"],
        "source_limit_unknown"
    );

    let mut strict = market;
    strict.require_observed_continuous_length = true;
    assert_eq!(
        evaluate_quotes(&quotes, &strict)["complete_offer_coverage"],
        false
    );
    let mut long = source;
    long["offer_id"] = json!("long");
    long["sku"] = json!("long");
    long["price"] = json!(10.0);
    long["maximum_continuous_quantity"] = json!(0.12);
    quotes.push(long);
    let admitted = evaluate_quotes(&quotes, &strict);
    assert_eq!(admitted["complete_continuous_length_limit_coverage"], true);
    assert_eq!(
        admitted["roll_prices"][0]["continuity"],
        "within_observed_limit"
    );
    assert_eq!(
        admitted["roll_prices"][0]["least_observed_order"]["quote"]["offer_id"],
        "long"
    );
    assert_eq!(
        admitted["roll_prices"][0]["sample"]["excluded_offers"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
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

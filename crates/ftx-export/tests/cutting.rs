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

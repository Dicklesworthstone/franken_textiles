//! Synthetic irregular-stock manufacturing answers, not leather measurements or hardware qualification.
use ftx_core::{RefusalCode, Vec2};
use ftx_export::{
    Ctx, ExportOptions, Source,
    cutter::cutter_ring,
    cutting::{
        CutOptions, CutPlan,
        stock::{Plan, Request},
    },
};
use ftx_pattern::{Pattern, material_binding::Changes};
use serde_json::{Value, json};

fn material(id: &str, form: &str) -> Value {
    json!({"id":id,"allow_estimates":true,"record":{"id":id,"name":"Synthetic hide / remnant", "form":form,
        "properties":{"areal_density":{"value":0.5,"unit":"kg/m2","source":"test","evidence":"estimate","method":"Synthetic areal mass"}},
        "sources":{"test":{"reference":"test:finite-stock","accessed_on":"2026-10-15","note":"Synthetic, not measured"}}}})
}
fn piece(id: &str, w: f64, h: f64) -> Value {
    json!({"id":id,"cut":{"qty":1},"grain":[[5,5],[5,15]],"vertices":{"a":[0,0],"b":[w,0],"c":[w,h],"d":[0,h]},
        "edges":[{"id":"bottom","from":"a","to":"b"},{"id":"right","from":"b","to":"c"},
            {"id":"top","from":"c","to":"d"},{"id":"left","from":"d","to":"a"}]})
}
fn pattern(parts: Vec<Value>) -> Pattern {
    Pattern::new(
        json!({"schema":"fabric-pattern/1","units":"mm","fabric":{"material":material("hide","hide")},
        "meta":{"sa_mm":0},"pieces":parts,"seams":[]}),
    )
}
fn stock(id: &str, m: Value, outline: Value) -> Value {
    json!({"id":id,"material":m,"outline_mm":outline,"grain_mm":[[0,0],[0,10]],
        "geometry_source":{"reference":"test:stock-outline","accessed_on":"2026-10-15","note":"Synthetic geometry and preferred direction"}})
}
fn rect(w: f64, h: f64) -> Value {
    json!([[0, 0], [w, 0], [w, h], [0, h]])
}
fn request(stocks: Vec<Value>) -> Request {
    serde_json::from_value(
        json!({"schema":"ftx-finite-stock-request/1","stocks":stocks,
        "options":{"copies":1,"gap_mm":2,"boundary_margin_mm":2,"step_mm":2,"work_limit":2000000}}),
    )
    .unwrap()
}
fn v(p: &Plan) -> Value {
    serde_json::to_value(p).unwrap()
}
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-10, "{a} != {b}");
}

#[test]
fn hides_without_roll_width_use_real_boundary_defects_and_source_evidence() {
    let p = pattern(vec![piece("front", 20., 20.), piece("pocket", 15., 20.)]);
    let before = p.raw().clone();
    let outline = json!([[0, 0], [100, 0], [100, 45], [45, 45], [45, 100], [0, 100]]);
    let mut s = stock("hide-001", material("hide", "hide"), outline);
    s["defects_mm"] = json!([[[5, 5], [20, 5], [20, 20], [5, 20]]]);
    let r = request(vec![s]);
    let plan = r.build(&p).unwrap();
    let m = &plan.markers()[0];
    near(m.gross_area_m2, 0.006975);
    near(m.defect_free_area_m2, 0.006750);
    near(m.external_cut_area_m2, 0.000700);
    near(m.stock_mass_kg.unwrap(), 0.0034875);
    assert!(m.material.mechanics().is_err());
    assert_eq!(p.raw(), &before);
    assert_eq!(m.placements.len(), 2);
    for cut in &m.placements {
        let b = cut.bounds_mm;
        assert!(b[0] >= 2. && b[1] >= 2. && b[2] <= 98. && b[3] <= 98.);
        assert!(b[2] <= 43. || b[3] <= 43.); // L-shape, not the whole bounding rectangle
        assert!(b[0] >= 22. || b[1] >= 22.); // neither cut consumes the excluded square
        let d = cut.grain_mm[1] - cut.grain_mm[0];
        near(d.x, 0.);
        assert!(d.y > 0.);
    }
    let roll = CutOptions {
        copies: 1,
        gap_mm: 2.,
        edge_margin_mm: 2.,
        end_margin_mm: 2.,
        seam_allowance_mm: None,
        nesting: None,
    };
    assert!(CutPlan::build(&p, &roll).is_err()); // supported via finite stock, not a fake roll
    assert_eq!(v(&plan), v(&r.build(&p).unwrap()));
    assert_eq!(v(&plan), v(&plan.request().build(&p).unwrap()));
    assert!(!v(&plan)["hardware_execution_authorized"].as_bool().unwrap());
}

#[test]
fn concavity_and_contained_defects_cannot_be_hidden_by_bounds_or_inside_vertices() {
    let p = pattern(vec![piece("square", 55., 55.)]);
    let r = request(vec![stock(
        "triangle",
        material("hide", "hide"),
        json!([[0, 0], [100, 0], [0, 100]]),
    )]);
    let err = r.build(&p).unwrap_err();
    assert!(err.message.contains("not a proof of infeasibility"));
    let p = pattern(vec![piece("small", 20., 20.)]);
    let mut s = stock("hole", material("hide", "hide"), rect(26., 26.));
    s["defects_mm"] = json!([[[12, 12], [14, 12], [14, 14], [12, 14]]]);
    assert!(request(vec![s]).build(&p).is_err()); // defect wholly inside would escape edge-only tests
}

#[test]
fn actual_inventory_is_consumed_once_and_copies_never_round_down_to_partial_success() {
    let p = pattern(vec![piece("panel", 40., 40.)]);
    let mut r = request(vec![
        stock("small-first", material("hide", "hide"), rect(45., 45.)),
        stock("second", material("hide", "hide"), rect(90., 50.)),
    ]);
    r.options.copies = 3;
    let plan = r.build(&p).unwrap();
    assert_eq!(plan.markers()[0].placements.len(), 1);
    assert_eq!(plan.markers()[1].placements.len(), 2);
    assert_eq!(v(&plan)["used_stock_count"], 2);
    let mut copies: Vec<_> = plan
        .markers()
        .iter()
        .flat_map(|m| m.placements.iter().map(|p| p.copy))
        .collect();
    copies.sort();
    assert_eq!(copies, vec![0, 1, 2]);
    r.options.copies = 4;
    assert!(r.build(&p).is_err());
    let before = serde_json::to_value(&r).unwrap();
    r.options.work_limit = 1;
    assert_eq!(r.build(&p).unwrap_err().code, RefusalCode::BudgetExceeded);
    r.options.work_limit = 2000000;
    assert_eq!(serde_json::to_value(&r).unwrap(), before);
}

#[test]
fn residual_request_prevents_reusing_prior_cuts_and_chains_receipts() {
    let p = pattern(vec![piece("panel", 20., 20.)]);
    let r = request(vec![stock(
        "reusable-hide",
        material("hide", "hide"),
        rect(50., 25.),
    )]);
    let first = r.build(&p).unwrap();
    let residual = first.residual_request().unwrap();
    assert_eq!(residual.stocks[0].occupied_mm.len(), 1);
    assert!(
        residual.stocks[0]
            .occupied_basis
            .as_deref()
            .unwrap()
            .starts_with("ftx-finite-stock-plan/1:")
    );
    let first_cut = first.markers()[0].placements[0].cut_mm.clone();
    let second = residual.build(&p).unwrap();
    assert_eq!(second.markers()[0].placements.len(), 1);
    let second_cut = &second.markers()[0].placements[0].cut_mm;
    assert_ne!(second_cut, &first_cut);
    near(first.markers()[0].defect_free_area_m2, 0.00125);
    near(second.markers()[0].available_area_m2, 0.00085);

    let residual2 = second.residual_request().unwrap();
    assert_eq!(residual2.stocks[0].occupied_mm.len(), 2);
    assert!(
        residual2.stocks[0]
            .occupied_basis
            .as_deref()
            .unwrap()
            .contains(" -> ftx-finite-stock-plan/1:")
    );
    assert!(residual2.build(&p).is_err());
    assert_eq!(r.stocks[0].occupied_mm.len(), 0);
}

#[test]
fn authored_occupied_regions_require_receipts_and_survive_rotated_stock_frames() {
    let p = pattern(vec![piece("panel", 20., 20.)]);
    let mut s = stock("rotated", material("hide", "hide"), rect(25., 50.));
    s["grain_mm"] = json!([[0, 0], [10, 0]]);
    let first = request(vec![s]).build(&p).unwrap();
    let residual = first.residual_request().unwrap();
    let second = residual.build(&p).unwrap();
    assert_eq!(second.markers()[0].placements.len(), 1);
    near(second.markers()[0].available_area_m2, 0.00085);

    let mut raw = serde_json::to_value(&residual).unwrap();
    raw["stocks"][0]["occupied_basis"] = Value::Null;
    let unproven: Request = serde_json::from_value(raw).unwrap();
    assert!(unproven.build(&p).is_err());
}

#[test]
fn material_only_substitution_changes_stock_assignment_and_missing_mass_stays_unknown() {
    let p = pattern(vec![piece("shell", 30., 40.)]);
    let m = material("knit", "knit");
    let c: Changes =
        serde_json::from_value(json!({"schema":"ftx-material-bindings/1","default":m})).unwrap();
    let bound = c.apply(&p).unwrap();
    let mut r = request(vec![
        stock("leather", material("hide", "hide"), rect(100., 100.)),
        stock("jersey", m, rect(80., 80.)),
    ]);
    let a = r.build(&p).unwrap();
    let b = r.build(&Pattern::new(bound["pattern"].clone())).unwrap();
    assert_eq!(a.markers()[0].placements.len(), 1);
    assert_eq!(b.markers()[0].placements.len(), 0);
    assert_eq!(a.markers()[1].placements.len(), 0);
    assert_eq!(b.markers()[1].placements.len(), 1);
    near(
        a.markers()[0].external_cut_area_m2,
        b.markers()[1].external_cut_area_m2,
    );
    assert_eq!(p.raw()["pieces"], bound["pattern"]["pieces"]);
    r.stocks[0]
        .material
        .record
        .as_mut()
        .unwrap()
        .properties
        .clear();
    assert!(r.build(&p).is_err()); // same ID with changed observations must NOT match
    let mut raw = p.raw().clone();
    raw["fabric"]["material"]["record"]["properties"] = json!({});
    let unknown = r.build(&Pattern::new(raw)).unwrap();
    assert!(unknown.markers()[0].stock_mass_kg.is_none());
    assert!(unknown.markers()[0].mass_unavailable.is_some());
    let mut frozen = b.request().clone();
    frozen.stocks[1].material.record.as_mut().unwrap().name = "Changed source".into();
    assert!(
        frozen
            .build(&Pattern::new(bound["pattern"].clone()))
            .is_err()
    );
}

#[test]
fn mirrored_unfolded_curved_cutters_and_details_use_exact_export_transforms() {
    let mut fold = piece("fold", 20., 30.);
    fold["cut"]["on_fold"] = json!("left");
    let mut pair = piece("pair", 25., 40.);
    pair["cut"]["qty"] = json!(2);
    pair["edges"][1]["curve"] = json!({"type":"quad","c":[[0.5,-0.05]]});
    pair["notches"] = json!([{"id":"n","edge":"top","s":0.5}]);
    pair["internal"] = json!([{"kind":"cut","pts":[[7,8],[12,8]]}]);
    pair["drills"] = json!([{"p":[10,15],"d":2}]);
    pair["sim"] = json!(false);
    pair["sim_scale"] = json!([9, 9]);
    let p = pattern(vec![fold, pair]);
    let mut r = request(vec![stock(
        "large",
        material("hide", "hide"),
        rect(300., 300.),
    )]);
    r.options.copies = 2;
    r.options.step_mm = 10.;
    r.options.seam_allowance_mm = Some(2.);
    let plan = r.build(&p).unwrap();
    assert_eq!(plan.markers()[0].placements.len(), 6);
    let ctx = Ctx::new(
        &p,
        ExportOptions {
            sa: Some(2.),
            ..ExportOptions::default()
        },
    )
    .unwrap();
    for placed in &plan.markers()[0].placements {
        let i = p.instance(&placed.instance).unwrap();
        let g = ctx.geom(Source::Instance(i), 6.).unwrap();
        let expected = cutter_ring(&g).unwrap();
        let t = placed.transform;
        let map = |v: Vec2| {
            Vec2::new(
                t[0][0] * v.x + t[0][1] * v.y + t[0][2],
                t[1][0] * v.x + t[1][1] * v.y + t[1][2],
            )
        };
        for (a, b) in expected.into_iter().map(map).zip(&placed.cut_mm) {
            assert!((a - *b).norm() < 1e-9);
        }
        for ((path, _), (actual, _)) in g.internal_cut.iter().zip(&placed.internal_cut_mm) {
            for (a, b) in path.iter().copied().map(map).zip(actual) {
                assert!((a - *b).norm() < 1e-9);
            }
        }
        for ((point, diameter), (actual, d)) in g.drills.iter().zip(&placed.drills_mm) {
            assert!((map(*point) - *actual).norm() < 1e-9);
            assert_eq!(diameter, d);
        }
    }
}

#[test]
fn stock_axis_is_explicit_and_no_crossgrain_or_reflection_is_inferred() {
    let p = pattern(vec![piece("panel", 70., 20.)]);
    let mut s = stock("narrow", material("hide", "hide"), rect(30., 100.));
    assert!(request(vec![s.clone()]).build(&p).is_err());
    s["grain_mm"] = json!([[0, 0], [10, 0]]); // rotate measured stock into cutting frame
    let plan = request(vec![s]).build(&p).unwrap();
    let m = &plan.markers()[0];
    let a = m.source_to_marker;
    near(a[0][0] * a[1][1] - a[0][1] * a[1][0], 1.);
    assert!(m.outline_mm.iter().any(|p| p.x < 0.));
    let d = m.placements[0].grain_mm[1] - m.placements[0].grain_mm[0];
    near(d.x, 0.);
    assert!(d.y > 0.);
}

#[test]
fn invalid_sources_polygons_policies_and_internal_cuts_refuse_without_changing_inputs() {
    let p = pattern(vec![piece("small", 10., 10.)]);
    let r = request(vec![stock(
        "one",
        material("hide", "hide"),
        rect(100., 100.),
    )]);
    for case in 0..10 {
        let mut bad = serde_json::to_value(&r).unwrap();
        match case {
            0 => {
                let duplicate = bad["stocks"][0].clone();
                bad["stocks"].as_array_mut().unwrap().push(duplicate);
            }
            1 => bad["stocks"][0]["outline_mm"] = json!([[0, 0], [100, 100], [0, 100], [100, 0]]),
            2 => bad["stocks"][0]["grain_mm"] = json!([[0, 0], [0, 0]]),
            3 => bad["stocks"][0]["geometry_source"]["note"] = json!(""),
            4 => bad["stocks"][0]["material"]["allow_estimates"] = json!(false),
            5 => {
                bad["stocks"][0]["defects_mm"] = json!([[[90, 90], [110, 90], [110, 95], [90, 95]]])
            }
            6 => {
                bad["stocks"][0]["defects_mm"] = json!([
                    [[10, 10], [30, 10], [30, 30], [10, 30]],
                    [[20, 20], [40, 20], [40, 40], [20, 40]]
                ])
            }
            7 => bad["options"]["step_mm"] = json!(0),
            8 => bad["stocks"][0]["material"]["record"]["form"] = json!("fill"),
            _ => bad["options"]["copies"] = json!(4097),
        }
        let req: Request = serde_json::from_value(bad.clone()).unwrap();
        let before = serde_json::to_value(&req).unwrap();
        assert!(req.build(&p).is_err(), "case {case}");
        assert_eq!(serde_json::to_value(req).unwrap(), before);
    }
    let mut raw = p.raw().clone();
    raw["pieces"][0]["internal"] = json!([{"kind":"cut","pts":[[5,5],[200,5]]}]);
    assert!(r.build(&Pattern::new(raw)).is_err());
    let mut raw = p.raw().clone();
    raw["pieces"][0]["grain"] = json!(null);
    assert!(r.build(&Pattern::new(raw)).is_err());
}

//! Synthetic source-bearing placement regions, not measured leather grades or physical qualification.
use ftx_core::{RefusalCode, Vec2};
use ftx_export::cutting::stock::Request;
use ftx_pattern::Pattern;
use serde_json::{Value, json};

const PATTERN: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-stock-pattern.json");
const STOCK: &str = include_str!("../../../tests/fixtures/materials/synthetic-stock-request.json");
fn rect(x: f64, y: f64, w: f64, h: f64) -> Value {
    json!([[x, y], [x + w, y], [x + w, y + h], [x, y + h]])
}
fn zone(id: &str, outline: Value) -> Value {
    json!({"id":id,"outline_mm":outline,"geometry_source":{
        "reference":"test:placement-zone","accessed_on":"2026-10-20",
        "note":"Authored synthetic placement region; not an empirical quality measurement"}})
}
fn inputs() -> (Value, Value) {
    let mut p: Value = serde_json::from_str(PATTERN).unwrap();
    // A larger unrestricted piece would consume the scarce prime region if placed first.
    p["pieces"][1]["id"] = json!("lining");
    p["pieces"][1]["vertices"] = json!({"a":[0,0],"b":[40,0],"c":[40,20],"d":[0,20]});
    let mut r: Value = serde_json::from_str(STOCK).unwrap();
    r["stocks"][0]["outline_mm"] = rect(0., 0., 70., 25.);
    r["stocks"][0]["defects_mm"] = json!([]);
    r["stocks"][0]["zones"] = json!([zone("prime", rect(1., 1., 23., 23.))]);
    r["options"]["piece_zones"] = json!({"front":"prime"});
    r["options"]["zone_margin_mm"] = json!(1.);
    (p, r)
}
fn request(v: Value) -> Request {
    serde_json::from_value(v).unwrap()
}
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-10, "{a} != {b}");
}

#[test]
fn constrained_panels_precede_larger_unrestricted_parts_without_changing_patterns() {
    let (p, r) = inputs();
    let original = p.clone();
    let pattern = Pattern::new(p);
    let req = request(r);
    let plan = req.build(&pattern).unwrap();
    assert_eq!(pattern.raw(), &original);
    let m = &plan.markers()[0];
    let front = m.placements.iter().find(|p| p.piece == "front").unwrap();
    let lining = m.placements.iter().find(|p| p.piece == "lining").unwrap();
    assert!(front.bounds_mm[0] >= 2. && front.bounds_mm[2] <= 23.);
    assert!(front.bounds_mm[1] >= 2. && front.bounds_mm[3] <= 23.);
    assert!(lining.bounds_mm[0] >= front.bounds_mm[2] + req.options.gap_mm);
    near(m.gross_area_m2, 0.00175);
    near(m.available_area_m2, 0.00175); // A suitability zone is not missing material.
    near(m.external_cut_area_m2, 0.0012);
    assert_eq!(m.zones[0].geometry_source.reference, "test:placement-zone");
    assert_eq!(
        serde_json::to_value(&plan).unwrap(),
        serde_json::to_value(plan.request().build(&pattern).unwrap()).unwrap()
    );
    let mut plain = req.clone();
    plain.options.piece_zones.clear();
    plain.options.zone_margin_mm = 0.;
    let plain = plain.build(&pattern).unwrap();
    assert!(
        plain.markers()[0]
            .placements
            .iter()
            .find(|p| p.piece == "lining")
            .unwrap()
            .bounds_mm[0]
            < 3.
    );
}

#[test]
fn whole_cutter_must_fit_concave_zone_not_only_its_vertices_or_bounding_box() {
    let (mut p, mut r) = inputs();
    p["pieces"].as_array_mut().unwrap().truncate(1);
    r["stocks"][0]["outline_mm"] = rect(0., 0., 40., 40.);
    r["stocks"][0]["zones"][0]["outline_mm"] = json!([
        [1, 1],
        [31, 1],
        [31, 31],
        [21, 31],
        [21, 11],
        [11, 11],
        [11, 31],
        [1, 31]
    ]);
    r["options"]["zone_margin_mm"] = json!(0.);
    let req = request(r);
    let p = Pattern::new(p);
    assert!(req.build(&p).unwrap_err().message.contains("no checked"));
    let mut unconstrained = req;
    unconstrained.options.piece_zones.clear();
    assert!(unconstrained.build(&p).is_ok());
}

#[test]
fn overlapping_regions_do_not_remove_area_or_override_defects_and_consumed_cuts() {
    let (mut p, mut r) = inputs();
    p["pieces"].as_array_mut().unwrap().truncate(1);
    let duplicate = zone("other-qualification", rect(1., 1., 23., 23.));
    r["stocks"][0]["zones"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    let p = Pattern::new(p);
    let base = request(r.clone()).build(&p).unwrap();
    near(base.markers()[0].available_area_m2, 0.00175);
    for occupied in [false, true] {
        let mut bad = r.clone();
        let key = if occupied {
            "occupied_mm"
        } else {
            "defects_mm"
        };
        bad["stocks"][0][key] = json!([rect(10., 10., 2., 2.)]);
        if occupied {
            bad["stocks"][0]["occupied_basis"] = json!("test:prior-consumption");
        }
        assert!(
            request(bad)
                .build(&p)
                .unwrap_err()
                .message
                .contains("no checked")
        );
    }
}

#[test]
fn regions_follow_authored_rotated_frames_and_survive_residual_inventory() {
    let (mut p, mut r) = inputs();
    p["pieces"].as_array_mut().unwrap().truncate(1);
    p["pieces"][0]["cut"]["qty"] = json!(2);
    r["stocks"][0]["outline_mm"] = rect(0., 0., 140., 140.);
    r["stocks"][0]["zones"][0]["outline_mm"] = rect(10., 10., 115., 115.);
    r["options"]["copies"] = json!(2);
    r["options"]["step_mm"] = json!(5.);
    // Rotate every authored geometry and preferred direction, with a nonzero scanner origin.
    for key in ["outline_mm", "grain_mm"] {
        for v in r["stocks"][0][key].as_array_mut().unwrap() {
            let (x, y) = (v[0].as_f64().unwrap(), v[1].as_f64().unwrap());
            *v = json!([y + 200., -x + 300.]);
        }
    }
    for v in r["stocks"][0]["zones"][0]["outline_mm"]
        .as_array_mut()
        .unwrap()
    {
        let (x, y) = (v[0].as_f64().unwrap(), v[1].as_f64().unwrap());
        *v = json!([y + 200., -x + 300.]);
    }
    let pattern = Pattern::new(p);
    let req = request(r);
    let plan = req.build(&pattern).unwrap();
    assert_eq!(plan.markers()[0].placements.len(), 4); // All mirrored instances and garment copies.
    let m = &plan.markers()[0];
    for (original, mapped) in req.stocks[0].zones[0]
        .outline_mm
        .iter()
        .zip(&m.zones[0].outline_mm)
    {
        let a = m.source_to_marker;
        near(
            (Vec2::new(
                a[0][0] * original.x + a[0][1] * original.y + a[0][2],
                a[1][0] * original.x + a[1][1] * original.y + a[1][2],
            ) - *mapped)
                .norm(),
            0.,
        );
    }
    let residual = plan.residual_request().unwrap();
    assert_eq!(
        serde_json::to_value(&residual.stocks[0].zones).unwrap(),
        serde_json::to_value(&req.stocks[0].zones).unwrap()
    );
    assert_eq!(residual.options.piece_zones, req.options.piece_zones);
    assert_eq!(residual.stocks[0].occupied_mm.len(), 4);
    let next = residual.build(&pattern).unwrap();
    assert_eq!(next.markers()[0].placements.len(), 4);
    near(
        next.markers()[0].available_area_m2,
        m.available_area_m2 - 0.0016,
    );
}

#[test]
fn zone_constrained_half_turn_requires_exact_material_permission() {
    let (mut p, mut r) = inputs();
    p["pieces"].as_array_mut().unwrap().truncate(1);
    p["pieces"][0]["vertices"] = json!({"a":[0,0],"b":[60,0],"c":[0,60]});
    p["pieces"][0]["edges"] = json!([
        {"id":"bottom","from":"a","to":"b"},
        {"id":"diagonal","from":"b","to":"c"},
        {"id":"left","from":"c","to":"a"}]);
    r["stocks"][0]["outline_mm"] = rect(0., 0., 80., 80.);
    r["stocks"][0]["zones"][0]["outline_mm"] = json!([[3, 73], [73, 3], [73, 73]]);
    let p = Pattern::new(p);
    let mut req = request(r);
    assert!(req.build(&p).unwrap_err().message.contains("no checked"));
    let m = ftx_pattern::textile_import::resolve_fabric(&p.raw()["fabric"])
        .unwrap()
        .unwrap();
    req.options
        .grain_reversal_materials
        .push(m.sha256().to_owned());
    let plan = req.build(&p).unwrap();
    assert!(plan.markers()[0].placements[0].grain_reversed);
    for v in &plan.markers()[0].placements[0].cut_mm {
        assert!(v.x + v.y > 76.);
    }
    let residual = plan.residual_request().unwrap();
    assert!(residual.build(&p).is_err());
}

#[test]
fn zone_labels_never_substitute_for_exact_stock_material_identity() {
    let (p, mut r) = inputs();
    let mut with_zone = r["stocks"][0].clone();
    with_zone["id"] = json!("other-lot");
    with_zone["material"]["record"]["name"] = json!("different material observations");
    r["stocks"][0]["zones"] = json!([]);
    r["stocks"].as_array_mut().unwrap().push(with_zone);
    let p = Pattern::new(p);
    assert!(
        request(r.clone())
            .build(&p)
            .unwrap_err()
            .message
            .contains("required zone")
    );
    r["stocks"][1]["material"] = r["stocks"][0]["material"].clone();
    let plan = request(r).build(&p).unwrap();
    assert_eq!(plan.markers()[1].placements[0].piece, "front");
    assert_eq!(plan.markers()[0].placements[0].piece, "lining");
}

#[test]
fn invalid_region_evidence_mapping_clearance_and_budgets_refuse_atomically() {
    let (p, r) = inputs();
    let p = Pattern::new(p);
    for case in 0..12 {
        let mut bad = r.clone();
        match case {
            0 => bad["stocks"][0]["zones"][0]["id"] = json!(""),
            1 => {
                let duplicate = bad["stocks"][0]["zones"][0].clone();
                bad["stocks"][0]["zones"]
                    .as_array_mut()
                    .unwrap()
                    .push(duplicate);
            }
            2 => bad["stocks"][0]["zones"][0]["geometry_source"]["note"] = json!(""),
            3 => bad["stocks"][0]["zones"][0]["outline_mm"] = rect(-1., 1., 25., 23.),
            4 => {
                bad["stocks"][0]["zones"][0]["outline_mm"] =
                    json!([[1, 1], [23, 23], [1, 23], [23, 1]])
            }
            5 => bad["options"]["piece_zones"] = json!({"not-a-piece":"prime"}),
            6 => bad["options"]["piece_zones"] = json!({"front":"not-a-zone"}),
            7 => bad["options"]["piece_zones"] = json!({"front":" "}),
            8 => bad["options"]["zone_margin_mm"] = json!(-1.),
            9 => bad["options"]["piece_zones"] = json!({}),
            10 => bad["options"]["work_limit"] = json!(1),
            _ => {
                let z = bad["stocks"][0]["zones"][0].clone();
                bad["stocks"][0]["zones"] = json!(vec![z; 257]);
            }
        }
        let req = request(bad);
        let before = serde_json::to_value(&req).unwrap();
        let error = req.build(&p).unwrap_err();
        if case >= 10 {
            assert_eq!(error.code, RefusalCode::BudgetExceeded);
        }
        assert_eq!(before, serde_json::to_value(&req).unwrap());
    }
}

#[test]
fn omitted_regions_preserve_legacy_serialized_requests_and_results() {
    let p = Pattern::from_json(PATTERN).unwrap();
    let legacy: Value = serde_json::from_str(STOCK).unwrap();
    let mut explicit = legacy.clone();
    explicit["stocks"][0]["zones"] = json!([]);
    explicit["options"]["piece_zones"] = json!({});
    explicit["options"]["zone_margin_mm"] = json!(0.);
    let a = request(legacy).build(&p).unwrap();
    let b = request(explicit).build(&p).unwrap();
    assert_eq!(
        serde_json::to_value(&a).unwrap(),
        serde_json::to_value(&b).unwrap()
    );
    let r = serde_json::to_value(a.request()).unwrap();
    assert!(r["stocks"][0].get("zones").is_none());
    assert!(r["options"].get("piece_zones").is_none());
    assert!(r["options"].get("zone_margin_mm").is_none());
}

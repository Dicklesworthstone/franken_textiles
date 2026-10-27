//! Geometry-level treatment checks using synthetic dimensions, without a measured-garment claim.

use std::collections::BTreeMap;

use ftx_core::{Vec2, curve, polyline};
use ftx_pattern::{
    Pattern,
    material_catalog::Selection,
    material_treatment::{self, geometry},
    materials::{CATALOG_SCHEMA, Catalog},
    textile_import,
};
use serde_json::{Value, json};

fn transition() -> material_treatment::Input {
    let catalog = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: Vec::new(),
    };
    let source = json!({"reference":"local:ftx/synthetic-treatment-geometry/1",
        "accessed_on":"2026-10-26","note":"Independent synthetic geometry; no empirical textile claim"});
    let selection = |after: bool| {
        let record = json!({"id":if after {"test/after"} else {"test/before"},
            "name":"Synthetic dimensional test sheet","form":"woven",
            "composition":[],"composition_complete":false,
            "structure":if after {json!({"treatment_process":"synthetic cycle","treatment_cycles":"1"})} else {json!({})},
            "properties":{},"sources":{"synthetic":source}});
        catalog
            .resolve(&Selection {
                id: record["id"].as_str().unwrap().into(),
                record: Some(serde_json::from_value(record).unwrap()),
                ..Selection::default()
            })
            .unwrap()
            .frozen_selection()
    };
    let quantity = |value| {
        json!({"value":value,"unit":"1","evidence":"estimate",
        "source":"synthetic","method":"Independently authored free-dimensional change",
        "conditions":{"specimen":"test-sheet","process":"synthetic cycle","cycles":"1",
            "conditioning_before":"before","conditioning_after":"after"}})
    };
    serde_json::from_value(json!({"schema":material_treatment::SCHEMA,
        "before":selection(false),"after":selection(true),"specimen":"test-sheet",
        "process":"synthetic cycle","cycles":1,"conditioning_before":"before",
        "conditioning_after":"after","shrinkage_warp":quantity(0.1),
        "shrinkage_weft":quantity(0.2),"sources":{"synthetic":source},"allow_estimates":true}))
    .unwrap()
}

fn request(mode: geometry::Mode) -> geometry::Input {
    geometry::Input {
        schema: geometry::SCHEMA.into(),
        mode,
        transitions: BTreeMap::from([("wash".into(), transition())]),
        pieces: BTreeMap::from([("panel".into(), "wash".into())]),
        interfaces: None,
        seam_tolerance_mm: 3.,
    }
}

fn piece(id: &str) -> Value {
    json!({"id":id,"vertices":{"a":[0.,0.],"b":[100.,0.],"c":[100.,200.],"d":[0.,200.]},
        "edges":[{"id":"bottom","from":"a","to":"b"},{"id":"right","from":"b","to":"c"},
            {"id":"top","from":"c","to":"d"},{"id":"left","from":"d","to":"a"}],
        "grain":[[0.,0.],[0.,100.]],"sa":10.,"note":"Source manufacturing note"})
}

fn pattern() -> Value {
    json!({"schema":"fabric-pattern/1","units":"mm",
        "fabric":{"material":transition().before},"pieces":[piece("panel")],"seams":[]})
}

fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-9, "{a} != {b}");
}

fn point(value: &Value) -> Vec2 {
    serde_json::from_value(value.clone()).unwrap()
}

fn close_point(a: Vec2, b: Vec2) {
    close(a.x, b.x);
    close(a.y, b.y);
}

fn diagonal_map(p: Vec2) -> Vec2 {
    // Warp at 45 degrees: R diag(0.9,0.8) R^T, independently expanded.
    Vec2::new(0.85 * p.x + 0.05 * p.y, 0.05 * p.x + 0.85 * p.y)
}

#[test]
fn material_axes_rotate_anisotropy_and_preserve_placed_anchor() {
    let input = request(geometry::Mode::Treat);
    let raw = pattern();
    let original = raw.clone();
    let report = input.apply(&Pattern::new(raw.clone())).unwrap();
    close_point(
        point(&report["pattern"]["pieces"][0]["vertices"]["c"]),
        Vec2::new(80., 180.),
    );
    assert_eq!(raw, original);
    assert_eq!(report["pattern"]["pieces"][0]["sa"], 10.);
    let mut raw = raw;
    raw["pieces"][0]["grain"] = json!([[0., 0.], [100., 100.]]);
    raw["pieces"][0]["placement"] = json!({"x0":10.,"y0":20.,"y0_landmark":"waist"});
    let report = input.apply(&Pattern::new(raw)).unwrap();
    let expected = Vec2::new(10., 20.) + diagonal_map(Vec2::new(90., 180.));
    close_point(
        point(&report["pattern"]["pieces"][0]["vertices"]["c"]),
        expected,
    );
    let after = Pattern::new(report["pattern"].clone());
    close(after.instances[0].area().unwrap(), 20000. * 0.72);
    assert_eq!(report["pattern"]["pieces"][0]["placement"]["y0"], 20.);
}

#[test]
fn absolute_curve_controls_notch_locations_and_material_marks_follow_the_same_map() {
    let mut raw = pattern();
    let p = &mut raw["pieces"][0];
    p["grain"] = json!([[0., 0.], [100., 100.]]);
    p["edges"][1]["curve"] =
        json!({"type":"cubic","c":[[0.25,0.18],[0.75,0.12]],"source_note":"keep"});
    p["notches"] = json!([{"edge":"right","s":0.37,"id":"balance","depth":3.}]);
    p["internal"] = json!([{"kind":"draw","pts":[[10.,20.],[30.,40.]]}]);
    p["decor"] = json!([{"kind":"embroidery","pts":[[25.,40.],[50.,65.]],"width_mm":2.}]);
    p["drills"] = json!([{"p":[25.,30.],"d":2.}]);
    let before = Pattern::new(raw);
    let source_edge = before.instances[0]
        .edge_dense("right", curve::EDGE_DENSE)
        .unwrap();
    let source_notch = polyline::at_fractions(&source_edge, &[0.37])[0];
    let report = request(geometry::Mode::Treat).apply(&before).unwrap();
    let after = Pattern::new(report["pattern"].clone());
    let target = &after.doc.as_ref().unwrap().pieces[0];
    let curve = target.edges[1].curve.as_ref().unwrap();
    for (actual, original) in curve
        .controls()
        .iter()
        .zip([Vec2::new(64., 50.), Vec2::new(76., 150.)])
    {
        close_point(
            curve::rel_to_abs(
                target.vertices.get("b").unwrap(),
                target.vertices.get("c").unwrap(),
                *actual,
            ),
            diagonal_map(original),
        );
    }
    let target_edge = after.instances[0]
        .edge_dense("right", curve::EDGE_DENSE)
        .unwrap();
    for (&a, &b) in source_edge.iter().zip(&target_edge) {
        close_point(b, diagonal_map(a));
    }
    let fraction = report["pattern"]["pieces"][0]["notches"][0]["s"]
        .as_f64()
        .unwrap();
    close_point(
        polyline::at_fractions(&target_edge, &[fraction])[0],
        diagonal_map(source_notch),
    );
    let p = &report["pattern"]["pieces"][0];
    close_point(
        point(&p["internal"][0]["pts"][0]),
        diagonal_map(Vec2::new(10., 20.)),
    );
    close_point(
        point(&p["decor"][0]["pts"][1]),
        diagonal_map(Vec2::new(50., 65.)),
    );
    close_point(
        point(&p["drills"][0]["p"]),
        diagonal_map(Vec2::new(25., 30.)),
    );
    assert_eq!(p["drills"][0]["d"], 2.);
    assert_eq!(p["edges"][1]["curve"]["source_note"], "keep");
}

#[test]
fn mirror_pairs_and_principal_folds_keep_physical_instances() {
    let mut raw = pattern();
    raw["pieces"][0]["cut"] = json!({"qty":2});
    raw["pieces"][0]["grain"] = json!([[0., 0.], [100., 100.]]);
    let report = request(geometry::Mode::Treat)
        .apply(&Pattern::new(raw))
        .unwrap();
    let after = Pattern::new(report["pattern"].clone());
    assert_eq!(after.instances.len(), 2);
    for instance in &after.instances {
        close(instance.area().unwrap(), 14400.);
    }
    let right = after
        .instances
        .iter()
        .find(|i| i.id == "panel.R")
        .unwrap()
        .verts
        .get("c")
        .unwrap();
    let left = after
        .instances
        .iter()
        .find(|i| i.id == "panel.L")
        .unwrap()
        .verts
        .get("c")
        .unwrap();
    close_point(left, Vec2::new(-right.x, right.y));
    let mut raw = pattern();
    raw["pieces"][0]["cut"] = json!({"on_fold":"left"});
    let report = request(geometry::Mode::Treat)
        .apply(&Pattern::new(raw.clone()))
        .unwrap();
    close(
        Pattern::new(report["pattern"].clone()).instances[0]
            .area()
            .unwrap(),
        28800.,
    );
    raw["pieces"][0]["grain"] = json!([[0., 0.], [100., 100.]]);
    assert!(
        request(geometry::Mode::Treat)
            .apply(&Pattern::new(raw))
            .is_err()
    );
}

#[test]
fn compensation_then_treatment_recovers_authored_geometry_with_new_material_only_once() {
    let mut raw = pattern();
    raw["pieces"][0]["grain"] = json!([[0., 0.], [100., 100.]]);
    raw["pieces"][0]["edges"][1]["curve"] = json!({"type":"quad","c":[[0.5,0.2]]});
    raw["pieces"][0]["notches"] = json!([{"edge":"right","s":0.41}]);
    let original = Pattern::new(raw);
    let compensation = request(geometry::Mode::Compensate);
    let cut_report = compensation.apply(&original).unwrap();
    let cut_pattern = Pattern::new(cut_report["pattern"].clone());
    let cut_material =
        textile_import::resolve_fabric(textile_import::piece_fabric(&cut_pattern, 0).unwrap())
            .unwrap()
            .unwrap();
    assert_eq!(
        Some(cut_material.sha256()),
        transition().before.expected_sha256.as_deref()
    );
    assert!(compensation.apply(&cut_pattern).is_err());
    let mut revised = compensation.clone();
    revised
        .transitions
        .get_mut("wash")
        .unwrap()
        .sources
        .get_mut("synthetic")
        .unwrap()
        .note
        .push_str(" revised metadata");
    assert!(revised.apply(&cut_pattern).is_err());
    let treated = request(geometry::Mode::Treat).apply(&cut_pattern).unwrap();
    let finished = Pattern::new(treated["pattern"].clone());
    for edge in ["bottom", "right", "top", "left"] {
        let a = original.instances[0]
            .edge_dense(edge, curve::EDGE_DENSE)
            .unwrap();
        let b = finished.instances[0]
            .edge_dense(edge, curve::EDGE_DENSE)
            .unwrap();
        for (a, b) in a.into_iter().zip(b) {
            close_point(a, b);
        }
    }
    close(
        treated["pattern"]["pieces"][0]["notches"][0]["s"]
            .as_f64()
            .unwrap(),
        0.41,
    );
    let final_material =
        textile_import::resolve_fabric(textile_import::piece_fabric(&finished, 0).unwrap())
            .unwrap()
            .unwrap();
    assert_eq!(
        Some(final_material.sha256()),
        transition().after.expected_sha256.as_deref()
    );
    assert!(request(geometry::Mode::Treat).apply(&finished).is_err());
}

#[test]
fn partial_treatment_recomputes_seam_compatibility_and_preserves_untouched_pieces() {
    let mut raw = pattern();
    raw["pieces"].as_array_mut().unwrap().push(piece("lining"));
    raw["seams"] = json!([{"id":"join","a":"panel/right","b":"lining/left","reverse":true}]);
    let before = Pattern::new(raw.clone());
    let input = request(geometry::Mode::Treat);
    let report = input.apply(&before).unwrap();
    assert_eq!(report["seams_before"][0]["status"], "PASS");
    assert_eq!(report["seams_after"][0]["status"], "FAIL");
    assert_eq!(report["pattern"]["pieces"][1], raw["pieces"][1]);
    assert_eq!(report["pattern"]["seams"], raw["seams"]);
    close(report["seams_after"][0]["len_a_mm"].as_f64().unwrap(), 180.);
    close(report["seams_after"][0]["len_b_mm"].as_f64().unwrap(), 200.);
    let mut both = input;
    both.pieces.insert("lining".into(), "wash".into());
    assert_eq!(
        both.apply(&before).unwrap()["seams_after"][0]["status"],
        "PASS"
    );
}

#[test]
fn stale_sources_missing_grain_and_noncommuting_simulation_scale_refuse_atomically() {
    let input = request(geometry::Mode::Treat);
    let mut raw = pattern();
    raw["fabric"]["material"] = serde_json::to_value(transition().after).unwrap();
    assert!(input.apply(&Pattern::new(raw)).is_err());
    let mut raw = pattern();
    raw["pieces"][0]["grain"] = Value::Null;
    assert!(input.apply(&Pattern::new(raw)).is_err());
    let mut raw = pattern();
    raw["pieces"][0]["grain"] = json!([[0., 0.], [100., 100.]]);
    raw["pieces"][0]["sim_scale"] = json!([0.8, 0.9]);
    let original = raw.clone();
    assert!(input.apply(&Pattern::new(raw.clone())).is_err());
    assert_eq!(raw, original);
    raw["pieces"][0]["sim_scale"] = json!([0.8, 0.8]);
    assert!(input.apply(&Pattern::new(raw)).is_ok());
    let mut invalid = input;
    invalid.pieces.insert("missing".into(), "wash".into());
    assert!(invalid.apply(&Pattern::new(pattern())).is_err());
}

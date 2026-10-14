//! Synthetic software answers, not measurements or qualified sewing-machine recipes.
use ftx_export::sewing::{SewOptions, SewPlan};
use ftx_pattern::Pattern;
use serde_json::{Value, json};

fn fabric(id: &str, thickness_mm: f64) -> Value {
    json!({"material":{"id":id,"allow_estimates":true,"record":{
        "id":id,"name":"Synthetic sewing sheet","form":"woven",
        "properties":{"thickness":{"value":thickness_mm,"unit":"mm","evidence":"estimate",
            "source":"t","method":"Invented test thickness"}},
        "sources":{"t":{"reference":"test:sewing","accessed_on":"2026-10-14",
            "note":"Software fixture, no physical measurement"}}
    }}})
}
fn piece(id: &str, length: f64) -> Value {
    json!({"id":id,"cut":{"qty":1},"grain":[[0,0],[0,10]],
        "vertices":{"a":[0,0],"b":[20,0],"c":[20,length],"d":[0,length]},
        "edges":[{"id":"bottom","from":"a","to":"b"},{"id":"right","from":"b","to":"c"},
            {"id":"top","from":"c","to":"d"},{"id":"left","from":"d","to":"a"}]})
}
fn pattern() -> Value {
    json!({"schema":"fabric-pattern/1","units":"mm","fabric":fabric("thin",0.4),
        "meta":{"sa_mm":10},"pieces":[piece("front",100.),piece("back",100.)],
        "seams":[{"id":"side","a":"front/right","b":"back/left","reverse":true}]})
}
fn options() -> Value {
    json!({"machine":{"id":"synthetic-machine","stitch_mm":[0.1,5.],"feed_b_over_a":[0.5,2.],
        "max_stack_mm":3.,"max_stitches_per_minute":1000.},
        "seam_tolerance_mm":0.01,
        "passes":[{"seam":"side","max_stitch_mm":2.5,"plies":[1,1],"minimum_allowance_mm":6.,
            "stitches_per_minute":600.,"needle":"test-needle","thread":"test-thread"}]})
}
fn plan(p: Value, o: Value) -> Result<SewPlan, ftx_core::Refusal> {
    SewPlan::build(
        &Pattern::new(p),
        &serde_json::from_value::<SewOptions>(o).unwrap(),
    )
}

#[test]
fn unchanged_pattern_material_swap_changes_stack_and_receipt_not_station_geometry() {
    let p = pattern();
    let a = plan(p.clone(), options()).unwrap();
    let mut leather = p;
    leather["fabric"] = fabric("leather", 1.1);
    leather["fabric"]["material"]["record"]["form"] = json!("hide");
    let b = plan(leather.clone(), options()).unwrap();
    assert_eq!(a.seams[0].stations, b.seams[0].stations);
    assert_ne!(a.pattern_sha256, b.pattern_sha256);
    assert_ne!(
        a.seams[0].sides[0].material_sha256,
        b.seams[0].sides[0].material_sha256
    );
    assert!((a.seams[0].stack_mm - 0.8).abs() < 1e-12);
    assert!((b.seams[0].stack_mm - 2.2).abs() < 1e-12);
    assert_eq!(a.stitch_intervals, 40);
    assert!((a.feed_seconds - 4.).abs() < 1e-12);
    assert!(!a.hardware_execution_authorized && !a.physical_seam_quality_validated);
    leather["fabric"] = fabric("thick-leather", 2.);
    assert!(
        plan(leather, options())
            .unwrap_err()
            .message
            .contains("stack thickness")
    );
    for snapshot in b.materials.values() {
        let resolved = ftx_pattern::materials::Catalog::builtin()
            .unwrap()
            .resolve(snapshot)
            .unwrap();
        assert_eq!(Some(resolved.sha256()), snapshot.expected_sha256.as_deref());
        assert!(
            resolved.mechanics().is_err(),
            "sewing does not need invented moduli"
        );
    }
}

#[test]
fn per_piece_plies_and_reported_upper_thickness_are_not_averaged_away() {
    let mut p = pattern();
    p["pieces"][1]["fabric"] = fabric("lining", 0.2);
    p["pieces"][1]["fabric"]["material"]["record"]["properties"]["thickness"]["bounds"] =
        json!([0.1, 0.3]);
    let mut o = options();
    o["passes"][0]["plies"] = json!([2, 3]);
    let a = plan(p, o).unwrap();
    assert!((a.seams[0].stack_mm - 1.7).abs() < 1e-12);
    assert_eq!(a.materials.len(), 2);
    assert!(a.seams[0].sides[1].uses_thickness_upper_bound);
}

#[test]
fn matched_notches_drive_local_feed_not_just_the_whole_seam_ratio() {
    let mut p = pattern();
    p["pieces"][0]["notches"] = json!([{"edge":"right","s":0.4}]);
    p["pieces"][1]["notches"] = json!([{"edge":"left","s":0.5}]);
    let mut o = options();
    o["seam_tolerance_mm"] = json!(11.);
    let a = plan(p.clone(), o.clone()).unwrap();
    let feeds = &a.seams[0].feed_b_over_a;
    assert!(feeds.iter().any(|x| (x - 1.25).abs() < 1e-9));
    assert!(feeds.iter().any(|x| (x - 5. / 6.).abs() < 1e-9));
    assert!(a.seams[0].stations.fa.contains(&0.4));
    o["machine"]["feed_b_over_a"] = json!([0.9, 1.1]);
    assert!(
        plan(p, o)
            .unwrap_err()
            .message
            .contains("differential feed")
    );
}

#[test]
fn reverse_composite_edges_and_simulation_flags_preserve_manufacturing_correspondence() {
    let mut p = pattern();
    p["seams"][0]["a"] = json!("front/bottom+right");
    p["seams"][0]["b"] = json!("back/top+left");
    let a = plan(p.clone(), options()).unwrap();
    assert!((a.seams[0].stations.len_a - 120.).abs() < 1e-12);
    let last = a.seams[0].stations.b.last().unwrap();
    assert_eq!([last.x, last.y], [20., 100.]);
    p["pieces"][1]["sim"] = json!(false);
    p["pieces"][1]["sim_scale"] = json!([0.7, 0.8]);
    p["seams"][0]["sim"] = json!(false);
    let b = plan(p, options()).unwrap();
    assert_eq!(a.seams[0].stations, b.seams[0].stations);
}

#[test]
fn allowance_hierarchy_and_short_stitch_intervals_are_checked() {
    let mut p = pattern();
    p["pieces"][0]["edges"][1]["sa"] = json!(3.);
    assert!(
        plan(p, options())
            .unwrap_err()
            .message
            .contains("allowance")
    );
    let mut o = options();
    o["seam_allowance_mm"] = json!(4.);
    assert!(plan(pattern(), o).is_err());
    let mut p = pattern();
    p["pieces"][0]["notches"] = json!([{"edge":"right","s":0.001}]);
    p["pieces"][1]["notches"] = json!([{"edge":"left","s":0.999}]);
    let mut o = options();
    o["machine"]["stitch_mm"] = json!([0.5, 5.]);
    assert!(plan(p, o).unwrap_err().message.contains("stitch advance"));
}

#[test]
fn dependency_order_is_stable_and_cycles_or_partial_jobs_refuse() {
    let mut p = pattern();
    p["seams"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"top","a":"front/top","b":"back/top"}));
    let mut o = options();
    let mut top = o["passes"][0].clone();
    top["seam"] = json!("top");
    o["passes"].as_array_mut().unwrap().push(top);
    o["passes"][0]["after"] = json!(["top"]);
    let a = plan(p.clone(), o.clone()).unwrap();
    assert_eq!(a.seams[0].pass.seam, "top");
    o["passes"].as_array_mut().unwrap().reverse();
    let b = plan(p.clone(), o.clone()).unwrap();
    assert_eq!(a.seams[0].pass.seam, b.seams[0].pass.seam);
    o["passes"][0]["after"] = json!(["side"]);
    assert!(plan(p.clone(), o).unwrap_err().message.contains("cyclic"));
    assert!(
        plan(p, options())
            .unwrap_err()
            .message
            .contains("every pattern seam")
    );
}

#[test]
fn missing_estimated_loose_fill_and_mismatched_notch_inputs_refuse() {
    let base = pattern();
    let mut p = base.clone();
    p["fabric"]["material"]["allow_estimates"] = json!(false);
    assert!(plan(p, options()).is_err());
    let mut p = base.clone();
    p["fabric"]["material"]["record"]["properties"] = json!({});
    assert!(
        plan(p, options())
            .unwrap_err()
            .message
            .contains("thickness")
    );
    let mut p = base.clone();
    p["fabric"]["material"]["record"]["form"] = json!("fill");
    assert!(plan(p, options()).is_err());
    let mut p = base.clone();
    p["fabric"]["layers"] = json!([{"role":"fill"}]);
    assert!(
        plan(p, options())
            .unwrap_err()
            .message
            .contains("implicit fabric layers")
    );
    let mut p = base.clone();
    p["pieces"][0]["notches"] = json!([{"edge":"right","s":0.5}]);
    assert!(plan(p, options()).unwrap_err().message.contains("notches"));
    let mut p = base;
    p["meta"]["scale_status"] = json!("UNSCALED");
    assert!(plan(p, options()).is_err());
}

#[test]
fn duplicate_notches_bad_flat_ratios_and_allocation_bombs_refuse() {
    let mut p = pattern();
    p["pieces"][0]["notches"] = json!([{"edge":"right","s":0.5},{"edge":"right","s":0.5}]);
    assert!(
        plan(p, options())
            .unwrap_err()
            .message
            .contains("duplicate notch")
    );
    let mut p = pattern();
    p["seams"][0]["ratio"] = json!(0.5);
    assert!(
        plan(p, options())
            .unwrap_err()
            .message
            .contains("flat seams")
    );
    let mut o = options();
    o["passes"][0]["max_stitch_mm"] = json!(1e-100);
    assert!(plan(pattern(), o).is_err());
    let mut p = pattern();
    p["pieces"] = json!([piece("front", 90000.), piece("back", 90000.)]);
    let mut o = options();
    o["passes"][0]["max_stitch_mm"] = json!(0.01);
    o["machine"]["stitch_mm"] = json!([0.001, 5.]);
    assert!(plan(p, o).unwrap_err().message.contains("budget"));
}

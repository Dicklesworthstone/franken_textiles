//! Synthetic known answers for material preimages under declared flat folds, not physical seam data.
use ftx_export::sewing::{SewOptions, SewPlan, program::SewProgram};
use ftx_pattern::Pattern;
use serde_json::{Value, json};

fn fabric(id: &str, mm: f64) -> Value {
    json!({"material":{"id":id,"allow_estimates":true,"record":{
        "id":id,"name":"Synthetic assembly sheet","form":"woven",
        "properties":{"thickness":{"value":mm,"unit":"mm","evidence":"estimate",
            "source":"t","method":"Invented software-test thickness"}},
        "sources":{"t":{"reference":"test:flat-fold","accessed_on":"2026-10-15",
            "note":"Synthetic software fixture; no physical measurement"}}
    }}})
}
fn piece(id: &str, h: f64) -> Value {
    json!({"id":id,"cut":{"qty":1},"grain":[[0,0],[0,10]],
        "vertices":{"a":[0,0],"b":[20,0],"c":[20,h],"d":[0,h]},
        "edges":[{"id":"bottom","from":"a","to":"b"},{"id":"right","from":"b","to":"c"},
            {"id":"top","from":"c","to":"d"},{"id":"left","from":"d","to":"a"}]})
}
fn pattern(h: f64) -> Value {
    let mut back = piece("back", h);
    back["fabric"] = fabric("lining", 0.2);
    json!({"schema":"fabric-pattern/1","units":"mm","fabric":fabric("shell",0.4),
        "meta":{"sa_mm":10},"pieces":[piece("front",h),back],
        "seams":[{"id":"side","a":"front/right","b":"back/left","reverse":true}]})
}
fn placement(instance: &str, x: f64, y: f64, turn: bool) -> Value {
    json!({"instance":instance,"origin_mm":[x,y],"x_axis":if turn {[-1,0]} else {[1,0]},
        "face_down":turn})
}
fn options(fold: bool) -> Value {
    json!({"machine":{"id":"synthetic","stitch_mm":[0.1,5],"feed_b_over_a":[0.5,2],
        "max_stack_mm":1.5,"max_stitches_per_minute":1000},"seam_tolerance_mm":0.01,
        "passes":[{"seam":"side","max_stitch_mm":2.5,"minimum_allowance_mm":5,
        "stitches_per_minute":600,"needle":"test-needle","thread":"test-thread",
        "assembly":{"pieces":[placement("front",0.,0.,false),placement("back",if fold {40.} else {20.},0.,true)],
            "folds":if fold {json!([{"line_mm":[[30,-20],[30,120]],"moving_side":"right","stack":"over"}])} else {json!([])},
            "registration_tolerance_mm":1e-6,"minimum_boundary_clearance_mm":1.0}}]})
}
fn plan(p: Value, o: Value) -> Result<SewPlan, ftx_core::Refusal> {
    SewPlan::build(
        &Pattern::new(p),
        &serde_json::from_value::<SewOptions>(o).unwrap(),
    )
}
fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-10, "{a} != {b}");
}

#[test]
fn registered_needle_has_exact_preimages_on_both_original_pieces() {
    let plan = plan(pattern(100.), options(false)).unwrap();
    assert_eq!(plan.schema, "ftx-sewing-plan/4");
    let seam = &plan.seams[0];
    let a = seam.assembly.as_ref().unwrap();
    assert_eq!(a.layers.len(), 2);
    assert_eq!(a.side_layers, [0, 1]);
    assert!(a.layers[0].public_face_up);
    assert!(!a.layers[1].public_face_up);
    close(a.stack_mm, 0.6);
    close(seam.stack_mm, a.stack_mm);
    close(a.minimum_boundary_clearance_mm, 10.);
    close(a.maximum_registration_error_mm, 0.);
    for (k, p) in a.needle_mm.iter().enumerate() {
        close(p.x, 20.);
        close(p.y, k as f64 * 2.5);
        assert_eq!(a.layers[0].source_stations_mm[k], seam.stations.a[k]);
        assert_eq!(a.layers[1].source_stations_mm[k], seam.stations.b[k]);
    }
    assert_eq!(a.layers[0].z_bounds_mm, [0., 0.4]);
    close(a.layers[1].z_bounds_mm[1], 0.6);
    assert!(!plan.hardware_execution_authorized && !plan.physical_seam_quality_validated);
}

#[test]
fn partial_operations_sew_a_local_reinforcement_without_changing_the_source_seam() {
    let mut p = pattern(100.);
    let mut patch = piece("reinforcement", 50.);
    patch["fabric"] = fabric("patch", 0.3);
    p["pieces"].as_array_mut().unwrap().push(patch);
    let mut o = options(false);
    o["passes"][0]["id"] = json!("join-lower");
    o["passes"][0]["source_range"] = json!([0., 0.5]);
    let mut upper = o["passes"][0].clone();
    upper["id"] = json!("join-reinforced-upper");
    upper["after"] = json!(["join-lower"]);
    upper["source_range"] = json!([0.5, 1.]);
    upper["assembly"]["pieces"]
        .as_array_mut()
        .unwrap()
        .push(placement("reinforcement", 0., 50., false));
    o["passes"].as_array_mut().unwrap().push(upper);
    let result = plan(p.clone(), o.clone()).unwrap();
    assert_eq!(result.schema, "ftx-sewing-plan/10");
    assert_eq!(result.seams.len(), 2);
    let lower = result.seams[0].assembly.as_ref().unwrap();
    let upper = result.seams[1].assembly.as_ref().unwrap();
    assert_eq!(lower.layers.len(), 2);
    assert_eq!(upper.layers.len(), 3);
    close(lower.stack_mm, 0.6);
    close(upper.stack_mm, 0.9);
    assert_eq!(upper.layers[2].instance, "reinforcement");
    for (needle, source) in upper
        .needle_mm
        .iter()
        .zip(&upper.layers[2].source_stations_mm)
    {
        close(needle.x, source.x);
        close(needle.y, source.y + 50.);
        assert!((50. ..=100.).contains(&needle.y));
    }
    close(upper.minimum_boundary_clearance_mm, 10.);
    assert_eq!(
        result.seams[0].stations.a.last(),
        result.seams[1].stations.a.first()
    );
    let program = SewProgram::build(
        &Pattern::new(p.clone()),
        &serde_json::from_value(o.clone()).unwrap(),
    )
    .unwrap();
    assert_eq!(program.schema, "ftx-sewing-program/3");
    assert_eq!(program.plan.pattern_sha256, result.pattern_sha256);
    assert!(program.stitches.iter().all(|s| s.assembly_mm.is_some()));

    // The same assembly cannot silently sew its unreinforced lower region as a three-ply stack.
    let mut whole = o.clone();
    whole["passes"] = json!([whole["passes"][1].clone()]);
    whole["passes"][0]["after"] = json!([]);
    whole["passes"][0]["source_range"] = json!([0., 1.]);
    assert!(
        plan(p.clone(), whole)
            .unwrap_err()
            .message
            .contains("extra material ply")
    );

    p["pieces"][2]["fabric"] = fabric("thicker-patch", 0.6);
    let swapped = plan(p.clone(), o.clone()).unwrap();
    assert_eq!(result.seams[1].stations, swapped.seams[1].stations);
    close(swapped.seams[0].stack_mm, 0.6);
    close(swapped.seams[1].stack_mm, 1.2);
    p["pieces"][2]["fabric"] = fabric("too-thick-patch", 1.0);
    let refusal = plan(p, o).unwrap_err();
    assert!(
        refusal.message.contains("join-reinforced-upper")
            && refusal.message.contains("stack thickness")
    );
}

#[test]
fn every_folded_ply_uses_independent_unloaded_thickness_and_its_reported_bound() {
    let mut p = pattern(100.);
    let baseline = plan(p.clone(), options(true)).unwrap();
    let properties = &mut p["pieces"][1]["fabric"]["material"]["record"]["properties"];
    properties["thickness"]["conditions"]["measurement_pressure"] =
        json!("Gauge pressure not specified");
    assert!(plan(p.clone(), options(true)).is_err());
    p["pieces"][1]["fabric"]["material"]["record"]["properties"]["unloaded_thickness"] = json!({
        "value":0.3,"bounds":[0.25,0.4],"unit":"mm","source":"t","evidence":"estimate",
        "method":"Independent synthetic unloaded lining",
        "conditions":{"specimen_id":"lining","conditioning":"synthetic dry","thickness_state":"unloaded"}});
    let got = plan(p.clone(), options(true)).unwrap();
    let assembly = got.seams[0].assembly.as_ref().unwrap();
    assert_eq!(
        assembly
            .layers
            .iter()
            .map(|l| l.instance.as_str())
            .collect::<Vec<_>>(),
        ["front", "back", "back"]
    );
    close(assembly.stack_mm, 1.2);
    assert!(
        assembly.layers[1..]
            .iter()
            .all(|l| l.uses_thickness_upper_bound)
    );
    assert_eq!(
        assembly.needle_mm,
        baseline.seams[0].assembly.as_ref().unwrap().needle_mm
    );
    let mut o = options(true);
    o["machine"]["max_stack_mm"] = json!(1.1);
    assert!(plan(p, o).unwrap_err().message.contains("stack thickness"));
}

#[test]
fn adjacent_source_spans_can_use_different_declared_fold_recipes() {
    let p = pattern(100.);
    let mut o = options(false);
    o["passes"][0]["id"] = json!("plain-lower");
    o["passes"][0]["source_range"] = json!([0., 0.5]);
    let mut upper = options(true)["passes"][0].clone();
    upper["id"] = json!("folded-upper");
    upper["after"] = json!(["plain-lower"]);
    upper["source_range"] = json!([0.5, 1.]);
    o["passes"].as_array_mut().unwrap().push(upper);
    let result = plan(p, o).unwrap();
    let lower = result.seams[0].assembly.as_ref().unwrap();
    let upper = result.seams[1].assembly.as_ref().unwrap();
    assert_eq!(lower.layers.len(), 2);
    assert_eq!(upper.layers.len(), 3);
    close(result.seams[0].stack_mm, 0.6);
    close(result.seams[1].stack_mm, 0.8);
    assert_eq!(lower.needle_mm.last(), upper.needle_mm.first());
    assert_eq!(
        result.seams[1].pass.assembly.as_ref().unwrap().folds.len(),
        1
    );
    assert!(!result.hardware_execution_authorized);
}

#[test]
fn flat_fold_derives_three_plies_and_material_swap_changes_stack_not_geometry() {
    let p = pattern(100.);
    let a = plan(p.clone(), options(true)).unwrap();
    let aa = a.seams[0].assembly.as_ref().unwrap();
    assert_eq!(
        aa.layers
            .iter()
            .map(|l| l.instance.as_str())
            .collect::<Vec<_>>(),
        ["front", "back", "back"]
    );
    assert_eq!(aa.side_layers, [0, 2]);
    assert_eq!(
        aa.layers
            .iter()
            .map(|l| l.public_face_up)
            .collect::<Vec<_>>(),
        [true, false, true]
    );
    for p in &aa.layers[1].source_stations_mm {
        close(p.x, 20.);
    }
    for p in &aa.layers[2].source_stations_mm {
        close(p.x, 0.);
    }
    close(aa.stack_mm, 0.8);
    let mut swapped = p;
    swapped["pieces"][1]["fabric"] = fabric("thicker-lining", 0.5);
    let b = plan(swapped.clone(), options(true)).unwrap();
    let ba = b.seams[0].assembly.as_ref().unwrap();
    close(ba.stack_mm, 1.4);
    assert_eq!(aa.needle_mm, ba.needle_mm);
    assert_eq!(a.seams[0].stations, b.seams[0].stations);
    for (l, r) in aa.layers.iter().zip(&ba.layers) {
        assert_eq!(l.source_stations_mm, r.source_stations_mm);
    }
    assert_ne!(a.pattern_sha256, b.pattern_sha256);
    assert_ne!(aa.layers[1].material_sha256, ba.layers[1].material_sha256);
    close(ba.layers[2].z_bounds_mm[0], 0.9);
    swapped["pieces"][1]["fabric"] = fabric("too-thick-lining", 0.8);
    assert!(
        plan(swapped, options(true))
            .unwrap_err()
            .message
            .contains("stack thickness")
    );
    for (digest, frozen) in &b.materials {
        let resolved = ftx_pattern::materials::Catalog::builtin()
            .unwrap()
            .resolve(frozen)
            .unwrap();
        assert_eq!(resolved.sha256(), digest);
    }
}

#[test]
fn underfold_reverses_order_without_changing_source_correspondence() {
    let mut o = options(true);
    o["passes"][0]["assembly"]["folds"][0]["stack"] = json!("under");
    let plan = plan(pattern(100.), o).unwrap();
    let a = plan.seams[0].assembly.as_ref().unwrap();
    assert_eq!(
        a.layers
            .iter()
            .map(|l| l.instance.as_str())
            .collect::<Vec<_>>(),
        ["back", "front", "back"]
    );
    assert_eq!(a.side_layers, [1, 0]);
    assert_eq!(
        a.layers
            .iter()
            .map(|l| l.public_face_up)
            .collect::<Vec<_>>(),
        [true, true, false]
    );
    close(a.layers[0].source_stations_mm[0].x, 0.);
    close(a.stack_mm, 0.8);
}

#[test]
fn multiple_moving_layers_reverse_as_a_stack_and_noop_fold_preserves_needle() {
    let mut p = pattern(100.);
    let mut extra = piece("interlining", 100.);
    extra["fabric"] = fabric("interlining", 0.1);
    p["pieces"].as_array_mut().unwrap().push(extra);
    let mut o = options(true);
    o["passes"][0]["assembly"]["pieces"]
        .as_array_mut()
        .unwrap()
        .push(placement("interlining", 40., 0., true));
    let a = plan(p.clone(), o.clone()).unwrap();
    let aa = a.seams[0].assembly.as_ref().unwrap();
    assert_eq!(
        aa.layers
            .iter()
            .map(|l| l.instance.as_str())
            .collect::<Vec<_>>(),
        ["front", "back", "interlining", "interlining", "back"]
    );
    close(aa.stack_mm, 1.0);
    o["passes"][0]["assembly"]["folds"]
        .as_array_mut()
        .unwrap()
        .push(json!({"line_mm":[[1000,0],[1000,10]],"moving_side":"right","stack":"over"}));
    let b = plan(p, o).unwrap();
    let ba = b.seams[0].assembly.as_ref().unwrap();
    assert_eq!(aa.needle_mm, ba.needle_mm);
    assert_eq!(aa.layers.len(), ba.layers.len());
    close(aa.stack_mm, ba.stack_mm);
}

#[test]
fn additional_layer_uses_its_own_evidence_and_reported_thickness_bound() {
    let mut p = pattern(100.);
    let mut extra = piece("interlining", 100.);
    extra["fabric"] = fabric("interlining", 0.1);
    extra["fabric"]["material"]["record"]["properties"]["thickness"]["bounds"] = json!([0.05, 0.3]);
    p["pieces"].as_array_mut().unwrap().push(extra);
    let mut o = options(false);
    o["passes"][0]["assembly"]["pieces"]
        .as_array_mut()
        .unwrap()
        .push(placement("interlining", 0., 0., false));
    let a = plan(p.clone(), o.clone()).unwrap();
    let aa = a.seams[0].assembly.as_ref().unwrap();
    assert_eq!(aa.layers.len(), 3);
    close(aa.stack_mm, 0.9);
    assert!(aa.layers[2].uses_thickness_upper_bound);
    assert_eq!(a.materials.len(), 3);
    p["pieces"][2]["fabric"]["material"]["allow_estimates"] = json!(false);
    assert!(plan(p, o).is_err());
}

#[test]
fn registration_is_checked_not_fitted_and_contradictory_ply_policies_refuse() {
    let mut o = options(false);
    o["passes"][0]["assembly"]["pieces"][1]["origin_mm"] = json!([21, 0]);
    assert!(
        plan(pattern(100.), o)
            .unwrap_err()
            .message
            .contains("do not coincide")
    );
    let mut o = options(false);
    o["passes"][0]["plies"] = json!([1, 1]);
    assert!(plan(pattern(100.), o).is_err());
    let mut o = options(false);
    o["passes"][0]["assembly"]["pieces"][1]["x_axis"] = json!([-2, 0]);
    assert!(
        plan(pattern(100.), o)
            .unwrap_err()
            .message
            .contains("unit x_axis")
    );
    let mut o = options(false);
    o["passes"][0]["assembly"]["pieces"]
        .as_array_mut()
        .unwrap()
        .pop();
    assert!(
        plan(pattern(100.), o)
            .unwrap_err()
            .message
            .contains("missing from declared assembly")
    );
}

#[test]
fn needle_targets_on_a_crease_or_path_crossing_a_crease_refuse() {
    let mut o = options(false);
    o["passes"][0]["assembly"]["folds"] = json!([
        {"line_mm":[[20,-20],[20,120]],"moving_side":"right","stack":"over"}]);
    assert!(
        plan(pattern(100.), o)
            .unwrap_err()
            .message
            .contains("ambiguous fold crease")
    );
    let mut o = options(false);
    o["passes"][0]["assembly"]["folds"] = json!([
        {"line_mm":[[0,51],[30,51]],"moving_side":"left","stack":"over"}]);
    assert!(
        plan(pattern(100.), o)
            .unwrap_err()
            .message
            .contains("changes fold region")
    );
}

#[test]
fn a_small_extra_ply_between_stations_cannot_escape_stack_checks() {
    let mut p = pattern(10.);
    let mut extra = piece("island", 0.5);
    extra["sa"] = json!(0);
    p["pieces"].as_array_mut().unwrap().push(extra);
    let mut o = options(false);
    o["passes"][0]["assembly"]["pieces"]
        .as_array_mut()
        .unwrap()
        .push(placement("island", 10., 1., false));
    let e = plan(p, o).unwrap_err();
    assert!(
        e.message.contains("extra material ply between stations"),
        "{e:?}"
    );
}

#[test]
fn folded_concave_cut_masks_do_not_create_phantom_fabric_across_a_gap() {
    let mut p = pattern(10.);
    // Gap at x>5, 40<y<60. Clipping the right arm by x=10 disconnects the source cloth.
    p["pieces"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"concave","cut":{"qty":1},"sa":0,
        "vertices":{"a":[0,0],"b":[20,0],"c":[20,40],"d":[5,40],
            "e":[5,60],"f":[20,60],"g":[20,100],"h":[0,100]},
        "edges":[{"id":"a","from":"a","to":"b"},{"id":"b","from":"b","to":"c"},
            {"id":"c","from":"c","to":"d"},{"id":"d","from":"d","to":"e"},
            {"id":"e","from":"e","to":"f"},{"id":"f","from":"f","to":"g"},
            {"id":"g","from":"g","to":"h"},{"id":"h","from":"h","to":"a"}]}));
    let mut o = options(false);
    o["machine"]["max_stack_mm"] = json!(3);
    o["passes"][0]["assembly"]["pieces"] = json!([
        placement("front", -2., 45., false),
        placement("back", 18., 45., true),
        placement("concave", 10., 0., false)
    ]);
    o["passes"][0]["assembly"]["folds"] = json!([
        {"line_mm":[[20,-20],[20,120]],"moving_side":"right","stack":"over"}]);
    let a = plan(p, o).unwrap();
    let aa = a.seams[0].assembly.as_ref().unwrap();
    assert_eq!(aa.layers.len(), 4);
    assert!(aa.layers.iter().all(|l| l.instance != "concave"));
    close(aa.stack_mm, 1.2);
}

#[test]
fn entire_needle_chord_must_clear_a_concave_layer_not_just_its_endpoints() {
    let mut p = pattern(10.);
    p["pieces"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"notched","cut":{"qty":1},"sa":0,
        "vertices":{"a":[0,-10],"b":[40,-10],"c":[40,1],"d":[19,1],
            "e":[19,1.5],"f":[40,1.5],"g":[40,20],"h":[0,20]},
        "edges":[{"id":"a","from":"a","to":"b"},{"id":"b","from":"b","to":"c"},
            {"id":"c","from":"c","to":"d"},{"id":"d","from":"d","to":"e"},
            {"id":"e","from":"e","to":"f"},{"id":"f","from":"f","to":"g"},
            {"id":"g","from":"g","to":"h"},{"id":"h","from":"h","to":"a"}]}));
    let mut o = options(false);
    o["passes"][0]["assembly"]["pieces"]
        .as_array_mut()
        .unwrap()
        .push(placement("notched", 0., 0., false));
    assert!(plan(p, o).unwrap_err().message.contains("clearance"));
}

#[test]
fn reverse_and_backtacks_expand_in_the_same_shared_assembly_frame() {
    let p = Pattern::new(pattern(100.));
    let mut o = options(true);
    o["machine"]["reverse"] =
        json!({"stitch_mm":[0.1,5],"feed_b_over_a":[0.5,2],"max_stitches_per_minute":1000});
    o["passes"][0]["direction"] = json!("reverse");
    o["passes"][0]["backtack"] = json!({"start_stitches":3,"end_stitches":2});
    let prog = SewProgram::build(&p, &serde_json::from_value(o).unwrap()).unwrap();
    assert_eq!(prog.schema, "ftx-sewing-program/3");
    assert_eq!(prog.plan.schema, "ftx-sewing-plan/4");
    assert_eq!(prog.stitches.len(), 50);
    let a = prog.plan.seams[0].assembly.as_ref().unwrap();
    for step in &prog.stitches {
        assert_eq!(step.assembly_mm, Some(a.needle_mm[step.to_station]));
    }
}

#[test]
fn fold_job_budgets_internal_obstacles_and_duplicate_instances_refuse() {
    let mut o = options(false);
    o["passes"][0]["assembly"]["folds"] = json!(vec![
        json!({"line_mm":[[0,0],[0,1]],"moving_side":"left","stack":"over"});
        17
    ]);
    assert!(
        plan(pattern(100.), o)
            .unwrap_err()
            .message
            .contains("at most 16")
    );
    let mut o = options(false);
    o["passes"][0]["assembly"]["pieces"]
        .as_array_mut()
        .unwrap()
        .push(placement("front", 0., 0., false));
    assert!(
        plan(pattern(100.), o)
            .unwrap_err()
            .message
            .contains("duplicate assembly instance")
    );
    let mut p = pattern(100.);
    p["pieces"][0]["drills"] = json!([{"at":[10,10]}]);
    assert!(plan(p, options(false)).is_err());
}

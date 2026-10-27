//! Synthetic planar known answers, not calibrated clamp, foot, grip or machine observations.

use ftx_core::RefusalCode;
use ftx_export::sewing::{SewOptions, SewPlan, production::workholding, program::SewProgram};
use ftx_pattern::Pattern;
use serde_json::{Value, json};

fn material(id: &str, thickness: f64) -> Value {
    json!({"material":{"id":id,"allow_estimates":true,"record":{
        "id":id,"name":"Synthetic planar access sheet","form":"woven",
        "properties":{"thickness":{"value":thickness,"unit":"mm","evidence":"estimate",
            "source":"t","method":"Invented software-test thickness"}},
        "sources":{"t":{"reference":"test:planar-access","accessed_on":"2026-10-27",
            "note":"Synthetic geometry fixture; no physical measurement"}}
    }}})
}

fn piece(id: &str, height: f64) -> Value {
    json!({"id":id,"cut":{"qty":1},"grain":[[0,0],[0,10]],
        "vertices":{"a":[0,0],"b":[20,0],"c":[20,height],"d":[0,height]},
        "edges":[{"id":"bottom","from":"a","to":"b"},
            {"id":"right","from":"b","to":"c"},
            {"id":"top","from":"c","to":"d"},
            {"id":"left","from":"d","to":"a"}]})
}

fn build(pattern: &Value, options: &Value) -> Result<SewPlan, ftx_core::Refusal> {
    SewPlan::build(
        &Pattern::new(pattern.clone()),
        &serde_json::from_value::<SewOptions>(options.clone()).unwrap(),
    )
}

fn rectangle(x0: f64, y0: f64, x1: f64, y1: f64) -> Value {
    json!([[x0, y0], [x1, y0], [x1, y1], [x0, y1]])
}

fn qualify(pattern: &Value, options: &mut Value) {
    let mut source = options.clone();
    source.as_object_mut().unwrap().remove("qualifications");
    source
        .as_object_mut()
        .unwrap()
        .remove("require_qualification");
    source["passes"][0]
        .as_object_mut()
        .unwrap()
        .remove("access");
    let plan = build(pattern, &source).unwrap();
    let assembly = plan.seams[0].assembly.as_ref().unwrap();
    options["qualifications"] = json!([{
        "id":"qualified-assembly",
        "materials":assembly.layers.iter().map(|l| &l.material_sha256).collect::<Vec<_>>(),
        "machine":"synthetic-access","needle":"test-needle","thread":"test-thread",
        "stitch_mm":[0.01,5.0],"feed_b_over_a":[0.5,2.0],"stack_mm":[0.01,3.0],
        "stitches_per_minute":[1.0,1000.0],"allow_reverse":true,
        "source":{"reference":"test:planar-process","accessed_on":"2026-10-27",
            "note":"Invented process envelope for a software geometry test"},
        "method":"Synthetic complete operation qualification",
        "evidence":"estimate","allow_estimates":true
    }]);
    options["require_qualification"] = json!(true);
}

fn fixture(folded: bool) -> (Value, Value) {
    let mut back = piece("back", 10.0);
    back["fabric"] = material("lining", 0.2);
    let pattern = json!({"schema":"fabric-pattern/1","units":"mm",
        "fabric":material("shell",0.4),"meta":{"sa_mm":10},
        "pieces":[piece("front",10.0),back],
        "seams":[{"id":"side","a":"front/right","b":"back/left","reverse":true}]});
    let mut options = json!({"machine":{"id":"synthetic-access","stitch_mm":[0.01,5],
        "feed_b_over_a":[0.5,2],"max_stack_mm":1.5,"max_stitches_per_minute":1000,
        "reverse":{"stitch_mm":[0.01,5],"feed_b_over_a":[0.5,2],"max_stitches_per_minute":1000}},
        "seam_tolerance_mm":0.01,
        "passes":[{"seam":"side","max_stitch_mm":2.5,"minimum_allowance_mm":5,
            "stitches_per_minute":600,"needle":"test-needle","thread":"test-thread",
            "assembly":{"pieces":[
                {"instance":"front","origin_mm":[0,0],"x_axis":[1,0]},
                {"instance":"back","origin_mm":[if folded {40} else {20},0],
                    "x_axis":[-1,0],"face_down":true}],
                "folds":if folded {json!([{"line_mm":[[30,-20],[30,120]],
                    "moving_side":"right","stack":"over"}])} else {json!([])},
                "registration_tolerance_mm":1e-6,"minimum_boundary_clearance_mm":1.0}}]});
    qualify(&pattern, &mut options);
    let typed: SewOptions = serde_json::from_value(options.clone()).unwrap();
    options["passes"][0]["access"] = json!({
        "model":"planar_disk_sweep/1",
        "machine_sha256":workholding::machine_sha256(&typed.machine).unwrap(),
        "qualifications":[{"id":typed.qualifications[0].id,
            "sha256":workholding::qualification_sha256(&typed.qualifications[0]).unwrap()}],
        "tool_radius_mm":2.0,"minimum_clearance_mm":1.0,
        "fixtures":[{"id":"clamp","outline_mm":rectangle(25.0,-2.0,27.0,12.0)}],
        "source":{"reference":"test:planar-fixture","accessed_on":"2026-10-27",
            "note":"Synthetic tool disk and fixed clamp; no physical access validation"},
        "method":"Invented static planar geometry and applicability",
        "evidence":"estimate","allow_estimates":true
    });
    (pattern, options)
}

#[test]
fn exact_whole_sweep_distance_and_witness_reach_the_stitch_program() {
    let (pattern, options) = fixture(false);
    let plan = build(&pattern, &options).unwrap();
    assert_eq!(plan.schema, "ftx-sewing-plan/14");
    let access = plan.seams[0].access.as_ref().unwrap();
    assert_eq!(access.minimum_centerline_distance_mm, 5.0);
    assert_eq!(access.minimum_clearance_mm, 3.0);
    assert_eq!(access.required_clearance_mm, 1.0);
    assert_eq!(access.checked_segments, 4);
    assert_eq!(access.fixture_count, 1);
    assert_eq!(access.witness.fixture, "clamp");
    assert_eq!(access.witness.needle_segment, [0, 1]);
    assert_eq!(access.witness.fixture_edge, [3, 0]);
    assert_eq!(access.witness.needle_mm.x, 20.0);
    assert_eq!(access.witness.fixture_mm.x, 25.0);
    assert!(!access.physical_access_validated);
    assert!(!plan.hardware_execution_authorized && !plan.physical_seam_quality_validated);
    let program = SewProgram::build(
        &Pattern::new(pattern),
        &serde_json::from_value(options).unwrap(),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(program.plan.seams[0].access.as_ref()).unwrap(),
        serde_json::to_value(access).unwrap()
    );
    assert!(
        program
            .stitches
            .iter()
            .all(|step| step.assembly_mm.is_some())
    );
}

#[test]
fn documented_batch_combines_fixture_inventory_and_actual_planar_clearance() {
    let pattern: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-sewing-pattern.json"
    ))
    .unwrap();
    let options: Value =
        serde_json::from_str(include_str!("../../../examples/sewing-access-options.json")).unwrap();
    let plan = build(&pattern, &options).unwrap();
    assert_eq!(plan.schema, "ftx-sewing-plan/14");
    let report = serde_json::to_value(&plan).unwrap();
    assert_eq!(report["production"]["stitch_intervals"], 240);
    assert_eq!(
        report["production"]["workholding"]["resources"][0]["capacity"],
        2
    );
    for seam in &plan.seams {
        let access = seam.access.as_ref().unwrap();
        assert_eq!(access.minimum_clearance_mm, 3.0);
        assert_eq!(access.checked_segments, 40);
        assert!(!access.physical_access_validated);
    }
    let program = SewProgram::build(
        &Pattern::new(pattern),
        &serde_json::from_value(options).unwrap(),
    )
    .unwrap();
    assert_eq!(program.stitches.len(), 240);
    assert!(program.stitches.iter().all(|stitch| {
        let point = stitch.assembly_mm.unwrap();
        point.x == 20.0 && (0.0..=100.0).contains(&point.y)
    }));
}

#[test]
fn disk_collisions_between_stations_and_containment_all_refuse() {
    let (pattern, options) = fixture(false);
    let cases = [
        (rectangle(21.0, -2.0, 23.0, 12.0), 2.0, "tool sweep"),
        (rectangle(19.9, 1.0, 20.1, 1.5), 0.1, "intersects"),
        (rectangle(19.0, -1.0, 21.0, 11.0), 0.1, "enters"),
    ];
    for (polygon, radius, message) in cases {
        let mut changed = options.clone();
        let access = &mut changed["passes"][0]["access"];
        access["fixtures"][0]["outline_mm"] = polygon;
        access["tool_radius_mm"] = json!(radius);
        access["minimum_clearance_mm"] = json!(0.0);
        let refusal = build(&pattern, &changed).unwrap_err();
        assert!(refusal.message.contains(message), "{}", refusal.message);
        // The underlying registered needle/material path is otherwise admitted.
        changed["passes"][0]
            .as_object_mut()
            .unwrap()
            .remove("access");
        assert!(build(&pattern, &changed).is_ok());
    }
}

#[test]
fn tool_contact_refuses_but_exact_positive_margin_is_inclusive() {
    let (pattern, mut options) = fixture(false);
    options["passes"][0]["access"]["fixtures"][0]["outline_mm"] = rectangle(23.0, -2.0, 25.0, 12.0);
    assert_eq!(
        build(&pattern, &options).unwrap().seams[0]
            .access
            .as_ref()
            .unwrap()
            .minimum_clearance_mm,
        1.0
    );
    options["passes"][0]["access"]["minimum_clearance_mm"] = json!(1.0001);
    assert!(
        build(&pattern, &options)
            .unwrap_err()
            .message
            .contains("tool sweep")
    );
    options["passes"][0]["access"]["minimum_clearance_mm"] = json!(0.0);
    options["passes"][0]["access"]["fixtures"][0]["outline_mm"] = rectangle(22.0, -2.0, 24.0, 12.0);
    assert!(
        build(&pattern, &options)
            .unwrap_err()
            .message
            .contains("tool sweep")
    );
}

#[test]
fn concave_fixtures_winding_and_folded_material_preimages_are_supported() {
    let (pattern, mut options) = fixture(true);
    options["passes"][0]["access"]["fixtures"][0]["outline_mm"] = json!([
        [24, -2],
        [28, -2],
        [28, 12],
        [24, 12],
        [24, 9],
        [26, 9],
        [26, 1],
        [24, 1]
    ]);
    let plan = build(&pattern, &options).unwrap();
    assert_eq!(plan.seams[0].assembly.as_ref().unwrap().layers.len(), 3);
    assert_eq!(
        plan.seams[0].access.as_ref().unwrap().minimum_clearance_mm,
        2.0
    );
    options["passes"][0]["access"]["fixtures"][0]["outline_mm"]
        .as_array_mut()
        .unwrap()
        .reverse();
    assert_eq!(
        build(&pattern, &options).unwrap().seams[0]
            .access
            .as_ref()
            .unwrap()
            .minimum_clearance_mm,
        2.0
    );
}

#[test]
fn translated_assembly_uses_its_shared_frame_and_rechecks_altered_geometry() {
    let (mut pattern, mut options) = fixture(false);
    options["passes"][0]["assembly"]["pieces"][0]["origin_mm"] = json!([100, -40]);
    options["passes"][0]["assembly"]["pieces"][1]["origin_mm"] = json!([120, -40]);
    options["passes"][0]["access"]["fixtures"][0]["outline_mm"] =
        rectangle(125.0, -42.0, 127.0, -28.0);
    let plan = build(&pattern, &options).unwrap();
    assert_eq!(
        plan.seams[0].access.as_ref().unwrap().minimum_clearance_mm,
        3.0
    );
    assert_eq!(
        plan.seams[0].access.as_ref().unwrap().witness.needle_mm.x,
        120.0
    );
    pattern["pieces"][0]["vertices"]["b"] = json!([23, 0]);
    pattern["pieces"][0]["vertices"]["c"] = json!([23, 10]);
    options["passes"][0]["assembly"]["pieces"][1]["origin_mm"] = json!([123, -40]);
    assert!(
        build(&pattern, &options)
            .unwrap_err()
            .message
            .contains("tool sweep")
    );
}

#[test]
fn reverse_backtacks_and_production_reuse_the_same_proven_planar_sweep() {
    let (pattern, mut options) = fixture(false);
    let baseline = build(&pattern, &options).unwrap();
    options["passes"][0]["direction"] = json!("reverse");
    options["passes"][0]["backtack"] = json!({"start_stitches":1,"end_stitches":2});
    options["production"] = json!({"production_copies":3,
        "ordering":{"objective":"minimize_setup_changes","max_search_nodes":100}});
    let program = SewProgram::build(
        &Pattern::new(pattern),
        &serde_json::from_value(options).unwrap(),
    )
    .unwrap();
    assert_eq!(program.stitches.len(), 30);
    assert_eq!(
        serde_json::to_value(&program.plan.seams[0].access).unwrap(),
        serde_json::to_value(&baseline.seams[0].access).unwrap()
    );
    assert_eq!(
        program.plan.production.as_ref().unwrap().operations.len(),
        3
    );
}

#[test]
fn source_locks_follow_machine_process_and_material_changes() {
    let (pattern, options) = fixture(false);
    let mut machine = options.clone();
    machine["machine"]["max_stack_mm"] = json!(2.0);
    assert!(
        build(&pattern, &machine)
            .unwrap_err()
            .message
            .contains("machine content lock")
    );
    let mut process = options.clone();
    process["qualifications"][0]["source"]["note"] = json!("Changed synthetic acquisition");
    assert!(
        build(&pattern, &process)
            .unwrap_err()
            .message
            .contains("exact admitted process")
    );
    let mut swapped = pattern.clone();
    swapped["fabric"] = material("different-shell", 0.4);
    let mut requalified = options.clone();
    qualify(&swapped, &mut requalified);
    assert!(
        build(&swapped, &requalified)
            .unwrap_err()
            .message
            .contains("exact admitted process")
    );
    let mut missing = options.clone();
    missing["qualifications"] = json!([]);
    missing["require_qualification"] = json!(false);
    assert!(
        build(&pattern, &missing)
            .unwrap_err()
            .message
            .contains("admitted source process")
    );
}

#[test]
fn access_needs_explicit_frame_independent_estimate_consent_and_valid_polygons() {
    let (pattern, options) = fixture(false);
    let mut legacy = options.clone();
    legacy["passes"][0]
        .as_object_mut()
        .unwrap()
        .remove("assembly");
    legacy["passes"][0]["plies"] = json!([1, 1]);
    assert!(
        build(&pattern, &legacy)
            .unwrap_err()
            .message
            .contains("explicit flat assembly")
    );
    let mut estimates = options.clone();
    estimates["passes"][0]["access"]["allow_estimates"] = json!(false);
    assert!(
        build(&pattern, &estimates)
            .unwrap_err()
            .message
            .contains("own explicit consent")
    );
    for polygon in [
        json!([[24, 0], [28, 4], [24, 4], [28, 0]]),
        json!([[24, 0], [28, 0], [24, 0]]),
    ] {
        let mut malformed = options.clone();
        malformed["passes"][0]["access"]["fixtures"][0]["outline_mm"] = polygon;
        assert!(
            build(&pattern, &malformed)
                .unwrap_err()
                .message
                .contains("simple polygons")
        );
    }
    let mut extra = options.clone();
    let fixture_rows = vec![options["passes"][0]["access"]["fixtures"][0].clone(); 65];
    extra["passes"][0]["access"]["fixtures"] = json!(fixture_rows);
    assert_eq!(
        build(&pattern, &extra).unwrap_err().code,
        RefusalCode::BudgetExceeded
    );
}

#[test]
fn source_geometry_work_is_bounded_across_all_operations_before_path_expansion() {
    let (pattern, mut options) = fixture(false);
    let ring: Vec<_> = (0..1024)
        .map(|i| {
            let angle = i as f64 * std::f64::consts::TAU / 1024.0;
            [1000.0 + 50.0 * angle.cos(), 50.0 * angle.sin()]
        })
        .collect();
    options["passes"][0]["access"]["fixtures"][0]["outline_mm"] = json!(ring);
    let original = options["passes"][0].clone();
    options["passes"] = json!(
        (0..51)
            .map(|i| {
                let mut pass = original.clone();
                pass["id"] = json!(format!("operation-{i}"));
                pass
            })
            .collect::<Vec<_>>()
    );
    let refusal = build(&pattern, &options).unwrap_err();
    assert_eq!(refusal.code, RefusalCode::BudgetExceeded);
    assert!(refusal.message.contains("edge-work budget"));
}

#[test]
fn omitted_policy_keeps_the_existing_qualified_plan_shape() {
    let (pattern, mut options) = fixture(false);
    options["passes"][0]
        .as_object_mut()
        .unwrap()
        .remove("access");
    let plan = build(&pattern, &options).unwrap();
    assert_eq!(plan.schema, "ftx-sewing-plan/5");
    let value = serde_json::to_value(plan).unwrap();
    assert!(value["seams"][0].get("access").is_none());
    assert!(value["seams"][0]["pass"].get("access").is_none());
    assert!(value["options"]["passes"][0].get("access").is_none());
}

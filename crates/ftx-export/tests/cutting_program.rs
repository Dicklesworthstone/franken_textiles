//! Synthetic geometric/process known answers; these values do not qualify a physical cutter.

use ftx_core::{Vec2, receipt::sha256_hex};
use ftx_export::cutting::{
    CutOptions, CutPlan,
    program::{self, Machine, Ordering, Policy},
};
use ftx_pattern::Pattern;
use serde_json::{Value, json};

fn piece(id: &str, width: f64, height: f64) -> Value {
    json!({"id":id,"cut":{"qty":1},"grain":[[0,0],[0,10]],
        "vertices":{"a":[0,0],"b":[width,0],"c":[width,height],"d":[0,height]},
        "edges":[{"id":"bottom","from":"a","to":"b"},{"id":"right","from":"b","to":"c"},
            {"id":"top","from":"c","to":"d"},{"id":"left","from":"d","to":"a"}]})
}

fn pattern(pieces: Vec<Value>) -> Pattern {
    Pattern::new(
        json!({"schema":"fabric-pattern/1","units":"mm","meta":{"sa_mm":0},
        "fabric":{"material":{"id":"test-sheet","allow_estimates":true,"record":{
            "id":"test-sheet","name":"Synthetic knife test sheet","form":"woven",
            "properties":{
                "width":{"value":0.12,"unit":"m","source":"test","evidence":"estimate","method":"Synthetic software case"},
                "thickness":{"value":1.0,"unit":"mm","bounds":[0.8,1.2],"source":"test","evidence":"estimate","method":"Synthetic software case"}
            },"sources":{"test":{"reference":"test:knife-sheet","accessed_on":"2026-10-30","note":"Invented values; no measured cutter or textile claim"}}
        }}},"pieces":pieces,"seams":[]}),
    )
}

fn options() -> CutOptions {
    serde_json::from_value(json!({"copies":1,"gap_mm":5,"edge_margin_mm":5,"end_margin_mm":5}))
        .unwrap()
}

fn machine() -> Machine {
    serde_json::from_value(
        json!({"id":"synthetic-table","blade":"synthetic-cold-knife",
        "bed_mm":[500,500],"maximum_thickness_mm":2,"maximum_feed_mm_s":100,
        "travel_mm_s":100,"lift_lower_seconds":0.25,"start_mm":[0,0],"return_to_start":true}),
    )
    .unwrap()
}

fn policy(plan: &CutPlan, machine: Machine) -> Policy {
    let lock = program::machine_sha256(&machine).unwrap();
    let qualifications: Vec<_> = plan.rolls.iter().enumerate().filter(|(i,r)| {
        !plan.rolls[..*i].iter().any(|old| old.material.sha256() == r.material.sha256())
    }).map(|(i,r)| json!({
        "id":format!("synthetic-recipe-{i}"),"material_sha256":r.material.sha256(),"machine_sha256":lock,
        "thickness_mm":[0.5,1.5],"feed_mm_s":20,"directions":"all_in_plane","evidence":"estimate","allow_estimates":true,
        "source":{"reference":"test:knife-process","accessed_on":"2026-10-30","note":"Synthetic timing and applicability; no measured process"},
        "method":"Invented single-ply constant-speed geometry/timing test, complete all-direction cut and lift/travel phases"
    })).collect();
    serde_json::from_value(
        json!({"model":"qualified_cold_knife_paths/1","machine":machine,
        "qualifications":qualifications,"ordering":"authored","work_limit":10000}),
    )
    .unwrap()
}

fn qualified_options(p: &Pattern) -> CutOptions {
    let mut options = options();
    let baseline = CutPlan::build(p, &options).unwrap();
    options.program = Some(policy(&baseline, machine()));
    options
}

fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-10, "{a} != {b}");
}
fn value<T: serde::Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap()
}
fn process_refusal(pattern: &Pattern, options: &CutOptions) -> ftx_core::Refusal {
    let plan = CutPlan::build(pattern, options).unwrap();
    assert!(
        plan.program.is_none(),
        "expected independent process refusal, got {:?}",
        plan.program
    );
    let mut ordinary_options = options.clone();
    ordinary_options.program = None;
    let ordinary = CutPlan::build(pattern, &ordinary_options).unwrap();
    assert_eq!(value(&plan.rolls), value(&ordinary.rolls));
    assert_eq!(plan.pattern_sha256, ordinary.pattern_sha256);
    plan.program_refusal.expect("independent process refusal")
}

fn fixture_policy(opts: &mut CutOptions, corners: [[f64; 2]; 4]) {
    let policy = opts.program.as_mut().unwrap();
    policy.machine.fixture_clearance = Some(serde_json::from_value(json!({
        "model":"planar_disk_sweep/1","cutting_radius_mm":0.2,"raised_radius_mm":0.2,
        "fixtures":[{"id":"declared-clamp","outline_mm":corners}],
        "source":{"reference":"test:knife-fixtures","accessed_on":"2026-10-30",
            "note":"Synthetic stationary envelopes at both elevations; no physical machine claim"},
        "method":"Independently authored machine-frame polygon and full circular tool envelopes",
        "evidence":"estimate","allow_estimates":true
    })).unwrap());
    relock(opts);
}

fn relock(opts: &mut CutOptions) {
    let policy = opts.program.as_mut().unwrap();
    let lock = program::machine_sha256(&policy.machine).unwrap();
    for q in &mut policy.qualifications {
        q.machine_sha256.clone_from(&lock);
    }
}

fn motion_profile() -> Value {
    json!({"model":"trapezoidal_exact_stop/1", "cutting_acceleration_mm_s2":40.,
        "travel_acceleration_mm_s2":25.})
}

fn set_motion_profile(opts: &mut CutOptions, profile: Value) {
    let machine = &mut opts.program.as_mut().unwrap().machine;
    let mut raw = value(machine);
    raw["motion_profile"] = profile;
    *machine = serde_json::from_value(raw).unwrap();
}

fn qualify_drilling(opts: &mut CutOptions) {
    let mut raw = value(opts);
    raw["program"]["machine"]["drilling"] = json!({
        "model":"coaxial_cold_through_drill/1","tool":"synthetic-2mm-bit",
        "diameter_mm":2.,"maximum_thickness_mm":2.,
        "select_drill_seconds":0.6,"select_knife_seconds":0.4
    });
    for q in raw["program"]["qualifications"].as_array_mut().unwrap() {
        q["drilling"] = json!({"id":format!("{}-drill",q["id"].as_str().unwrap()),
            "thickness_mm":[0.5,1.5],"plunge_seconds":0.3,"dwell_seconds":0.2,
            "retract_seconds":0.5,"evidence":"estimate","allow_estimates":true,
            "source":{"reference":"test:cold-drill","accessed_on":"2026-11-01",
                "note":"Synthetic coaxial tool and one-second through-hole cycle, not measured cutting quality"},
            "method":"Independently declared plunge, dwell, retract and complete clearance for this material/tool",
            "conditions":{"specimen":"synthetic-single-ply"}});
    }
    *opts = serde_json::from_value(raw).unwrap();
    relock(opts);
}

fn routed_fixture() -> (Pattern, CutOptions) {
    let p = pattern(vec![piece("front", 40., 60.)]);
    let mut opts = qualified_options(&p);
    opts.program.as_mut().unwrap().machine.start_mm = Vec2::new(150., 30.);
    fixture_policy(
        &mut opts,
        [[100., 0.], [110., 0.], [110., 90.], [100., 90.]],
    );
    opts.program.as_mut().unwrap().travel_routing = Some(program::TravelRouting {
        model: "fixture_box_visibility/1".into(),
        waypoint_offset_mm: 1.,
    });
    (p, opts)
}

#[test]
fn explicit_stock_loading_enables_a_blocked_cut_and_retains_complete_machine_motion() {
    let p = pattern(vec![piece("front", 40., 60.)]);
    let mut opts = qualified_options(&p);
    let origin = CutPlan::build(&p, &opts).unwrap();
    fixture_policy(
        &mut opts,
        [[44.8, 34.], [45.2, 34.], [45.2, 36.], [44.8, 36.]],
    );
    assert!(
        process_refusal(&p, &opts)
            .message
            .contains("cutting tool envelope")
    );
    let lock = program::machine_sha256(&opts.program.as_ref().unwrap().machine).unwrap();
    opts.program.as_mut().unwrap().stock_loading = vec![program::StockLoading {
        marker: 0,
        lower_left_mm: Vec2::new(200., 100.),
    }];
    let plan = CutPlan::build(&p, &opts).unwrap();
    assert_eq!(value(&plan.rolls), value(&origin.rolls));
    assert_eq!(value(&plan), value(&CutPlan::build(&p, &opts).unwrap()));
    let report = plan.program.as_ref().unwrap();
    assert_eq!(report.machine_sha256, lock);
    assert_eq!(
        report.stock_loading,
        opts.program.as_ref().unwrap().stock_loading
    );
    let marker = &report.markers[0];
    assert_ne!(
        marker.geometry_sha256,
        origin.program.as_ref().unwrap().markers[0].geometry_sha256
    );
    assert_eq!(
        value(&marker.loading),
        json!({"lower_left_mm":[200.,100.],
        "marker_to_machine_offset_mm":[200.,100.], "stock_bounds_mm":[200.,100.,320.,170.]})
    );
    let route = &marker.selected;
    let end = Vec2::new(245., 105.);
    assert_eq!(route.runs[0].cut_endpoints_mm, [end; 2]);
    assert_eq!(route.runs[0].travel_mm, [Vec2::ZERO, end]);
    assert_eq!(route.return_mm, Some([end, Vec2::ZERO]));
    near(route.cut_length_mm, 200.);
    near(route.travel_length_mm, 2. * end.norm());
    near(route.modeled_path_seconds, 10.25 + 2. * end.norm() / 100.);
    let svg = ftx_export::cutting::preview::svg(&plan, 0).unwrap();
    assert!(svg.contains("viewBox=\"0 0 500 500\""));
    assert!(svg.contains("id=\"stock-loading\" transform=\"translate(200 100)\""));
    assert!(
        svg.contains(
            "id=\"loaded-stock-envelope\" x=\"200\" y=\"100\" width=\"120\" height=\"70\""
        )
    );
    assert!(svg.contains("data-phase=\"raised-travel\" d=\"M0 0 L245 105 \""));
    assert!(svg.contains("data-phase=\"raised-return\""));
    // The additional loading/frame checks share the geometric budget and cannot be skipped.
    let mut short = opts.clone();
    short.program.as_mut().unwrap().geometry_work_limit = report.geometry_work - 1;
    assert_eq!(
        process_refusal(&p, &short).code,
        ftx_core::RefusalCode::BudgetExceeded
    );
    let mut changed = opts;
    changed.program.as_mut().unwrap().qualifications[0].material_sha256 = "0".repeat(64);
    assert!(
        process_refusal(&p, &changed)
            .message
            .contains("qualification")
    );
}

#[test]
fn stock_loading_requires_unique_existing_markers_and_the_entire_source_width_on_bed() {
    let p = pattern(vec![piece("front", 40., 60.)]);
    let mut raw = p.raw().clone();
    raw["fabric"]["material"]["record"]["properties"]["width"]["bounds"] = json!([0.11, 0.13]);
    let p = Pattern::new(raw);
    let mut opts = qualified_options(&p);
    opts.width_basis = ftx_export::cutting::WidthBasis::ReportedLowerBound;
    let origin = CutPlan::build(&p, &opts).unwrap();
    assert!(value(&origin.program).get("stock_loading").is_none());
    let mut empty = value(&opts);
    empty["program"]["stock_loading"] = json!([]);
    let empty: CutOptions = serde_json::from_value(empty).unwrap();
    assert_eq!(value(&origin), value(&CutPlan::build(&p, &empty).unwrap()));
    assert_eq!(
        ftx_export::cutting::preview::svg(&origin, 0).unwrap(),
        ftx_export::cutting::preview::svg(&CutPlan::build(&p, &empty).unwrap(), 0).unwrap()
    );
    let item = program::StockLoading {
        marker: 0,
        lower_left_mm: Vec2::new(375., 0.),
    };
    opts.program.as_mut().unwrap().stock_loading = vec![item.clone()];
    // The 110 mm layout and nominal 120 mm roll fit; the full upper source width of 130 mm does not.
    assert!(
        process_refusal(&p, &opts)
            .message
            .contains("complete roll marker")
    );
    opts.program.as_mut().unwrap().stock_loading[0]
        .lower_left_mm
        .x = 370.;
    let loaded = CutPlan::build(&p, &opts).unwrap();
    let loading = loaded.program.as_ref().unwrap().markers[0]
        .loading
        .as_ref()
        .unwrap();
    assert_eq!(loading.stock_bounds_mm, [370., 0., 500., 70.]);
    assert!(
        ftx_export::cutting::preview::svg(&loaded, 0)
            .unwrap()
            .contains("width=\"130\" height=\"70\"")
    );
    for entries in [
        vec![item.clone(), item.clone()],
        vec![program::StockLoading {
            marker: 1,
            ..item.clone()
        }],
        vec![program::StockLoading {
            lower_left_mm: Vec2::new(-1., 0.),
            ..item.clone()
        }],
        vec![program::StockLoading {
            lower_left_mm: Vec2::new(f64::NAN, 0.),
            ..item.clone()
        }],
        vec![item; 4097],
    ] {
        opts.program.as_mut().unwrap().stock_loading = entries;
        assert!(process_refusal(&p, &opts).message.contains("stock loading"));
    }
}

#[test]
fn translated_stock_and_fixture_detours_use_one_machine_frame() {
    let (p, mut opts) = routed_fixture();
    opts.program.as_mut().unwrap().stock_loading = vec![program::StockLoading {
        marker: 0,
        lower_left_mm: Vec2::new(0., 10.),
    }];
    let plan = CutPlan::build(&p, &opts).unwrap();
    let route = &plan.program.as_ref().unwrap().markers[0].selected;
    assert_eq!(route.runs[0].cut_endpoints_mm, [Vec2::new(45., 15.); 2]);
    assert_eq!(route.runs[0].travel_waypoints_mm.len(), 2);
    near(route.runs[0].travel_waypoints_mm[0].y, 91.2);
    near(route.return_waypoints_mm[0].y, 91.2);
    near(
        route.travel_length_mm,
        route.checked_travel_length().unwrap(),
    );
    let svg = ftx_export::cutting::preview::svg(&plan, 0).unwrap();
    assert!(svg.contains("id=\"stock-loading\" transform=\"translate(0 10)\""));
    assert!(svg.contains("L111.2 91.2 L98.8 91.2 L45 15"));
}

#[test]
fn checked_detours_enable_a_blocked_job_and_retain_the_return_and_full_bed_preview() {
    let (p, opts) = routed_fixture();
    let mut straight = opts.clone();
    straight.program.as_mut().unwrap().travel_routing = None;
    assert!(
        process_refusal(&p, &straight)
            .message
            .contains("raised-travel")
    );
    let plan = CutPlan::build(&p, &opts).unwrap();
    let report = plan.program.as_ref().unwrap();
    assert_eq!(
        report.machine_sha256,
        straight.program.as_ref().unwrap().qualifications[0].machine_sha256
    );
    assert_eq!(
        value(&report.travel_routing),
        value(&opts.program.as_ref().unwrap().travel_routing)
    );
    assert_eq!(value(&plan), value(&CutPlan::build(&p, &opts).unwrap()));
    let route = &report.markers[0].selected;
    assert_eq!(route.runs.len(), 1);
    let run = &route.runs[0];
    assert_eq!(run.travel_mm, [Vec2::new(150., 30.), Vec2::new(45., 5.)]);
    assert_eq!(run.cut_endpoints_mm, [Vec2::new(45., 5.); 2]);
    let known = [Vec2::new(111.2, 91.2), Vec2::new(98.8, 91.2)];
    assert_eq!(run.travel_waypoints_mm.len(), known.len());
    assert_eq!(route.return_waypoints_mm.len(), known.len());
    for (got, expected) in run.travel_waypoints_mm.iter().zip(known) {
        near(got.dist(expected), 0.);
    }
    for (got, expected) in route
        .return_waypoints_mm
        .iter()
        .zip(known.into_iter().rev())
    {
        near(got.dist(expected), 0.);
    }
    // The bottom corners are outside the bed. Both upper corners are needed to clear this wall.
    let one_way = Vec2::new(150., 30.).dist(known[0]) + 12.4 + known[1].dist(Vec2::new(45., 5.));
    near(route.cut_length_mm, 200.);
    near(route.travel_length_mm, 2. * one_way);
    near(route.checked_travel_length().unwrap(), 2. * one_way);
    near(route.modeled_path_seconds, 10.25 + 2. * one_way / 100.);
    assert!(known[0].y > plan.rolls[0].length_m * 1000.);
    let svg = ftx_export::cutting::preview::svg(&plan, 0).unwrap();
    assert!(svg.contains("viewBox=\"0 0 500 500\""));
    assert!(svg.contains("L111.2 91.2 L98.8 91.2"));
    assert!(svg.contains("data-phase=\"raised-return\""));
    assert!(svg.contains("data-fixture=\"declared-clamp\""));
    assert!(
        !report.optimality_proven
            && !report.physical_cutting_validated
            && !report.hardware_execution_authorized
    );
}

#[test]
fn routing_refuses_blocked_cuts_unreachable_graphs_and_incomplete_budgeted_routes() {
    let (p, original) = routed_fixture();
    let complete = CutPlan::build(&p, &original).unwrap();
    let report = complete.program.as_ref().unwrap();
    let mut opts = original.clone();
    opts.program.as_mut().unwrap().work_limit = report.search_work - 1;
    assert_eq!(
        process_refusal(&p, &opts).code,
        ftx_core::RefusalCode::BudgetExceeded
    );
    opts = original.clone();
    opts.program.as_mut().unwrap().geometry_work_limit = report.geometry_work - 1;
    assert_eq!(
        process_refusal(&p, &opts).code,
        ftx_core::RefusalCode::BudgetExceeded
    );
    opts = original.clone();
    opts.program.as_mut().unwrap().work_limit = 1;
    assert_eq!(
        process_refusal(&p, &opts).code,
        ftx_core::RefusalCode::BudgetExceeded
    );
    opts = original.clone();
    fixture_policy(
        &mut opts,
        [[100., -10.], [110., -10.], [110., 510.], [100., 510.]],
    );
    assert!(
        process_refusal(&p, &opts)
            .message
            .contains("no complete checked raised-travel route")
    );
    opts = original.clone();
    fixture_policy(&mut opts, [[20., 4.], [25., 4.], [25., 6.], [20., 6.]]);
    assert!(
        process_refusal(&p, &opts)
            .message
            .contains("cutting tool envelope")
    );
    // The authored traversal is complete. Only the optional ordering candidate exhausts search.
    opts = original;
    let policy = opts.program.as_mut().unwrap();
    policy.ordering = Ordering::NearestPath;
    policy.work_limit = report.search_work + 1;
    let bounded = CutPlan::build(&p, &opts).unwrap();
    let bounded = bounded.program.as_ref().unwrap();
    assert!(bounded.budget_exhausted);
    assert_eq!(
        value(&bounded.markers[0].selected),
        value(&report.markers[0].selected)
    );
    assert_eq!(
        bounded.markers[0]
            .candidate_clearance_refusal
            .as_ref()
            .unwrap()
            .code,
        ftx_core::RefusalCode::BudgetExceeded
    );
}

#[test]
fn fixture_clearance_checks_complete_cut_and_raised_sweeps_independently() {
    let p = pattern(vec![piece("front", 40., 60.)]);
    let mut opts = qualified_options(&p);
    let original_lock = program::machine_sha256(&opts.program.as_ref().unwrap().machine).unwrap();
    assert!(
        value(&opts.program.as_ref().unwrap().machine)
            .get("fixture_clearance")
            .is_none()
    );
    // Both vertices of the bottom cut are outside this clamp; the complete chord crosses it.
    fixture_policy(&mut opts, [[24., 4.], [26., 4.], [26., 6.], [24., 6.]]);
    assert!(
        process_refusal(&p, &opts)
            .message
            .contains("cutting tool envelope")
    );
    // Every cutting path clears this clamp, but the initial raised motion crosses it.
    fixture_policy(&mut opts, [[20., 2.], [25., 2.], [25., 3.], [20., 3.]]);
    assert!(
        process_refusal(&p, &opts)
            .message
            .contains("raised-travel tool envelope")
    );
    fixture_policy(
        &mut opts,
        [[200., 200.], [210., 200.], [210., 210.], [200., 210.]],
    );
    let clear = CutPlan::build(&p, &opts).unwrap();
    let report = clear.program.as_ref().unwrap();
    assert_eq!(
        value(&report.fixture_clearance),
        value(&opts.program.as_ref().unwrap().machine.fixture_clearance)
    );
    assert_ne!(report.machine_sha256, original_lock);
    assert_eq!(value(&clear), value(&CutPlan::build(&p, &opts).unwrap()));
    // A new envelope or footprint cannot reuse a formerly qualified machine lock.
    opts.program
        .as_mut()
        .unwrap()
        .machine
        .fixture_clearance
        .as_mut()
        .unwrap()
        .cutting_radius_mm = 0.3;
    assert!(
        process_refusal(&p, &opts)
            .message
            .contains("exact machine/material locks")
    );
}

#[test]
fn fixture_tool_envelopes_include_tangency_stationary_heads_and_internal_cuts() {
    let p = pattern(vec![piece("front", 40., 60.)]);
    let mut opts = qualified_options(&p);
    // The emitted exterior starts at the lower-right corner, so this removes unrelated travel.
    opts.program.as_mut().unwrap().machine.start_mm = Vec2::new(45., 5.);
    fixture_policy(&mut opts, [[20., 2.], [25., 2.], [25., 3.], [20., 3.]]);
    opts.program
        .as_mut()
        .unwrap()
        .machine
        .fixture_clearance
        .as_mut()
        .unwrap()
        .cutting_radius_mm = 2.;
    relock(&mut opts);
    assert!(
        process_refusal(&p, &opts)
            .message
            .contains("cutting tool envelope")
    );
    opts.program
        .as_mut()
        .unwrap()
        .machine
        .fixture_clearance
        .as_mut()
        .unwrap()
        .cutting_radius_mm = 1.9;
    relock(&mut opts);
    assert!(CutPlan::build(&p, &opts).unwrap().program.is_some());

    fixture_policy(&mut opts, [[44., 3.], [46., 3.], [46., 4.], [44., 4.]]);
    opts.program
        .as_mut()
        .unwrap()
        .machine
        .fixture_clearance
        .as_mut()
        .unwrap()
        .raised_radius_mm = 1.;
    relock(&mut opts);
    assert!(
        process_refusal(&p, &opts)
            .message
            .contains("raised-travel tool envelope")
    );

    let mut sheet = piece("front", 40., 60.);
    sheet["internal"] = json!([{"kind":"cut","closed":false,"pts":[[10,10],[30,10]]}]);
    let p = pattern(vec![sheet]);
    opts = qualified_options(&p);
    fixture_policy(&mut opts, [[24., 14.], [26., 14.], [26., 16.], [24., 16.]]);
    assert!(
        process_refusal(&p, &opts)
            .message
            .contains("cutting tool envelope")
    );
}

#[test]
fn blocked_nearest_travel_keeps_the_complete_checked_authored_baseline() {
    let mut sheet = piece("front", 100., 100.);
    sheet["internal"] = json!([
        {"kind":"cut","closed":false,"pts":[[10,10],[11,10]]},
        {"kind":"cut","closed":false,"pts":[[70,10],[71,10]]},
        {"kind":"cut","closed":false,"pts":[[10,70],[11,70]]}
    ]);
    let p = pattern(vec![sheet]);
    let mut opts = qualified_options(&p);
    let policy = opts.program.as_mut().unwrap();
    policy.machine.start_mm = Vec2::new(100., 90.);
    policy.machine.return_to_start = false;
    policy.ordering = Ordering::NearestPath;
    relock(&mut opts);
    assert!(CutPlan::build(&p, &opts).unwrap().program.unwrap().markers[0].improved);
    fixture_policy(&mut opts, [[87., 52.], [88., 52.], [88., 53.], [87., 53.]]);
    let report = CutPlan::build(&p, &opts).unwrap().program.unwrap();
    let marker = &report.markers[0];
    assert!(!marker.improved);
    assert_eq!(value(&marker.selected), value(&marker.baseline));
    assert!(
        marker
            .candidate_clearance_refusal
            .as_ref()
            .unwrap()
            .message
            .contains("declared-clamp")
    );
    assert!(!report.budget_exhausted);
}

#[test]
fn declared_final_return_and_geometry_budgets_cannot_skip_fixture_checks() {
    let p = pattern(vec![piece("a", 20., 20.), piece("b", 20., 20.)]);
    let mut opts = qualified_options(&p);
    opts.program.as_mut().unwrap().machine.start_mm = Vec2::new(0., 50.);
    opts.program.as_mut().unwrap().machine.return_to_start = false;
    fixture_policy(
        &mut opts,
        [[24.9, 27.4], [25.1, 27.4], [25.1, 27.6], [24.9, 27.6]],
    );
    assert!(CutPlan::build(&p, &opts).unwrap().program.is_some());
    opts.program.as_mut().unwrap().machine.return_to_start = true;
    relock(&mut opts);
    assert!(
        process_refusal(&p, &opts)
            .message
            .contains("raised-travel tool envelope")
    );
    fixture_policy(
        &mut opts,
        [[200., 200.], [210., 200.], [210., 210.], [200., 210.]],
    );
    opts.program.as_mut().unwrap().geometry_work_limit = 1;
    assert_eq!(
        process_refusal(&p, &opts).code,
        ftx_core::RefusalCode::BudgetExceeded
    );
}

#[test]
fn fixture_geometry_requires_independent_estimate_consent_and_valid_polygons() {
    let p = pattern(vec![piece("front", 40., 60.)]);
    let mut opts = qualified_options(&p);
    fixture_policy(
        &mut opts,
        [[200., 200.], [210., 200.], [210., 210.], [200., 210.]],
    );
    opts.program
        .as_mut()
        .unwrap()
        .machine
        .fixture_clearance
        .as_mut()
        .unwrap()
        .allow_estimates = false;
    relock(&mut opts);
    assert!(
        process_refusal(&p, &opts)
            .message
            .contains("its own explicit consent")
    );
    fixture_policy(
        &mut opts,
        [[200., 200.], [210., 210.], [200., 210.], [210., 200.]],
    );
    assert!(process_refusal(&p, &opts).message.contains("fixture"));
    fixture_policy(
        &mut opts,
        [[200., 200.], [210., 200.], [210., 210.], [200., 210.]],
    );
    opts.program
        .as_mut()
        .unwrap()
        .machine
        .fixture_clearance
        .as_mut()
        .unwrap()
        .source
        .note
        .clear();
    relock(&mut opts);
    assert!(
        process_refusal(&p, &opts)
            .message
            .contains("source geometry")
    );
}

#[test]
fn complete_authored_paths_have_known_length_and_time_and_release_exteriors_last() {
    let mut sheet = piece("front", 40., 60.);
    sheet["internal"] = json!([
        {"kind":"cut","closed":true,"pts":[[5,5],[15,5],[15,15],[5,15],[5,5]]},
        {"kind":"cut","closed":false,"pts":[[25,30],[30,30]]}
    ]);
    let p = pattern(vec![sheet]);
    let raw = p.raw().clone();
    let options = qualified_options(&p);
    let cut = CutPlan::build(&p, &options).unwrap();
    let report = cut.program.as_ref().unwrap();
    let marker = &report.markers[0];
    let selected = &marker.selected;
    assert_eq!(selected.runs.len(), 3);
    assert_eq!(selected.runs[0].path.internal_cut, Some(0));
    assert_eq!(selected.runs[1].path.internal_cut, Some(1));
    assert_eq!(selected.runs[2].path.internal_cut, None);
    assert_eq!(
        selected.runs[0].cut_endpoints_mm,
        [Vec2::new(10., 10.), Vec2::new(10., 10.)]
    );
    assert_eq!(
        selected.runs[1].cut_endpoints_mm,
        [Vec2::new(30., 35.), Vec2::new(35., 35.)]
    );
    near(selected.cut_length_mm, 245.);
    near(selected.cutting_seconds, 12.25);
    near(selected.lift_lower_seconds, 0.75);
    let outer_start = cut.rolls[0].placements[0].cut_mm[0];
    let travel = Vec2::ZERO.dist(Vec2::new(10., 10.))
        + Vec2::new(10., 10.).dist(Vec2::new(30., 35.))
        + Vec2::new(35., 35.).dist(outer_start)
        + outer_start.dist(Vec2::ZERO);
    near(selected.travel_length_mm, travel);
    near(selected.modeled_path_seconds, 13.0 + travel / 100.);
    assert_eq!(marker.thickness_mm, [0.8, 1.2]);
    assert_eq!(marker.material_sha256, cut.rolls[0].material.sha256());
    assert_eq!(
        marker.geometry_sha256,
        sha256_hex(&serde_json::to_vec(&cut.rolls[0].placements).unwrap())
    );
    assert_eq!(value(&marker.baseline), value(selected));
    assert!(
        !report.hardware_execution_authorized
            && !report.physical_cutting_validated
            && !report.optimality_proven
    );
    assert_eq!(report.search_work, 0);
    assert_eq!(p.raw(), &raw);
    assert_eq!(value(&cut), value(&CutPlan::build(&p, &options).unwrap()));
}

#[test]
fn qualified_drills_finish_before_every_knife_path_with_independent_complete_cycle_times() {
    let mut sheet = piece("front", 40., 60.);
    sheet["drills"] = json!([{"p":[10,10],"d":2},{"p":[30,30],"d":2}]);
    sheet["internal"] = json!([{"kind":"cut","closed":false,"pts":[[15,45],[25,45]]}]);
    let p = pattern(vec![sheet]);
    let ordinary = CutPlan::build(&p, &options()).unwrap();
    let mut opts = qualified_options(&p);
    qualify_drilling(&mut opts);
    let plan = CutPlan::build(&p, &opts).unwrap();
    assert_eq!(value(&plan.rolls), value(&ordinary.rolls));
    assert_eq!(value(&plan), value(&CutPlan::build(&p, &opts).unwrap()));
    let report = plan.program.as_ref().expect("qualified drilling program");
    let report_json = value(report);
    assert_eq!(
        report_json["drilling"],
        value(&opts)["program"]["machine"]["drilling"]
    );
    assert_eq!(
        report_json["markers"][0]["drill_qualification"],
        "synthetic-recipe-0-drill"
    );
    let route = &report.markers[0].selected;
    let drills = report_json["markers"][0]["selected"]["drills"]
        .as_array()
        .unwrap();
    assert_eq!(drills.len(), 2);
    for (i, center) in [[15., 15.], [35., 35.]].into_iter().enumerate() {
        assert_eq!(drills[i]["placement"], 0);
        assert_eq!(drills[i]["drill"], i);
        assert_eq!(drills[i]["piece"], "front");
        assert_eq!(
            drills[i]["instance"],
            ordinary.rolls[0].placements[0].instance
        );
        assert_eq!(drills[i]["copy"], 0);
        assert_eq!(drills[i]["center_mm"], json!(center));
        assert_eq!(drills[i]["diameter_mm"], 2.);
    }
    assert_eq!(drills[0]["travel_mm"], json!([[0., 0.], [15., 15.]]));
    assert_eq!(drills[1]["travel_mm"], json!([[15., 15.], [35., 35.]]));
    assert_eq!(
        route.runs[0].travel_mm,
        [Vec2::new(35., 35.), Vec2::new(20., 50.)]
    );
    assert_eq!(route.runs[0].path.internal_cut, Some(0));
    assert_eq!(route.runs[1].path.internal_cut, None);
    near(route.cut_length_mm, 210.);
    near(route.cutting_seconds, 10.5);
    near(route.lift_lower_seconds, 0.5);
    let row = &report_json["markers"][0]["selected"];
    near(row["drilling_seconds"].as_f64().unwrap(), 2.);
    near(row["tool_change_seconds"].as_f64().unwrap(), 1.);
    let travel = 2. * 450.0_f64.sqrt() + 800.0_f64.sqrt() + 2250.0_f64.sqrt() + 2050.0_f64.sqrt();
    near(route.travel_length_mm, travel);
    near(route.checked_travel_length().unwrap(), travel);
    near(route.travel_seconds, travel / 100.);
    near(route.modeled_path_seconds, 14. + travel / 100.);
    assert!(!report.physical_cutting_validated && !report.hardware_execution_authorized);
    let mut changed = CutPlan::build(&p, &opts).unwrap();
    changed
        .program
        .as_mut()
        .unwrap()
        .drilling
        .as_mut()
        .unwrap()
        .diameter_mm = 3.;
    assert!(ftx_export::cutting::preview::svg(&changed, 0).is_err());
    let mut changed = CutPlan::build(&p, &opts).unwrap();
    changed.program.as_mut().unwrap().markers[0].selected.drills[0]
        .center_mm
        .x += 1.;
    assert!(ftx_export::cutting::preview::svg(&changed, 0).is_err());

    let mut without_holes = p.raw().clone();
    without_holes["pieces"][0]["drills"] = json!([]);
    let no_drills = CutPlan::build(&Pattern::new(without_holes), &opts).unwrap();
    let row = &value(no_drills.program.as_ref().unwrap())["markers"][0]["selected"];
    for key in ["drills", "drilling_seconds", "tool_change_seconds"] {
        assert!(row.get(key).is_none(), "unused drill phase must omit {key}");
    }
}

#[test]
fn drill_source_geometry_and_process_failures_preserve_the_ordinary_roll_marker() {
    let mut sheet = piece("front", 40., 60.);
    sheet["drills"] = json!([{"p":[10,10],"d":2}]);
    let p = pattern(vec![sheet]);
    let mut opts = qualified_options(&p);
    qualify_drilling(&mut opts);
    assert!(CutPlan::build(&p, &opts).unwrap().program.is_some());
    for drills in [
        json!([{"p":[10,10]}]),
        json!([{"p":[10,10],"d":null}]),
        json!([{"p":[10,10],"d":"unknown"}]),
        json!([{"p":[10,10],"d":0}]),
        json!([{"p":[10,10],"d":3}]),
        json!([{"p":[1,10],"d":2}]), // Whole hole touches the exterior, although its centre is inside.
        json!([{"p":[10,10],"d":2},{"p":[11,10],"d":2}]),
        json!([{"p":[10,10],"d":2},{"p":[10,10],"d":2}]),
    ] {
        let mut raw = p.raw().clone();
        raw["pieces"][0]["drills"] = drills;
        assert_eq!(
            process_refusal(&Pattern::new(raw), &opts).code,
            ftx_core::RefusalCode::InvalidDocument
        );
    }
    for internal in [
        json!([{"kind":"cut","closed":false,"pts":[[5,10],[15,10]]}]),
        json!([{"kind":"cut","closed":true,"pts":[[5,5],[15,5],[15,15],[5,15]]}]),
    ] {
        let mut raw = p.raw().clone();
        raw["pieces"][0]["internal"] = internal;
        assert_eq!(
            process_refusal(&Pattern::new(raw), &opts).code,
            ftx_core::RefusalCode::InvalidDocument
        );
    }
    for path in [
        "/program/machine/drilling/tool",
        "/program/machine/drilling/select_drill_seconds",
    ] {
        let mut raw = value(&opts);
        *raw.pointer_mut(path).unwrap() = if path.ends_with("/tool") {
            json!("different-bit")
        } else {
            json!(0.7)
        };
        let changed: CutOptions = serde_json::from_value(raw).unwrap();
        assert!(
            process_refusal(&p, &changed)
                .message
                .contains("exact machine/material locks")
        );
    }
    for (path, invalid) in [
        ("/program/qualifications/0/drilling", Value::Null),
        ("/program/qualifications/0/drilling/source/note", json!("")),
        (
            "/program/qualifications/0/drilling/allow_estimates",
            json!(false),
        ),
        (
            "/program/qualifications/0/drilling/thickness_mm",
            json!([0.5, 1.1]),
        ),
        (
            "/program/qualifications/0/drilling/plunge_seconds",
            json!(-1.),
        ),
    ] {
        let mut raw = value(&opts);
        *raw.pointer_mut(path).unwrap() = invalid;
        let changed: CutOptions = serde_json::from_value(raw).unwrap();
        assert_eq!(
            process_refusal(&p, &changed).code,
            ftx_core::RefusalCode::InvalidDocument
        );
    }
    let mut raw = value(&opts);
    raw["program"]["machine"]["drilling"]["maximum_thickness_mm"] = json!(1.1);
    let mut too_thick: CutOptions = serde_json::from_value(raw).unwrap();
    relock(&mut too_thick);
    assert_eq!(
        process_refusal(&p, &too_thick).code,
        ftx_core::RefusalCode::InvalidDocument
    );
    // A 0.2 mm knife/raised envelope cannot qualify a fixed bit with a 1 mm radius.
    fixture_policy(
        &mut opts,
        [[15.7, 15.5], [15.8, 15.5], [15.8, 15.6], [15.7, 15.6]],
    );
    assert_eq!(
        process_refusal(&p, &opts).code,
        ftx_core::RefusalCode::InvalidDocument
    );
}

#[test]
fn exact_stop_timing_counts_each_cut_chord_closure_and_final_raised_return() {
    let mut sheet = piece("front", 40., 60.);
    sheet["internal"] = json!([
        {"kind":"cut","closed":true,"pts":[[5,5],[15,5],[15,15],[5,15]]},
        {"kind":"cut","closed":false,"pts":[[25,30],[30,30]]}
    ]);
    let p = pattern(vec![sheet]);
    let mut opts = qualified_options(&p);
    let legacy = CutPlan::build(&p, &opts).unwrap();
    assert!(
        value(legacy.program.as_ref().unwrap())
            .get("motion_profile")
            .is_none()
    );
    set_motion_profile(&mut opts, motion_profile());
    relock(&mut opts);
    let plan = CutPlan::build(&p, &opts).unwrap();
    assert_eq!(value(&plan.rolls), value(&legacy.rolls));
    assert_eq!(value(&plan), value(&CutPlan::build(&p, &opts).unwrap()));
    let report = plan.program.as_ref().unwrap();
    assert_eq!(value(report)["motion_profile"], motion_profile());
    let traversal = &report.markers[0].selected;
    let before = &legacy.program.as_ref().unwrap().markers[0].selected;
    assert_eq!(value(&traversal.runs), value(&before.runs));
    assert_eq!(traversal.return_mm, before.return_mm);
    assert_eq!(traversal.runs.len(), 3);
    near(traversal.cut_length_mm, 245.);
    // At 20 mm/s and 40 mm/s², a 10 mm chord reaches the speed limit exactly.
    // Four 10 mm sides take 4 s; the 40/60 mm exterior takes 12 s, including closure;
    // the open 5 mm slit takes 2 sqrt(5/40) s and never reaches its feed limit.
    let cutting = 16. + 2. * (5.0_f64 / 40.).sqrt();
    // The unchanged four raised legs have squared lengths 200, 1025, 1000, 2050 mm².
    // Each is triangular at 100 mm/s and 25 mm/s²; the fourth is the final return.
    let travel = [200.0_f64, 1025., 1000., 2050.]
        .into_iter()
        .map(|squared_length| 2. * (squared_length.sqrt() / 25.).sqrt())
        .sum::<f64>();
    near(traversal.cutting_seconds, cutting);
    near(traversal.travel_seconds, travel);
    near(traversal.lift_lower_seconds, 0.75);
    near(traversal.modeled_path_seconds, cutting + travel + 0.75);
    assert!(!report.physical_cutting_validated && !report.hardware_execution_authorized);
}

#[test]
fn exact_stop_retains_collinear_cut_vertices_instead_of_timing_only_total_length() {
    let mut sheet = piece("front", 40., 60.);
    sheet["internal"] = json!([{"kind":"cut","closed":false,"pts":[[10,20],[30,20]]}]);
    let p = pattern(vec![sheet]);
    let mut opts = qualified_options(&p);
    set_motion_profile(&mut opts, motion_profile());
    relock(&mut opts);
    let coarse = CutPlan::build(&p, &opts).unwrap();
    let mut raw = p.raw().clone();
    raw["pieces"][0]["internal"][0]["pts"] = json!([[10, 20], [20, 20], [30, 20]]);
    let refined = CutPlan::build(&Pattern::new(raw), &opts).unwrap();
    assert_eq!(coarse.rolls[0].placements[0].internal_cut_mm[0].0.len(), 2);
    assert_eq!(refined.rolls[0].placements[0].internal_cut_mm[0].0.len(), 3);
    let coarse = &coarse.program.as_ref().unwrap().markers[0].selected;
    let refined = &refined.program.as_ref().unwrap().markers[0].selected;
    near(coarse.cut_length_mm, 220.);
    near(refined.cut_length_mm, coarse.cut_length_mm);
    near(coarse.cutting_seconds, 13.5);
    near(refined.cutting_seconds, 14.);
    near(refined.travel_seconds, coarse.travel_seconds);
    near(
        refined.modeled_path_seconds - coarse.modeled_path_seconds,
        0.5,
    );
}

#[test]
fn motion_changes_require_new_machine_qualification_and_invalid_profiles_preserve_nesting() {
    let p = pattern(vec![piece("front", 40., 60.)]);
    let mut opts = qualified_options(&p);
    let legacy_lock = program::machine_sha256(&opts.program.as_ref().unwrap().machine).unwrap();
    set_motion_profile(&mut opts, motion_profile());
    let motion_lock = program::machine_sha256(&opts.program.as_ref().unwrap().machine).unwrap();
    assert_ne!(motion_lock, legacy_lock);
    assert!(
        process_refusal(&p, &opts)
            .message
            .contains("exact machine/material locks")
    );
    relock(&mut opts);
    let admitted = CutPlan::build(&p, &opts).unwrap();
    assert_eq!(
        admitted.program.as_ref().unwrap().machine_sha256,
        motion_lock
    );
    for field in ["cutting_acceleration_mm_s2", "travel_acceleration_mm_s2"] {
        let mut changed = opts.clone();
        let mut profile = motion_profile();
        profile[field] = json!(30.);
        set_motion_profile(&mut changed, profile);
        assert!(
            process_refusal(&p, &changed)
                .message
                .contains("exact machine/material locks")
        );
    }
    for (field, invalid) in [
        ("model", json!("unqualified_blending/1")),
        ("cutting_acceleration_mm_s2", json!(0.)),
        ("travel_acceleration_mm_s2", json!(-1.)),
    ] {
        let mut changed = opts.clone();
        let mut profile = motion_profile();
        profile[field] = invalid;
        set_motion_profile(&mut changed, profile);
        relock(&mut changed);
        assert_eq!(
            process_refusal(&p, &changed).code,
            ftx_core::RefusalCode::InvalidDocument
        );
    }
}

#[test]
fn material_substitution_invalidates_process_then_changes_feed_without_solver_or_geometry_edits() {
    let p = pattern(vec![piece("front", 40., 60.)]);
    let mut opts = qualified_options(&p);
    let initial = CutPlan::build(&p, &opts).unwrap();
    let mut raw = p.raw().clone();
    raw["fabric"]["material"]["record"]["structure"] = json!({"finish":"Synthetic second finish"});
    let changed = Pattern::new(raw);
    assert!(
        process_refusal(&changed, &opts)
            .message
            .contains("no exact knife process qualification")
    );
    let unqualified = CutPlan::build(&changed, &options()).unwrap();
    let recipe = &mut opts.program.as_mut().unwrap().qualifications[0];
    recipe.material_sha256 = unqualified.rolls[0].material.sha256().into();
    recipe.feed_mm_s = 10.;
    recipe.source.note = "Independently authored slower synthetic recipe for the new finish".into();
    let updated = CutPlan::build(&changed, &opts).unwrap();
    assert_eq!(
        value(&initial.rolls[0].placements),
        value(&updated.rolls[0].placements)
    );
    let old = &initial.program.as_ref().unwrap().markers[0];
    let new = &updated.program.as_ref().unwrap().markers[0];
    assert_ne!(old.material_sha256, new.material_sha256);
    near(
        new.selected.cutting_seconds,
        old.selected.cutting_seconds * 2.,
    );
    near(new.selected.travel_length_mm, old.selected.travel_length_mm);
}

#[test]
fn exact_machine_lock_and_full_thickness_range_are_required() {
    let p = pattern(vec![piece("front", 40., 60.)]);
    let original = qualified_options(&p);
    let mut opts = original.clone();
    opts.program.as_mut().unwrap().machine.travel_mm_s = 90.;
    assert!(
        process_refusal(&p, &opts)
            .message
            .contains("exact machine/material locks")
    );
    opts = original.clone();
    opts.program.as_mut().unwrap().qualifications[0].thickness_mm = [0.9, 1.5];
    assert!(
        process_refusal(&p, &opts)
            .message
            .contains("full source thickness range")
    );
    opts = original.clone();
    opts.program.as_mut().unwrap().qualifications[0].thickness_mm = [0.5, 1.1];
    assert!(
        process_refusal(&p, &opts)
            .message
            .contains("full source thickness range")
    );
    opts = original.clone();
    let policy = opts.program.as_mut().unwrap();
    policy.machine.maximum_thickness_mm = 1.1;
    policy.qualifications[0].machine_sha256 = program::machine_sha256(&policy.machine).unwrap();
    assert!(
        process_refusal(&p, &opts)
            .message
            .contains("full source thickness range")
    );
    opts = original;
    let policy = opts.program.as_mut().unwrap();
    policy.machine.bed_mm = [100., 60.];
    policy.qualifications[0].machine_sha256 = program::machine_sha256(&policy.machine).unwrap();
    assert!(
        process_refusal(&p, &opts)
            .message
            .contains("exceeds the declared knife bed")
    );
}

#[test]
fn bounded_nearest_paths_retain_complete_baselines_and_authored_direction() {
    let p = pattern(vec![
        piece("a", 20., 20.),
        piece("b", 20., 20.),
        piece("c", 20., 20.),
    ]);
    let mut opts = qualified_options(&p);
    let policy = opts.program.as_mut().unwrap();
    policy.machine.start_mm = Vec2::new(100., 5.);
    policy.machine.return_to_start = false;
    policy.ordering = Ordering::NearestPath;
    policy.qualifications[0].machine_sha256 = program::machine_sha256(&policy.machine).unwrap();
    let full = CutPlan::build(&p, &opts).unwrap();
    let report = full.program.as_ref().unwrap();
    let marker = &report.markers[0];
    assert!(marker.improved);
    assert!(marker.selected.travel_length_mm < marker.baseline.travel_length_mm);
    assert_eq!(marker.selected.runs.len(), 3);
    near(marker.selected.cut_length_mm, 240.);
    for run in &marker.selected.runs {
        let original = &full.rolls[0].placements[run.path.placement].cut_mm;
        assert_eq!(run.cut_endpoints_mm, [original[0], original[0]]);
        assert_eq!(
            run.instance,
            full.rolls[0].placements[run.path.placement].instance
        );
    }
    assert!(!report.budget_exhausted && !report.optimality_proven);
    opts.program.as_mut().unwrap().work_limit = 1;
    let bounded = CutPlan::build(&p, &opts).unwrap();
    let report = bounded.program.as_ref().unwrap();
    assert!(report.budget_exhausted);
    assert_eq!(report.search_work, 1);
    assert!(!report.markers[0].improved);
    assert_eq!(
        value(&report.markers[0].selected),
        value(&report.markers[0].baseline)
    );
}

#[test]
fn lower_width_layout_does_not_shrink_the_actual_stock_envelope_for_the_machine() {
    let p = pattern(vec![piece("front", 40., 60.)]);
    let mut raw = p.raw().clone();
    raw["fabric"]["material"]["record"]["properties"]["width"]["bounds"] = json!([0.11, 0.13]);
    let p = Pattern::new(raw);
    let mut opts = qualified_options(&p);
    opts.width_basis = ftx_export::cutting::WidthBasis::ReportedLowerBound;
    let policy = opts.program.as_mut().unwrap();
    policy.machine.bed_mm[0] = 125.;
    policy.qualifications[0].machine_sha256 = program::machine_sha256(&policy.machine).unwrap();
    assert!(
        process_refusal(&p, &opts)
            .message
            .contains("exceeds the declared knife bed")
    );
    let policy = opts.program.as_mut().unwrap();
    policy.machine.bed_mm[0] = 130.;
    policy.qualifications[0].machine_sha256 = program::machine_sha256(&policy.machine).unwrap();
    let plan = CutPlan::build(&p, &opts).unwrap();
    assert_eq!(plan.rolls[0].layout_width.as_ref().unwrap().width_m, 0.11);
    assert!(plan.program.is_some());
}

#[test]
fn shared_search_budget_spans_multiple_complete_markers() {
    let p = pattern(vec![piece("front", 20., 20.)]);
    let mut opts = qualified_options(&p);
    opts.copies = 3;
    opts.maximum_marker_length_m = Some(0.03);
    // The width admits all copies side by side, so explicitly narrow the placement with margins.
    opts.edge_margin_mm = 45.;
    opts.program.as_mut().unwrap().ordering = Ordering::NearestPath;
    opts.program.as_mut().unwrap().work_limit = 1;
    let plan = CutPlan::build(&p, &opts).unwrap();
    let report = plan.program.as_ref().unwrap();
    assert_eq!(report.markers.len(), 3);
    assert_eq!(report.search_work, 1);
    assert!(report.budget_exhausted);
    for marker in &report.markers {
        assert_eq!(marker.selected.runs.len(), 1);
        near(marker.selected.cut_length_mm, 80.);
        assert_eq!(value(&marker.baseline), value(&marker.selected));
    }
}

#[test]
fn unsupported_or_ambiguous_source_features_are_not_silently_dropped() {
    for (feature, expected) in [
        (json!({"drills":[{"p":[20,20],"d":2}]}), "drills"),
        (
            json!({"internal":[{"kind":"cut","pts":[[5,5],[15,5]]}]}),
            "explicit boolean closed",
        ),
        (
            json!({"internal":[{"kind":"cut","closed":false,"pts":[[35,5],[45,5]]}]}),
            "strictly inside",
        ),
        (
            json!({"internal":[{"kind":"cut","closed":false,"pts":[[5,5],[15,15],[5,15],[15,5]]}]}),
            "intersects itself",
        ),
        (
            json!({"internal":[{"kind":"cut","closed":false,"pts":[[5,5],[15,5],[10,5]]}]}),
            "reverses",
        ),
        (
            json!({"internal":[{"kind":"cut","closed":true,"pts":[[5,5],[25,5],[25,25],[5,25]]},
            {"kind":"cut","closed":false,"pts":[[10,10],[15,10]]}]}),
            "removed regions",
        ),
        (
            json!({"internal":[{"kind":"cut","closed":false,"pts":[[5,5],[15,15]]},
            {"kind":"cut","closed":false,"pts":[[5,15],[15,5]]}]}),
            "intersecting knife features",
        ),
    ] {
        let mut sheet = piece("front", 40., 60.);
        for (key, value) in feature.as_object().unwrap() {
            sheet[key] = value.clone();
        }
        let p = pattern(vec![sheet]);
        let opts = qualified_options(&p);
        let error = process_refusal(&p, &opts);
        assert!(
            error.message.contains(expected),
            "expected {expected}, got {}",
            error.message
        );
    }
}

#[test]
fn malformed_source_cut_points_and_drills_refuse_without_losing_the_ordinary_marker() {
    let ordinary_options = options();
    let process_options = qualified_options(&pattern(vec![piece("front", 40., 60.)]));
    // Invalid typed coordinates and containers already refuse the whole document, with or
    // without a requested process. They cannot reach the permissive annotation reader.
    for feature in [
        json!({"internal":[{"kind":"cut","closed":false,
            "pts":[[5,5],["bad",15],[25,5]]}]}),
        json!({"internal":[{"kind":"cut","closed":false,
            "pts":[[5,5,99],[25,5]]}]}),
        json!({"internal":[{"kind":"cut","closed":false,
            "pts":[[5,5],null,[25,5]]}]}),
        json!({"internal":[{"kind":"cut","closed":false,"pts":{}}]}),
        json!({"internal":{"kind":"cut","closed":false,"pts":[[5,5],[25,5]]}}),
        json!({"drills":[{"p":["bad",10],"d":2}]}),
        json!({"drills":[{}]}),
        json!({"drills":{"p":[10,10],"d":2}}),
    ] {
        let mut sheet = piece("front", 40., 60.);
        for (key, value) in feature.as_object().unwrap() {
            sheet[key] = value.clone();
        }
        let p = pattern(vec![sheet]);
        for opts in [&ordinary_options, &process_options] {
            let refusal = CutPlan::build(&p, opts).unwrap_err();
            assert_eq!(refusal.code, ftx_core::RefusalCode::InvalidDocument);
        }
    }
    // Missing/short point lists and null optional inventories are accepted by the typed
    // document. The ordinary marker survives, but the knife cannot assume omitted cuts.
    for (feature, expected) in [
        (
            json!({"internal":[{"kind":"cut","closed":false}]}),
            "complete finite 2D source coordinates",
        ),
        (
            json!({"internal":[{"kind":"cut","closed":false,"pts":[]}]}),
            "complete finite 2D source coordinates",
        ),
        (
            json!({"internal":[{"kind":"cut","closed":false,"pts":[[5,5]]}]}),
            "complete finite 2D source coordinates",
        ),
        (json!({"internal":null}), "internal paths must be an array"),
        (json!({"drills":null}), "declared drills"),
    ] {
        let mut sheet = piece("front", 40., 60.);
        for (key, value) in feature.as_object().unwrap() {
            sheet[key] = value.clone();
        }
        let p = pattern(vec![sheet]);
        let opts = qualified_options(&p);
        let refusal = process_refusal(&p, &opts);
        assert!(refusal.message.contains(expected), "{refusal:?}");
    }
    let mut sheet = piece("front", 40., 60.);
    sheet["internal"] = json!([]);
    sheet["drills"] = json!([]);
    let p = pattern(vec![sheet]);
    let plan = CutPlan::build(&p, &qualified_options(&p)).unwrap();
    assert!(plan.program.is_some());
    assert!(plan.program_refusal.is_none());
}

#[test]
fn evidence_directions_duplicates_and_arithmetic_limits_refuse() {
    let p = pattern(vec![piece("front", 40., 60.)]);
    let original = qualified_options(&p);
    let mut opts = original.clone();
    opts.program.as_mut().unwrap().qualifications[0].allow_estimates = false;
    assert!(
        process_refusal(&p, &opts)
            .message
            .contains("explicit consent")
    );
    opts = original.clone();
    opts.program.as_mut().unwrap().qualifications[0].directions = "warp_only".into();
    assert!(
        process_refusal(&p, &opts)
            .message
            .contains("supported directions")
    );
    opts = original.clone();
    let policy = opts.program.as_mut().unwrap();
    let mut duplicate = policy.qualifications[0].clone();
    duplicate.id = "second-recipe-for-same-material".into();
    policy.qualifications.push(duplicate);
    assert!(
        process_refusal(&p, &opts)
            .message
            .contains("unique identities")
    );
    opts = original.clone();
    opts.program.as_mut().unwrap().qualifications[0].feed_mm_s = 1e-320;
    assert!(
        process_refusal(&p, &opts)
            .message
            .contains("arithmetic overflows")
    );
    opts = original;
    opts.program.as_mut().unwrap().model = "laser".into();
    assert!(process_refusal(&p, &opts).message.contains("cold-knife"));
}

#[test]
fn omission_keeps_ordinary_marker_output_and_source_geometry_unchanged() {
    let p = pattern(vec![piece("front", 40., 60.)]);
    let ordinary = CutPlan::build(&p, &options()).unwrap();
    assert!(value(&ordinary).get("program").is_none());
    assert!(value(&ordinary).get("program_refusal").is_none());
    assert!(value(&ordinary.options).get("program").is_none());
    let qualified = CutPlan::build(&p, &qualified_options(&p)).unwrap();
    assert_eq!(value(&ordinary.rolls), value(&qualified.rolls));
    assert_eq!(ordinary.pattern_sha256, qualified.pattern_sha256);
    assert!(qualified.program_refusal.is_none());
    let mut invalid_geometry = qualified_options(&p);
    invalid_geometry.copies = 0;
    assert!(CutPlan::build(&p, &invalid_geometry).is_err());
}

#[test]
fn material_studies_replay_programs_and_reserve_both_work_limits_before_callbacks() {
    let p = pattern(vec![piece("front", 40., 60.)]);
    let options = qualified_options(&p);
    let mut raw = json!({"schema":"ftx-material-study/1","cutting":options,
    "variants":[
        {"key":"first","changes":{"schema":"ftx-material-bindings/1","default":p.raw()["fabric"]["material"]}},
        {"key":"second","changes":{"schema":"ftx-material-bindings/1","default":p.raw()["fabric"]["material"]}}
    ]});
    let request: ftx_export::material_study::Request = serde_json::from_value(raw.clone()).unwrap();
    let report = value(&request.evaluate(&p).unwrap());
    for row in report["alternatives"].as_array().unwrap() {
        assert_eq!(row["cutting"]["status"], "available");
        let bound = Pattern::new(row["binding"]["result"]["pattern"].clone());
        assert_eq!(
            row["cutting"]["result"],
            value(&CutPlan::build(&bound, &options).unwrap())
        );
    }
    for field in ["work_limit", "geometry_work_limit"] {
        raw["cutting"]["program"][field] = json!(if field == "work_limit" {
            1_000_001
        } else {
            10_000_001
        });
        let request: ftx_export::material_study::Request =
            serde_json::from_value(raw.clone()).unwrap();
        let mut calls = 0;
        let refusal = request
            .evaluate_with(&p, &json!({"synthetic":"budget"}), |_, _| {
                calls += 1;
                Ok(json!({}))
            })
            .unwrap_err();
        assert_eq!(refusal.code, ftx_core::RefusalCode::BudgetExceeded);
        assert!(refusal.message.contains("aggregate knife-program"));
        assert_eq!(calls, 0);
        raw["cutting"]["program"][field] = json!(10000);
    }
    let mut opts = options;
    opts.program.as_mut().unwrap().geometry_work_limit = 1;
    let refusal = process_refusal(&p, &opts);
    assert_eq!(refusal.code, ftx_core::RefusalCode::BudgetExceeded);
    assert!(refusal.message.contains("geometry/work budget"));
}

#[test]
fn unavailable_process_preserves_study_geometry_and_independent_purchasing() {
    let p = pattern(vec![piece("front", 40., 60.)]);
    let options = qualified_options(&p);
    let mut raw = json!({"schema":"ftx-material-study/1","cutting":options,
        "variants":[{"key":"sheet","changes":{"schema":"ftx-material-bindings/1","default":p.raw()["fabric"]["material"]}}],
        "purchasing":{
            "quotes":{"schema":"ftx-textile-quotes/1","quotes":[{
                "offer_id":"test:sheet-offer","material_id":"test-sheet","supplier":"Synthetic supplier","sku":"synthetic-sheet",
                "price":10,"quantity":1,"unit":"linear_meter","minimum_quantity":0.1,"order_increment":0.1,
                "currency":"USD","region":"test","quality":"synthetic","terms":"no-shipping",
                "observed_on":"2026-10-30","available":true,"source":"test:knife-stock-quote","source_sha256":"0".repeat(64)
            }]},
            "market":{"currency":"USD","region":"test","quality":"synthetic","terms":"no-shipping","as_of":"2026-10-30","max_age_days":0}
        }
    });
    let request: ftx_export::material_study::Request = serde_json::from_value(raw.clone()).unwrap();
    let qualified = value(&request.evaluate(&p).unwrap());
    raw["cutting"]["program"]["qualifications"][0]["material_sha256"] = json!("f".repeat(64));
    let request: ftx_export::material_study::Request = serde_json::from_value(raw).unwrap();
    let unavailable = value(&request.evaluate(&p).unwrap());
    let a = &qualified["alternatives"][0];
    let b = &unavailable["alternatives"][0];
    assert_eq!(b["cutting"]["status"], "available");
    assert!(b["cutting"]["result"]["program"].is_null());
    assert_eq!(
        b["cutting"]["result"]["program_refusal"]["code"],
        "invalid_document"
    );
    assert_eq!(
        b["cutting"]["result"]["rolls"],
        a["cutting"]["result"]["rolls"]
    );
    assert_eq!(b["purchasing"]["status"], "available");
    for field in [
        "schema",
        "roll_prices",
        "material_orders",
        "complete_offer_coverage",
        "currency",
        "complete_observed_quantity_coverage",
        "complete_continuous_length_limit_coverage",
        "least_observed_independent_order_total",
        "limits",
    ] {
        assert_eq!(
            b["purchasing"]["result"][field], a["purchasing"]["result"][field],
            "independent procurement field {field} changed"
        );
    }
    // Procurement retains the actual cut-plan provenance, including the local process
    // refusal, while its prices, order quantities, coverage and scope remain unchanged.
    assert_eq!(
        b["purchasing"]["result"]["cut_plan"],
        b["cutting"]["result"]
    );
    assert_eq!(
        b["purchasing"]["result"]["least_observed_independent_order_total"],
        1.0
    );
    // Named process metrics must be present and positive before they disappear.
    // Every other cutting_* reading also disappears; stock, mass and purchasing stay exact.
    for field in [
        "cutting_cut_length_m",
        "cutting_modeled_path_seconds",
        "cutting_raised_travel_length_m",
    ] {
        assert!(a["metrics"][field].as_f64().unwrap() > 0.0, "{field}");
    }
    for (metric, expected) in a["metrics"].as_object().unwrap() {
        if metric.starts_with("cutting_") {
            assert!(expected.is_number(), "{metric}");
            assert!(
                b["metrics"].get(metric).is_none(),
                "unavailable knife effort must not survive as {metric}"
            );
        } else {
            assert_eq!(
                &b["metrics"][metric], expected,
                "independent metric {metric} changed"
            );
        }
    }
}

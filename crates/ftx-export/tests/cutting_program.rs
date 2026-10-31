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
    for (metric, expected) in a["metrics"].as_object().unwrap() {
        if metric.starts_with("cutting_") {
            assert!(expected.is_number());
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

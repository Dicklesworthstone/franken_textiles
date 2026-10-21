//! Directed multi-pass answers on synthetic geometry/materials, not physical seam qualification.
use ftx_core::{RefusalCode, Vec2};
use ftx_export::sewing::{
    SewOptions, SewPlan,
    program::{FeedDirection, SewProgram},
};
use ftx_pattern::Pattern;
use serde_json::{Value, json};

fn pattern() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-sewing-pattern.json"
    ))
    .unwrap()
}
fn options() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-sewing-options.json"
    ))
    .unwrap()
}
fn reversible() -> Value {
    let mut o = options();
    o["machine"]["reverse"] = json!({"stitch_mm":[0.5,5.0],"feed_b_over_a":[0.5,2.0],
        "max_stitches_per_minute":800.0});
    o
}
fn build(p: Value, o: Value) -> Result<SewPlan, ftx_core::Refusal> {
    SewPlan::build(
        &Pattern::new(p),
        &serde_json::from_value::<SewOptions>(o).unwrap(),
    )
}
fn travel(plan: &SewPlan) -> Vec<(usize, usize)> {
    plan.seams[0]
        .runs
        .iter()
        .map(|r| (r.from_station, r.to_station))
        .collect()
}
fn split(mut options: Value, ranges: &[[f64; 2]]) -> Value {
    let base = options["passes"][0].clone();
    options["passes"] = json!(
        ranges
            .iter()
            .enumerate()
            .map(|(i, range)| {
                let mut pass = base.clone();
                pass["id"] = json!(format!("span-{i}"));
                pass["source_range"] = json!(range);
                pass
            })
            .collect::<Vec<_>>()
    );
    options
}
fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-10, "{a} != {b}");
}

#[test]
fn legacy_job_retains_v1_wire_shape_and_arithmetic() {
    let mut o = options();
    // The original serializer includes this absent optional export override as null.
    o["seam_allowance_mm"] = Value::Null;
    let a = build(pattern(), o.clone()).unwrap();
    let raw = serde_json::to_value(&a).unwrap();
    assert_eq!(a.schema, "ftx-sewing-plan/1");
    assert_eq!(raw["options"], o);
    assert!(raw.get("ordering").is_none());
    assert!(raw["options"].get("ordering").is_none());
    assert!(raw["seams"][0].get("runs").is_none());
    assert!(raw["seams"][0].get("source_span").is_none());
    assert!(raw["seams"][0]["pass"].get("source_range").is_none());
    assert_eq!(a.stitch_intervals, 40);
    assert_eq!(a.feed_seconds, 4.0);
}

#[test]
fn backtacks_repeat_real_intervals_without_changing_station_geometry() {
    let legacy = build(pattern(), options()).unwrap();
    let mut o = reversible();
    o["passes"][0]["backtack"] = json!({"start_stitches":3,"end_stitches":2});
    let plan = build(pattern(), o).unwrap();
    assert_eq!(plan.schema, "ftx-sewing-plan/2");
    assert_eq!(plan.seams[0].stations, legacy.seams[0].stations);
    assert_eq!(travel(&plan), [(0, 3), (3, 0), (0, 40), (40, 38), (38, 40)]);
    assert_eq!(plan.stitch_intervals, 50);
    assert_eq!(plan.feed_seconds, 5.0);
    for w in plan.seams[0].runs.windows(2) {
        assert_eq!(w[0].to_station, w[1].from_station);
    }
    let mut side_a_mm = 0.0;
    for run in &plan.seams[0].runs {
        assert_eq!(
            run.stitch_intervals,
            run.from_station.abs_diff(run.to_station)
        );
        let fa = &plan.seams[0].stations.fa;
        side_a_mm += (fa[run.from_station] - fa[run.to_station]).abs() * 100.0;
    }
    assert!((side_a_mm - 125.0).abs() < 1e-12);
    assert!(!plan.hardware_execution_authorized && !plan.physical_seam_quality_validated);
}

#[test]
fn reverse_pass_and_locks_reverse_traversal_not_pattern_side_identity() {
    let baseline = build(pattern(), options()).unwrap();
    let mut o = reversible();
    o["passes"][0]["direction"] = json!("reverse");
    o["passes"][0]["backtack"] = json!({"start_stitches":3,"end_stitches":2});
    let plan = build(pattern(), o).unwrap();
    assert_eq!(plan.seams[0].stations, baseline.seams[0].stations);
    assert_eq!(travel(&plan), [(40, 37), (37, 40), (40, 0), (0, 2), (2, 0)]);
    assert_eq!(plan.seams[0].sides[0].reference, "front/right");
    assert_eq!(plan.seams[0].sides[1].reference, "back/left");
    assert_eq!(plan.stitch_intervals, 50);
    assert_eq!(plan.seams[0].runs[2].direction, FeedDirection::Reverse);
}

#[test]
fn repeated_seam_operations_obey_operation_dependencies_and_charge_each_pass() {
    let mut o = reversible();
    o["passes"][0]["id"] = json!("join");
    let mut reinforce = o["passes"][0].clone();
    reinforce["id"] = json!("reinforce");
    reinforce["direction"] = json!("reverse");
    reinforce["after"] = json!(["join"]);
    reinforce["plies"] = json!([2, 1]);
    reinforce["stitches_per_minute"] = json!(300.0);
    o["passes"].as_array_mut().unwrap().insert(0, reinforce);
    let a = build(pattern(), o.clone()).unwrap();
    assert_eq!(a.seams.len(), 2);
    assert_eq!(a.seams[0].pass.operation_id(), "join");
    assert_eq!(a.seams[1].pass.operation_id(), "reinforce");
    assert_eq!(a.seams[0].pass.seam, a.seams[1].pass.seam);
    assert_eq!(a.seams[0].stations, a.seams[1].stations);
    assert!((a.seams[1].stack_mm - 1.2).abs() < 1e-12);
    assert_eq!(a.stitch_intervals, 80);
    assert_eq!(a.feed_seconds, 12.0);
    o["passes"].as_array_mut().unwrap().reverse();
    let b = build(pattern(), o).unwrap();
    assert_eq!(
        serde_json::to_value(a.seams).unwrap(),
        serde_json::to_value(b.seams).unwrap()
    );
}

#[test]
fn only_actually_reversed_intervals_need_reverse_feed_ratio_admission() {
    let mut p = pattern();
    p["pieces"][0]["notches"] = json!([{"edge":"right","s":0.4}]);
    p["pieces"][1]["notches"] = json!([{"edge":"left","s":0.5}]);
    let mut o = reversible();
    o["seam_tolerance_mm"] = json!(11.0);
    o["machine"]["reverse"]["feed_b_over_a"] = json!([1.2, 1.3]);
    o["passes"][0]["backtack"] = json!({"start_stitches":3});
    let a = build(p.clone(), o.clone()).unwrap();
    assert!((a.seams[0].feed_b_over_a[0] - 1.25).abs() < 1e-12);
    assert!(a.seams[0].feed_b_over_a.iter().any(|f| *f < 1.0));
    // The reverse start lock uses 1.25, not its reciprocal 0.8, while the end has a different ratio.
    o["passes"][0]["backtack"]["end_stitches"] = json!(3);
    let refusal = build(p.clone(), o.clone()).unwrap_err();
    assert!(
        refusal.message.contains("Reverse local") && refusal.message.contains("differential feed")
    );
    o["passes"][0]["backtack"] = json!({});
    o["passes"][0]["direction"] = json!("reverse");
    assert!(
        build(p, o)
            .unwrap_err()
            .message
            .contains("differential feed")
    );
}

#[test]
fn reverse_limits_are_explicit_and_rate_and_pitch_are_checked_per_traversal() {
    let mut o = options();
    o["passes"][0]["backtack"] = json!({"start_stitches":1});
    assert!(
        build(pattern(), o)
            .unwrap_err()
            .message
            .contains("explicit reverse")
    );
    let mut o = reversible();
    o["machine"]["reverse"]["max_stitches_per_minute"] = json!(500.0);
    assert!(
        build(pattern(), o.clone()).is_ok(),
        "unused reverse speed is not a forward limit"
    );
    o["passes"][0]["backtack"] = json!({"end_stitches":1});
    assert!(
        build(pattern(), o)
            .unwrap_err()
            .message
            .contains("Reverse stitch rate")
    );
    let mut o = reversible();
    o["passes"][0]["direction"] = json!("reverse");
    o["machine"]["stitch_mm"] = json!([0.5, 1.0]);
    assert!(
        build(pattern(), o.clone()).is_ok(),
        "pure reverse never uses forward pitch limits"
    );
    o["passes"][0]["backtack"] = json!({"start_stitches":1});
    assert!(
        build(pattern(), o)
            .unwrap_err()
            .message
            .contains("Forward local stitch")
    );
}

#[test]
fn material_substitution_rechecks_each_operation_without_rewriting_paths() {
    let mut o = reversible();
    o["passes"][0]["backtack"] = json!({"start_stitches":2});
    let mut second = o["passes"][0].clone();
    second["id"] = json!("reinforce");
    second["plies"] = json!([2, 2]);
    second["after"] = json!(["side"]);
    o["passes"].as_array_mut().unwrap().push(second);
    let a = build(pattern(), o.clone()).unwrap();
    let mut p = pattern();
    p["fabric"]["material"]["record"]["properties"]["thickness"]["value"] = json!(0.6);
    let b = build(p.clone(), o.clone()).unwrap();
    assert_ne!(a.pattern_sha256, b.pattern_sha256);
    for (a, b) in a.seams.iter().zip(&b.seams) {
        assert_eq!(a.stations, b.stations);
        assert_eq!(
            serde_json::to_value(&a.runs).unwrap(),
            serde_json::to_value(&b.runs).unwrap()
        );
        assert_ne!(a.stack_mm, b.stack_mm);
        assert_ne!(a.sides[0].material_sha256, b.sides[0].material_sha256);
    }
    p["fabric"]["material"]["record"]["properties"]["thickness"]["value"] = json!(1.0);
    let refused = build(p, o).unwrap_err();
    assert!(refused.message.contains("reinforce") && refused.message.contains("stack thickness"));
}

#[test]
fn locks_cannot_cross_seam_ends_or_overflow_budgets() {
    let mut o = reversible();
    o["passes"][0]["backtack"] = json!({"start_stitches":41});
    assert!(
        build(pattern(), o.clone())
            .unwrap_err()
            .message
            .contains("exceeds the seam")
    );
    o["passes"][0]["backtack"]["start_stitches"] = json!(150000);
    assert_eq!(
        build(pattern(), o.clone()).unwrap_err().code,
        RefusalCode::BudgetExceeded
    );
    o["passes"][0]["backtack"]["start_stitches"] = json!(usize::MAX);
    assert_eq!(
        build(pattern(), o).unwrap_err().code,
        RefusalCode::BudgetExceeded
    );
}

#[test]
fn duplicate_unknown_and_cyclic_operations_and_uncovered_seams_refuse() {
    let mut o = reversible();
    let pass = o["passes"][0].clone();
    o["passes"].as_array_mut().unwrap().push(pass);
    assert!(
        build(pattern(), o.clone())
            .unwrap_err()
            .message
            .contains("unique")
    );
    o["passes"][1]["id"] = json!("reinforce");
    o["passes"][1]["after"] = json!(["missing"]);
    assert!(
        build(pattern(), o.clone())
            .unwrap_err()
            .message
            .contains("unknown predecessor")
    );
    o["passes"][1]["after"] = json!(["side"]);
    o["passes"][0]["after"] = json!(["reinforce"]);
    assert!(
        build(pattern(), o.clone())
            .unwrap_err()
            .message
            .contains("cyclic")
    );
    o["passes"][0]["after"] = json!([]);
    let mut p = pattern();
    p["seams"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"top","a":"front/top","b":"back/top"}));
    assert!(
        build(p, o)
            .unwrap_err()
            .message
            .contains("every pattern seam")
    );
}

#[test]
fn source_spans_keep_original_notch_mapping_and_source_coordinates() {
    let mut p = pattern();
    p["pieces"][0]["notches"] = json!([{"edge":"right","s":0.4}]);
    p["pieces"][1]["notches"] = json!([{"edge":"left","s":0.5}]);
    let mut o = options();
    o["seam_tolerance_mm"] = json!(11.0);
    let whole = build(p.clone(), o.clone()).unwrap();
    let plan = build(p, split(o, &[[0., 0.2], [0.2, 0.7], [0.7, 1.]])).unwrap();
    assert_eq!(plan.schema, "ftx-sewing-plan/10");
    assert_eq!(plan.pattern_sha256, whole.pattern_sha256);
    let middle = &plan.seams[1];
    let s = &middle.stations;
    assert_eq!(s.fa.first(), Some(&0.2));
    assert_eq!(s.fa.last(), Some(&0.7));
    close(s.fb[0], 0.25);
    close(*s.fb.last().unwrap(), 0.75);
    assert_eq!(s.a[0], Vec2::new(20., 20.));
    assert_eq!(s.b[0], Vec2::new(0., 25.));
    let notch = s.fa.iter().position(|&f| f == 0.4).unwrap();
    close(s.fb[notch], 0.5);
    close(s.a[notch].y, 40.);
    close(s.b[notch].y, 50.);
    assert!(
        middle
            .feed_b_over_a
            .iter()
            .any(|&f| (f - 1.25).abs() < 1e-10)
    );
    assert!(
        middle
            .feed_b_over_a
            .iter()
            .any(|&f| (f - 5. / 6.).abs() < 1e-10)
    );
    let span = middle.source_span.as_ref().unwrap();
    assert_eq!(span.source_ranges[0], [0.2, 0.7]);
    assert_eq!(span.source_lengths_mm, [100., 100.]);
    close(span.sewn_lengths_mm[0], 50.);
    close(span.sewn_lengths_mm[1], 50.);
    for side in 0..2 {
        close(
            plan.seams
                .iter()
                .map(|s| s.source_span.as_ref().unwrap().sewn_lengths_mm[side])
                .sum(),
            100.,
        );
    }
    assert!(!plan.hardware_execution_authorized && !plan.physical_seam_quality_validated);
}

#[test]
fn source_ranges_refuse_invalid_values_and_every_unsewn_gap_but_allow_reinforcement() {
    for range in [[-0.1, 1.], [0.5, 0.5], [0.7, 0.5], [0., 1.01]] {
        assert!(
            build(pattern(), split(options(), &[range]))
                .unwrap_err()
                .message
                .contains("source_range")
        );
    }
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut o: SewOptions = serde_json::from_value(options()).unwrap();
        o.passes[0].source_range = Some([0., value]);
        assert!(
            SewPlan::build(&Pattern::new(pattern()), &o)
                .unwrap_err()
                .message
                .contains("source_range")
        );
    }
    for ranges in [
        vec![[0.1, 1.]],
        vec![[0., 0.9]],
        vec![[0., 0.5], [0.5000000000000001, 1.]],
    ] {
        assert!(
            build(pattern(), split(options(), &ranges))
                .unwrap_err()
                .message
                .contains("source coverage gap on side a")
        );
    }
    let overlapping = split(options(), &[[0., 0.6], [0.4, 1.]]);
    let plan = build(pattern(), overlapping).unwrap();
    close(
        plan.seams
            .iter()
            .map(|s| s.source_span.as_ref().unwrap().sewn_lengths_mm[0])
            .sum(),
        120.,
    );
    let mut o = options();
    let mut reinforce = o["passes"][0].clone();
    reinforce["id"] = json!("reinforce-middle");
    reinforce["source_range"] = json!([0.2, 0.7]);
    o["passes"].as_array_mut().unwrap().push(reinforce);
    assert!(
        build(pattern(), o).is_ok(),
        "a whole pass already establishes complete coverage"
    );
}

#[test]
fn reverse_locks_and_expanded_stitches_stay_inside_the_selected_span() {
    let mut o = split(reversible(), &[[0., 0.5], [0.5, 1.]]);
    o["passes"][1]["direction"] = json!("reverse");
    o["passes"][1]["backtack"] = json!({"start_stitches":3,"end_stitches":2});
    let program = SewProgram::build(
        &Pattern::new(pattern()),
        &serde_json::from_value(o.clone()).unwrap(),
    )
    .unwrap();
    let s = &program.plan.seams[1];
    assert_eq!(
        s.runs
            .iter()
            .map(|r| (r.from_station, r.to_station))
            .collect::<Vec<_>>(),
        [(20, 17), (17, 20), (20, 0), (0, 2), (2, 0)]
    );
    assert_eq!(program.plan.stitch_intervals, 50);
    close(program.plan.feed_seconds, 5.);
    for step in program.stitches.iter().filter(|s| s.operation == 1) {
        assert!((50. ..=100.).contains(&step.a_mm.y));
        assert!((50. ..=100.).contains(&step.b_mm.y));
        close(step.advance_mm[0].abs(), 2.5);
        close(step.advance_mm[1].abs(), 2.5);
    }
    assert!(
        program
            .stitches
            .iter()
            .any(|s| s.operation == 1 && s.advance_mm[0] < 0.)
    );
    o["passes"][1]["backtack"]["start_stitches"] = json!(21);
    assert!(
        build(pattern(), o)
            .unwrap_err()
            .message
            .contains("exceeds the seam")
    );
}

#[test]
fn curved_span_endpoints_sample_original_geometry_and_zero_offsets_match() {
    let mut p = pattern();
    p["pieces"][0]["edges"][1]["curve"] = json!({"type":"quad","c":[[0.5,0.1]]});
    p["pieces"][1]["edges"][3]["curve"] = json!({"type":"quad","c":[[0.5,0.1]]});
    let mut o = split(
        options(),
        &[[0., 0.23456789123], [0.23456789123, 0.75], [0.75, 1.]],
    );
    let source = Pattern::new(p.clone());
    let (dense, _) = source.side_polyline("front/right").unwrap();
    let expected = ftx_core::polyline::at_fractions(&dense, &[0.23456789123, 0.75]);
    let center = build(p.clone(), o.clone()).unwrap();
    let middle = &center.seams[1].stations;
    assert_eq!(middle.a[0], expected[0]);
    assert_eq!(*middle.a.last().unwrap(), expected[1]);
    for pass in o["passes"].as_array_mut().unwrap() {
        pass["stitch_path"] =
            json!({"offset_mm":[0,0],"mitre_limit":4,"minimum_cut_clearance_mm":1});
    }
    let zero = build(p, o).unwrap();
    for (a, b) in center.seams.iter().zip(&zero.seams) {
        assert_eq!(a.stations, b.stations);
        let receipt = b.path.as_ref().unwrap();
        assert_eq!(receipt.polylines_mm[0][0], a.stations.a[0]);
        assert_eq!(receipt.polylines_mm[0].last(), a.stations.a.last());
    }
}

#[test]
fn offset_clearance_checks_only_selected_geometry_and_both_source_sides_need_coverage() {
    let mut p = pattern();
    p["pieces"][0]["drills"] = json!([{"p":[17,75],"d":2}]);
    let mut whole = options();
    whole["passes"][0]["stitch_path"] =
        json!({"offset_mm":[3,3],"mitre_limit":4,"minimum_cut_clearance_mm":1});
    assert!(
        build(p.clone(), whole.clone())
            .unwrap_err()
            .message
            .contains("clearance")
    );
    let mut o = split(whole, &[[0., 0.5], [0.5, 1.]]);
    o["passes"][1]["stitch_path"]["offset_mm"] = json!([7, 7]);
    let plan = build(p.clone(), o.clone()).unwrap();
    let a = plan.seams[0].path.as_ref().unwrap();
    let b = plan.seams[1].path.as_ref().unwrap();
    assert_eq!(a.polylines_mm[0], [Vec2::new(17., 0.), Vec2::new(17., 50.)]);
    assert_eq!(
        b.polylines_mm[0],
        [Vec2::new(13., 50.), Vec2::new(13., 100.)]
    );
    close(b.minimum_cut_clearance_mm[0], 3.);
    p["pieces"][0]["drills"][0]["p"] = json!([17, 25]);
    assert!(build(p, o).unwrap_err().message.contains("clearance"));

    let mut p = pattern();
    p["seams"][0]["a"] = json!("front/bottom+right");
    p["seams"][0]["b"] = json!("back/top+left");
    let mut o = split(options(), &[[0., 0.5], [0.5, 1.]]);
    o["passes"][0]["stitch_path"] =
        json!({"offset_mm":[5,3],"mitre_limit":4,"minimum_cut_clearance_mm":1});
    // Different offset arclengths put B's first operation end before its second operation start.
    assert!(
        build(p, o)
            .unwrap_err()
            .message
            .contains("source coverage gap on side b")
    );
}

#[test]
fn partial_sampling_charges_traveled_stations_and_still_rejects_whole_job_overflow() {
    let mut p = pattern();
    for piece in p["pieces"].as_array_mut().unwrap() {
        piece["vertices"]["c"][1] = json!(3000.);
        piece["vertices"]["d"][1] = json!(3000.);
    }
    let mut o = options();
    o["machine"]["stitch_mm"] = json!([0.005, 1000.]);
    o["passes"][0]["max_stitch_mm"] = json!(0.01);
    let mut o = split(o, &[[0., 0.001], [0.001, 1.]]);
    o["passes"][1]["max_stitch_mm"] = json!(1000.);
    let plan = build(p.clone(), o.clone()).unwrap();
    assert_eq!(plan.seams[0].stitch_intervals, 300);
    assert_eq!(plan.seams[1].stitch_intervals, 3);
    o["passes"][0]["source_range"] = json!([0., 0.5]);
    o["passes"][1]["source_range"] = json!([0.5, 1.]);
    o["passes"][1]["max_stitch_mm"] = json!(0.01);
    assert_eq!(build(p, o).unwrap_err().code, RefusalCode::BudgetExceeded);
}

#[test]
fn partial_passes_do_not_bypass_whole_source_seam_acceptance() {
    let mut p = pattern();
    p["pieces"][1]["vertices"]["c"][1] = json!(101.);
    p["pieces"][1]["vertices"]["d"][1] = json!(101.);
    let o = split(options(), &[[0., 0.5], [0.5, 1.]]);
    assert!(
        build(p, o)
            .unwrap_err()
            .message
            .contains("passing flat seams")
    );
}

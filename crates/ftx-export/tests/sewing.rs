//! Synthetic software answers, not measurements or qualified sewing-machine recipes.
use ftx_export::sewing::{SewOptions, SewPlan, SewingQualification};
use ftx_pattern::Pattern;
use ftx_pattern::materials::{Evidence, Source};
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
fn fixed_liquid_sewing_requires_wet_thickness_without_requiring_unrelated_membrane_data() {
    let mut raw = pattern();
    let record = &mut raw["fabric"]["material"]["record"];
    record["structure"]["mechanical_mass_model"] =
        json!(ftx_pattern::material_mass::RETAINED_LIQUID_MODEL);
    record["properties"]["areal_density"] = json!({"value":0.2,"unit":"kg/m2",
        "source":"t","evidence":"estimate","method":"Synthetic dry mass",
        "conditions":{"specimen_id":"synthetic-wet-sewing","conditioning":"closed numerical test",
            "reference_area_basis":"dry_textile_reference_area","mass_basis":"dry_textile_excluding_retained_liquid"}});
    record["properties"]["retained_liquid_areal_density"] = json!({"value":0.1,"unit":"kg/m2",
        "source":"t","evidence":"estimate","method":"Synthetic additional liquid",
        "conditions":{"specimen_id":"synthetic-wet-sewing","conditioning":"closed numerical test",
            "reference_area_basis":"dry_textile_reference_area","mass_basis":"additional_retained_liquid",
            "retention_basis":"closed_co_moving_no_exchange","qualification":"Independent synthetic fixed liquid retention"}});
    assert!(
        plan(raw.clone(), options())
            .unwrap_err()
            .message
            .contains("wet mechanics")
    );
    raw["fabric"]["material"]["record"]["properties"]["thickness"]["conditions"] = json!({
        "specimen_id":"synthetic-wet-sewing","conditioning":"closed numerical test","retained_liquid_kg_m2":"0.1"});
    let result = plan(raw, options()).unwrap();
    assert!((result.seams[0].stack_mm - 0.8).abs() < 1e-12);
    assert!(
        result
            .materials
            .values()
            .all(|s| s.record.as_ref().unwrap().missing_mechanics().len() > 1)
    );
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

fn qualification_for(plan: &SewPlan) -> SewingQualification {
    let seam = &plan.seams[0];
    SewingQualification {
        id: "measured-coupon-v1".into(),
        materials: vec![
            seam.sides[0].material_sha256.clone(),
            seam.sides[1].material_sha256.clone(),
        ],
        machine: "synthetic-machine".into(),
        needle: "test-needle".into(),
        thread: "test-thread".into(),
        stitch_mm: [2.0, 3.0],
        feed_b_over_a: [0.9, 1.1],
        stack_mm: [0.7, 0.9],
        stitches_per_minute: [500.0, 700.0],
        allow_reverse: false,
        presser_foot: None,
        thread_consumption: None,
        evidence: Evidence::Measured,
        source: Source {
            reference: "test:sewing-coupon".into(),
            accessed_on: "2026-10-18".into(),
            note: "Synthetic known-answer qualification; not a physical coupon".into(),
        },
        method: "Synthetic seam qualification fixture".into(),
        conditions: std::collections::BTreeMap::from([(
            "purpose".into(),
            "software regression only".into(),
        )]),
        allow_estimates: false,
    }
}

#[test]
fn exact_source_qualification_follows_material_and_actual_operation_envelopes() {
    let p = pattern();
    let baseline = plan(p.clone(), options()).unwrap();
    let mut o: SewOptions = serde_json::from_value(options()).unwrap();
    o.require_qualification = true;
    o.qualifications.push(qualification_for(&baseline));
    let admitted = SewPlan::build(&Pattern::new(p.clone()), &o).unwrap();
    assert_eq!(admitted.schema, "ftx-sewing-plan/5");
    assert_eq!(
        admitted.seams[0].qualification_id.as_deref(),
        Some("measured-coupon-v1")
    );
    assert!(!admitted.hardware_execution_authorized);
    assert!(!admitted.physical_seam_quality_validated);

    let mut changed = p.clone();
    changed["fabric"] = fabric("same-thickness-different-lot", 0.4);
    let err = SewPlan::build(&Pattern::new(changed), &o).unwrap_err();
    assert!(err.message.contains("exact stack"), "{err}");

    let mut wrong_needle = o.clone();
    wrong_needle.passes[0].needle = "other-needle".into();
    assert!(
        SewPlan::build(&Pattern::new(p.clone()), &wrong_needle)
            .unwrap_err()
            .message
            .contains("needle")
    );

    let mut too_fast = o.clone();
    too_fast.passes[0].stitches_per_minute = 800.0;
    assert!(
        SewPlan::build(&Pattern::new(p), &too_fast)
            .unwrap_err()
            .message
            .contains("qualified")
    );
}

#[test]
fn estimated_qualification_and_reverse_travel_need_separate_explicit_admission() {
    let p = pattern();
    let baseline = plan(p.clone(), options()).unwrap();
    let mut o: SewOptions = serde_json::from_value(options()).unwrap();
    o.require_qualification = true;
    let mut q = qualification_for(&baseline);
    q.evidence = Evidence::Estimate;
    o.qualifications.push(q);
    assert!(SewPlan::build(&Pattern::new(p.clone()), &o).is_err());

    o.qualifications[0].allow_estimates = true;
    SewPlan::build(&Pattern::new(p.clone()), &o).unwrap();

    o.machine.reverse = Some(ftx_export::sewing::program::ReverseFeed {
        stitch_mm: [0.1, 5.0],
        feed_b_over_a: [0.5, 2.0],
        max_stitches_per_minute: 1000.0,
    });
    o.passes[0].backtack.start_stitches = 1;
    assert!(
        SewPlan::build(&Pattern::new(p.clone()), &o)
            .unwrap_err()
            .message
            .contains("qualified")
    );
    o.qualifications[0].allow_reverse = true;
    let admitted = SewPlan::build(&Pattern::new(p), &o).unwrap();
    assert_eq!(
        admitted.seams[0].qualification_id.as_deref(),
        Some("measured-coupon-v1")
    );
}

fn setup_job() -> Value {
    let mut o = options();
    let original = o["passes"][0].clone();
    o["passes"] = json!(
        (0..4)
            .map(|i| {
                let mut pass = original.clone();
                pass["id"] = json!(["A", "B", "C", "D"][i]);
                pass["needle"] = json!(if i % 2 == 0 { "needle-X" } else { "needle-Y" });
                pass["thread"] = json!(if i % 2 == 0 { "thread-X" } else { "thread-Y" });
                pass
            })
            .collect::<Vec<_>>()
    );
    o
}

fn ordering(nodes: usize) -> Value {
    json!({"objective":"minimize_setup_changes","max_search_nodes":nodes})
}

#[test]
fn setup_order_reduces_changes_without_changing_any_admitted_operation() {
    let p = pattern();
    let mut o = setup_job();
    let baseline = plan(p.clone(), o.clone()).unwrap();
    o["ordering"] = ordering(20000);
    let optimized = plan(p.clone(), o.clone()).unwrap();
    assert_eq!(optimized.schema, "ftx-sewing-plan/6");
    let report = optimized.ordering.as_ref().unwrap();
    assert_eq!(report.baseline_order, ["A", "B", "C", "D"]);
    assert_eq!(report.selected_order, ["A", "C", "B", "D"]);
    assert_eq!(report.baseline_changes.total_changes, 6);
    assert_eq!(report.selected_changes.needle_changes, 1);
    assert_eq!(report.selected_changes.thread_changes, 1);
    assert_eq!(report.selected_changes.total_changes, 2);
    assert!(report.optimality_proven && report.search_complete && !report.budget_exhausted);
    assert_eq!(report.lower_bound_total_changes, 2);
    assert_eq!(optimized.pattern_sha256, baseline.pattern_sha256);
    assert_eq!(optimized.stitch_intervals, baseline.stitch_intervals);
    assert_eq!(optimized.feed_seconds, baseline.feed_seconds);
    assert_eq!(
        serde_json::to_value(&optimized.materials).unwrap(),
        serde_json::to_value(&baseline.materials).unwrap()
    );
    for seam in &optimized.seams {
        let old = baseline
            .seams
            .iter()
            .find(|s| s.pass.id == seam.pass.id)
            .unwrap();
        assert_eq!(
            serde_json::to_value(seam).unwrap(),
            serde_json::to_value(old).unwrap()
        );
    }
    o["passes"].as_array_mut().unwrap().reverse();
    let repeated = plan(p.clone(), o.clone()).unwrap();
    assert_eq!(
        serde_json::to_value(&repeated.ordering).unwrap(),
        serde_json::to_value(&optimized.ordering).unwrap()
    );

    let program = ftx_export::sewing::program::SewProgram::build(
        &Pattern::new(p),
        &serde_json::from_value(o).unwrap(),
    )
    .unwrap();
    assert_eq!(program.plan.schema, "ftx-sewing-plan/6");
    assert_eq!(program.stitches.len(), baseline.stitch_intervals);
    assert_eq!(
        serde_json::to_value(program.plan.ordering).unwrap(),
        serde_json::to_value(optimized.ordering).unwrap()
    );
    assert!(
        !program.plan.hardware_execution_authorized
            && !program.plan.physical_seam_quality_validated
    );
}

#[test]
fn setup_order_respects_initial_state_precedence_and_budget_without_partial_schedules() {
    let mut o = setup_job();
    o["ordering"] = ordering(1);
    let limited = plan(pattern(), o.clone()).unwrap();
    let r = limited.ordering.unwrap();
    assert_eq!(r.selected_order, r.baseline_order);
    assert_eq!(r.selected_order.len(), 4);
    assert_eq!(r.search_nodes, 1);
    assert!(r.budget_exhausted && !r.optimality_proven && !r.search_complete);
    assert_eq!(r.lower_bound_total_changes, 2);

    o["ordering"] = ordering(20000);
    o["ordering"]["initial_setup"] = json!({"needle":"needle-Y","thread":"thread-Y"});
    let initial = plan(pattern(), o.clone()).unwrap().ordering.unwrap();
    assert_eq!(initial.baseline_changes.total_changes, 8);
    assert_eq!(initial.selected_order, ["B", "D", "A", "C"]);
    assert_eq!(initial.selected_changes.total_changes, 2);

    // Complete declared chain defeats grouping; reverse travel and locks stay attached to their pass.
    o["machine"]["reverse"] =
        json!({"stitch_mm":[0.1,5.],"feed_b_over_a":[0.5,2.],"max_stitches_per_minute":1000.});
    for i in 1..4 {
        let predecessor = ["A", "B", "C"][i - 1];
        o["passes"][i]["after"] = json!([predecessor]);
    }
    o["passes"][1]["direction"] = json!("reverse");
    o["passes"][3]["backtack"] = json!({"start_stitches":2,"end_stitches":1});
    let chained = plan(pattern(), o.clone()).unwrap();
    let r = chained.ordering.as_ref().unwrap();
    assert_eq!(r.selected_order, ["A", "B", "C", "D"]);
    assert_eq!(r.selected_changes.total_changes, 8);
    assert!(r.optimality_proven);
    assert_eq!(r.lower_bound_total_changes, 8);
    assert_eq!(chained.stitch_intervals, 166);
    let mut legacy = o;
    legacy.as_object_mut().unwrap().remove("ordering");
    assert_eq!(
        serde_json::to_value(chained.seams).unwrap(),
        serde_json::to_value(plan(pattern(), legacy).unwrap().seams).unwrap()
    );
}

#[test]
fn setup_search_preserves_source_qualification_and_early_refusals() {
    let p = pattern();
    let base = plan(p.clone(), options()).unwrap();
    let mut o = setup_job();
    let mut qualifiers = Vec::new();
    for name in ["X", "Y"] {
        let mut q = qualification_for(&base);
        q.id = format!("coupon-{name}");
        q.needle = format!("needle-{name}");
        q.thread = format!("thread-{name}");
        qualifiers.push(q);
    }
    o["qualifications"] = json!(qualifiers);
    o["require_qualification"] = json!(true);
    let baseline = plan(p.clone(), o.clone()).unwrap();
    o["ordering"] = ordering(20000);
    let optimized = plan(p.clone(), o.clone()).unwrap();
    for seam in optimized.seams {
        let old = baseline
            .seams
            .iter()
            .find(|s| s.pass.id == seam.pass.id)
            .unwrap();
        assert_eq!(seam.qualification_id, old.qualification_id);
        assert_eq!(
            serde_json::to_value(seam).unwrap(),
            serde_json::to_value(old).unwrap()
        );
    }
    let mut changed = p.clone();
    changed["fabric"] = fabric("other-lot", 0.4);
    assert!(
        plan(changed, o.clone())
            .unwrap_err()
            .message
            .contains("exact stack")
    );
    o["passes"][0]["after"] = json!(["D"]);
    o["passes"][3]["after"] = json!(["A"]);
    assert!(
        plan(p.clone(), o.clone())
            .unwrap_err()
            .message
            .contains("cyclic")
    );
    for limit in [0, 200001] {
        o["ordering"] = ordering(limit);
        assert_eq!(
            plan(p.clone(), o.clone()).unwrap_err().code,
            ftx_core::RefusalCode::BudgetExceeded
        );
    }
    let mut oversized = setup_job();
    let original = oversized["passes"][0].clone();
    oversized["passes"] = json!(
        (0..21)
            .map(|i| {
                let mut p = original.clone();
                p["id"] = json!(format!("pass-{i}"));
                p
            })
            .collect::<Vec<_>>()
    );
    oversized["ordering"] = ordering(20000);
    assert_eq!(
        plan(p, oversized).unwrap_err().code,
        ftx_core::RefusalCode::BudgetExceeded
    );
}

#[test]
fn setup_optimum_matches_exhaustive_permutations_with_crossed_components() {
    // Independent full permutation oracle: no pruning, setup bitsets or production search logic.
    fn visit(passes: &[Value], order: &mut Vec<usize>, best: &mut usize) {
        if order.len() == passes.len() {
            let cost = order
                .windows(2)
                .map(|w| {
                    usize::from(passes[w[0]]["needle"] != passes[w[1]]["needle"])
                        + usize::from(passes[w[0]]["thread"] != passes[w[1]]["thread"])
                })
                .sum();
            *best = (*best).min(cost);
            return;
        }
        for i in 0..passes.len() {
            if !order.contains(&i)
                && passes[i]["after"].as_array().is_none_or(|deps| {
                    deps.iter()
                        .all(|id| order.iter().any(|&j| &passes[j]["id"] == id))
                })
            {
                order.push(i);
                visit(passes, order, best);
                order.pop();
            }
        }
    }
    for dependencies in [false, true] {
        let mut o = setup_job();
        o["passes"][1]["thread"] = json!("thread-X");
        o["passes"][2]["thread"] = json!("thread-Y");
        let mut extra = o["passes"][0].clone();
        extra["id"] = json!("E");
        extra["needle"] = json!("needle-Z");
        o["passes"].as_array_mut().unwrap().push(extra);
        if dependencies {
            o["passes"][2]["after"] = json!(["B"]);
            o["passes"][4]["after"] = json!(["A"]);
        }
        let mut expected = usize::MAX;
        visit(
            o["passes"].as_array().unwrap(),
            &mut Vec::new(),
            &mut expected,
        );
        if !dependencies {
            o["ordering"] = ordering(5);
            let limited = plan(pattern(), o.clone()).unwrap().ordering.unwrap();
            assert!(limited.budget_exhausted && !limited.optimality_proven);
            assert_eq!(limited.selected_order.len(), 5);
            assert_eq!(limited.baseline_changes.total_changes, 6);
            assert_eq!(limited.selected_changes.total_changes, 5);
        }
        o["ordering"] = ordering(20000);
        let r = plan(pattern(), o).unwrap().ordering.unwrap();
        assert!(r.optimality_proven && r.search_complete);
        assert_eq!(r.selected_changes.total_changes, expected);
    }
}

fn timed_setup(label: &str) -> Value {
    json!({"needle":format!("needle-{label}"),"thread":format!("thread-{label}")})
}

fn timed_job() -> Value {
    let mut job = setup_job();
    for (pass, label) in job["passes"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .zip(["X", "Y", "Z", "X"])
    {
        pass["needle"] = timed_setup(label)["needle"].clone();
        pass["thread"] = timed_setup(label)["thread"].clone();
    }
    let mut rows = Vec::new();
    for from in ["X", "Y", "Z"] {
        for to in ["X", "Y", "Z"] {
            if from != to {
                rows.push(json!({"from":timed_setup(from),"to":timed_setup(to),
                    "milliseconds":if from != "X" && to != "X" {100000} else {1000}}));
            }
        }
    }
    job["ordering"] = json!({"objective":"minimize_setup_time","max_search_nodes":20000,
        "initial_setup":timed_setup("X"),"setup_times":{
            "machine":"synthetic-machine","source":{"reference":"test:setup-time",
                "accessed_on":"2026-10-20","note":"Synthetic, not measured machine times"},
            "method":"Synthetic directed complete-setup duration table", "conditions":{"scope":"test"},
            "evidence":"estimate","allow_estimates":true,"transitions":rows}});
    job
}

#[test]
fn timed_order_can_prefer_more_changes_and_preserves_the_actual_program() {
    let job = timed_job();
    let p = pattern();
    let timed = plan(p.clone(), job.clone()).unwrap();
    assert_eq!(timed.schema, "ftx-sewing-plan/9");
    let report = timed.ordering.as_ref().unwrap();
    assert_eq!(report.selected_order, ["A", "B", "D", "C"]);
    assert_eq!(report.selected_changes.total_changes, 6);
    assert_eq!(report.lower_bound_total_changes, 4); // time minimization is not count minimization
    let time = report.timing.as_ref().unwrap();
    assert_eq!(time.baseline_setup_milliseconds, 102000);
    assert_eq!(time.selected_setup_milliseconds, 3000);
    assert_eq!(time.lower_bound_setup_milliseconds, 3000);
    assert_eq!(time.feed_and_setup_seconds, 19.0); // 16 feed, 3 setup; not 3 seconds total
    assert!(report.optimality_proven);

    let mut counts = job.clone();
    counts["ordering"] = json!({"objective":"minimize_setup_changes", "max_search_nodes":20000,
        "initial_setup":timed_setup("X")});
    let counts = plan(p.clone(), counts).unwrap();
    assert_eq!(
        counts
            .ordering
            .as_ref()
            .unwrap()
            .selected_changes
            .total_changes,
        4
    );
    for op in &timed.seams {
        let same = counts
            .seams
            .iter()
            .find(|old| old.pass.id == op.pass.id)
            .unwrap();
        assert_eq!(
            serde_json::to_value(op).unwrap(),
            serde_json::to_value(same).unwrap()
        );
    }
    let program = ftx_export::sewing::program::SewProgram::build(
        &Pattern::new(p),
        &serde_json::from_value(job).unwrap(),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(&program.plan).unwrap(),
        serde_json::to_value(timed).unwrap()
    );
    assert_eq!(program.stitches.len(), 160);
    assert!(!program.plan.hardware_execution_authorized);
}

#[test]
fn directed_time_search_matches_independent_permutations_with_dependencies_and_zero_times() {
    fn permutations(v: &mut [usize], index: usize, out: &mut Vec<Vec<usize>>) {
        if index == v.len() {
            out.push(v.to_vec());
            return;
        }
        for next in index..v.len() {
            v.swap(index, next);
            permutations(v, index + 1, out);
            v.swap(index, next);
        }
    }
    let mut orders = Vec::new();
    permutations(&mut [0, 1, 2, 3], 0, &mut orders);
    let labels = ["X", "Y", "Z", "X"];
    for seed in 0..12 {
        let mut job = timed_job();
        let rows = job["ordering"]["setup_times"]["transitions"]
            .as_array_mut()
            .unwrap();
        for (i, row) in rows.iter_mut().enumerate() {
            row["milliseconds"] = json!(((i + 1) * 17 + seed * 7) % 13 * 1000);
        }
        let rows = rows.clone();
        if seed % 2 == 0 {
            job["passes"][1]["after"] = json!(["C"]);
        }
        let duration = |order: &[usize]| {
            let mut from = timed_setup("X");
            let mut sum = 0;
            for &i in order {
                let to = timed_setup(labels[i]);
                if from != to {
                    sum += rows
                        .iter()
                        .find(|r| r["from"] == from && r["to"] == to)
                        .unwrap()["milliseconds"]
                        .as_u64()
                        .unwrap();
                }
                from = to;
            }
            sum
        };
        let feasible: Vec<_> = orders
            .iter()
            .filter(|o| {
                seed % 2 != 0 || o.iter().position(|i| *i == 2) < o.iter().position(|i| *i == 1)
            })
            .collect();
        let best = feasible.iter().map(|o| duration(o)).min().unwrap();
        let result = plan(pattern(), job.clone()).unwrap();
        let r = result.ordering.unwrap();
        assert_eq!(r.timing.as_ref().unwrap().selected_setup_milliseconds, best);
        assert!(r.optimality_proven);
        job["passes"].as_array_mut().unwrap().reverse();
        job["ordering"]["setup_times"]["transitions"]
            .as_array_mut()
            .unwrap()
            .reverse();
        let repeat = plan(pattern(), job).unwrap().ordering.unwrap();
        assert_eq!(
            serde_json::to_value(r).unwrap(),
            serde_json::to_value(repeat).unwrap()
        );
    }
}

#[test]
fn time_budget_retains_a_complete_incumbent_and_a_sound_bound() {
    let mut job = timed_job();
    job["ordering"]["max_search_nodes"] = json!(1);
    let result = plan(pattern(), job).unwrap().ordering.unwrap();
    assert_eq!(result.selected_order, result.baseline_order);
    assert_eq!(result.search_nodes, 1);
    assert!(result.budget_exhausted && !result.optimality_proven);
    let time = result.timing.unwrap();
    assert_eq!(time.selected_setup_milliseconds, 102000);
    assert!(time.lower_bound_setup_milliseconds <= 3000);
}

#[test]
fn source_timing_never_defaults_missing_transitions_machine_or_permission() {
    for change in 0..9 {
        let mut job = timed_job();
        let policy = &mut job["ordering"];
        match change {
            0 => {
                policy.as_object_mut().unwrap().remove("initial_setup");
            }
            1 => {
                policy["setup_times"]["transitions"]
                    .as_array_mut()
                    .unwrap()
                    .pop();
            }
            2 => policy["setup_times"]["machine"] = json!("other-machine"),
            3 => policy["setup_times"]["allow_estimates"] = json!(false),
            4 => policy["setup_times"]["source"]["reference"] = json!(" "),
            5 => {
                let row = policy["setup_times"]["transitions"][0].clone();
                policy["setup_times"]["transitions"]
                    .as_array_mut()
                    .unwrap()
                    .push(row);
            }
            6 => policy["setup_times"]["transitions"][0]["milliseconds"] = json!(86400001),
            7 => policy["objective"] = json!("minimize_setup_changes"),
            _ => {
                policy.as_object_mut().unwrap().remove("setup_times");
            }
        }
        assert!(plan(pattern(), job).is_err(), "case {change}");
    }
    let mut same = timed_job();
    for pass in same["passes"].as_array_mut().unwrap() {
        pass["needle"] = timed_setup("X")["needle"].clone();
        pass["thread"] = timed_setup("X")["thread"].clone();
    }
    same["ordering"]["setup_times"]["transitions"] = json!([]);
    let r = plan(pattern(), same).unwrap().ordering.unwrap();
    assert_eq!(r.timing.unwrap().selected_setup_milliseconds, 0);
    assert!(r.optimality_proven);
}

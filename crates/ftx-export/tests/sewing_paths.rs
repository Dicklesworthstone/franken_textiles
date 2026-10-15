//! Independent flat-geometry answers. Synthetic materials/machines do not qualify a real seam.
use ftx_core::{Refusal, RefusalCode, Vec2};
use ftx_export::sewing::{SewOptions, SewPlan, program::SewProgram};
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
fn offset(a: f64, b: f64) -> Value {
    let mut o = options();
    o["passes"][0]["stitch_path"] =
        json!({"offset_mm":[a,b],"mitre_limit":4.0,"minimum_cut_clearance_mm":1.0});
    o
}
fn build(p: Value, o: Value) -> Result<SewPlan, Refusal> {
    SewPlan::build(
        &Pattern::new(p),
        &serde_json::from_value::<SewOptions>(o).unwrap(),
    )
}
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-8, "{a} != {b}");
}
fn composite() -> Value {
    let mut p = pattern();
    p["seams"][0]["a"] = json!("front/bottom+right");
    p["seams"][0]["b"] = json!("back/top+left");
    p
}

#[test]
fn straight_offsets_use_inward_instance_normals_before_seam_reversal() {
    let a = build(pattern(), offset(3.0, 4.0)).unwrap();
    assert_eq!(a.schema, "ftx-sewing-plan/3");
    let s = &a.seams[0];
    for (pa, pb) in s.stations.a.iter().zip(&s.stations.b) {
        near(pa.x, 17.0);
        near(pb.x, 4.0);
        near(pa.y, pb.y);
    }
    near(s.stations.len_a, 100.0);
    near(s.stations.len_b, 100.0);
    let receipt = s.path.as_ref().unwrap();
    assert_eq!(receipt.source_lengths_mm, [100.0, 100.0]);
    for i in 0..s.stations.fa.len() {
        near(receipt.source_fractions[0][i], s.stations.fa[i]);
    }
    assert_eq!(a.stitch_intervals, 40);
    let outward = build(pattern(), offset(-3.0, -4.0)).unwrap();
    near(outward.seams[0].stations.a[0].x, 23.0);
    near(outward.seams[0].stations.b[0].x, -4.0);
}

#[test]
fn mitred_paths_change_lengths_feed_and_counts_not_just_coordinates() {
    let baseline = build(composite(), options()).unwrap();
    let a = build(composite(), offset(5.0, 3.0)).unwrap();
    let s = &a.seams[0];
    near(s.stations.len_a, 110.0);
    near(s.stations.len_b, 114.0);
    near(baseline.seams[0].stations.len_a, 120.0);
    assert_eq!(
        s.path.as_ref().unwrap().polylines_mm[0],
        [
            Vec2::new(0.0, 5.0),
            Vec2::new(15.0, 5.0),
            Vec2::new(15.0, 100.0)
        ]
    );
    assert!(
        s.feed_b_over_a
            .iter()
            .all(|&f| (f - 114.0 / 110.0).abs() < 1e-10)
    );
    let mut tight = offset(5.0, 3.0);
    tight["machine"]["feed_b_over_a"] = json!([0.99, 1.01]);
    assert!(
        build(composite(), tight)
            .unwrap_err()
            .message
            .contains("differential feed")
    );
    let shorter = build(composite(), offset(5.0, 5.0)).unwrap();
    assert!(shorter.stitch_intervals < baseline.stitch_intervals);
    for (i, w) in s.stations.fa.windows(2).enumerate() {
        assert!((w[1] - w[0]) * 110.0 <= 2.5 + 1e-10);
        assert!((s.stations.fb[i + 1] - s.stations.fb[i]) * 114.0 <= 2.5 + 1e-10);
    }
}

#[test]
fn curve_offsets_retain_notch_identity_and_use_actual_path_arclength() {
    let mut p = pattern();
    // Equal quadratic seam curves bulging into each original piece.
    p["pieces"][0]["edges"][1]["curve"] = json!({"type":"quad","c":[[0.5,0.1]]});
    p["pieces"][1]["edges"][3]["curve"] = json!({"type":"quad","c":[[0.5,0.1]]});
    p["pieces"][0]["notches"] = json!([{"edge":"right","s":0.3}]);
    p["pieces"][1]["notches"] = json!([{"edge":"left","s":0.6}]);
    let mut o = offset(2.0, -2.0);
    o["seam_tolerance_mm"] = json!(12.0);
    o["machine"]["stitch_mm"] = json!([0.01, 5.0]);
    let a = build(p, o).unwrap();
    let s = &a.seams[0];
    assert!((s.stations.len_a - s.stations.len_b).abs() > 0.1);
    let ix = s
        .stations
        .fa
        .iter()
        .position(|f| (*f - s.stations.notches_a[0]).abs() < 1e-12)
        .unwrap();
    near(s.stations.fb[ix], s.stations.notches_b[0]);
    let r = s.path.as_ref().unwrap();
    near(r.source_fractions[0][ix], 0.3);
    near(r.source_fractions[1][ix], 0.4);
    assert!(r.polylines_mm[0].len() >= 200);
    for i in 0..s.feed_b_over_a.len() {
        let da = (s.stations.fa[i + 1] - s.stations.fa[i]) * s.stations.len_a;
        let db = (s.stations.fb[i + 1] - s.stations.fb[i]) * s.stations.len_b;
        near(s.feed_b_over_a[i], db / da);
    }
}

#[test]
fn mirror_instances_choose_material_inward_not_world_left() {
    let mut p = pattern();
    p["pieces"] = json!([p["pieces"][0].clone()]);
    p["pieces"][0]["cut"]["qty"] = json!(2);
    p["seams"][0]["a"] = json!("front.R/right");
    p["seams"][0]["b"] = json!("front.L/right");
    let a = build(p, offset(3.0, 3.0)).unwrap();
    for (pa, pb) in a.seams[0].stations.a.iter().zip(&a.seams[0].stations.b) {
        near(pa.x, -pb.x);
        near(pa.y, pb.y);
        near(pa.x, 17.0);
    }
}

#[test]
fn zero_offset_keeps_all_legacy_station_values_and_serialized_legacy_jobs_unchanged() {
    let p = composite();
    let old = build(p.clone(), options()).unwrap();
    let zero = build(p, offset(0.0, 0.0)).unwrap();
    assert_eq!(old.seams[0].stations, zero.seams[0].stations);
    let raw = serde_json::to_value(old).unwrap();
    assert!(raw["seams"][0].get("path").is_none());
    assert!(raw["seams"][0]["pass"].get("stitch_path").is_none());
    assert_eq!(raw["schema"], "ftx-sewing-plan/1");
}

#[test]
fn cut_clearance_checks_inward_and_outward_paths_and_allowance_precedence() {
    assert!(build(pattern(), offset(35.0, 0.0)).is_err());
    assert!(build(pattern(), offset(-10.0, 0.0)).is_err());
    let mut p = pattern();
    p["pieces"][0]["edges"][1]["sa"] = json!(2.0);
    let mut o = offset(-1.0, 0.0);
    o["passes"][0]["minimum_allowance_mm"] = json!(1.0);
    let a = build(p.clone(), o.clone()).unwrap();
    near(
        a.seams[0].path.as_ref().unwrap().minimum_cut_clearance_mm[0],
        1.0,
    );
    o["passes"][0]["stitch_path"]["minimum_cut_clearance_mm"] = json!(1.01);
    assert!(build(p, o).unwrap_err().message.contains("clearance"));
}

#[test]
fn folded_back_collapsed_and_unbounded_mitres_refuse_without_path_repair() {
    assert!(
        build(composite(), offset(21.0, 0.0))
            .unwrap_err()
            .message
            .contains("collapses")
    );
    let mut o = offset(2.0, 0.0);
    o["passes"][0]["stitch_path"]["mitre_limit"] = json!(1.1);
    assert!(
        build(composite(), o)
            .unwrap_err()
            .message
            .contains("mitre limit")
    );
    let mut p = pattern();
    p["pieces"][0]["edges"][1]["curve"] = json!({"type":"poly","c":[[0.6,0.0],[0.4,0.0]]});
    assert!(build(p, offset(1.0, 0.0)).is_err());
    let mut p = pattern();
    p["pieces"][0]["internal"] = json!([{"kind":"cut","pts":[[0,50],[20,50]]}]);
    assert!(
        build(p, offset(1.0, 1.0))
            .unwrap_err()
            .message
            .contains("internal cuts")
    );
}

#[test]
fn bounded_offset_station_generation_refuses_before_large_allocation() {
    let mut p = pattern();
    for side in p["pieces"].as_array_mut().unwrap() {
        side["vertices"]["c"][1] = json!(99000.0);
        side["vertices"]["d"][1] = json!(99000.0);
    }
    let mut o = offset(1.0, 1.0);
    o["machine"]["stitch_mm"] = json!([0.001, 5.0]);
    o["passes"][0]["max_stitch_mm"] = json!(0.01);
    assert_eq!(build(p, o).unwrap_err().code, RefusalCode::BudgetExceeded);
    let mut p = pattern();
    p["pieces"][0]["edges"][1]["curve"] = json!({"type":"poly","c":vec![[0.5,0.1];9000]});
    assert_eq!(
        build(p, offset(1.0, 1.0)).unwrap_err().code,
        RefusalCode::BudgetExceeded
    );
}

#[test]
fn offset_program_reverse_locks_charge_actual_path_and_preserve_geometry() {
    let p = Pattern::new(composite());
    let mut o = offset(5.0, 3.0);
    o["machine"]["reverse"] =
        json!({"stitch_mm":[0.01,5.0],"feed_b_over_a":[0.5,2.0],"max_stitches_per_minute":1000.0});
    o["passes"][0]["direction"] = json!("reverse");
    o["passes"][0]["backtack"] = json!({"start_stitches":2,"end_stitches":2});
    let options: SewOptions = serde_json::from_value(o).unwrap();
    let program = SewProgram::build(&p, &options).unwrap();
    assert_eq!(program.schema, "ftx-sewing-program/2");
    assert_eq!(program.plan.schema, "ftx-sewing-plan/3");
    let s = &program.plan.seams[0];
    let travel: f64 = program.stitches.iter().map(|x| x.advance_mm[0].abs()).sum();
    let lock_mm = 2.0
        * ((s.stations.fa[2] - s.stations.fa[0]) + (1.0 - s.stations.fa[s.stations.fa.len() - 3]))
        * 110.0;
    near(travel, 110.0 + lock_mm);
    assert!(program.stitches[0].advance_mm[0] < 0.0);
    for step in &program.stitches {
        assert_eq!(step.a_mm, s.stations.a[step.to_station]);
        near(step.feed_b_over_a, 114.0 / 110.0);
    }
}

#[test]
fn material_swap_keeps_offset_paths_but_rechecks_every_ply_stack() {
    let p = pattern();
    let o = offset(3.0, 4.0);
    let a = build(p.clone(), o.clone()).unwrap();
    let mut thick = p;
    thick["fabric"]["material"]["record"]["properties"]["thickness"]["value"] = json!(0.8);
    let b = build(thick.clone(), o.clone()).unwrap();
    assert_eq!(a.seams[0].stations, b.seams[0].stations);
    assert_eq!(
        serde_json::to_value(&a.seams[0].path).unwrap(),
        serde_json::to_value(&b.seams[0].path).unwrap()
    );
    near(b.seams[0].stack_mm, 2.0 * a.seams[0].stack_mm);
    assert_ne!(a.pattern_sha256, b.pattern_sha256);
    thick["fabric"]["material"]["record"]["properties"]["thickness"]["value"] = json!(2.0);
    assert!(
        build(thick, o)
            .unwrap_err()
            .message
            .contains("stack thickness")
    );
}

#[test]
fn closed_offsets_use_cyclic_mitres_and_retain_the_closing_station() {
    let mut p = pattern();
    p["seams"][0]["a"] = json!("front/bottom+right+top+left");
    p["seams"][0]["b"] = json!("back/bottom+right+top+left");
    p["seams"][0]["reverse"] = json!(false);
    let mut o = offset(3.0, 4.0);
    o["machine"]["stitch_mm"] = json!([0.01, 5.0]);
    let a = build(p, o).unwrap();
    let s = &a.seams[0];
    near(s.stations.len_a, 216.0);
    near(s.stations.len_b, 208.0);
    assert_eq!(s.stations.a.first(), s.stations.a.last());
    assert_eq!(s.stations.b.first(), s.stations.b.last());
    assert_eq!(s.stations.a[0], Vec2::new(3.0, 3.0));
    assert_eq!(s.stations.b[0], Vec2::new(4.0, 4.0));
}

//! Synthetic independent pressure/stack answers through the production seam planner.
use ftx_export::sewing::{SewOptions, SewPlan, pressure::MODEL, program::SewProgram};
use ftx_pattern::Pattern;
use ftx_pattern::material_compression::{Input, condition};
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

fn plan(p: &Value, o: &Value) -> Result<SewPlan, ftx_core::Refusal> {
    SewPlan::build(
        &Pattern::new(p.clone()),
        &serde_json::from_value(o.clone()).unwrap(),
    )
}

fn pressure(p: &Value, mut o: Value, pa: f64) -> Value {
    let baseline = plan(p, &o).unwrap();
    let mut materials = serde_json::Map::new();
    for (hash, selection) in &baseline.materials {
        let material = selection.record.as_ref().unwrap();
        let mm = material.properties["thickness"].value;
        assert_eq!(material.properties["thickness"].unit, "mm");
        let source = material.properties["thickness"].source.clone();
        let row = |pressure_pa, value| {
            json!({"pressure_pa":pressure_pa,"properties":{
            "thickness":{"value":value,"unit":"mm","bounds":[0.9*value,1.1*value],
                "evidence":"estimate","source":source,"method":"Synthetic pressure/loft oracle",
                "conditions":{"specimen":material.id,"conditioning":"synthetic dry",
                    "loading_branch":"first loading"}}}})
        };
        materials.insert(
            hash.clone(),
            json!({
                "schema":"ftx-material-compression/1","material":material,"specimen":material.id,
                "conditioning":"synthetic dry","loading_branch":"first loading",
                "method":"Independent synthetic ply observations, not a garment measurement",
                "conditioned_on":"2026-10-20","pressure_pa":pa,
                "samples":[row(0.0,mm),row(500.0,mm*0.5)],"allow_estimates":true
            }),
        );
    }
    o["machine"]["presser_foot"] = json!({"pressure_pa":[100,1000],"max_loaded_stack_mm":0.7});
    o["passes"][0]["presser_foot"] = json!({"model":MODEL,"pressure_pa":pa,
        "conditioning":"synthetic dry","loading_branch":"first loading","materials":materials});
    o
}

fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-12, "{a} != {b}");
}

fn clearance(p: &Value, o: Value, gap_mm: f64, area_mm2: f64) -> Value {
    let mut o = pressure(p, o, 0.0);
    o["machine"]["presser_foot"]["normal_force_n"] = json!([0.001, 0.2]);
    let foot = &mut o["passes"][0]["presser_foot"];
    foot.as_object_mut().unwrap().remove("pressure_pa");
    foot["model"] = json!(ftx_export::sewing::pressure::CLEARANCE_MODEL);
    foot["clearance"] = json!({"gap_mm":gap_mm,"reference_area_mm2":area_mm2,
        "direction":"loading","allow_estimates":true});
    o
}

#[test]
fn clearance_infers_force_and_work_through_the_same_stack_authority() {
    let p = pattern();
    let baseline = plan(&p, &options()).unwrap();
    let o = clearance(&p, options(), 0.6, 200.0);
    let got = plan(&p, &o).unwrap();
    assert_eq!(got.schema, "ftx-sewing-plan/8");
    assert_eq!(got.seams[0].stations, baseline.seams[0].stations);
    assert_eq!(got.pattern_sha256, baseline.pattern_sha256);
    assert_eq!(
        serde_json::to_value(&got.materials).unwrap(),
        serde_json::to_value(&baseline.materials).unwrap()
    );
    close(got.seams[0].stack_mm, 0.8);
    close(got.feed_seconds, baseline.feed_seconds);
    let foot = got.seams[0].presser_foot.as_ref().unwrap();
    let r = foot.closure.as_ref().unwrap();
    close(foot.pressure_pa, 250.0);
    close(foot.nominal_stack_mm, 0.6);
    close(foot.stack_mm, 0.66);
    close(r.normal_force_n, 0.05);
    close(r.external_work_j, 0.000005);
    assert!(foot.materials.is_empty()); // inverse receipt retains each physical ply instead
    assert_eq!(r.layers.len(), 2);
    assert_eq!(
        serde_json::to_value(r).unwrap(),
        serde_json::to_value(ftx_pattern::material_compression::stack::close(&r.input).unwrap())
            .unwrap()
    );
    for (layer, solved) in foot.layers.iter().zip(&r.layers) {
        assert_eq!(
            Some(layer.conditioned_material_sha256.as_str()),
            solved.conditioned.selection.expected_sha256.as_deref()
        );
        close(layer.nominal_thickness_mm, solved.thickness_m * 1000.0);
    }
    assert_ne!(
        foot.layers[0].conditioned_material_sha256,
        foot.layers[1].conditioned_material_sha256
    );
    let program = SewProgram::build(&Pattern::new(p), &serde_json::from_value(o).unwrap()).unwrap();
    assert_eq!(program.plan.schema, "ftx-sewing-plan/8");
    assert_eq!(program.stitches.len(), 40);
    assert!(!got.hardware_execution_authorized && !got.physical_seam_quality_validated);
}

#[test]
fn explicit_area_scales_force_and_work_without_changing_pressure_or_geometry() {
    let p = pattern();
    let a = plan(&p, &clearance(&p, options(), 0.6, 200.0)).unwrap();
    let b = plan(&p, &clearance(&p, options(), 0.6, 400.0)).unwrap();
    let fa = a.seams[0].presser_foot.as_ref().unwrap();
    let fb = b.seams[0].presser_foot.as_ref().unwrap();
    assert_eq!(a.seams[0].stations, b.seams[0].stations);
    assert_eq!(fa.pressure_pa, fb.pressure_pa);
    assert_eq!(fa.nominal_stack_mm, fb.nominal_stack_mm);
    close(
        fb.closure.as_ref().unwrap().normal_force_n,
        2.0 * fa.closure.as_ref().unwrap().normal_force_n,
    );
    close(
        fb.closure.as_ref().unwrap().external_work_j,
        2.0 * fa.closure.as_ref().unwrap().external_work_j,
    );
    let mut blocked = clearance(&p, options(), 0.6, 2000.0);
    assert!(
        plan(&p, &blocked)
            .unwrap_err()
            .message
            .contains("normal force")
    );
    blocked["machine"]["presser_foot"]["normal_force_n"] = json!([0.001, 0.6]);
    plan(&p, &blocked).unwrap();
}

#[test]
fn every_fold_is_part_of_the_inverse_not_just_two_named_seam_sides() {
    let p = pattern();
    let mut o = options();
    o["passes"][0].as_object_mut().unwrap().remove("plies");
    o["passes"][0]["assembly"] = json!({
        "pieces":[{"instance":"front","origin_mm":[0,0],"x_axis":[1,0],"face_down":false},
            {"instance":"back","origin_mm":[40,0],"x_axis":[-1,0],"face_down":true}],
        "folds":[{"line_mm":[[30,-20],[30,120]],"moving_side":"right","stack":"over"}],
        "registration_tolerance_mm":1e-6,"minimum_boundary_clearance_mm":1.0});
    let mut o = clearance(&p, o, 0.9, 200.0);
    o["machine"]["presser_foot"]["max_loaded_stack_mm"] = json!(1.0);
    let got = plan(&p, &o).unwrap();
    let seam = &got.seams[0];
    let foot = seam.presser_foot.as_ref().unwrap();
    assert_eq!(foot.layers.len(), 3);
    close(seam.stack_mm, 1.2);
    close(foot.nominal_stack_mm, 0.9);
    close(foot.stack_mm, 0.99);
    close(foot.pressure_pa, 250.0);
    close(foot.closure.as_ref().unwrap().normal_force_n, 0.05);
    for (ply, original) in foot
        .layers
        .iter()
        .zip(&seam.assembly.as_ref().unwrap().layers)
    {
        assert_eq!(ply.material_sha256, original.material_sha256);
    }
}

#[test]
fn clearance_refuses_unsupported_branch_unknown_area_conflicting_controls_and_weak_consent() {
    let p = pattern();
    let base = clearance(&p, options(), 0.6, 200.0);
    for (field, value) in [
        ("gap_mm", json!(0.3)),
        ("gap_mm", json!(0.9)),
        ("reference_area_mm2", json!(0)),
        ("allow_estimates", json!(false)),
        ("direction", json!("unloading")),
    ] {
        let mut o = base.clone();
        o["passes"][0]["presser_foot"]["clearance"][field] = value;
        assert!(plan(&p, &o).is_err(), "{field}");
    }
    let mut conflicting = base.clone();
    conflicting["passes"][0]["presser_foot"]["pressure_pa"] = json!(250);
    assert!(plan(&p, &conflicting).is_err());
    let mut no_force = base.clone();
    no_force["machine"]["presser_foot"]
        .as_object_mut()
        .unwrap()
        .remove("normal_force_n");
    assert!(plan(&p, &no_force).is_err());
    let mut low_pressure = base.clone();
    low_pressure["machine"]["presser_foot"]["pressure_pa"] = json!([100, 200]);
    assert!(plan(&p, &low_pressure).is_err());
    let mut entry = base;
    entry["machine"]["max_stack_mm"] = json!(0.7);
    assert!(
        plan(&p, &entry)
            .unwrap_err()
            .message
            .contains("stack thickness")
    );
}

#[test]
fn central_clearance_does_not_hide_upper_bounds_or_nonunique_pressure() {
    let p = pattern();
    let mut o = clearance(&p, options(), 0.6, 200.0);
    o["machine"]["presser_foot"]["max_loaded_stack_mm"] = json!(0.62);
    assert!(
        plan(&p, &o)
            .unwrap_err()
            .message
            .contains("loaded machine limit")
    );
    o = clearance(&p, options(), 0.8, 200.0);
    for table in o["passes"][0]["presser_foot"]["materials"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        table["samples"][1]["properties"] = table["samples"][0]["properties"].clone();
    }
    assert!(plan(&p, &o).unwrap_err().message.contains("nonunique"));
}

#[test]
fn an_unloading_branch_has_negative_external_work_not_invented_dissipated_heat() {
    let p = pattern();
    let mut o = clearance(&p, options(), 0.6, 200.0);
    let foot = &mut o["passes"][0]["presser_foot"];
    foot["clearance"]["direction"] = json!("unloading");
    foot["loading_branch"] = json!("qualified unloading");
    for table in foot["materials"].as_object_mut().unwrap().values_mut() {
        table["pressure_pa"] = json!(500);
        table["loading_branch"] = json!("qualified unloading");
        for row in table["samples"].as_array_mut().unwrap() {
            row["properties"]["thickness"]["conditions"]["loading_branch"] =
                json!("qualified unloading");
        }
    }
    let got = plan(&p, &o).unwrap();
    let r = got.seams[0]
        .presser_foot
        .as_ref()
        .unwrap()
        .closure
        .as_ref()
        .unwrap();
    close(r.pressure_pa, 250.0);
    close(r.initial_thickness_m, 0.0004);
    close(r.external_work_j, -0.000015);
}

#[test]
fn material_compressibility_alone_changes_force_feasibility_at_the_same_gap() {
    let p = pattern();
    let mut soft = clearance(&p, options(), 0.6, 200.0);
    soft["machine"]["presser_foot"]["normal_force_n"] = json!([0.001, 0.075]);
    let a = plan(&p, &soft).unwrap();
    let mut firm_pattern = p.clone();
    firm_pattern["fabric"]["material"]["record"]["name"] = json!("Another synthetic construction");
    let mut firm = clearance(&firm_pattern, options(), 0.6, 200.0);
    for table in firm["passes"][0]["presser_foot"]["materials"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        table["samples"][1]["properties"]["thickness"]["value"] = json!(0.3);
        table["samples"][1]["properties"]["thickness"]["bounds"] = json!([0.27, 0.33]);
    }
    firm["machine"]["presser_foot"]["normal_force_n"] = json!([0.001, 0.075]);
    assert!(
        plan(&firm_pattern, &firm)
            .unwrap_err()
            .message
            .contains("normal force")
    );
    firm["machine"]["presser_foot"]["normal_force_n"] = json!([0.001, 0.2]);
    let b = plan(&firm_pattern, &firm).unwrap();
    assert_eq!(p["pieces"], firm_pattern["pieces"]);
    assert_eq!(p["seams"], firm_pattern["seams"]);
    assert_eq!(a.seams[0].stations, b.seams[0].stations);
    assert_eq!(a.seams[0].stack_mm, b.seams[0].stack_mm);
    let fa = a.seams[0].presser_foot.as_ref().unwrap();
    let fb = b.seams[0].presser_foot.as_ref().unwrap();
    close(fa.nominal_stack_mm, fb.nominal_stack_mm);
    close(fb.pressure_pa, 2.0 * fa.pressure_pa);
    close(
        fb.closure.as_ref().unwrap().normal_force_n,
        2.0 * fa.closure.as_ref().unwrap().normal_force_n,
    );
}

#[test]
fn a_pressure_recipe_does_not_silently_qualify_clearance_area_or_direction() {
    let p = pattern();
    let mut o = clearance(&p, options(), 0.6, 200.0);
    let baseline = plan(&p, &o).unwrap();
    let hash = baseline.materials.keys().next().unwrap();
    o["require_qualification"] = json!(true);
    o["qualifications"] = json!([{"id":"synthetic-closure-process","materials":[hash,hash],
        "machine":o["machine"]["id"],"needle":o["passes"][0]["needle"],"thread":o["passes"][0]["thread"],
        "stitch_mm":[0.5,5],"feed_b_over_a":[0.5,2],"stack_mm":[0.7,0.9],
        "stitches_per_minute":[500,700],"allow_reverse":false,"evidence":"estimate","allow_estimates":true,
        "source":{"reference":"test:clearance-coupon","accessed_on":"2026-10-20","note":"Synthetic process only"},
        "method":"Synthetic closure envelope",
        "presser_foot":{"pressure_pa":[200,300],"loaded_stack_mm":[0.6,0.7],
            "conditioning":"synthetic dry","loading_branch":"first loading"}}]);
    assert!(plan(&p, &o).unwrap_err().message.contains("qualified"));
    o["qualifications"][0]["presser_foot"]["clearance"] = json!({"gap_mm":[0.59,0.61],
        "reference_area_mm2":[190,210],"normal_force_n":[0.04,0.06],"direction":"loading"});
    let got = plan(&p, &o).unwrap();
    assert_eq!(
        got.seams[0].qualification_id.as_deref(),
        Some("synthetic-closure-process")
    );
    for (field, value) in [
        ("gap_mm", json!([0.7, 0.8])),
        ("reference_area_mm2", json!([300, 400])),
        ("normal_force_n", json!([0.01, 0.03])),
        ("direction", json!("unloading")),
    ] {
        let mut changed = o.clone();
        changed["qualifications"][0]["presser_foot"]["clearance"][field] = value;
        assert!(plan(&p, &changed).is_err(), "{field}");
    }
}

#[test]
fn inverse_limits_and_sheet_consumption_policy_are_not_relaxed() {
    let mut p = pattern();
    p["fabric"]["material"]["allow_estimates"] = json!(false);
    p["fabric"]["material"]["record"]["properties"]["thickness"]["evidence"] = json!("published");
    let o = clearance(&p, options(), 0.6, 200.0);
    let error = plan(&p, &o).unwrap_err();
    assert!(error.message.contains("estimate"), "{error}");
    let p = pattern();
    let mut o = options();
    o["passes"][0]["plies"] = json!([17, 16]);
    o["machine"]["max_stack_mm"] = json!(20.0);
    let mut o = clearance(&p, o, 9.9, 200.0);
    o["machine"]["presser_foot"]["max_loaded_stack_mm"] = json!(20.0);
    assert!(
        plan(&p, &o)
            .unwrap_err()
            .message
            .contains("32 actual plies")
    );
}

#[test]
fn prescribed_pressure_reuses_material_authority_without_rewriting_pattern_or_stitches() {
    let p = pattern();
    let baseline = plan(&p, &options()).unwrap();
    let o = pressure(&p, options(), 250.0);
    let before = o.clone();
    let got = plan(&p, &o).unwrap();
    assert_eq!(got.schema, "ftx-sewing-plan/7");
    assert_eq!(got.seams[0].stations, baseline.seams[0].stations);
    assert_eq!(got.pattern_sha256, baseline.pattern_sha256);
    assert_eq!(
        serde_json::to_value(&got.materials).unwrap(),
        serde_json::to_value(&baseline.materials).unwrap()
    );
    close(got.seams[0].stack_mm, 0.8);
    close(got.feed_seconds, baseline.feed_seconds);
    let foot = got.seams[0].presser_foot.as_ref().unwrap();
    close(foot.nominal_stack_mm, 0.6);
    close(foot.stack_mm, 0.66);
    assert_eq!(foot.layers.len(), 2);
    close(foot.layers[1].z_bounds_mm[0], 0.33);
    close(foot.layers[1].z_bounds_mm[1], 0.66);
    for layer in &foot.layers {
        assert!(layer.uses_thickness_upper_bound);
        let report = &foot.materials[&layer.material_sha256];
        assert_eq!(
            Some(layer.conditioned_material_sha256.as_str()),
            report.selection.expected_sha256.as_deref()
        );
        assert!(report.interpolated);
        assert_eq!(report.bracket_pa, [0.0, 500.0]);
        assert_eq!(
            serde_json::to_value(report).unwrap(),
            serde_json::to_value(condition(&report.input).unwrap()).unwrap()
        );
    }
    assert_eq!(before, o);
    assert!(!got.hardware_execution_authorized && !got.physical_seam_quality_validated);
    let program = SewProgram::build(&Pattern::new(p), &serde_json::from_value(o).unwrap()).unwrap();
    assert_eq!(program.plan.schema, "ftx-sewing-plan/7");
    assert_eq!(program.stitches.len(), 40);
    assert_eq!(
        serde_json::to_value(&program.plan).unwrap(),
        serde_json::to_value(&got).unwrap()
    );
}

#[test]
fn loaded_bounds_and_unloaded_entry_limits_are_independent() {
    let p = pattern();
    let mut low = pressure(&p, options(), 250.0);
    low["machine"]["presser_foot"]["max_loaded_stack_mm"] = json!(0.5);
    assert!(
        plan(&p, &low)
            .unwrap_err()
            .message
            .contains("loaded machine limit")
    );
    let mut high = pressure(&p, options(), 500.0);
    high["machine"]["presser_foot"]["max_loaded_stack_mm"] = json!(0.5);
    let accepted = plan(&p, &high).unwrap();
    close(
        accepted.seams[0].presser_foot.as_ref().unwrap().stack_mm,
        0.44,
    );
    high["machine"]["max_stack_mm"] = json!(0.7);
    assert!(
        plan(&p, &high)
            .unwrap_err()
            .message
            .contains("stack thickness")
    );
    high["machine"]["max_stack_mm"] = json!(3.0);
    high["machine"]["presser_foot"]["max_loaded_stack_mm"] = json!(0.42);
    assert!(
        plan(&p, &high)
            .unwrap_err()
            .message
            .contains("loaded machine limit")
    );
    high["machine"]["presser_foot"]["max_loaded_stack_mm"] = json!(0.5);
    high["machine"]["presser_foot"]["pressure_pa"] = json!([100, 499]);
    assert!(plan(&p, &high).is_err());
}

#[test]
fn every_folded_ply_has_its_own_material_loaded_thickness_and_original_preimage() {
    let mut p = pattern();
    let mut lining = p["fabric"].clone();
    lining["material"]["id"] = json!("synthetic-lining");
    lining["material"]["record"]["id"] = json!("synthetic-lining");
    lining["material"]["record"]["properties"]["thickness"]["value"] = json!(0.2);
    p["pieces"][1]["fabric"] = lining;
    let mut o = options();
    o["passes"][0].as_object_mut().unwrap().remove("plies");
    o["passes"][0]["assembly"] = json!({
        "pieces":[{"instance":"front","origin_mm":[0,0],"x_axis":[1,0],"face_down":false},
            {"instance":"back","origin_mm":[40,0],"x_axis":[-1,0],"face_down":true}],
        "folds":[{"line_mm":[[30,-20],[30,120]],"moving_side":"right","stack":"over"}],
        "registration_tolerance_mm":1e-6,"minimum_boundary_clearance_mm":1.0
    });
    let baseline = plan(&p, &o).unwrap();
    let o = pressure(&p, o, 500.0);
    let got = plan(&p, &o).unwrap();
    let seam = &got.seams[0];
    assert_eq!(
        serde_json::to_value(&seam.assembly).unwrap(),
        serde_json::to_value(&baseline.seams[0].assembly).unwrap()
    );
    let assembly = seam.assembly.as_ref().unwrap();
    let foot = seam.presser_foot.as_ref().unwrap();
    assert_eq!(foot.layers.len(), 3);
    assert_eq!(foot.materials.len(), 2);
    close(foot.stack_mm, 0.44);
    close(foot.layers[0].thickness_mm, 0.22);
    close(foot.layers[1].thickness_mm, 0.11);
    close(foot.layers[2].thickness_mm, 0.11);
    for (a, b) in foot.layers.iter().zip(&assembly.layers) {
        assert_eq!(a.material_sha256, b.material_sha256);
    }
    let program = SewProgram::build(&Pattern::new(p), &serde_json::from_value(o).unwrap()).unwrap();
    assert_eq!(program.schema, "ftx-sewing-program/3");
    assert_eq!(program.plan.schema, "ftx-sewing-plan/7");
    assert!(program.stitches.iter().all(|s| s.assembly_mm.is_some()));
}

#[test]
fn a_material_only_substitution_cannot_reuse_another_lots_compression() {
    let p = pattern();
    let o = pressure(&p, options(), 500.0);
    let mut swapped = p.clone();
    swapped["fabric"]["material"]["record"]["name"] = json!("Another lot with the same thickness");
    assert!(
        plan(&swapped, &o)
            .unwrap_err()
            .message
            .contains("every distinct stack material")
    );
    let mut stale = o.clone();
    let table = stale["passes"][0]["presser_foot"]["materials"]
        .as_object_mut()
        .unwrap()
        .values_mut()
        .next()
        .unwrap();
    table["material"]["sources"]["test"]["note"] = json!("Rebound acquisition");
    assert!(
        plan(&p, &stale)
            .unwrap_err()
            .message
            .contains("exact original material")
    );
    let mut missing = o.clone();
    missing["passes"][0]["presser_foot"]["materials"] = json!({});
    assert!(plan(&p, &missing).is_err());
    let mut extra = o;
    let tables = extra["passes"][0]["presser_foot"]["materials"]
        .as_object_mut()
        .unwrap();
    tables.insert("unused".into(), tables.values().next().unwrap().clone());
    assert!(plan(&p, &extra).is_err());
}

#[test]
fn unsupported_pressure_conditions_missing_foot_and_unadmitted_interpolation_refuse() {
    let p = pattern();
    let o = pressure(&p, options(), 250.0);
    let mut no_foot = o.clone();
    no_foot["machine"]
        .as_object_mut()
        .unwrap()
        .remove("presser_foot");
    assert!(
        plan(&p, &no_foot)
            .unwrap_err()
            .message
            .contains("loaded machine limits")
    );
    for (key, value) in [
        ("conditioning", json!("wet")),
        ("loading_branch", json!("unloading")),
        ("pressure_pa", json!(500)),
        ("allow_estimates", json!(false)),
    ] {
        let mut changed = o.clone();
        let table = changed["passes"][0]["presser_foot"]["materials"]
            .as_object_mut()
            .unwrap()
            .values_mut()
            .next()
            .unwrap();
        table[key] = value;
        assert!(plan(&p, &changed).is_err(), "{key}");
    }
    let outside = pressure(&p, options(), 501.0);
    assert!(
        plan(&p, &outside)
            .unwrap_err()
            .message
            .contains("no extrapolation")
    );
    let mut model = o;
    model["passes"][0]["presser_foot"]["model"] = json!("invent-a-contact-area");
    assert!(plan(&p, &model).is_err());
}

#[test]
fn interpolation_permission_cannot_override_the_original_sheet_policy() {
    let mut p = pattern();
    // Measured-class flags test the permission boundary, not empirical truth of the synthetic data.
    p["fabric"]["material"]["allow_estimates"] = json!(false);
    p["fabric"]["material"]["record"]["properties"]["thickness"]["evidence"] = json!("measured");
    let mut o = pressure(&p, options(), 250.0);
    for table in o["passes"][0]["presser_foot"]["materials"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        for row in table["samples"].as_array_mut().unwrap() {
            row["properties"]["thickness"]["evidence"] = json!("measured");
        }
    }
    assert!(plan(&p, &o).is_err());
    o["passes"][0]["presser_foot"]["pressure_pa"] = json!(500);
    for table in o["passes"][0]["presser_foot"]["materials"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        table["pressure_pa"] = json!(500);
    }
    plan(&p, &o).unwrap(); // Exact measured-class observation, with no interpolated estimate.
}

#[test]
fn a_dry_recipe_never_implicitly_qualifies_a_pressure_conditioned_operation() {
    let p = pattern();
    let mut o = pressure(&p, options(), 500.0);
    let baseline = plan(&p, &o).unwrap();
    let hash = baseline.materials.keys().next().unwrap();
    o["require_qualification"] = json!(true);
    o["qualifications"] = json!([{"id":"synthetic-process","materials":[hash,hash],
        "machine":o["machine"]["id"],"needle":o["passes"][0]["needle"],"thread":o["passes"][0]["thread"],
        "stitch_mm":[0.5,5],"feed_b_over_a":[0.5,2],"stack_mm":[0.7,0.9],
        "stitches_per_minute":[500,700],"allow_reverse":false,"evidence":"estimate","allow_estimates":true,
        "source":{"reference":"test:presser-coupon","accessed_on":"2026-10-20","note":"Synthetic, not a measured seam"},
        "method":"Synthetic process envelope"}]);
    assert!(plan(&p, &o).unwrap_err().message.contains("qualified"));
    o["qualifications"][0]["presser_foot"] = json!({"pressure_pa":[250,500],"loaded_stack_mm":[0.4,0.45],
        "conditioning":"synthetic dry","loading_branch":"first loading"});
    let qualified = plan(&p, &o).unwrap();
    assert_eq!(
        qualified.seams[0].qualification_id.as_deref(),
        Some("synthetic-process")
    );
    let program = SewProgram::build(
        &Pattern::new(p.clone()),
        &serde_json::from_value(o.clone()).unwrap(),
    )
    .unwrap();
    assert_eq!(program.plan.schema, "ftx-sewing-plan/7");
    assert_eq!(
        program.plan.seams[0].qualification_id.as_deref(),
        Some("synthetic-process")
    );
    for (key, value) in [
        ("loaded_stack_mm", json!([0.4, 0.42])),
        ("pressure_pa", json!([250, 499])),
        ("conditioning", json!("wet")),
        ("loading_branch", json!("unloading")),
    ] {
        let mut changed = o.clone();
        changed["qualifications"][0]["presser_foot"][key] = value;
        assert!(plan(&p, &changed).is_err(), "{key}");
    }
}

#[test]
fn ordinary_plan_serialization_is_unchanged_when_pressure_is_absent() {
    let original = options();
    let o: SewOptions = serde_json::from_value(original.clone()).unwrap();
    let got = serde_json::to_value(o).unwrap();
    assert!(got["machine"].get("presser_foot").is_none());
    assert!(got["passes"][0].get("presser_foot").is_none());
    let result = serde_json::to_value(plan(&pattern(), &original).unwrap()).unwrap();
    assert_eq!(result["schema"], "ftx-sewing-plan/1");
    assert!(result["seams"][0].get("presser_foot").is_none());
}

#[test]
fn request_roundtrip_retains_original_compression_observations() {
    let p = pattern();
    let o = pressure(&p, options(), 500.0);
    let source = o["passes"][0]["presser_foot"]["materials"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap();
    let input: Input = serde_json::from_value(source.clone()).unwrap();
    let result = plan(&p, &o).unwrap();
    let retained = result.seams[0]
        .presser_foot
        .as_ref()
        .unwrap()
        .materials
        .values()
        .next()
        .unwrap();
    assert_eq!(
        retained.input_sha256,
        condition(&input).unwrap().input_sha256
    );
    assert!(!retained.interpolated);
    assert_eq!(
        serde_json::to_value(&retained.input).unwrap(),
        serde_json::to_value(input).unwrap()
    );
}

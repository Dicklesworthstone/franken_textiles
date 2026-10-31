//! Same file-free entry point called by the WASM bindings, checked against the production core.
use ftx_export::sewing::{SewOptions, SewPlan, program::SewProgram};
use ftx_pattern::Pattern;
use ftx_wasm::manufacturing;
use serde_json::{Value, json};

const PATTERN: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-sewing-pattern.json");
const OPTIONS: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-sewing-multipass-options.json");

#[test]
fn roll_job_keeps_exact_native_plan_and_each_preview_with_one_shared_result() {
    let mut raw: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-stock-pattern.json"
    ))
    .unwrap();
    raw["fabric"]["material"]["record"]["form"] = json!("woven");
    raw["fabric"]["material"]["record"]["properties"]["width"] = json!({
        "value":0.03,"unit":"m","evidence":"estimate","source":"test",
        "method":"Synthetic width for same-plan marker replay, not production evidence"
    });
    let pattern = raw.to_string();
    let options = json!({"copies":3,"gap_mm":1,"edge_margin_mm":1,"end_margin_mm":1,
        "seam_allowance_mm":0,"maximum_marker_length_m":0.05})
    .to_string();
    let native = ftx_export::cutting::CutPlan::build(
        &Pattern::from_json(&pattern).unwrap(),
        &serde_json::from_str(&options).unwrap(),
    )
    .unwrap();
    assert!(native.rolls.len() > 1);
    let expected = serde_json::to_string(&native).unwrap();
    for index in 0..native.rolls.len() {
        let job: Value =
            serde_json::from_str(&manufacturing::roll_job(&pattern, &options, index).unwrap())
                .unwrap();
        assert_eq!(job["schema"], "ftx-roll-job/1");
        assert_eq!(job["roll_index"], index);
        assert_eq!(job["plan_json"], expected);
        assert_eq!(
            job["preview_svg"],
            ftx_export::cutting::preview::svg(&native, index).unwrap()
        );
        assert!(job["preview_refusal"].is_null());
    }
    assert!(manufacturing::roll_job(&pattern, &options, native.rolls.len()).is_err());
    assert!(manufacturing::roll_job("invalid", &options, 0).is_err());
}

#[test]
fn browser_entry_point_preserves_core_material_receipts_and_directed_program() {
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let options: SewOptions = serde_json::from_str(OPTIONS).unwrap();
    let plan: Value =
        serde_json::from_str(&manufacturing::sew_plan(PATTERN, OPTIONS).unwrap()).unwrap();
    assert_eq!(
        plan,
        serde_json::to_value(SewPlan::build(&pattern, &options).unwrap()).unwrap()
    );
    let program: Value =
        serde_json::from_str(&manufacturing::sew_stitches(PATTERN, OPTIONS).unwrap()).unwrap();
    assert_eq!(
        program,
        serde_json::to_value(SewProgram::build(&pattern, &options).unwrap()).unwrap()
    );
    assert_eq!(program["stitches"].as_array().unwrap().len(), 90);
    assert_eq!(program["plan"]["hardware_execution_authorized"], false);
}

#[test]
fn browser_entry_point_refuses_missing_reverse_and_unqualified_materials() {
    let mut options: Value = serde_json::from_str(OPTIONS).unwrap();
    options["machine"]
        .as_object_mut()
        .unwrap()
        .remove("reverse");
    assert!(
        manufacturing::sew_stitches(PATTERN, &options.to_string())
            .unwrap_err()
            .message
            .contains("explicit reverse")
    );
    let mut p: Value = serde_json::from_str(PATTERN).unwrap();
    p["fabric"]["material"]["allow_estimates"] = json!(false);
    assert!(manufacturing::sew_plan(&p.to_string(), OPTIONS).is_err());
    p["fabric"]["material"]["allow_estimates"] = json!(true);
    p["fabric"]["material"]["record"]["properties"]["thickness"]["value"] = json!(1.1);
    assert!(
        manufacturing::sew_stitches(&p.to_string(), OPTIONS)
            .unwrap_err()
            .message
            .contains("reinforce")
    );
    assert!(manufacturing::sew_stitches("not JSON", OPTIONS).is_err());
    assert!(manufacturing::sew_plan(PATTERN, "{}").is_err());
}

#[test]
fn offset_paths_and_svg_share_the_exact_native_planner() {
    let options =
        include_str!("../../../tests/fixtures/materials/synthetic-sewing-topstitch-options.json");
    let p = Pattern::from_json(PATTERN).unwrap();
    let o: SewOptions = serde_json::from_str(options).unwrap();
    let plan: Value =
        serde_json::from_str(&manufacturing::sew_plan(PATTERN, options).unwrap()).unwrap();
    assert_eq!(
        plan,
        serde_json::to_value(SewPlan::build(&p, &o).unwrap()).unwrap()
    );
    assert_eq!(plan["schema"], "ftx-sewing-plan/3");
    let program: Value =
        serde_json::from_str(&manufacturing::sew_stitches(PATTERN, options).unwrap()).unwrap();
    assert_eq!(
        program,
        serde_json::to_value(SewProgram::build(&p, &o).unwrap()).unwrap()
    );
    assert_eq!(
        manufacturing::sew_svg(PATTERN, options, "topstitch").unwrap(),
        ftx_export::sewing::preview::svg(&p, &o, "topstitch").unwrap()
    );
    assert!(manufacturing::sew_svg(PATTERN, options, "unknown").is_err());
    let mut bad: Value = serde_json::from_str(options).unwrap();
    bad["passes"][1]["stitch_path"]["offset_mm"] = json!([-11.0, 0.0]);
    assert!(manufacturing::sew_svg(PATTERN, &bad.to_string(), "join").is_err());
}

#[test]
fn setup_ordering_survives_native_wasm_boundary_and_stitch_expansion() {
    let mut options: Value = serde_json::from_str(OPTIONS).unwrap();
    let first = options["passes"][0].clone();
    let mut second = first.clone();
    second["id"] = json!("middle");
    second["needle"] = json!("another-needle");
    second["thread"] = json!("another-thread");
    let last = options["passes"][1].clone();
    options["passes"] = json!([first, second, last]);
    options["ordering"] = json!({"objective":"minimize_setup_changes","max_search_nodes":1000,
        "initial_setup":{"needle":"operator-selected-needle","thread":"operator-selected-thread"}});
    let input = options.to_string();
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let typed: SewOptions = serde_json::from_str(&input).unwrap();
    let plan: Value =
        serde_json::from_str(&manufacturing::sew_plan(PATTERN, &input).unwrap()).unwrap();
    let program: Value =
        serde_json::from_str(&manufacturing::sew_stitches(PATTERN, &input).unwrap()).unwrap();
    assert_eq!(
        plan,
        serde_json::to_value(SewPlan::build(&pattern, &typed).unwrap()).unwrap()
    );
    assert_eq!(
        program,
        serde_json::to_value(SewProgram::build(&pattern, &typed).unwrap()).unwrap()
    );
    assert_eq!(program["plan"], plan);
    assert_eq!(plan["ordering"]["baseline_changes"]["total_changes"], 4);
    assert_eq!(plan["ordering"]["selected_changes"]["total_changes"], 2);
    assert_eq!(
        plan["ordering"]["selected_order"],
        json!(["join", "reinforce", "middle"])
    );
    assert_eq!(plan["ordering"]["optimality_proven"], true);
    assert_eq!(plan["hardware_execution_authorized"], false);
    let mut denied: Value = serde_json::from_str(PATTERN).unwrap();
    denied["fabric"]["material"]["allow_estimates"] = json!(false);
    assert!(manufacturing::sew_stitches(&denied.to_string(), &input).is_err());
}

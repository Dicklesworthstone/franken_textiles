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

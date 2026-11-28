//! Measured girths and an accepted synthetic shape reach the real three-branch knit compiler.
//! The existing synthetic closed prism tests source reuse, never anatomical or physical fit.

use super::{invoke, model_bytes_with_locals, receipt, setup};
use ftx_core::receipt::sha256_hex;
use ftx_pattern::BodyMeasure;
use serde_json::{Value, json};
use std::path::Path;

const CREATED: &str = "2026-11-28T12:00";
const SHAPE: &str = "synthetic-head-radius";

fn input() -> Value {
    json!({
        "machine":"hand-knit", "use_priors":false,
        "gauge":{"wales_per_100mm":20.0,"courses_per_100mm":25.0,"needle_pitch_mm":4.0,
            "yarn_tex":180.0,"thickness_mm":2.0,"relaxed_after_wash":true},
        "design":{"name":"synthetic-measured-pullover","wearer_age_class":"adult",
            "chest_ease_mm":21.0,"neck_ease_mm":10.0,"upper_arm_ease_mm":21.0,"wrist_ease_mm":-1.0,
            "yoke_depth_mm":40.0,"body_length_below_arm_mm":80.0,"sleeve_length_below_arm_mm":60.0,
            "hem_rib_courses":2,"collar_rib_courses":2,"cuff_rib_courses":2,
            "cam_setting":13.0,"yarn_id":"synthetic-yarn","body_carrier_id":"2",
            "left_sleeve_carrier_id":"1","right_sleeve_carrier_id":"3","has_hood_or_neck_drawstring":false}
    })
}

fn measurements() -> Value {
    let records: Vec<_> = [
        ("chest", 180.0),
        ("neck_base", 90.0),
        ("upper_arm", 60.0),
        ("wrist", 40.0),
    ]
    .into_iter()
    .map(|(name, value)| {
        ftx_measure::make_record(
            name,
            value,
            1.0,
            "MEASURED_TAPE",
            "synthetic numerical test only",
            None,
            "",
        )
        .unwrap()
    })
    .collect();
    json!({"schema":"fabric-measurements/1","wearer":{},"records":records,
        "fused":{"chest":{"value_mm":999.0}}})
}

fn text(value: &Value) -> String {
    format!("\n{}\n", serde_json::to_string_pretty(value).unwrap())
}

fn run(dir: &Path, output: &str, accepted: bool, expected: i32) -> Value {
    let mut args = vec![
        "knit",
        "measured",
        "--input",
        "design.json",
        "--measurements",
        "measurements.json",
        "--created",
        CREATED,
        "--out",
        output,
    ];
    if accepted {
        args.extend(["--body-fit", "fit.json", "--model", "model.ftxbody"]);
    }
    serde_json::from_slice(&invoke(dir, &args, expected).stdout).unwrap()
}

fn check_artifacts(dir: &Path, native: &Value, portable: &Value) {
    for key in [
        "pattern_sha256",
        "drafting",
        "body_source",
        "vm_report",
        "cert",
        "files",
    ] {
        assert_eq!(native["result"][key], portable[key], "{key}");
    }
    assert_eq!(
        std::fs::read_to_string(dir.join("pattern.knit.json")).unwrap(),
        portable["pattern_json"].as_str().unwrap()
    );
    for entry in portable["files"].as_array().unwrap() {
        let bytes = std::fs::read(dir.join(entry["name"].as_str().unwrap())).unwrap();
        assert_eq!(entry["sha256"], sha256_hex(&bytes));
        assert_eq!(entry["bytes"], bytes.len());
    }
    assert_eq!(portable["fit_claimed"], false);
    assert_eq!(portable["hardware_execution_authorized"], false);
    assert!(!portable["archive_base64"].as_str().unwrap().is_empty());
}

#[test]
fn original_measurements_reach_identical_native_and_portable_pullover_artifacts() {
    let dir = setup();
    let design = text(&input());
    let evidence = text(&measurements());
    std::fs::write(dir.join("design.json"), &design).unwrap();
    std::fs::write(dir.join("measurements.json"), &evidence).unwrap();
    let native = run(&dir, "complete", false, 0);
    let request = json!({"input_json":design,"measurements_json":evidence,"created":CREATED});
    let portable: Value = serde_json::from_str(
        &ftx_wasm::knit_body::knit_body_json(&request.to_string(), &[]).unwrap(),
    )
    .unwrap();
    check_artifacts(&dir.join("complete"), &native, &portable);
    assert_eq!(
        std::fs::read_to_string(dir.join("complete/input.source.json")).unwrap(),
        design
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("complete/measurements.source.json")).unwrap(),
        evidence
    );
    assert_eq!(
        portable["drafting"]["dimensions"]["chest_girth_mm"]["body_value_mm"],
        180.0
    );
    let saved = std::fs::read(dir.join("complete/cert.json")).unwrap();
    run(&dir, "complete", false, 3);
    assert_eq!(
        std::fs::read(dir.join("complete/cert.json")).unwrap(),
        saved
    );

    let mut incomplete = measurements();
    incomplete["records"].as_array_mut().unwrap().pop();
    std::fs::write(dir.join("measurements.json"), text(&incomplete)).unwrap();
    run(&dir, "missing-wrist", false, 3);
    assert!(!dir.join("missing-wrist").exists());

    std::fs::write(dir.join("measurements.json"), &evidence).unwrap();
    let mut unqualified = input();
    unqualified["machine"] = json!("kniterate-7e");
    unqualified["gauge"]["needle_pitch_mm"] = json!(25.4 / 7.0);
    unqualified["design"]["cam_setting"] = json!(7.0);
    std::fs::write(dir.join("design.json"), text(&unqualified)).unwrap();
    let refused = run(&dir, "unqualified", false, 3);
    assert_eq!(refused["refusal"]["code"], "unqualified_process");
    assert!(!dir.join("unqualified").exists());
}

#[test]
fn accepted_shape_supplies_model_girths_without_refitting_or_promoting_tape_readiness() {
    let dir = setup();
    let bytes = model_bytes_with_locals(true, &[SHAPE]);
    let hash = sha256_hex(&bytes);
    let model = ftx_wasm::fold::load_body(&bytes).unwrap();
    std::fs::write(dir.join("model.ftxbody"), &bytes).unwrap();
    let evidence = text(&measurements());
    std::fs::write(dir.join("measurements.json"), &evidence).unwrap();
    let mut design = input();
    // This synthetic prism has a roughly 595 mm chest, 623 mm neck, 694 mm upper arm and
    // 655 mm wrist at the first accepted shape. Author a representable numerical design;
    // source reuse is separate from infant garment construction and its stricter float caps.
    design["design"]["chest_ease_mm"] = json!(100.0);
    design["design"]["neck_ease_mm"] = json!(0.0);
    design["design"]["upper_arm_ease_mm"] = json!(0.0);
    design["design"]["wrist_ease_mm"] = json!(0.0);
    design["design"]["yoke_depth_mm"] = json!(160.0);
    design["design"]["sleeve_length_below_arm_mm"] = json!(160.0);
    let design_text = text(&design);
    std::fs::write(dir.join("design.json"), &design_text).unwrap();
    let fused = ftx_measure::fuse(&measurements(), false).unwrap();
    let mut previous = None;
    for (i, change) in [0.25, 0.75].into_iter().enumerate() {
        let mut accepted = receipt(&hash);
        let layout = json!([{"name":SHAPE,"lower":0.0,"upper":1.0}]);
        accepted["model"]["shape"] = layout.clone();
        accepted["anny_configuration"]["shape_parameters"] = layout;
        accepted["anny_configuration"]["fixed_shape"] = json!({"height":0.6});
        accepted["anny_configuration"]["age_years"] = json!(18.0);
        accepted["solution"]["anny_parameters"]["phenotype"]["age"] =
            json!(ftx_body::age_parameter_for_years(18.0));
        accepted["solution"]["shape"] = json!([change]);
        accepted["solution"]["anny_parameters"]["local_changes"] = json!([change]);
        let source = text(&accepted);
        std::fs::write(dir.join("fit.json"), &source).unwrap();
        let body = ftx_drape::drafting::PreparedPatternBody::prepare(
            &model,
            &hash,
            &fused,
            Some(&source),
            &[],
        )
        .unwrap();
        let output = format!("accepted-{i}");
        let native = run(&dir, &output, true, 0);
        let request = json!({"input_json":design_text,"measurements_json":evidence,
            "body_fit_json":source,"created":CREATED});
        let portable: Value = serde_json::from_str(
            &ftx_wasm::knit_body::knit_body_json(&request.to_string(), &bytes).unwrap(),
        )
        .unwrap();
        check_artifacts(&dir.join(&output), &native, &portable);
        for (dimension, measurement) in [
            ("chest_girth_mm", "chest"),
            ("neck_girth_mm", "neck_base"),
            ("sleeve_upper_arm_girth_mm", "upper_arm"),
            ("sleeve_cuff_girth_mm", "wrist"),
        ] {
            let value = &portable["drafting"]["dimensions"][dimension];
            assert_eq!(value["basis"], "MODEL_FIT");
            assert_eq!(value["body_value_mm"], body.value(measurement).unwrap());
            assert!(value.get("sigma_mm").is_none());
        }
        assert_eq!(portable["body_source"]["body_refitted"], false);
        assert_eq!(
            portable["body_source"]["body_fit_sha256"],
            sha256_hex(source.as_bytes())
        );
        assert!(
            portable["body_source"]["measurement_comparisons"]
                .as_array()
                .unwrap()
                .iter()
                .all(|comparison| comparison["used_to_refit"] == false)
        );
        assert_eq!(
            std::fs::read_to_string(dir.join(&output).join("body_fit.source.json")).unwrap(),
            source
        );
        if let Some(before) = previous {
            assert_ne!(
                portable["drafting"]["dimensions"]["neck_girth_mm"]["body_value_mm"],
                before
            );
        }
        previous =
            Some(portable["drafting"]["dimensions"]["neck_girth_mm"]["body_value_mm"].clone());
    }
    let mut child_source: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("fit.json")).unwrap()).unwrap();
    child_source["anny_configuration"]["age_years"] = json!(2.5);
    child_source["solution"]["anny_parameters"]["phenotype"]["age"] =
        json!(ftx_body::age_parameter_for_years(2.5));
    std::fs::write(dir.join("fit.json"), text(&child_source)).unwrap();
    let refused = run(&dir, "wrong-age-class", true, 3);
    assert_eq!(refused["refusal"]["code"], "material_process_hazard");
    assert!(
        refused["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("wearer_age_class")
    );
    assert!(!dir.join("wrong-age-class").exists());
}

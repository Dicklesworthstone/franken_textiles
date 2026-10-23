//! Actual common material binding and downstream numerical consumers; no browser doubles here.
use ftx_export::cutting::{CutOptions, CutPlan, procurement};
use ftx_pattern::{Pattern, material_binding::Changes};
use ftx_wasm::{manufacturing, materials};
use serde_json::{Value, json};

const PATTERN: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-sewing-pattern.json");
const SEW: &str = include_str!("../../../tests/fixtures/materials/synthetic-sewing-options.json");

fn selection(width: f64, thickness: f64) -> Value {
    let mut s: Value = serde_json::from_str(PATTERN).unwrap();
    s = s["fabric"]["material"].clone();
    let q = |value, unit| {
        json!({"value":value,"unit":unit,"evidence":"estimate",
        "source":"test","method":"Artificial known-answer input"})
    };
    for (key, value, unit) in [
        ("width", width, "m"),
        ("thickness", thickness, "mm"),
        ("areal_density", 0.2, "kg/m2"),
        ("warp_stiffness", 8000., "N/m"),
        ("weft_stiffness", 4000., "N/m"),
        ("poisson_warp_weft", 0.2, "1"),
        ("shear_stiffness", 1000., "N/m"),
        ("bending_rigidity", 1e-4, "N*m"),
        ("friction_skin", 0.3, "1"),
        ("friction_self", 0.3, "1"),
        ("thermal_resistance", 0.2, "m2*K/W"),
        ("specific_heat", 1000., "J/(kg*K)"),
    ] {
        s["record"]["properties"][key] = q(value, unit);
    }
    s
}
fn changes(default: Value) -> Value {
    json!({"schema":"ftx-material-bindings/1","default":default,"pieces":{}})
}

#[test]
fn linear_and_bending_identification_reach_the_actual_material_binding() {
    let linear = include_str!("../../../tests/fixtures/materials/synthetic-swatch.json");
    let bending = include_str!("../../../tests/fixtures/materials/synthetic-bending-coupons.json");
    let expected_linear = serde_json::to_value(
        ftx_pattern::material_calibration::calibrate(&serde_json::from_str(linear).unwrap())
            .unwrap(),
    )
    .unwrap();
    let expected_bending = serde_json::to_value(
        ftx_pattern::material_bending_calibration::calibrate(
            &serde_json::from_str(bending).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    for (input, calibrate, expected) in [
        (
            linear,
            materials::calibrate_linear as fn(&str, bool) -> _,
            expected_linear,
        ),
        (bending, materials::calibrate_bending, expected_bending),
    ] {
        for allow in [false, true] {
            let output: Value = serde_json::from_str(&calibrate(input, allow).unwrap()).unwrap();
            assert_eq!(output["calibration"], expected);
            assert_eq!(output["simulation_ready"], allow);
            assert_eq!(output["selection"]["allow_estimates"], allow);
            assert_eq!(output["empirical_validation_claim"], false);
            assert!(output["calibration"]["check"].is_object());
            let selection = output["selection_json"].as_str().unwrap();
            let inspected: Value =
                serde_json::from_str(&materials::inspect(selection).unwrap()).unwrap();
            assert_eq!(inspected, output["material"]);
            let edits = format!(r#"{{"schema":"ftx-material-bindings/1","default":{selection}}}"#);
            let bound: Value =
                serde_json::from_str(&materials::bind(PATTERN, &edits).unwrap()).unwrap();
            assert_eq!(bound["geometry_changed"], false);
            assert_eq!(bound["drape_materials_with_self_contact"]["ready"], allow);
            assert_eq!(
                bound["pieces"][0]["material_sha256"],
                output["selection"]["expected_sha256"]
            );
        }
        let mut denied: Value = serde_json::from_str(input).unwrap();
        denied["allow_estimated_observations"] = json!(false);
        assert!(calibrate(&denied.to_string(), true).is_err());
        let mut incomplete: Value = serde_json::from_str(input).unwrap();
        incomplete["material"]["properties"]
            .as_object_mut()
            .unwrap()
            .remove("areal_density");
        let partial: Value =
            serde_json::from_str(&calibrate(&incomplete.to_string(), true).unwrap()).unwrap();
        assert_eq!(partial["simulation_ready"], false);
        assert!(partial["selection"]["record"]["properties"]["areal_density"].is_null());
        assert!(partial["mechanics_refusal"].is_object());
        assert!(calibrate("{}", true).is_err());
    }
}

#[test]
fn friction_identification_preserves_pair_identity_and_separate_estimate_admission() {
    let input = include_str!("../../../tests/fixtures/materials/synthetic-friction-sliding.json");
    let catalog = ftx_pattern::textile_import::catalog().unwrap();
    let native =
        ftx_pattern::material_friction::calibrate(&serde_json::from_str(input).unwrap(), &catalog)
            .unwrap();
    for allow in [false, true] {
        let output: Value =
            serde_json::from_str(&materials::calibrate_friction(input, allow).unwrap()).unwrap();
        assert_eq!(
            output["calibration"],
            serde_json::to_value(&native).unwrap()
        );
        assert_eq!(
            output["binding"],
            serde_json::to_value(native.binding(&catalog, allow).unwrap()).unwrap()
        );
        assert_eq!(
            serde_json::from_str::<Value>(output["binding_json"].as_str().unwrap()).unwrap(),
            output["binding"]
        );
        assert!(output["selection"].is_null());
        assert!(output["selection_json"].is_null());
        assert!(output["material"].is_null());
        assert_eq!(output["readiness"]["scope"], "interface_only");
        assert_eq!(output["readiness"]["ready"], allow);
        assert_eq!(output["readiness"]["whole_garment_checked"], false);
        for sheet in output["readiness"]["sheets"].as_array().unwrap() {
            assert_eq!(sheet["ready"], true); // Pair consent never changes either sheet's policy.
        }
        let mut raw: Value = serde_json::from_str(PATTERN).unwrap();
        raw["fabric"]["material"] = output["binding"]["materials"][0].clone();
        raw["pieces"][1]["fabric"] = json!({"material":output["binding"]["materials"][1]});
        let staged: Value = serde_json::from_str(
            &materials::stage_interface(
                &raw.to_string(),
                r#"{"schema":"ftx-material-bindings/1"}"#,
                output["interface_json"].as_str().unwrap(),
                false,
            )
            .unwrap(),
        )
        .unwrap();
        let bound: Value = serde_json::from_str(
            &materials::bind(&raw.to_string(), staged["changes_json"].as_str().unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(bound["drape_materials_with_self_contact"]["ready"], allow);
        assert_eq!(
            bound["pattern"]["fabric"]["interfaces"][0]["allow_estimates"],
            allow
        );
        assert_eq!(
            bound["pieces"][0]["material_sha256"],
            output["binding"]["materials"][0]["expected_sha256"]
        );
    }
    let mut denied: Value = serde_json::from_str(input).unwrap();
    denied["allow_estimated_observations"] = json!(false);
    assert!(materials::calibrate_friction(&denied.to_string(), true).is_err());
}

#[test]
fn friction_body_and_self_outputs_change_only_the_named_contact_property() {
    let input: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-friction-sliding.json"
    ))
    .unwrap();
    let material = input["target"]["materials"][0].clone();
    let original: Value =
        serde_json::from_str(&materials::inspect(&material.to_string()).unwrap()).unwrap();
    for (kind, key) in [("self_contact", "friction_self"), ("body", "friction_skin")] {
        let mut request = input.clone();
        request["target"] = json!({"kind":kind,"material":material});
        if kind == "body" {
            request["target"]["counterface"] = json!({"reference":"test:synthetic-body-surrogate",
                "accessed_on":"2026-10-23","note":"Synthetic counterface for software regression only"});
        }
        assert!(materials::calibrate_friction(&request.to_string(), true).is_err());
        request["replace_existing"] = json!(true);
        for allow in [false, true] {
            let output: Value = serde_json::from_str(
                &materials::calibrate_friction(&request.to_string(), allow).unwrap(),
            )
            .unwrap();
            assert_eq!(output["readiness"]["scope"], "material_mechanics");
            assert_eq!(output["simulation_ready"], allow);
            assert!(output["interface_json"].is_null());
            assert_eq!(output["selection"], output["binding"]["selection"]);
            for (property, quantity) in original["selection"]["record"]["properties"]
                .as_object()
                .unwrap()
            {
                if property != key {
                    assert_eq!(
                        &output["selection"]["record"]["properties"][property],
                        quantity
                    );
                }
            }
            assert_eq!(
                output["selection"]["record"]["properties"][key],
                output["calibration"]["coefficient"]
            );
            let selected = output["selection_json"].as_str().unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(&materials::inspect(selected).unwrap()).unwrap(),
                output["material"]
            );
        }
    }
}

#[test]
fn contact_pair_staging_preserves_other_evidence_and_requires_explicit_replacement() {
    let fit: Value = serde_json::from_str(
        &materials::calibrate_friction(
            include_str!("../../../tests/fixtures/materials/synthetic-friction-sliding.json"),
            true,
        )
        .unwrap(),
    )
    .unwrap();
    let mut raw: Value = serde_json::from_str(PATTERN).unwrap();
    raw["fabric"]["material"] = fit["binding"]["materials"][0].clone();
    raw["pieces"][1]["fabric"] = json!({"material":fit["binding"]["materials"][1]});
    let third: Value =
        serde_json::from_str(&materials::inspect(&selection(1.4, 0.8).to_string()).unwrap())
            .unwrap();
    let mut piece = raw["pieces"][1].clone();
    piece["id"] = json!("lining");
    piece["fabric"] = json!({"material":third["selection"]});
    raw["pieces"].as_array_mut().unwrap().push(piece);
    let original_pair = json!({"materials":[fit["binding"]["materials"][0]["expected_sha256"],third["selection"]["expected_sha256"]],
        "friction":{"value":1,"unit":"1","bounds":[-0.0,1],"source":"pair","evidence":"estimate",
            "method":"Synthetic corrected sliding acquisition; not textile evidence"},
        "sources":{"pair":{"reference":"test:retained-pair","accessed_on":"2026-10-23",
            "note":"Synthetic retained observation"}},"allow_estimates":false});
    raw["fabric"]["interfaces"] = json!([original_pair]);
    let original = raw.to_string();
    let empty = r#"{"schema":"ftx-material-bindings/1"}"#;
    let interface = fit["interface_json"].as_str().unwrap();
    let staged: Value = serde_json::from_str(
        &materials::stage_interface(&original, empty, interface, false).unwrap(),
    )
    .unwrap();
    assert_eq!(staged["interfaces"].as_array().unwrap().len(), 2);
    assert_eq!(
        staged["interfaces"][0].to_string(),
        original_pair.to_string()
    );
    assert!(staged["interfaces_json"].as_str().unwrap().contains("-0.0"));
    assert_eq!(raw.to_string(), original);
    let pending = staged["changes_json"].as_str().unwrap();
    assert!(materials::stage_interface(&original, pending, interface, false).is_err());
    let mut reversed: Value = serde_json::from_str(interface).unwrap();
    reversed["materials"].as_array_mut().unwrap().reverse();
    reversed["allow_estimates"] = json!(false);
    let replaced: Value = serde_json::from_str(
        &materials::stage_interface(&original, pending, &reversed.to_string(), true).unwrap(),
    )
    .unwrap();
    assert_eq!(replaced["interfaces"].as_array().unwrap().len(), 2);
    assert_eq!(
        replaced["interfaces"][0].to_string(),
        original_pair.to_string()
    );
    assert_eq!(replaced["interfaces"][1], reversed);
    let cleared: Value = serde_json::from_str(
        &materials::stage_interface(
            &original,
            r#"{"schema":"ftx-material-bindings/1","interfaces":[]}"#,
            interface,
            false,
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(cleared["interfaces"].as_array().unwrap().len(), 1);
    let duplicate =
        json!({"schema":"ftx-material-bindings/1","interfaces":[original_pair,original_pair]});
    assert!(
        materials::stage_interface(&original, &duplicate.to_string(), interface, true).is_err()
    );
    reversed["materials"][0] = json!("f".repeat(64));
    assert!(materials::stage_interface(&original, empty, &reversed.to_string(), true).is_err());
    let mut stale = raw;
    stale["fabric"]["interfaces"][0]["materials"][1] = json!("e".repeat(64));
    assert!(materials::stage_interface(&stale.to_string(), empty, interface, true).is_err());
}

#[test]
fn permanent_stretch_api_identifies_the_native_law_and_preserves_consumption_policy() {
    let input = include_str!(
        "../../../tests/fixtures/materials/synthetic-membrane-plasticity-calibration.json"
    );
    let native = ftx_pattern::material_membrane_plasticity::calibration::calibrate(
        &serde_json::from_str(input).unwrap(),
    )
    .unwrap();
    let expected = serde_json::to_value(native).unwrap();
    assert_eq!(expected["status"], "ACCEPTED");
    for admission in [false, true] {
        let fitted: Value = serde_json::from_str(
            &materials::calibrate_membrane_plasticity(input, admission).unwrap(),
        )
        .unwrap();
        assert_eq!(fitted["calibration"], expected);
        assert_eq!(fitted["selection"]["allow_estimates"], admission);
        assert_eq!(
            fitted["material"]["capabilities"]["mechanical_coefficients"]["ready"],
            admission
        );
        assert_eq!(fitted["fit_claim"], false);
        assert_eq!(fitted["empirical_validation_claim"], false);
        for (i, (axis, yield_value, hardening)) in [
            ("warp", 10.0, 100.0),
            ("weft", 8.0, 50.0),
            ("shear", 5.0, 25.0),
        ]
        .into_iter()
        .enumerate()
        {
            let parameters = &fitted["calibration"]["parameters"];
            assert!((parameters["yield_n_m"][i].as_f64().unwrap() - yield_value).abs() < 1e-3);
            assert!((parameters["hardening_n_m"][i].as_f64().unwrap() - hardening).abs() < 1e-3);
            for (prefix, field) in [
                ("membrane_yield", "yield_n_m"),
                ("membrane_hardening", "hardening_n_m"),
            ] {
                let quantity =
                    &fitted["selection"]["record"]["properties"][format!("{prefix}_{axis}")];
                assert_eq!(quantity["value"], parameters[field][i]);
                assert_eq!(quantity["evidence"], "estimate");
                assert!(
                    fitted["selection"]["record"]["sources"][quantity["source"].as_str().unwrap()]
                        .is_object()
                );
            }
        }
        let selection = fitted["selection_json"].as_str().unwrap();
        let inspected: Value =
            serde_json::from_str(&materials::inspect(selection).unwrap()).unwrap();
        assert_eq!(inspected, fitted["material"]);
        let request = format!(r#"{{"schema":"ftx-material-bindings/1","default":{selection}}}"#);
        let bound: Value =
            serde_json::from_str(&materials::bind(PATTERN, &request).unwrap()).unwrap();
        assert_eq!(bound["geometry_changed"], false);
        assert_eq!(
            bound["drape_materials_without_self_contact"]["ready"],
            admission
        );
        let original: Value = serde_json::from_str(PATTERN).unwrap();
        assert_eq!(bound["pattern"]["pieces"], original["pieces"]);
        assert_eq!(bound["pattern"]["seams"], original["seams"]);
        let mut altered: Value = serde_json::from_str(selection).unwrap();
        altered["record"]["properties"]["membrane_yield_warp"]["value"] = json!(999.0);
        assert!(materials::inspect(&altered.to_string()).is_err());
    }
    let mut unapproved: Value = serde_json::from_str(input).unwrap();
    unapproved["allow_estimated_observations"] = json!(false);
    assert!(materials::calibrate_membrane_plasticity(&unapproved.to_string(), true).is_err());
    assert!(materials::calibrate_membrane_plasticity("{}", true).is_err());
}

#[test]
fn unresolved_permanent_stretch_api_keeps_diagnostics_without_publishing_a_selection() {
    let input: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-membrane-plasticity-calibration.json"
    ))
    .unwrap();
    let mut failed = input.clone();
    let check = failed["experiments"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|experiment| experiment["partition"] == "check")
        .unwrap();
    check["samples"].as_array_mut().unwrap().last_mut().unwrap()["second_piola_n_m"][0] =
        json!(1_000_000.0);
    let mut limited = input.clone();
    limited["max_evaluations"] = json!(1);
    let mut unexcited = input;
    for experiment in unexcited["experiments"].as_array_mut().unwrap() {
        for sample in experiment["samples"].as_array_mut().unwrap() {
            sample["green_strain"][2] = json!(0.0);
            sample["second_piola_n_m"][2] = json!(0.0);
        }
    }
    for (request, status) in [
        (failed, "REJECTED"),
        (limited, "BUDGET_EXHAUSTED"),
        (unexcited, "AMBIGUOUS"),
    ] {
        let output: Value = serde_json::from_str(
            &materials::calibrate_membrane_plasticity(&request.to_string(), true).unwrap(),
        )
        .unwrap();
        assert_eq!(output["calibration"]["status"], status, "{output}");
        assert!(output["calibration"]["material"].is_null());
        assert!(output["calibration"]["diagnostics"].is_object());
        assert!(
            output["calibration"]["decision_reasons"]
                .as_array()
                .is_some_and(|v| !v.is_empty())
        );
        for field in ["selection", "selection_json", "material"] {
            assert!(output[field].is_null(), "{field}: {output}");
        }
    }
}

#[test]
fn nonlinear_swatch_api_reuses_native_identification_and_binding() {
    let input = include_str!("../../../tests/fixtures/materials/synthetic-nonlinear-swatch.json");
    let denied: Value =
        serde_json::from_str(&materials::calibrate_nonlinear(input, false).unwrap()).unwrap();
    let fitted: Value =
        serde_json::from_str(&materials::calibrate_nonlinear(input, true).unwrap()).unwrap();
    let native = ftx_pattern::material_calibration::nonlinear::calibrate(
        &serde_json::from_str(input).unwrap(),
    )
    .unwrap();
    assert_eq!(fitted["calibration"], serde_json::to_value(native).unwrap());
    assert_eq!(denied["calibration"], fitted["calibration"]);
    assert_eq!(
        denied["material"]["capabilities"]["mechanical_coefficients"]["ready"],
        false
    );
    assert_eq!(
        fitted["material"]["capabilities"]["mechanical_coefficients"]["ready"],
        true
    );
    let result =
        materials::bind(PATTERN, &changes(fitted["selection"].clone()).to_string()).unwrap();
    let result: Value = serde_json::from_str(&result).unwrap();
    assert_eq!(
        result["drape_materials_without_self_contact"]["ready"],
        true
    );
    assert_eq!(result["geometry_changed"], false);
    let mut tampered: Value = serde_json::from_str(input).unwrap();
    tampered["samples"][12]["nominal_n_m"][0][0] = json!(45.);
    assert!(materials::calibrate_nonlinear(&tampered.to_string(), true).is_err());
    assert!(materials::calibrate_nonlinear("{}", true).is_err());
}

#[test]
fn timed_history_api_identifies_and_binds_the_native_recovery_spectrum() {
    let input = include_str!("../../../tests/fixtures/materials/synthetic-relaxation-history.json");
    let denied: Value =
        serde_json::from_str(&materials::calibrate_relaxation(input, false).unwrap()).unwrap();
    let fitted: Value =
        serde_json::from_str(&materials::calibrate_relaxation(input, true).unwrap()).unwrap();
    let native = ftx_pattern::material_relaxation::calibration::calibrate(
        &serde_json::from_str(input).unwrap(),
    )
    .unwrap();
    assert_eq!(fitted["calibration"], serde_json::to_value(native).unwrap());
    assert_eq!(denied["calibration"], fitted["calibration"]);
    for (report, ready) in [(&denied, false), (&fitted, true)] {
        assert_eq!(
            report["material"]["capabilities"]["mechanical_coefficients"]["ready"],
            ready
        );
        let selection = report["selection_json"].as_str().unwrap();
        let inspected: Value =
            serde_json::from_str(&materials::inspect(selection).unwrap()).unwrap();
        assert_eq!(inspected, report["material"]);
        let changes = format!(r#"{{"schema":"ftx-material-bindings/1","default":{selection}}}"#);
        let bound: Value =
            serde_json::from_str(&materials::bind(PATTERN, &changes).unwrap()).unwrap();
        assert_eq!(
            bound["drape_materials_without_self_contact"]["ready"],
            ready
        );
        assert_eq!(bound["geometry_changed"], false);
    }
    let mut failed: Value = serde_json::from_str(input).unwrap();
    failed["experiments"][1]["samples"][8]["nominal_n_m"][0][0] = json!(50000.);
    assert!(materials::calibrate_relaxation(&failed.to_string(), true).is_err());
    assert!(materials::calibrate_relaxation("{}", true).is_err());
}

#[test]
fn recovery_comparison_preserves_ambiguity_and_binds_only_a_native_accepted_choice() {
    let mut calibration: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-orthotropic-relaxation-history.json"
    ))
    .unwrap();
    calibration.as_object_mut().unwrap().remove("orthotropic");
    let mut request = json!({"schema":"ftx-relaxation-model-comparison/1", "calibration":calibration,
    "fit_rms_equivalence_scaled":1e-7,"candidates":[
        {"id":"complete","orthotropic":{"times_seconds":[0.3,2,8],"zero_threshold_n_m":1e-7}},
        {"id":"extra","orthotropic":{"times_seconds":[0.3,2,8,32],"zero_threshold_n_m":1e-7}}
    ]});
    let unresolved: Value =
        serde_json::from_str(&materials::calibrate_relaxation(&request.to_string(), true).unwrap())
            .unwrap();
    assert_eq!(unresolved["comparison"]["status"], "AMBIGUOUS");
    for key in ["calibration", "material", "selection", "selection_json"] {
        assert!(unresolved[key].is_null());
    }
    request["select_candidate_id"] = json!("complete");
    let input = request.to_string();
    let native = ftx_pattern::material_relaxation::calibration::comparison::compare(
        &serde_json::from_str(&input).unwrap(),
    )
    .unwrap();
    for admission in [false, true] {
        let selected: Value =
            serde_json::from_str(&materials::calibrate_relaxation(&input, admission).unwrap())
                .unwrap();
        assert_eq!(
            selected["comparison"],
            serde_json::to_value(&native).unwrap()
        );
        assert_eq!(selected["comparison"]["status"], "ACCEPTED");
        assert_eq!(selected["comparison"]["fit_decision"]["ambiguous"], true);
        let selection = selected["selection_json"].as_str().unwrap();
        let inspected: Value =
            serde_json::from_str(&materials::inspect(selection).unwrap()).unwrap();
        assert_eq!(inspected, selected["material"]);
        let changes = format!(r#"{{"schema":"ftx-material-bindings/1","default":{selection}}}"#);
        let bound: Value =
            serde_json::from_str(&materials::bind(PATTERN, &changes).unwrap()).unwrap();
        assert_eq!(bound["geometry_changed"], false);
        assert_eq!(
            bound["drape_materials_without_self_contact"]["ready"],
            admission
        );
    }
}

#[test]
fn calibration_selection_text_preserves_signed_zero_through_actual_binding() {
    for (fixture, fit) in [
        (
            include_str!("../../../tests/fixtures/materials/synthetic-nonlinear-swatch.json"),
            materials::calibrate_nonlinear as fn(&str, bool) -> _,
        ),
        (
            include_str!("../../../tests/fixtures/materials/synthetic-relaxation-history.json"),
            materials::calibrate_relaxation,
        ),
    ] {
        let mut input: Value = serde_json::from_str(fixture).unwrap();
        input["material"]["properties"]["friction_skin"]["value"] = json!(-0.0);
        let fitted: Value = serde_json::from_str(&fit(&input.to_string(), true).unwrap()).unwrap();
        let text = fitted["selection_json"].as_str().unwrap();
        assert!(text.contains("-0.0"));
        assert!(materials::inspect(text).is_ok());
        let changes = format!(r#"{{"schema":"ftx-material-bindings/1","default":{text}}}"#);
        let bound: Value =
            serde_json::from_str(&materials::bind(PATTERN, &changes).unwrap()).unwrap();
        assert_eq!(bound["drape_materials_without_self_contact"]["ready"], true);
        // Browser JSON reserialization may erase this sign while keeping the stale hash.
        let changed = text.replace("-0.0", "0.0");
        assert!(materials::inspect(&changed).is_err());
    }
}
fn bind(p: &Value, c: &Value) -> Value {
    serde_json::from_str(&materials::bind(&p.to_string(), &c.to_string()).unwrap()).unwrap()
}
fn cut() -> String {
    json!({"copies":1,"gap_mm":2.,"edge_margin_mm":0.,"end_margin_mm":0.}).to_string()
}

#[test]
fn binding_freezes_defaults_and_pieces_without_geometry_or_evidence_changes() {
    let mut raw: Value = serde_json::from_str(PATTERN).unwrap();
    raw["pieces"][1]["fabric"] = json!({"material":selection(1.2,0.5),"role":"lining"});
    raw["pieces"][0]["material"] = json!("informational cutting label");
    let original = raw.clone();
    let c = changes(selection(1., 0.4));
    let result = bind(&raw, &c);
    assert_eq!(raw, original);
    assert_eq!(result["geometry_changed"], false);
    assert_eq!(result["pieces"][0]["inherited_default"], true);
    assert_eq!(result["pieces"][1]["inherited_default"], false);
    assert_ne!(
        result["pieces"][0]["material_sha256"],
        result["pieces"][1]["material_sha256"]
    );
    let p = &result["pattern"];
    assert_eq!(p["pieces"][1]["fabric"]["role"], "lining");
    assert!(p["pieces"][1]["fabric"]["material"]["expected_sha256"].is_string());
    let mut stripped = p.clone();
    stripped["fabric"] = raw["fabric"].clone();
    stripped["pieces"][1]["fabric"] = raw["pieces"][1]["fabric"].clone();
    assert_eq!(stripped, raw);
    let empty = json!({"schema":"ftx-material-bindings/1"});
    assert_eq!(bind(p, &empty)["pattern"], *p);
    let core: Changes = serde_json::from_value(c).unwrap();
    assert_eq!(core.apply(&Pattern::new(raw)).unwrap()["pattern"], *p);
}

#[test]
fn substitutions_drive_actual_cutting_sewing_and_drape_resolution_independently() {
    let raw: Value = serde_json::from_str(PATTERN).unwrap();
    let a = bind(&raw, &changes(selection(0.1, 0.4)));
    let b = bind(&raw, &changes(selection(0.06, 2.0)));
    assert_eq!(a["pattern"]["pieces"], b["pattern"]["pieces"]);
    assert_eq!(a["drape_materials_with_self_contact"]["ready"], true);
    assert_eq!(b["drape_materials_with_self_contact"]["ready"], true);
    let mut lengths = Vec::new();
    for result in [&a, &b] {
        let p = &result["pattern"];
        let plan: Value =
            serde_json::from_str(&manufacturing::cut_plan(&p.to_string(), &cut()).unwrap())
                .unwrap();
        let options: CutOptions = serde_json::from_str(&cut()).unwrap();
        let native = CutPlan::build(&Pattern::new(p.clone()), &options).unwrap();
        assert_eq!(plan, serde_json::to_value(&native).unwrap());
        assert_eq!(
            manufacturing::cut_svg(&p.to_string(), &cut(), 0).unwrap(),
            ftx_export::cutting::preview::svg(&native, 0).unwrap()
        );
        lengths.push(plan["rolls"][0]["length_m"].as_f64().unwrap());
    }
    assert!(lengths[1] > lengths[0]);
    assert!(manufacturing::sew_plan(&a["pattern"].to_string(), SEW).is_ok());
    assert!(
        manufacturing::sew_plan(&b["pattern"].to_string(), SEW)
            .unwrap_err()
            .message
            .contains("stack")
    );
    let mut denied = selection(0.1, 0.4);
    denied["allow_estimates"] = json!(false);
    let denied = bind(&raw, &changes(denied));
    assert_eq!(
        denied["drape_materials_without_self_contact"]["ready"],
        false
    );
    assert!(manufacturing::cut_plan(&denied["pattern"].to_string(), &cut()).is_err());
}

#[test]
fn partial_records_are_inspectable_not_automatically_simulation_ready() {
    let rows: Value = serde_json::from_str(&materials::catalog("cotton").unwrap()).unwrap();
    assert!(!rows["materials"].as_array().unwrap().is_empty());
    let s = json!({"id":"fsri/hfm/cotton-sheet/dry/220330-r1","allow_estimates":true});
    let d: Value = serde_json::from_str(&materials::inspect(&s.to_string()).unwrap()).unwrap();
    assert_eq!(d["capabilities"]["steady_conduction"]["ready"], true);
    assert_eq!(d["capabilities"]["transient_thermal"]["ready"], false);
    assert_eq!(d["capabilities"]["mechanical_coefficients"]["ready"], false);
    assert_eq!(
        d,
        serde_json::from_str::<Value>(&materials::inspect(&d["selection"].to_string()).unwrap())
            .unwrap()
    );
    let mut s = selection(1., 0.4);
    s["record"]["structure"] = json!({"bending_plasticity_model":"unknown-law"});
    let d: Value = serde_json::from_str(&materials::inspect(&s.to_string()).unwrap()).unwrap();
    assert_eq!(d["capabilities"]["mechanical_coefficients"]["ready"], false);
}

#[test]
fn permanent_membrane_selection_uses_the_same_law_and_independent_readiness() {
    use ftx_pattern::material_catalog::Selection;
    let text = include_str!("../../../tests/fixtures/materials/synthetic-membrane-plasticity.json");
    let mut selected: Value = serde_json::from_str(text).unwrap();
    let detail: Value = serde_json::from_str(&materials::inspect(text).unwrap()).unwrap();
    assert_eq!(
        detail["capabilities"]["mechanical_coefficients"]["ready"],
        true
    );
    let frozen = detail["selection_json"].as_str().unwrap();
    assert_eq!(
        detail,
        serde_json::from_str::<Value>(&materials::inspect(frozen).unwrap()).unwrap()
    );
    let snapshot = ftx_pattern::textile_import::catalog()
        .unwrap()
        .resolve(&serde_json::from_str::<Selection>(frozen).unwrap())
        .unwrap();
    let law = snapshot.membrane_plasticity().unwrap().unwrap();
    let edits = format!(r#"{{"schema":"ftx-material-bindings/1","default":{frozen}}}"#);
    let bound: Value = serde_json::from_str(&materials::bind(PATTERN, &edits).unwrap()).unwrap();
    assert_eq!(bound["drape_materials_with_self_contact"]["ready"], true);
    let pattern = Pattern::from_json(bound["pattern_json"].as_str().unwrap()).unwrap();
    let resolved =
        ftx_drape::material::resolve(&pattern, &ftx_drape::run::DrapeOptions::default()).unwrap();
    assert_eq!(
        resolved.report["membrane_plasticity_law_sha256"],
        law.fingerprint()
    );
    assert_eq!(
        bound["pattern"]["pieces"],
        serde_json::from_str::<Value>(PATTERN).unwrap()["pieces"]
    );
    selected["record"]["properties"]
        .as_object_mut()
        .unwrap()
        .remove("membrane_yield_weft");
    let detail: Value =
        serde_json::from_str(&materials::inspect(&selected.to_string()).unwrap()).unwrap();
    assert_eq!(
        detail["capabilities"]["mechanical_coefficients"]["ready"],
        false
    );
    assert_eq!(detail["capabilities"]["steady_conduction"]["ready"], true);
    let bound: Value =
        serde_json::from_str(&materials::bind(PATTERN, &changes(selected).to_string()).unwrap())
            .unwrap();
    assert_eq!(
        bound["drape_materials_without_self_contact"]["ready"],
        false
    );
}

#[test]
fn pressure_conditioned_selection_replays_through_binding_and_actual_consumers() {
    use ftx_pattern::{material_compression, materials::Catalog};
    let request = include_str!("../../../tests/fixtures/materials/synthetic-compression.json");
    let input: material_compression::Input = serde_json::from_str(request).unwrap();
    let core = material_compression::condition(&input).unwrap();
    let text = materials::pressure_condition(request).unwrap();
    assert_eq!(text, materials::pressure_condition(request).unwrap());
    let result: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(result, serde_json::to_value(&core).unwrap());
    let selection_text = result["selection_json"].as_str().unwrap();
    let selected = Catalog::builtin()
        .unwrap()
        .resolve(&serde_json::from_str(selection_text).unwrap())
        .unwrap();
    assert_eq!(selected.sha256(), core.selection.expected_sha256.unwrap());
    let detail: Value = serde_json::from_str(&materials::inspect(selection_text).unwrap()).unwrap();
    assert_eq!(detail["selection_json"], selection_text);
    for capability in [
        "mechanical_coefficients",
        "steady_conduction",
        "transient_thermal",
    ] {
        assert_eq!(
            detail["capabilities"][capability]["ready"], true,
            "{capability}"
        );
    }
    assert_eq!(detail["capabilities"]["sewing_thickness_m"]["ready"], false);

    // Keep the exact Rust selection text across the public binding boundary.
    let edits = format!(r#"{{"schema":"ftx-material-bindings/1","default":{selection_text}}}"#);
    let bound: Value = serde_json::from_str(&materials::bind(PATTERN, &edits).unwrap()).unwrap();
    assert_eq!(bound["geometry_changed"], false);
    let pattern = bound["pattern_json"].as_str().unwrap();
    let resolved = ftx_drape::material::resolve(
        &Pattern::from_json(pattern).unwrap(),
        &ftx_drape::run::DrapeOptions::default(),
    )
    .unwrap();
    assert!((resolved.fabric.areal_kg_m2 - 0.1).abs() < 1e-15);
    assert!((resolved.options.contact.thickness - 0.0075).abs() < 1e-15);
    let cut: Value =
        serde_json::from_str(&manufacturing::cut_plan(pattern, &cut()).unwrap()).unwrap();
    assert_eq!(cut["rolls"][0]["material"]["sha256"], selected.sha256());
    let refusal = manufacturing::sew_plan(pattern, SEW).unwrap_err();
    assert!(refusal.message.contains("uncompressed sheet thickness"));
    assert!(manufacturing::sew_plan(PATTERN, SEW).is_ok());
}

#[test]
fn pressure_api_refuses_unqualified_or_out_of_domain_requests() {
    let mut request: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-compression.json"
    ))
    .unwrap();
    request["pressure_pa"] = json!(501.0);
    assert!(materials::pressure_condition(&request.to_string()).is_err());
    request["pressure_pa"] = json!(250.0);
    request["invariants"]["areal_density"]["conditions"]["pressure_pa"] = json!("2000");
    assert!(materials::pressure_condition(&request.to_string()).is_err());
}

#[test]
fn interface_replacement_reaches_the_real_wasm_binding_and_contact_readiness() {
    let mut raw: Value = serde_json::from_str(PATTERN).unwrap();
    raw["fabric"]["material"] = selection(1.2, 0.4);
    raw["pieces"][1]["fabric"] = json!({"material":selection(1.4,0.5),"role":"lining"});
    let original = bind(&raw, &json!({"schema":"ftx-material-bindings/1"}));
    let inspected: Value =
        serde_json::from_str(&materials::inspect(&selection(1.6, 0.6).to_string()).unwrap())
            .unwrap();
    let source = json!({"reference":"test:atomic-interface","accessed_on":"2026-10-19",
        "note":"Synthetic source used to test transport of pair evidence"});
    let pair = json!({"materials":[inspected["selection"]["expected_sha256"],original["pieces"][1]["material_sha256"]],
        "friction":{"value":1,"unit":"1","bounds":[-0.0,1],"source":"pair","evidence":"estimate",
            "method":"Synthetic corrected steady sliding observation"},
        "sources":{"pair":source},"allow_estimates":true});
    let mut edits = changes(inspected["selection"].clone());
    edits["interfaces"] = json!([pair]);
    let result = bind(&raw, &edits);
    assert_eq!(result["drape_materials_with_self_contact"]["ready"], true);
    assert_eq!(
        result["pattern"]["fabric"]["interfaces"],
        edits["interfaces"]
    );
    assert_eq!(result["pattern"]["pieces"], original["pattern"]["pieces"]);
    assert!(result["pattern_json"].as_str().unwrap().contains("-0.0"));
    let replay = bind(
        &result["pattern"],
        &json!({"schema":"ftx-material-bindings/1"}),
    );
    assert_eq!(result["pattern_json"], replay["pattern_json"]);
    edits["interfaces"][0]["allow_estimates"] = json!(false);
    let unadmitted = bind(&raw, &edits);
    assert_eq!(
        unadmitted["drape_materials_with_self_contact"]["ready"],
        false
    );
    assert!(manufacturing::cut_plan(unadmitted["pattern_json"].as_str().unwrap(), &cut()).is_ok());
    edits["interfaces"][0]["materials"][0] = original["pieces"][0]["material_sha256"].clone();
    assert!(materials::bind(&raw.to_string(), &edits.to_string()).is_err());
}

#[test]
fn malformed_binding_and_stale_material_or_pair_evidence_do_not_publish_success() {
    let raw: Value = serde_json::from_str(PATTERN).unwrap();
    let mut c = changes(selection(0.1, 0.4));
    c["pieces"]["not-a-piece"] = selection(0.1, 0.4);
    assert!(materials::bind(PATTERN, &c.to_string()).is_err());
    c["pieces"] = json!({"front":selection(0.2,0.5)});
    let result = bind(&raw, &c);
    assert_eq!(
        result["drape_materials_without_self_contact"]["ready"],
        true
    );
    assert_eq!(result["drape_materials_with_self_contact"]["ready"], false); // no unlike-pair evidence
    let mut tampered = result["pattern"].clone();
    tampered["fabric"]["material"]["record"]["name"] = json!("unapproved change");
    assert!(
        materials::bind(
            &tampered.to_string(),
            "{\"schema\":\"ftx-material-bindings/1\"}"
        )
        .is_err()
    );
    assert!(materials::bind(PATTERN, "{\"schema\":\"bad\"}").is_err());
    assert!(materials::inspect("{}").is_err());
    assert!(materials::bind("bad", &c.to_string()).is_err());
    assert!(manufacturing::cut_svg(&result["pattern"].to_string(), &cut(), 99).is_err());
}

#[test]
fn procurement_uses_marker_requirements_and_missing_offers_stay_unknown() {
    let raw: Value = serde_json::from_str(PATTERN).unwrap();
    let r = bind(&raw, &changes(selection(0.1, 0.4)));
    let p = r["pattern"].to_string();
    let quotes = json!({"schema":"ftx-textile-quotes/1","quotes":[]}).to_string();
    let market = json!({"currency":"USD","region":"US","quality":"test","terms":"test",
        "as_of":"2026-10-15","max_age_days":30})
    .to_string();
    let got: Value =
        serde_json::from_str(&manufacturing::cut_price(&p, &cut(), &quotes, &market).unwrap())
            .unwrap();
    let native = CutPlan::build(
        &Pattern::new(r["pattern"].clone()),
        &serde_json::from_str(&cut()).unwrap(),
    )
    .unwrap();
    let expected = procurement::evaluate(
        &native,
        &serde_json::from_str(&quotes).unwrap(),
        &serde_json::from_str(&market).unwrap(),
    )
    .unwrap();
    assert_eq!(got, expected);
    assert_eq!(got["complete_offer_coverage"], false);
    assert!(got["least_observed_independent_order_total"].is_null());
}

#[test]
fn continuous_marker_limits_reach_portable_purchasing_and_material_studies() {
    let raw: Value = serde_json::from_str(PATTERN).unwrap();
    let bound = bind(&raw, &changes(selection(0.1, 0.4)));
    let pattern = bound["pattern_json"].as_str().unwrap();
    let native = CutPlan::build(
        &Pattern::from_json(pattern).unwrap(),
        &serde_json::from_str(&cut()).unwrap(),
    )
    .unwrap();
    let length = native.rolls[0].length_m;
    let mut quote = json!({"offer_id":"synthetic/continuous","material_id":"synthetic-sewing-sheet",
        "material_sha256":native.rolls[0].material.sha256(),"supplier":"Synthetic supplier",
        "sku":"continuous","price":10.0,"quantity":1.0,"unit":"linear_meter",
        "minimum_quantity":0.1,"order_increment":0.1,"currency":"USD","region":"US",
        "quality":"test","terms":"test","observed_on":"2026-10-15",
        "available":true,"available_quantity":1000.0,"maximum_continuous_quantity":length * 0.5,
        "source":"test:continuous-limit","source_sha256":"0".repeat(64)});
    let market = json!({"currency":"USD","region":"US","quality":"test","terms":"test",
        "as_of":"2026-10-15","max_age_days":1,"require_observed_continuous_length":true});
    for admits in [false, true] {
        if admits {
            quote["maximum_continuous_quantity"] = json!(length);
        }
        let book = json!({"schema":"ftx-textile-quotes/1","quotes":[quote]});
        let got: Value = serde_json::from_str(
            &manufacturing::cut_price(pattern, &cut(), &book.to_string(), &market.to_string())
                .unwrap(),
        )
        .unwrap();
        let expected = procurement::evaluate(
            &native,
            &serde_json::from_value(book.clone()).unwrap(),
            &serde_json::from_value(market.clone()).unwrap(),
        )
        .unwrap();
        assert_eq!(got, expected);
        assert_eq!(got["complete_offer_coverage"], admits);
        assert_eq!(got["complete_continuous_length_limit_coverage"], admits);
        let study_request = json!({"schema":"ftx-material-study/1", "variants":[{"key":"same",
            "changes":{"schema":"ftx-material-bindings/1"}}],
            "cutting":serde_json::from_str::<Value>(&cut()).unwrap(),
            "purchasing":{"quotes":book,"market":market}});
        let study: Value =
            serde_json::from_str(&materials::study(pattern, &study_request.to_string()).unwrap())
                .unwrap();
        assert_eq!(
            study["alternatives"][0]["purchasing"]["result"]["roll_prices"],
            got["roll_prices"]
        );
        if !admits {
            assert_eq!(
                got["roll_prices"][0]["sample"]["excluded_offers"][0]["reason"],
                "insufficient_continuous_length"
            );
            assert!(
                study["alternatives"][0]["metrics"]["least_observed_independent_order_total"]
                    .is_null()
            );
        }
    }
}

#[test]
fn bounded_markers_share_one_order_in_portable_consumers_and_studies() {
    let raw: Value = serde_json::from_str(PATTERN).unwrap();
    let bound = bind(&raw, &changes(selection(0.1, 0.4)));
    let pattern = bound["pattern_json"].as_str().unwrap();
    let mut options: Value = serde_json::from_str(&cut()).unwrap();
    options["copies"] = json!(3);
    options["maximum_marker_length_m"] = json!(0.12);
    let plan = CutPlan::build(
        &Pattern::from_json(pattern).unwrap(),
        &serde_json::from_value(options.clone()).unwrap(),
    )
    .unwrap();
    assert_eq!(plan.rolls.len(), 3);
    assert!(plan.rolls.iter().all(|r| r.length_m <= 0.12));
    let portable: Value =
        serde_json::from_str(&manufacturing::cut_plan(pattern, &options.to_string()).unwrap())
            .unwrap();
    assert_eq!(portable, serde_json::to_value(&plan).unwrap());
    let svg = manufacturing::cut_svg(pattern, &options.to_string(), 2).unwrap();
    assert_eq!(svg, ftx_export::cutting::preview::svg(&plan, 2).unwrap());
    let mut quote = json!({"offer_id":"synthetic/partitioned","material_id":"synthetic-sewing-sheet",
        "material_sha256":plan.rolls[0].material.sha256(),"supplier":"Synthetic supplier",
        "sku":"partitioned","price":10.0,"quantity":1.0,"unit":"linear_meter",
        "minimum_quantity":0.05,"order_increment":0.05,"currency":"USD","region":"US",
        "quality":"test","terms":"test","observed_on":"2026-10-21",
        "available":true,"available_quantity":0.4,"maximum_continuous_quantity":0.12,
        "source":"test:partitioned","source_sha256":"0".repeat(64)});
    let market = json!({"currency":"USD","region":"US","quality":"test","terms":"test",
        "as_of":"2026-10-21","max_age_days":1,"require_observed_quantity":true,
        "require_observed_continuous_length":true});
    for enough in [true, false] {
        if !enough {
            quote["available_quantity"] = json!(0.2);
        }
        let book = json!({"schema":"ftx-textile-quotes/1","quotes":[quote]});
        let got: Value = serde_json::from_str(
            &manufacturing::cut_price(
                pattern,
                &options.to_string(),
                &book.to_string(),
                &market.to_string(),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            got,
            procurement::evaluate(
                &plan,
                &serde_json::from_value(book.clone()).unwrap(),
                &serde_json::from_value(market.clone()).unwrap()
            )
            .unwrap()
        );
        assert_eq!(got["schema"], "ftx-cut-procurement/2");
        assert_eq!(got["material_orders"].as_array().unwrap().len(), 1);
        assert_eq!(got["material_orders"][0]["roll_indices"], json!([0, 1, 2]));
        assert_eq!(got["complete_offer_coverage"], enough);
        if enough {
            assert!(
                (got["least_observed_independent_order_total"]
                    .as_f64()
                    .unwrap()
                    - 4.0)
                    .abs()
                    < 1e-10
            );
        } else {
            assert!(got["least_observed_independent_order_total"].is_null());
        }
        let request = json!({"schema":"ftx-material-study/1","variants":[{"key":"same",
            "changes":{"schema":"ftx-material-bindings/1"}}],"cutting":options,
            "purchasing":{"quotes":book,"market":market}});
        let study: Value =
            serde_json::from_str(&materials::study(pattern, &request.to_string()).unwrap())
                .unwrap();
        assert_eq!(study["alternatives"][0]["purchasing"]["result"], got);
        assert_eq!(
            study["alternatives"][0]["metrics"]["least_observed_independent_order_total"],
            got["least_observed_independent_order_total"]
        );
    }
}

#[test]
fn split_supplier_purchasing_keeps_native_allocations_and_incomplete_totals_in_portable_output() {
    let raw: Value = serde_json::from_str(PATTERN).unwrap();
    let bound = bind(&raw, &changes(selection(0.1, 0.4)));
    let pattern = bound["pattern_json"].as_str().unwrap();
    let mut options: Value = serde_json::from_str(&cut()).unwrap();
    options["copies"] = json!(3);
    options["maximum_marker_length_m"] = json!(0.12);
    let plan = CutPlan::build(
        &Pattern::from_json(pattern).unwrap(),
        &serde_json::from_value(options.clone()).unwrap(),
    )
    .unwrap();
    assert_eq!(plan.rolls.len(), 3);
    let first = json!({"offer_id":"first/retail","material_id":"synthetic-sewing-sheet",
        "material_sha256":plan.rolls[0].material.sha256(),"supplier":"Synthetic first supplier",
        "sku":"same-specimen","price":10.0,"quantity":1.0,"unit":"linear_meter",
        "minimum_quantity":0.05,"order_increment":0.05,"currency":"USD","region":"US",
        "quality":"test","terms":"test","observed_on":"2026-10-21","available":true,
        "available_quantity":0.25,"maximum_continuous_quantity":0.12,
        "source":"test:split-purchase","source_sha256":"0".repeat(64)});
    let mut second = first.clone();
    second["offer_id"] = json!("second/retail");
    second["supplier"] = json!("Synthetic second supplier");
    second["price"] = json!(12.0);
    let mut book = json!({"schema":"ftx-textile-quotes/1","quotes":[first,second]});
    let market = json!({"currency":"USD","region":"US","quality":"test","terms":"test",
        "as_of":"2026-10-21","max_age_days":1,"require_observed_quantity":true,
        "require_observed_continuous_length":true,
        "split_orders":{"schema":"ftx-cut-order-allocation/1","max_search_nodes":1000}});
    for enough in [true, false] {
        if !enough {
            for quote in book["quotes"].as_array_mut().unwrap() {
                quote["available_quantity"] = json!(0.1);
            }
        }
        let expected = procurement::evaluate(
            &plan,
            &serde_json::from_value(book.clone()).unwrap(),
            &serde_json::from_value(market.clone()).unwrap(),
        )
        .unwrap();
        let portable: Value = serde_json::from_str(
            &manufacturing::cut_price(
                pattern,
                &options.to_string(),
                &book.to_string(),
                &market.to_string(),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(portable, expected);
        assert_eq!(portable["schema"], "ftx-cut-procurement/3");
        assert_eq!(portable["complete_offer_coverage"], enough);
        assert_eq!(portable["market"], market);
        assert_eq!(portable["quotes"], book);
        let allocation = &portable["material_orders"][0]["allocation"];
        if enough {
            let orders = allocation["selected_orders"].as_array().unwrap();
            assert_eq!(orders.len(), 2);
            let mut assigned: Vec<_> = orders
                .iter()
                .flat_map(|order| order["roll_indices"].as_array().unwrap())
                .map(|index| index.as_u64().unwrap())
                .collect();
            assigned.sort_unstable();
            assert_eq!(assigned, vec![0, 1, 2]);
            assert_eq!(portable["order_allocation_optimality_proven"], true);
            assert_eq!(
                portable["selected_observed_order_total"],
                portable["least_observed_independent_order_total"]
            );
        } else {
            assert!(allocation["selected_orders"].is_null());
            assert!(portable["selected_observed_order_total"].is_null());
            assert!(portable["least_observed_independent_order_total"].is_null());
        }
    }
}

#[test]
fn intraday_procurement_and_studies_share_native_expiry_and_complete_order_semantics() {
    let raw: Value = serde_json::from_str(PATTERN).unwrap();
    let bound = bind(&raw, &changes(selection(0.1, 0.4)));
    let pattern = bound["pattern_json"].as_str().unwrap();
    let native = CutPlan::build(
        &Pattern::from_json(pattern).unwrap(),
        &serde_json::from_str(&cut()).unwrap(),
    )
    .unwrap();
    assert_eq!(native.rolls.len(), 1);
    let mut old = json!({"offer_id":"synthetic/roll","material_id":"synthetic-sewing-sheet",
        "material_sha256":native.rolls[0].material.sha256(),"supplier":"Synthetic supplier",
        "sku":"test-roll","price":10.0,"quantity":1.0,"unit":"linear_meter",
        "minimum_quantity":0.1,"order_increment":0.1,"currency":"USD","region":"US",
        "quality":"test","terms":"test","observed_on":"2026-10-14",
        "observed_at":"2026-10-14T09:00:00Z","available":true,"available_quantity":1.0,
        "source":"test:intraday","source_sha256":"0".repeat(64)});
    let mut later = old.clone();
    later["price"] = json!(25.0);
    later["observed_at"] = json!("2026-10-14T10:00:00Z");
    later["valid_through_at"] = json!("2026-10-14T11:00:00Z");
    later["source_sha256"] = json!("1".repeat(64));
    let book = json!({"schema":"ftx-textile-quotes/2","quotes":[later,old]}).to_string();
    let mut market = json!({"currency":"USD","region":"US","quality":"test","terms":"test",
        "as_of":"2026-10-14","max_age_days":0,"require_observed_quantity":true});
    let mut identities = std::collections::BTreeSet::new();
    for (instant, price, expired) in [
        ("2026-10-14T09:59:59.999999999Z", 10.0, false),
        ("2026-10-14T10:00:00Z", 25.0, false),
        ("2026-10-14T11:00:00Z", 25.0, false),
        ("2026-10-14T11:00:00.000000001Z", 25.0, true),
    ] {
        market["as_of_at"] = json!(instant);
        let portable: Value = serde_json::from_str(
            &manufacturing::cut_price(pattern, &cut(), &book, &market.to_string()).unwrap(),
        )
        .unwrap();
        let expected = procurement::evaluate(
            &native,
            &serde_json::from_str(&book).unwrap(),
            &serde_json::from_value(market.clone()).unwrap(),
        )
        .unwrap();
        assert_eq!(portable, expected);
        assert_eq!(portable["complete_offer_coverage"], !expired);
        let sample = &portable["roll_prices"][0]["sample"];
        assert_eq!(sample["scope"]["as_of_at"], instant);
        if expired {
            assert!(portable["least_observed_independent_order_total"].is_null());
            assert_eq!(sample["excluded_offers"][0]["reason"], "expired");
            assert_eq!(sample["excluded_offers"][0]["quote"], later);
            assert_eq!(sample["offers"], json!([]));
        } else {
            assert_eq!(sample["offers"][0]["quote"]["price"], price);
            assert_eq!(sample["offers"][0]["quantity_sufficiency_observed"], true);
        }
        let input = json!({"schema":"ftx-material-study/1","variants":[
            {"key":"unchanged","changes":{"schema":"ftx-material-bindings/1"}}],
            "cutting":serde_json::from_str::<Value>(&cut()).unwrap(),
            "purchasing":{"quotes":serde_json::from_str::<Value>(&book).unwrap(),"market":market}});
        let request: ftx_export::material_study::Request =
            serde_json::from_value(input.clone()).unwrap();
        let native_study = request
            .evaluate(&Pattern::from_json(pattern).unwrap())
            .unwrap();
        let study: Value =
            serde_json::from_str(&materials::study(pattern, &input.to_string()).unwrap()).unwrap();
        assert_eq!(study, serde_json::to_value(native_study).unwrap());
        assert_eq!(
            study["alternatives"][0]["purchasing"]["status"],
            "available"
        );
        assert_eq!(
            study["alternatives"][0]["purchasing"]["result"]["roll_prices"],
            portable["roll_prices"]
        );
        assert_eq!(
            study["alternatives"][0]["metrics"]["least_observed_independent_order_total"],
            portable["least_observed_independent_order_total"]
        );
        assert!(identities.insert(study["request_sha256"].as_str().unwrap().to_owned()));
    }
    // A time field cannot silently extend a legacy calendar-date book.
    old.as_object_mut().unwrap().remove("observed_at");
    let legacy = json!({"schema":"ftx-textile-quotes/1","quotes":[old]}).to_string();
    assert!(manufacturing::cut_price(pattern, &cut(), &legacy, &market.to_string()).is_err());
}

#[test]
fn combined_input_budget_precedes_json_parsing() {
    let huge = " ".repeat(64 * 1024 * 1024);
    assert_eq!(
        materials::bind(&huge, "{}").unwrap_err().code,
        ftx_core::RefusalCode::BudgetExceeded
    );
    assert_eq!(
        manufacturing::cut_price("{}", "{}", &huge, "{}")
            .unwrap_err()
            .code,
        ftx_core::RefusalCode::BudgetExceeded
    );
}

#[test]
fn portable_pattern_text_preserves_float_and_negative_zero_receipt_identity() {
    use ftx_core::receipt::sha256_hex;
    let mut raw: Value = serde_json::from_str(PATTERN).unwrap();
    raw["pieces"][0]["vertices"]["a"] = json!([-0.0, 0.0]);
    let original = serde_json::to_string(&raw).unwrap();
    let result = bind(&raw, &changes(selection(0.1, 0.4)));
    assert_eq!(
        result["source_pattern_sha256"],
        sha256_hex(original.as_bytes())
    );
    let text = result["pattern_json"].as_str().unwrap();
    assert!(text.contains("-0.0"));
    assert_eq!(result["pattern_sha256"], sha256_hex(text.as_bytes()));
    assert_eq!(
        serde_json::from_str::<Value>(text).unwrap(),
        result["pattern"]
    );
    let replay: Value = serde_json::from_str(
        &materials::bind(text, "{\"schema\":\"ftx-material-bindings/1\"}").unwrap(),
    )
    .unwrap();
    assert_eq!(replay["pattern_json"], result["pattern_json"]);
    assert_eq!(replay["pattern_sha256"], result["pattern_sha256"]);
}

#[test]
fn screening_reuses_canonical_library_and_requires_an_explicit_comparison_basis() {
    let input = include_str!("../../../tests/fixtures/materials/screen-insulation.json");
    let request: ftx_pattern::material_screen::Request = serde_json::from_str(input).unwrap();
    let expected = request
        .evaluate(&ftx_pattern::textile_import::catalog().unwrap())
        .unwrap();
    let output: Value = serde_json::from_str(&materials::screen(input).unwrap()).unwrap();
    assert_eq!(output, serde_json::to_value(expected).unwrap());
    assert!(!output["frontier"].as_array().unwrap().is_empty());
    assert!(materials::screen(r#"{"schema":"ftx-material-screen/1"}"#).is_err());
}

#[test]
fn study_reuses_production_planners_and_preserves_material_source_locks() {
    let input = include_str!("../../../tests/fixtures/materials/study-insulation.json");
    let request: ftx_export::material_study::Request = serde_json::from_str(input).unwrap();
    let source = Pattern::from_json(PATTERN).unwrap();
    let output: Value = serde_json::from_str(&materials::study(PATTERN, input).unwrap()).unwrap();
    assert_eq!(
        output,
        serde_json::to_value(request.evaluate(&source).unwrap()).unwrap()
    );
    for row in output["alternatives"].as_array().unwrap() {
        let bound = &row["binding"]["result"];
        assert_eq!(bound["pattern"]["pieces"], source.raw()["pieces"]);
        assert!(bound["pattern"]["fabric"]["material"]["expected_sha256"].is_string());
        let text = bound["pattern_json"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed, bound["pattern"]);
    }
    assert!(materials::study("{}", input).is_err());
}

#[test]
fn mass_only_studies_share_the_actual_inventory_consumer_and_require_completeness() {
    let input = include_str!("../../../tests/fixtures/materials/study-assembly-mass.json");
    let request: ftx_export::material_study::Request = serde_json::from_str(input).unwrap();
    let source = Pattern::from_json(PATTERN).unwrap();
    let output: Value = serde_json::from_str(&materials::study(PATTERN, input).unwrap()).unwrap();
    assert_eq!(
        output,
        serde_json::to_value(request.evaluate(&source).unwrap()).unwrap()
    );
    assert!(
        !output["comparison"]["frontier"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    for row in output["alternatives"].as_array().unwrap() {
        let pattern = Pattern::new(row["binding"]["result"]["pattern"].clone());
        let direct = request
            .assembly_mass
            .as_ref()
            .unwrap()
            .evaluate(&pattern)
            .unwrap();
        assert_eq!(
            row["assembly_mass"]["result"],
            serde_json::to_value(direct).unwrap()
        );
        assert_eq!(row["cutting"]["status"], "not_requested");
    }
    let mut incomplete: Value = serde_json::from_str(input).unwrap();
    incomplete["assembly_mass"]["inventory_complete"] = json!(false);
    let refused: Value =
        serde_json::from_str(&materials::study(PATTERN, &incomplete.to_string()).unwrap()).unwrap();
    assert_eq!(refused["comparison"]["frontier"], json!([]));
    assert_ne!(refused["request_sha256"], output["request_sha256"]);
    assert!(
        refused["alternatives"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["metrics"]["assembly_mass_kg"].is_null())
    );
}

#[test]
fn study_comparison_is_shared_with_native_and_retains_unknown_values() {
    let mut input: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/study-insulation.json"
    ))
    .unwrap();
    input["comparison"] = json!({"basis":"nominal", "objectives":[
        {"metric":"stock_mass_kg","direction":"minimize"}], "allow_mixed_protocols":true});
    let request: ftx_export::material_study::Request =
        serde_json::from_value(input.clone()).unwrap();
    let expected = request
        .evaluate(&Pattern::from_json(PATTERN).unwrap())
        .unwrap();
    let output: Value =
        serde_json::from_str(&materials::study(PATTERN, &input.to_string()).unwrap()).unwrap();
    assert_eq!(output, serde_json::to_value(expected).unwrap());
    assert!(
        !output["comparison"]["frontier"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        output["comparison"]["alternatives"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["eligible"] == false)
    );
}

#[test]
fn timed_strip_adapter_preserves_production_dynamics_and_exact_frozen_replay() {
    let input = include_str!("../../../tests/fixtures/materials/synthetic-strip-history.json");
    let request = serde_json::from_str(input).unwrap();
    let expected = ftx_drape::strip::history::evaluate(&request).unwrap();
    let actual = materials::strip_history(input).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&actual).unwrap(),
        serde_json::to_value(&expected).unwrap()
    );
    assert_eq!(
        actual,
        materials::strip_history(&expected.replay_request_json).unwrap()
    );
    let mut denied: Value = serde_json::from_str(input).unwrap();
    denied["material"]["allow_estimates"] = json!(false);
    assert!(materials::strip_history(&denied.to_string()).is_err());
    let mut changed: Value = serde_json::from_str(&expected.replay_request_json).unwrap();
    changed["material"]["record"]["properties"]["areal_density"]["value"] = json!(9.0);
    assert!(materials::strip_history(&changed.to_string()).is_err());
}

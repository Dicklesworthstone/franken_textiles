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

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

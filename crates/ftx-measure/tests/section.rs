//! Published spec 02 A.5/D.5 values and section-model invariants.
use ftx_core::{RefusalCode, receipt::sha256_hex};
use ftx_measure::{
    SectionOptions, calibrate_k, fuse, girth_from_section, make_record, photo_section, priors_for,
    section_measurement, validate,
};
use serde_json::{Value, json};

#[test]
fn published_value_sigma_and_calibration_match() {
    let o = SectionOptions {
        sigma_width_mm: 4.0,
        sigma_depth_mm: 5.0,
        ..Default::default()
    };
    let s = girth_from_section(160.0, 130.0, &o).unwrap();
    assert_eq!(s["girth_mm"], 484.2);
    assert_eq!(s["sigma_mm"], 14.6);
    assert_eq!(s["ellipse_perimeter_mm"], 456.8);
    assert_eq!(
        s["sigma_terms_mm"],
        json!({"width":6.99,"depth":7.87,"k":10.05})
    );
    let o = SectionOptions {
        level: "chest",
        ..Default::default()
    };
    let s = girth_from_section(170.0, 150.0, &o).unwrap();
    assert_eq!(s["girth_mm"], 538.4);
    assert_eq!(s["sigma_mm"], 15.1);
    assert!(s["notes"].as_str().unwrap().contains("UNCERTAIN"));
    let k = calibrate_k(560.0, 172.0, 150.0, 10.0, &o).unwrap();
    assert_eq!(k["k_child"], 1.1058);
    assert_eq!(k["k_child_sigma"], 0.0197);
    assert_eq!(k["k_ratio"], 1.0335);
    assert_eq!(k["k_ratio_sigma"], 0.0185);
    let calibrated = girth_from_section(
        172.0,
        150.0,
        &SectionOptions {
            k_ratio: k["k_ratio"].as_f64().unwrap(),
            k_ratio_sigma: k["k_ratio_sigma"].as_f64().unwrap(),
            ..o
        },
    )
    .unwrap();
    assert!((calibrated["girth_mm"].as_f64().unwrap() - 560.0).abs() < 0.2);
    assert!(calibrated["sigma_mm"].as_f64().unwrap() > 10.0); // Population k uncertainty remains.
}

#[test]
fn chart_sections_recover_their_girths_and_clothing_is_explicit() {
    for height in [800.0, 920.0, 1100.0, 1280.0] {
        let p = priors_for(height, None, None).unwrap();
        for level in ["chest", "waist", "hip"] {
            let w = p[format!("{level}_width")]["value_mm"].as_f64().unwrap();
            let d = p[format!("{level}_depth")]["value_mm"].as_f64().unwrap();
            let s = girth_from_section(
                w,
                d,
                &SectionOptions {
                    level,
                    ..Default::default()
                },
            )
            .unwrap();
            assert!(
                (s["girth_mm"].as_f64().unwrap() - p[level]["value_mm"].as_f64().unwrap()).abs()
                    < 1.0
            );
        }
    }
    let plain = girth_from_section(160.0, 130.0, &SectionOptions::default()).unwrap();
    let clothed = girth_from_section(
        166.0,
        136.0,
        &SectionOptions {
            clothing_mm: 3.0,
            ..Default::default()
        },
    )
    .unwrap();
    for field in ["girth_mm", "sigma_mm", "ellipse_perimeter_mm"] {
        assert_eq!(plain[field], clothed[field]);
    }
    assert!(
        clothed["notes"]
            .as_str()
            .unwrap()
            .contains("clothing offset subtracted")
    );
}

#[test]
fn physical_input_and_uncertainty_refusals_do_not_saturate() {
    for o in [
        SectionOptions {
            level: "unknown",
            ..Default::default()
        },
        SectionOptions {
            k: Some(0.0),
            ..Default::default()
        },
        SectionOptions {
            k_sigma: Some(-0.1),
            ..Default::default()
        },
        SectionOptions {
            k_ratio: 0.0,
            ..Default::default()
        },
        SectionOptions {
            sigma_width_mm: -1.0,
            ..Default::default()
        },
        SectionOptions {
            clothing_mm: 65.0,
            ..Default::default()
        },
        SectionOptions {
            clothing_mm: -1.0,
            ..Default::default()
        },
    ] {
        assert!(girth_from_section(160.0, 130.0, &o).is_err());
    }
    for x in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::MAX] {
        assert!(girth_from_section(x, 130.0, &SectionOptions::default()).is_err());
    }
    assert!(calibrate_k(560.0, 172.0, 150.0, -1.0, &SectionOptions::default()).is_err());
    assert!(calibrate_k(0.0, 172.0, 150.0, 10.0, &SectionOptions::default()).is_err());
}

fn photo_document(name: &str, value: f64, sigma: f64) -> Value {
    json!({
        "schema":"fabric-measurements/1","wearer":{"label":"synthetic section"},
        "taken_on":"2026-10-13","fused":{},
        "records":[make_record(name,value,sigma,"PHOTO_ESTIMATED","synthetic view",None,"").unwrap()]
    })
}

#[test]
fn separate_photo_sources_recompute_evidence_and_retain_original_text() {
    let mut width = photo_document("waist_width", 160.0, 4.0);
    width["fused"]["waist_width"] = json!({"value_mm":9000.0,"sigma_mm":0.1});
    width["capture_seed"] = json!(u64::MAX);
    let width_text = format!(" \r\n{width}\r\n");
    let mut depth = photo_document("waist_depth", 130.0, 5.0);
    depth["taken_on"] = json!("2026-10-11");
    depth["wearer"]["capture_note"] = json!("synthetic paired views");
    let depth_text = format!(" {depth}\n");
    let options = " {\"same_wearer\":true,\"clothing_mm\":0.0} \r\n";
    let out = photo_section(&width_text, &depth_text, options).unwrap();
    assert_eq!(out["girth_mm"], 484.2);
    assert_eq!(out["sigma_mm"], 14.6);
    assert_eq!(
        out["sigma_terms_mm"],
        json!({"width":6.99,"depth":7.87,"k":10.05})
    );
    assert_eq!(out["record"], out["records"][0]);
    assert_eq!(out["record"]["provenance"], "PHOTO_ESTIMATED");
    assert_eq!(out["taken_on"], "2026-10-11");
    assert_eq!(out["wearer"], depth["wearer"]);
    assert!(validate(&out).is_empty());
    assert_eq!(out["section_inputs"]["width"]["value_mm"], 160.0);
    for (key, text) in [
        ("width", &width_text),
        ("depth", &depth_text),
        ("options", &options.to_string()),
    ] {
        assert_eq!(out["section_sources"][key]["text"], *text);
        assert_eq!(
            out["section_sources"][key]["sha256"],
            sha256_hex(text.as_bytes())
        );
    }
    assert_eq!(out["section_sources"]["depth_uses_width_source"], false);
    let fused = fuse(&out, false).unwrap();
    assert_eq!(
        fused["fused"]["waist"]["primary_provenance"],
        "PHOTO_ESTIMATED"
    );
    assert!(out.get("readiness").is_none());
    depth["taken_on"] = Value::Null;
    assert_eq!(
        photo_section(&width_text, &depth.to_string(), options).unwrap()["taken_on"],
        Value::Null
    );
}

#[test]
fn combined_photo_document_and_numeric_record_use_one_construction() {
    let mut combined = photo_document("waist_width", 160.0, 4.0);
    combined["records"]
        .as_array_mut()
        .unwrap()
        .push(photo_document("waist_depth", 130.0, 5.0)["records"][0].clone());
    let text = combined.to_string();
    let actual = photo_section(&text, "", "{}").unwrap();
    let expected = section_measurement(
        160.0,
        130.0,
        &SectionOptions {
            sigma_width_mm: 4.0,
            sigma_depth_mm: 5.0,
            ..Default::default()
        },
        None,
    )
    .unwrap();
    assert_eq!(actual["record"], expected["record"]);
    assert_eq!(actual["section_sources"]["width"]["text"], text);
    assert_eq!(actual["section_sources"]["depth"], Value::Null);
    assert_eq!(actual["section_sources"]["depth_uses_width_source"], true);
    assert_eq!(actual["taken_on"], "2026-10-13");
}

#[test]
fn source_qualified_calibration_retains_assumptions_without_a_measurement() {
    let width = photo_document("chest_width", 172.0, 4.0).to_string();
    let depth = photo_document("chest_depth", 150.0, 5.0).to_string();
    let options =
        r#"{"level":"chest","same_wearer":true,"tape_girth_mm":560.0,"sigma_tape_mm":10.0}"#;
    let actual = photo_section(&width, &depth, options).unwrap();
    let expected = calibrate_k(
        560.0,
        172.0,
        150.0,
        10.0,
        &SectionOptions {
            level: "chest",
            sigma_width_mm: 4.0,
            sigma_depth_mm: 5.0,
            ..Default::default()
        },
    )
    .unwrap();
    for (key, value) in expected.as_object().unwrap() {
        assert_eq!(&actual[key], value, "{key}");
    }
    for key in ["record", "records", "schema", "girth_mm", "readiness"] {
        assert!(actual.get(key).is_none(), "calibration must not emit {key}");
    }
    assert_eq!(actual["section_sources"]["options"]["text"], options);
}

#[test]
fn photo_section_refuses_nonphoto_conflicting_or_cross_wearer_inputs() {
    let width = photo_document("waist_width", 160.0, 4.0);
    let depth = photo_document("waist_depth", 130.0, 5.0).to_string();
    let options = r#"{"same_wearer":true}"#;
    assert!(photo_section(&width.to_string(), &depth, "{}").is_err());
    for provenance in ["MODEL_FIT", "CHART_PRIOR", "MEASURED_TAPE"] {
        let mut wrong = width.clone();
        wrong["records"][0]["provenance"] = json!(provenance);
        assert!(
            photo_section(&wrong.to_string(), &depth, options).is_err(),
            "{provenance}"
        );
    }
    for (value, provenance) in [(160.0, "MODEL_FIT"), (240.0, "PHOTO_ESTIMATED")] {
        let mut mixed = width.clone();
        mixed["records"].as_array_mut().unwrap().push(
            make_record(
                "waist_width",
                value,
                4.0,
                provenance,
                "independent",
                None,
                "",
            )
            .unwrap(),
        );
        assert!(photo_section(&mixed.to_string(), &depth, options).is_err());
    }
    for reference in [json!(true), json!(1), json!("demo"), json!(["reference"])] {
        let mut wrong = width.clone();
        wrong["wearer"]["reference"] = reference;
        assert!(photo_section(&wrong.to_string(), &depth, options).is_err());
    }
    let mut wrong = width.clone();
    wrong["wearer"]["label"] = json!("another synthetic section");
    assert!(photo_section(&wrong.to_string(), &depth, options).is_err());
    wrong = width.clone();
    wrong["records"] = json!([]);
    wrong["fused"]["waist_width"] =
        json!({"value_mm":160,"sigma_mm":4,"provenance":"PHOTO_ESTIMATED"});
    assert!(photo_section(&wrong.to_string(), &depth, options).is_err());
}

#[test]
fn photo_section_options_and_work_budgets_refuse_before_fusion() {
    let width = photo_document("waist_width", 160.0, 4.0);
    let depth = photo_document("waist_depth", 130.0, 5.0);
    for option in [
        json!({"same_wearer":true,"unknown":1}),
        json!({"same_wearer":true,"k":null}),
        json!({"same_wearer":true,"k":0}),
        json!({"same_wearer":true,"clothing_mm":65}),
        json!({"same_wearer":true,"sigma_tape_mm":10}),
        json!({"same_wearer":true,"tape_girth_mm":560,"k_ratio":1}),
        json!({"same_wearer":true,"tape_girth_mm":560,"name":"waist"}),
        json!({"same_wearer":true,"name":"arm_length"}),
        json!({"same_wearer":"true"}),
    ] {
        assert!(
            photo_section(&width.to_string(), &depth.to_string(), &option.to_string()).is_err(),
            "{option}"
        );
    }
    let mut many = width.clone();
    many["records"] = json!(vec![width["records"][0].clone(); 4097]);
    assert_eq!(
        photo_section(&many.to_string(), "", "{}").unwrap_err().code,
        RefusalCode::BudgetExceeded
    );
    many["records"] = json!(vec![width["records"][0].clone(); 4096]);
    assert_eq!(
        photo_section(
            &many.to_string(),
            &depth.to_string(),
            r#"{"same_wearer":true}"#
        )
        .unwrap_err()
        .code,
        RefusalCode::BudgetExceeded
    );
    assert_eq!(
        photo_section(&" ".repeat(16 * 1024 * 1024 + 1), "", "{}")
            .unwrap_err()
            .code,
        RefusalCode::BudgetExceeded
    );
    assert_eq!(
        photo_section(&width.to_string(), "", &" ".repeat(256 * 1024 + 1))
            .unwrap_err()
            .code,
        RefusalCode::BudgetExceeded
    );
}

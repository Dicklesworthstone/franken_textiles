//! Published spec 02 A.5/D.5 values and section-model invariants.
use ftx_measure::{SectionOptions, calibrate_k, girth_from_section, priors_for};
use serde_json::json;

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

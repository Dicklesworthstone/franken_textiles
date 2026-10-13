//! Known answers from spec 04 §3.5 and §11.6, not candidate-generated oracle fixtures.

use ftx_pattern::round::{fit_contract_changes, infer_placements, round_status};
use ftx_pattern::{BodyMeasure, Measurements, PyError};
use serde_json::{Value, json};

#[test]
fn every_status_precedence_combination() {
    // Exercise lower-priority failures in every combination; a higher-priority failure always wins.
    for mask in 0..64 {
        let blocked = mask & 1 != 0;
        let unscaled = mask & 2 != 0;
        let flat_bad = mask & 4 != 0;
        let sim_bad = mask & 8 != 0;
        let export_bad = mask & 16 != 0;
        let not_ready = mask & 32 != 0;
        let flat = if blocked {
            "BLOCKED"
        } else if flat_bad {
            "NOT_CONVERGED"
        } else {
            "CONVERGED"
        };
        let sim = json!({"gating":if sim_bad {vec!["body crossing"]} else {vec![]}, "flags":["advisory"]});
        let exports = json!({"pass":!export_bad});
        let expected = if blocked {
            "BLOCKED"
        } else if unscaled {
            "UNSCALED"
        } else if flat_bad {
            "NOT_CONVERGED"
        } else if sim_bad {
            "SIM_GATING_FLAGS"
        } else if export_bad {
            "EXPORT_CHECKS_FAILED"
        } else if not_ready {
            "MEASUREMENTS_NOT_READY"
        } else {
            "CONVERGED"
        };
        assert_eq!(
            round_status(
                flat,
                Some(&sim),
                Some(&exports),
                if not_ready { "NOT_READY" } else { "READY" },
                unscaled
            ),
            expected,
            "mask {mask}"
        );
    }
    for ready in ["READY_REFERENCE", "NOT_CHECKED", "READY"] {
        assert_eq!(
            round_status("CONVERGED", None, None, ready, false),
            "CONVERGED"
        );
    }
    // A present export report lacking a positive acceptance result cannot pass.
    assert_eq!(
        round_status("CONVERGED", None, Some(&json!({})), "READY", false),
        "EXPORT_CHECKS_FAILED"
    );
}

struct Body;
impl BodyMeasure for Body {
    fn value(&self, name: &str) -> Result<f64, PyError> {
        Measurements(json!({"upper_arm":180.0})).value(name)
    }
    fn sigma(&self, _: &str) -> Option<f64> {
        None
    }
    fn landmark_h(&self, _: &str, name: &str) -> Result<f64, PyError> {
        Measurements(json!({"snp":750.0,"waist":530.0})).value(name)
    }
}

fn pieces() -> Value {
    json!({"pieces":(["bodice_front", "bodice_back", "skirt_front", "skirt_back",
        "pant_front", "pant_back", "sleeve", "waistband"].iter().map(|r| json!({
            "id":r,"role":r,"vertices":{"a":[0,0],"b":[100,0],"c":[40,300]}})).collect::<Vec<_>>())})
}

#[test]
fn all_inferred_placements_have_the_spec_frames_and_anchors() {
    let original = pieces();
    let (out, notes) = infer_placements(&original, &Body).unwrap();
    let ps = &out["pieces"];
    assert_eq!(notes.len(), 8);
    assert!(original["pieces"][0].get("placement").is_none());
    assert_eq!(
        ps[0]["placement"],
        json!({"part":"torso","theta_deg":-4.8,"x0":100.0,
        "y0_landmark":"waist","y0":70.0,"clearance":20.0,"inferred":true})
    );
    assert_eq!(ps[1]["placement"]["x0"], 0.0);
    assert_eq!(ps[1]["placement"]["theta_deg"], 180.0);
    assert_eq!(ps[2]["placement"]["x0"], 100.0);
    assert_eq!(ps[3]["placement"]["x0"], 0.0);
    for i in [4, 5] {
        assert_eq!(ps[i]["placement"]["part"], "leg");
        assert_eq!(ps[i]["placement"]["x0"], 50.0);
        assert_eq!(ps[i]["placement"]["y0"], 300.0);
    }
    assert_eq!(
        ps[6]["placement"],
        json!({"part":"arm","theta_deg":-90.0,"x0":40.0,
        "y0_landmark":"shoulder_tip","y0":300.0,"radius":38.95,"inferred":true})
    );
    assert!(ps[7].get("placement").is_none());
}

#[test]
fn existing_excluded_unscaled_and_failed_inputs_are_not_modified() {
    let mut p = pieces();
    p["pieces"][0]["placement"] = json!({"part":"torso","x0":42});
    p["pieces"][1]["sim"] = json!(false);
    let (out, _) = infer_placements(&p, &Body).unwrap();
    assert_eq!(out["pieces"][0], p["pieces"][0]);
    assert_eq!(out["pieces"][1], p["pieces"][1]);
    p["meta"] = json!({"scale_status":"UNSCALED"});
    assert_eq!(infer_placements(&p, &Body).unwrap(), (p.clone(), vec![]));
    let p = pieces();
    assert!(infer_placements(&p, &Measurements(json!({}))).is_err());
    assert_eq!(p, pieces());
}

#[test]
fn fit_contract_diff_only_compares_ids_and_bands() {
    let old = json!({"points":[{"id":"same","target_mm":[0,1,2],"label":"old"},
        {"id":"removed","target_mm":[0,1,2]}, {"id":"elastic_hold","target_mm":[-40,-30,-20]}]});
    let new = json!({"points":[{"id":"added","target_mm":[0,1,2]},
        {"id":"same","target_mm":[0,1,2],"label":"new"},
        {"id":"elastic_hold","target_mm":[-30,-20,-10]}]});
    assert_eq!(
        fit_contract_changes(&old, &new),
        json!({"new":["added"],"missing":["removed"],"changed":["elastic_hold"]})
    );
}

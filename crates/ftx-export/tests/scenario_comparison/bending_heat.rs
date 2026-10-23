//! Source-ledger admission tests; real bending mechanics and thermal conservation live in drape.
use super::*;

const CHANNEL: &str = "bending_plastic_dissipation_heat";
const PROPERTY: &str = "bending_plastic_heat_fraction";
const WORK: &str = "bending_plastic_dissipation_j";

fn bending_request() -> Request {
    let mut r = plastic_heat_request();
    for (i, variant) in r.variants.iter_mut().enumerate() {
        let mut selection: Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/materials/synthetic-plastic-bending.json"
        ))
        .unwrap();
        selection["record"]["properties"][PROPERTY] = json!({
            "value":if i == 0 {0.} else {0.8},"unit":"1","source":"test","evidence":"estimate",
            "method":"Synthetic curvature-work conversion, not measured textile data",
            "conditions":{"specimen_id":"artificial-coupon-A","conditioning":"synthetic constant state",
                "work_basis":"irreversible_bending_plastic_dissipation",
                "temperature_min_c":"-20","temperature_max_c":"80"}});
        selection["record"]["properties"]["width"] = json!({"value":1.4,"unit":"m",
            "source":"test","evidence":"estimate","method":"Synthetic cutting width"});
        variant.changes.default = Some(serde_json::from_value(selection).unwrap());
    }
    r
}

fn bending_descriptor() -> Value {
    let mut d = heat_descriptor();
    for name in ["transient_surface", "transient_coverage"] {
        d["scenario"]["motion"][name][CHANNEL] = json!("uniform_reference_thickness");
        d["scenario"]["motion"][name]["max_cell_steps"] = json!(5_000_000);
    }
    d
}

fn outcome(p: &Pattern, d: &Value) -> Value {
    let selection = serde_json::from_value(p.raw()["fabric"]["material"].clone()).unwrap();
    let material = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: vec![],
    }
    .resolve(&selection)
    .unwrap();
    let fraction = material.bending_plastic_heat_fraction().unwrap().unwrap();
    let law = material
        .bending_plasticity()
        .unwrap()
        .unwrap()
        .fingerprint();
    let mut result = synthetic_heat(p, -8., 0.);
    let m = &mut result["motion"]["result"];
    m["triangles"] = json!([[0, 1, 2]]);
    m["initial_sample"] = json!({"time_s":0.,"bending_plastic_dissipation_j":[2.]});
    let patch = json!({"triangle":0,"vertices":[0,1,2,3,4,5],"reference_area_m2":0.5,
        "law_sha256":law,"plastic_dissipation_j":2.});
    m["initial_bending_history"] = json!({"plastic_dissipation_j":2.,"patches":[patch]});
    let mut final_patch = patch;
    final_patch["plastic_dissipation_j"] = json!(5.);
    m["bending_history"] = json!({"plastic_dissipation_j":5.,"patches":[final_patch]});
    for (i, work) in [1., 3.].into_iter().enumerate() {
        m["samples"][i][WORK] = json!([2. + work]);
    }
    let snapshot = |work: f64| {
        json!({"distribution":"uniform_reference_thickness",
        "temporal_distribution":"uniform_over_accepted_interval","plastic_dissipation_j":work,
        "supplied_heat_j":fraction.fraction*work,"unconverted_dissipation_j":(1.-fraction.fraction)*work,
        "initial_plastic_dissipation_j":2.})
    };
    for (name, stored) in [
        ("transient_surface", "stored_change_j"),
        ("transient_coverage", "physical_stored_change_j"),
    ] {
        let h = &mut m[name]["result"];
        for (i, work) in [1., 3.].into_iter().enumerate() {
            let row = &mut h["trace"][i];
            row[CHANNEL] = snapshot(work);
            row["inside_energy_j"] = json!(-4. * (i + 1) as f64);
            row["outside_energy_j"] = row["inside_energy_j"].clone();
            row["net_boundary_energy_j"] = json!(0.);
            row[stored] = json!(fraction.fraction * work);
            row["balance_error_j"] = json!(0.);
            row["temperature_range_c"] = json!([-8., -2. + fraction.fraction * work]);
        }
        h["request"] = d["scenario"]["motion"][name].clone();
        h["cell_steps"] = json!(1032);
        h["inside_energy_j"] = json!(-8.);
        h["outside_energy_j"] = json!(-8.);
        h[stored] = json!(fraction.fraction * 3.);
        h[CHANNEL] = snapshot(3.);
        h[CHANNEL]["material_fractions"] = json!([{
            "material_sha256":material.sha256(),"law_sha256":law,"fraction":fraction.fraction,
            "temperature_range_c":fraction.temperature_range_c,
            "observation":material.authored().properties[PROPERTY]}]);
    }
    result
}

fn evaluate(r: &Request, d: &Value, mut alter: impl FnMut(&str, &mut Value)) -> Value {
    serde_json::to_value(
        r.evaluate_with(&Pattern::from_json(PATTERN).unwrap(), d, |key, p| {
            let mut result = outcome(p, d);
            alter(key, &mut result);
            Ok(result)
        })
        .unwrap(),
    )
    .unwrap()
}

#[test]
fn qualified_bending_heat_admits_both_domains_and_replays_exactly() {
    let r = bending_request();
    let d = bending_descriptor();
    let result = evaluate(&r, &d, |_, _| {});
    assert_eq!(result["comparison"]["frontier"], json!(["a"]));
    assert_eq!(result["comparison"]["incomparable"], json!([]));
    for metric in [
        "transient_surface_final_max_temperature_c",
        "transient_coverage_inside_energy_j",
    ] {
        assert!(
            result["comparison"]["alternatives"][0]["readings"][metric].is_object(),
            "{metric}: {result}"
        );
    }
    assert_eq!(evaluate(&r, &d, |_, _| {}), result);
}

#[test]
fn altered_bending_work_or_patch_ownership_cannot_rank() {
    let r = bending_request();
    let d = bending_descriptor();
    for fault in [
        "initial_work",
        "source_triangle",
        "final_work",
        "final_law",
        "final_stencil",
        "missing_patch",
        "sample",
        "regression",
    ] {
        let result = evaluate(&r, &d, |key, result| {
            if key != "a" {
                return;
            }
            let m = &mut result["motion"]["result"];
            match fault {
                "initial_work" => {
                    m["initial_bending_history"]["patches"][0]["plastic_dissipation_j"] = json!(0.)
                }
                "source_triangle" => {
                    m["initial_bending_history"]["patches"][0]["vertices"] =
                        json!([1, 0, 2, 3, 4, 5])
                }
                "final_work" => {
                    m["bending_history"]["patches"][0]["plastic_dissipation_j"] = json!(6.)
                }
                "final_law" => m["bending_history"]["patches"][0]["law_sha256"] = json!("stale"),
                "final_stencil" => {
                    m["bending_history"]["patches"][0]["vertices"] = json!([0, 1, 2, 3, 4, 6])
                }
                "missing_patch" => m["bending_history"]["patches"] = json!([]),
                "sample" => m["samples"][0][WORK] = Value::Null,
                "regression" => m["samples"][1][WORK] = json!([2.5]),
                _ => unreachable!(),
            }
        });
        assert_eq!(
            result["comparison"]["frontier"],
            json!(["b"]),
            "{fault}: {result}"
        );
        assert!(result["comparison"]["alternatives"][0]["readings"]["transient_surface_final_max_temperature_c"].is_null(), "{fault}");
        assert_eq!(
            result["simulations"][0]["evaluation"]["result"]["motion"]["status"],
            "available"
        );
        assert_eq!(
            result["manufacturing"]["alternatives"][0]["cutting"]["status"],
            "available"
        );
    }
}

#[test]
fn bending_receipt_failures_are_local_and_unconsumed_conversion_does_not_split_protocols() {
    let mut r = bending_request();
    let d = bending_descriptor();
    for fault in ["fraction", "observation", "heat", "policy", "missing"] {
        let result = evaluate(&r, &d, |key, result| {
            if key != "a" {
                return;
            }
            let h = &mut result["motion"]["result"]["transient_surface"]["result"];
            match fault {
                "fraction" => h[CHANNEL]["material_fractions"][0]["fraction"] = json!(0.5),
                "observation" => {
                    h[CHANNEL]["material_fractions"][0]["observation"]["method"] =
                        json!("unrelated source")
                }
                "heat" => h["trace"][0][CHANNEL]["supplied_heat_j"] = json!(1.),
                "policy" => h["request"][CHANNEL] = Value::Null,
                "missing" => h[CHANNEL] = Value::Null,
                _ => unreachable!(),
            }
        });
        assert_eq!(result["comparison"]["frontier"], json!(["b"]), "{fault}");
        let readings = &result["comparison"]["alternatives"][0]["readings"];
        assert!(
            readings["transient_surface_final_max_temperature_c"].is_null(),
            "{fault}"
        );
        assert!(
            readings["transient_coverage_inside_energy_j"].is_object(),
            "{fault}"
        );
    }
    r.variants[1]
        .changes
        .default
        .as_mut()
        .unwrap()
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut(PROPERTY)
        .unwrap()
        .method = "Different conversion procedure".into();
    let heated = evaluate(&r, &d, |_, _| {});
    assert_eq!(heated["comparison"]["frontier"], json!(["a", "b"]));
    assert_eq!(
        heated["comparison"]["incomparable"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let uncoupled = run_heat(&r, |key, p| {
        // Coverage energy is a declared objective; surface inside energy is not.
        // Keep the final surface temperatures equal and give a the better coverage value.
        Ok(synthetic_heat(p, 0., if key == "a" { -8. } else { -4. }))
    });
    assert_eq!(uncoupled["comparison"]["incomparable"], json!([]));
    assert_eq!(
        uncoupled["comparison"]["alternatives"][0]["readings"]["transient_coverage_inside_energy_j"]
            ["value"],
        json!(-8.)
    );
    assert_eq!(uncoupled["comparison"]["frontier"], json!(["a"]));
}

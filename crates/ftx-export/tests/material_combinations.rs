//! Heterogeneous design choices run actual binding, markers, assembly mass and whole-order pricing.
use ftx_core::RefusalCode;
use ftx_export::material_study::Request;
use ftx_pattern::{Pattern, material_catalog::Selection, textile_import};
use serde_json::{Value, json};

const PATTERN: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-sewing-pattern.json");

fn selection(id: &str, density: f64) -> Value {
    let raw: Value = serde_json::from_str(PATTERN).unwrap();
    let mut s = raw["fabric"]["material"].clone();
    s["id"] = json!(id);
    s["record"]["id"] = json!(id);
    for (key, value, unit) in [("width", 0.1, "m"), ("areal_density", density, "kg/m2")] {
        s["record"]["properties"][key] = json!({"value":value,"unit":unit,
            "evidence":"estimate","source":"test","method":"Synthetic software test only"});
    }
    s
}

fn pattern() -> Pattern {
    let mut p: Value = serde_json::from_str(PATTERN).unwrap();
    p["pieces"][0]["vertices"]["c"][1] = json!(90.);
    p["pieces"][0]["vertices"]["d"][1] = json!(90.);
    p["pieces"][1]["vertices"]["c"][1] = json!(40.);
    p["pieces"][1]["vertices"]["d"][1] = json!(40.);
    Pattern::new(p)
}

fn request() -> Value {
    let choices = [
        ("shell-heavy", 0.4, 5.),
        ("shell-light", 0.1, 20.),
        ("lining-heavy", 0.3, 5.),
        ("lining-light", 0.05, 6.),
    ];
    let quotes: Vec<_> = choices
        .iter()
        .map(|(id, _, price)| {
            json!({
                "offer_id":format!("synthetic/{id}"),"material_id":id,"supplier":"Synthetic only",
                "sku":id,"price":price,"quantity":1.,"unit":"linear_meter",
                "minimum_quantity":0.1,"order_increment":0.1,"currency":"USD","region":"test",
                "quality":"synthetic","terms":"no-shipping","observed_on":"2026-10-20",
                "available":true,"source":"test:combinations","source_sha256":"0".repeat(64)
            })
        })
        .collect();
    json!({"schema":"ftx-material-study/1","combinations":{"groups":[
        {"key":"shell","target":{"scope":"pieces","ids":["front"]},"choices":[
            {"key":"heavy","selection":selection("shell-heavy",0.4)},
            {"key":"light","selection":selection("shell-light",0.1)}]},
        {"key":"lining","target":{"scope":"pieces","ids":["back"]},"choices":[
            {"key":"heavy","selection":selection("lining-heavy",0.3)},
            {"key":"light","selection":selection("lining-light",0.05)}]}
        ]},
        "cutting":{"copies":1,"gap_mm":0.,"edge_margin_mm":0.,"end_margin_mm":0.,"seam_allowance_mm":0.},
        "assembly_mass":{"schema":"ftx-assembly-mass/1","inventory_complete":true,"extras":[],"seam_allowance_mm":0.},
        "purchasing":{"quotes":{"schema":"ftx-textile-quotes/1","quotes":quotes},"market":{
            "currency":"USD","region":"test","quality":"synthetic","terms":"no-shipping",
            "as_of":"2026-10-20","max_age_days":1}},
        "comparison":{"basis":"nominal","objectives":[
            {"metric":"assembly_mass_kg","direction":"minimize"},
            {"metric":"observed_order_cost","direction":"minimize"}],
            "constraints":[{"metric":"assembly_mass_kg","maximum":0.0009}]}
    })
}

fn evaluate(request: &Value, pattern: &Pattern) -> Value {
    let request: Request = serde_json::from_value(request.clone()).unwrap();
    serde_json::to_value(request.evaluate(pattern).unwrap()).unwrap()
}

fn chosen<'a>(report: &'a Value, shell: &str, lining: &str) -> &'a Value {
    report["alternatives"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["choices"]["shell"] == shell && row["choices"]["lining"] == lining)
        .unwrap()
}

#[test]
fn mixed_choices_run_real_consumers_and_match_manual_enumeration_without_mutating_inputs() {
    let p = pattern();
    let input = request();
    let original = p.raw().clone();
    let original_request = input.clone();
    let result = evaluate(&input, &p);
    assert_eq!(result["alternatives"].as_array().unwrap().len(), 4);
    for (shell, lining, mass, cost) in [
        ("heavy", "heavy", 0.00096, 1.0),
        ("heavy", "light", 0.00076, 1.1),
        ("light", "heavy", 0.00042, 2.5),
        ("light", "light", 0.00022, 2.6),
    ] {
        let row = chosen(&result, shell, lining);
        assert_eq!(
            serde_json::from_str::<Value>(row["key"].as_str().unwrap()).unwrap(),
            row["choices"]
        );
        assert_eq!(row["binding"]["status"], "available");
        assert_eq!(row["cutting"]["status"], "available");
        assert!((row["metrics"]["assembly_mass_kg"].as_f64().unwrap() - mass).abs() < 1e-15);
        assert!(
            (row["metrics"]["least_observed_independent_order_total"]
                .as_f64()
                .unwrap()
                - cost)
                .abs()
                < 1e-14
        );
        let frozen = &row["frozen_changes"]["pieces"];
        for id in ["front", "back"] {
            assert!(frozen[id]["record"].is_object());
            assert_eq!(frozen[id]["expected_sha256"].as_str().unwrap().len(), 64);
        }
    }
    let frontier = result["comparison"]["frontier"].as_array().unwrap();
    assert_eq!(frontier.len(), 3);
    assert!(!frontier.contains(&chosen(&result, "heavy", "heavy")["key"]));
    assert!(frontier.contains(&chosen(&result, "heavy", "light")["key"]));
    assert!(frontier.contains(&chosen(&result, "light", "light")["key"]));

    // The complete search uses the same bindings and planners as hand-authored heterogeneous rows.
    let mut manual = input.clone();
    manual.as_object_mut().unwrap().remove("combinations");
    manual["variants"] = json!(
        result["alternatives"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| { json!({"key":row["key"],"changes":row["frozen_changes"]}) })
            .collect::<Vec<_>>()
    );
    let replay = evaluate(&manual, &p);
    for (generated, explicit) in result["alternatives"]
        .as_array()
        .unwrap()
        .iter()
        .zip(replay["alternatives"].as_array().unwrap())
    {
        for key in [
            "binding",
            "frozen_changes",
            "cutting",
            "assembly_mass",
            "purchasing",
            "metrics",
        ] {
            assert_eq!(generated[key], explicit[key], "{key}");
        }
    }
    assert_eq!(
        result["comparison"]["frontier"],
        replay["comparison"]["frontier"]
    );
    assert_eq!(p.raw(), &original);
    assert_eq!(input, original_request);
}

#[test]
fn group_and_choice_order_does_not_change_generated_rows_or_ambiguous_text_keys() {
    let p = pattern();
    let mut input = request();
    input["combinations"]["groups"][0]["key"] = json!("shell=\" | ");
    input["combinations"]["groups"][0]["choices"][0]["key"] = json!("lining:{heavy}");
    let a = evaluate(&input, &p);
    input["combinations"]["groups"]
        .as_array_mut()
        .unwrap()
        .reverse();
    for group in input["combinations"]["groups"].as_array_mut().unwrap() {
        group["choices"].as_array_mut().unwrap().reverse();
    }
    let b = evaluate(&input, &p);
    assert_eq!(a["alternatives"], b["alternatives"]);
    assert_eq!(a["comparison"]["frontier"], b["comparison"]["frontier"]);
    for row in a["alternatives"].as_array().unwrap() {
        assert_eq!(
            serde_json::from_str::<Value>(row["key"].as_str().unwrap()).unwrap(),
            row["choices"]
        );
    }
}

#[test]
fn missing_choice_and_missing_width_preserve_other_combinations_and_mass() {
    let p = pattern();
    let mut input = request();
    input["combinations"]["groups"][0]["choices"][1]["selection"] =
        json!({"id":"no-such-material"});
    input["combinations"]["groups"][1]["choices"][1]["selection"]["record"]["properties"]
        .as_object_mut()
        .unwrap()
        .remove("width");
    let result = evaluate(&input, &p);
    assert_eq!(
        chosen(&result, "heavy", "heavy")["cutting"]["status"],
        "available"
    );
    let missing_width = chosen(&result, "heavy", "light");
    assert_eq!(missing_width["binding"]["status"], "available");
    assert_eq!(missing_width["cutting"]["status"], "refused");
    assert_eq!(missing_width["purchasing"]["status"], "refused");
    assert!(missing_width["metrics"]["assembly_mass_kg"].is_number());
    for lining in ["heavy", "light"] {
        let row = chosen(&result, "light", lining);
        assert_eq!(row["binding"]["status"], "refused");
        assert_eq!(row["assembly_mass"]["status"], "refused");
        assert!(row["frozen_changes"].is_null());
    }
}

#[test]
fn oversized_search_and_ambiguous_targets_refuse_before_any_additional_consumer() {
    let p = pattern();
    let mut oversized = request();
    for group in oversized["combinations"]["groups"].as_array_mut().unwrap() {
        group["choices"] = json!(
            (0..6)
                .map(|n| json!({"key":n.to_string(),"selection":{"id":"not-resolved"}}))
                .collect::<Vec<_>>()
        );
    }
    let r: Request = serde_json::from_value(oversized).unwrap();
    let mut called = false;
    let refused = r
        .evaluate_with(&p, &json!({}), |_, _| {
            called = true;
            Ok(())
        })
        .unwrap_err();
    assert_eq!(refused.code, RefusalCode::BudgetExceeded);
    assert!(!called);
    for kind in [
        "overlap",
        "duplicate-group",
        "duplicate-choice",
        "missing-piece",
        "empty",
        "two-defaults",
        "mixed-sources",
    ] {
        let mut input = request();
        match kind {
            "overlap" => input["combinations"]["groups"][1]["target"]["ids"] = json!(["front"]),
            "duplicate-group" => input["combinations"]["groups"][1]["key"] = json!("shell"),
            "duplicate-choice" => {
                input["combinations"]["groups"][0]["choices"][1]["key"] = json!("heavy")
            }
            "missing-piece" => {
                input["combinations"]["groups"][0]["target"]["ids"] = json!(["absent"])
            }
            "empty" => input["combinations"]["groups"] = json!([]),
            "two-defaults" => {
                for group in input["combinations"]["groups"].as_array_mut().unwrap() {
                    group["target"] = json!({"scope":"default"});
                }
            }
            "mixed-sources" => {
                input["variants"] =
                    json!([{"key":"extra","changes":{"schema":"ftx-material-bindings/1"}}])
            }
            _ => unreachable!(),
        }
        let r: Request = serde_json::from_value(input).unwrap();
        assert!(r.evaluate(&p).is_err(), "{kind}");
    }
}

fn material_hash(selection: &Value) -> String {
    let selection: Selection = serde_json::from_value(selection.clone()).unwrap();
    textile_import::catalog()
        .unwrap()
        .resolve(&selection)
        .unwrap()
        .sha256()
        .into()
}

fn interface(a: &Value, b: &Value, mu: f64) -> Value {
    json!({"materials":[material_hash(a),material_hash(b)],
        "friction":{"value":mu,"unit":"1","evidence":"estimate","source":"test","method":"Synthetic sliding pair only"},
        "sources":{"test":{"reference":"test:combinations-pair","accessed_on":"2026-10-20","note":"Synthetic test only"}},"allow_estimates":true})
}

#[test]
fn exact_pair_pool_follows_effective_materials_and_freezes_only_selected_evidence() {
    let p = pattern();
    let mut input = request();
    let groups = input["combinations"]["groups"].as_array().unwrap();
    let mut pool = Vec::new();
    for (i, shell) in groups[0]["choices"].as_array().unwrap().iter().enumerate() {
        for (j, lining) in groups[1]["choices"].as_array().unwrap().iter().enumerate() {
            pool.push(interface(
                &shell["selection"],
                &lining["selection"],
                0.1 + (2 * i + j) as f64 * 0.05,
            ));
        }
    }
    input["combinations"]["interfaces"] = json!(pool);
    let result = evaluate(&input, &p);
    for row in result["alternatives"].as_array().unwrap() {
        let bound = &row["binding"]["result"];
        let selected = bound["pattern"]["fabric"]["interfaces"].as_array().unwrap();
        assert_eq!(selected.len(), 1);
        assert!(pool.contains(&selected[0]));
        assert_eq!(row["frozen_changes"]["interfaces"], json!(selected));
        let assigned: Vec<_> = bound["pieces"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["material_sha256"].clone())
            .collect();
        for hash in selected[0]["materials"].as_array().unwrap() {
            assert!(assigned.contains(hash));
        }
        let frozen: ftx_pattern::material_binding::Changes =
            serde_json::from_value(row["frozen_changes"].clone()).unwrap();
        assert_eq!(frozen.apply(&p).unwrap(), *bound);
    }
    // Unsupported/malformed evidence cannot hide as an unused entry in the pool.
    let mut invalid = input.clone();
    let mut unused = pool[0].clone();
    unused["materials"] = json!(["a".repeat(64), "b".repeat(64)]);
    unused["friction"]["unit"] = json!("N");
    invalid["combinations"]["interfaces"]
        .as_array_mut()
        .unwrap()
        .push(unused);
    let r: Request = serde_json::from_value(invalid).unwrap();
    assert!(r.evaluate(&p).is_err());
    // Canonical identities are lowercase. Noncanonical spellings must refuse up front rather
    // than disappearing as unused evidence or bypassing duplicate detection in a mixed-case pool.
    let mut uppercase = pool[0].clone();
    for hash in uppercase["materials"].as_array_mut().unwrap() {
        let upper = hash.as_str().unwrap().to_ascii_uppercase();
        assert_ne!(upper, hash.as_str().unwrap());
        *hash = json!(upper);
    }
    for include_canonical in [false, true] {
        let mut noncanonical = input.clone();
        noncanonical["combinations"]["interfaces"] = if include_canonical {
            json!([pool[0].clone(), uppercase.clone()])
        } else {
            json!([uppercase.clone()])
        };
        let r: Request = serde_json::from_value(noncanonical).unwrap();
        let refusal = r.evaluate(&p).unwrap_err();
        assert_eq!(refusal.code, RefusalCode::InvalidDocument);
        assert!(refusal.message.contains("lowercase SHA-256"));
    }
    let mut duplicate = pool[0].clone();
    duplicate["materials"].as_array_mut().unwrap().reverse();
    input["combinations"]["interfaces"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    let r: Request = serde_json::from_value(input).unwrap();
    assert!(r.evaluate(&p).is_err());
}

#[test]
fn explicit_default_preserves_authored_piece_selection_and_named_group_overrides_only_its_target() {
    let mut p = pattern().raw().clone();
    let retained = selection("retained-lining", 0.2);
    p["pieces"][1]["fabric"] = json!({"material":retained});
    let p = Pattern::new(p);
    let mut input = request();
    input["combinations"]["groups"][0]["target"] = json!({"scope":"default"});
    input["combinations"]["groups"]
        .as_array_mut()
        .unwrap()
        .truncate(1);
    let result = evaluate(&input, &p);
    for row in result["alternatives"].as_array().unwrap() {
        let bound = &row["binding"]["result"];
        assert_eq!(bound["pieces"][1]["material_id"], "retained-lining");
        assert_eq!(bound["pieces"][1]["inherited_default"], false);
        assert_eq!(bound["pieces"][0]["inherited_default"], true);
    }
    input["combinations"]["groups"]
        .as_array_mut()
        .unwrap()
        .push(request()["combinations"]["groups"][1].clone());
    let result = evaluate(&input, &p);
    for row in result["alternatives"].as_array().unwrap() {
        let id = row["binding"]["result"]["pieces"][1]["material_id"]
            .as_str()
            .unwrap();
        assert!(id.starts_with("lining-"));
        assert_eq!(
            row["binding"]["result"]["pieces"][1]["inherited_default"],
            false
        );
    }
}

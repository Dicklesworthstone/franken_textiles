//! Native execution of the file-free browser study API, using real finite-stock/roll planners.
use ftx_export::{cutting::stock, material_study::Request};
use ftx_pattern::Pattern;
use ftx_wasm::materials;
use serde_json::{Value, json};

const PATTERN: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-stock-pattern.json");
const STOCK: &str = include_str!("../../../tests/fixtures/materials/synthetic-stock-request.json");

#[test]
fn portable_mixed_job_replays_every_physical_part_and_refuses_an_unmatched_material() {
    let pattern = include_str!("../../../examples/mixed-cutting/pattern.json");
    let input = include_str!("../../../examples/mixed-cutting/study.json");
    let request: Request = serde_json::from_str(input).unwrap();
    let expected = serde_json::to_value(
        request
            .evaluate(&Pattern::from_json(pattern).unwrap())
            .unwrap(),
    )
    .unwrap();
    let output = materials::study(pattern, input).unwrap();
    let report: Value = serde_json::from_str(&output).unwrap();
    assert_eq!(report, expected);
    let job = &report["alternatives"][0]["cutting_job"]["result"];
    assert_eq!(job["complete_inventory"], true);
    assert_eq!(job["inventory"].as_array().unwrap().len(), 6);
    assert_eq!(job["hardware_execution_authorized"], false);
    let replay: Value = serde_json::from_str(
        &ftx_wasm::manufacturing::cut_job(
            job["pattern_json"].as_str().unwrap(),
            job["replay_request_json"].as_str().unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(&replay, job);
    let mut unavailable: Value = serde_json::from_str(input).unwrap();
    let mut changed = serde_json::from_str::<Value>(pattern).unwrap()["fabric"]["material"].clone();
    changed["record"]["name"] = json!("Different synthetic hide; unqualified physical inventory");
    unavailable["variants"][0]["changes"]["default"] = changed;
    let refused: Value =
        serde_json::from_str(&materials::study(pattern, &unavailable.to_string()).unwrap())
            .unwrap();
    let row = &refused["alternatives"][0];
    assert_eq!(row["binding"]["status"], "available");
    assert_eq!(row["cutting_job"]["status"], "refused");
    assert!(row["cutting_job"].get("result").is_none());
    assert_eq!(refused["comparison"]["frontier"], json!([]));
    assert_eq!(output, materials::study(pattern, input).unwrap());
}

#[test]
fn portable_study_follows_material_routes_and_keeps_all_source_choices_for_replay() {
    let pattern = include_str!("../../../examples/mixed-cutting/pattern.json");
    let source: Value = serde_json::from_str(pattern).unwrap();
    let mut request: Value =
        serde_json::from_str(include_str!("../../../examples/mixed-cutting/study.json")).unwrap();
    let catalog = ftx_pattern::textile_import::catalog().unwrap();
    let hide = source["fabric"]["material"].clone();
    let mut linen = source["pieces"][1]["fabric"]["material"].clone();
    linen["id"] = json!("synthetic-linen-shell");
    linen["record"]["id"] = linen["id"].clone();
    let mut knit = linen.clone();
    knit["id"] = json!("synthetic-knit-shell");
    knit["record"]["id"] = knit["id"].clone();
    knit["record"]["form"] = json!("knit");
    let mut variants = Vec::new();
    let mut choices = Vec::new();
    for (key, selection, stock) in [
        ("hide", hide, "finite_stock"),
        ("linen", linen, "roll"),
        ("knit", knit, "roll"),
    ] {
        let material = catalog
            .resolve(&serde_json::from_value(selection.clone()).unwrap())
            .unwrap();
        choices.push(json!({"material_sha256":material.sha256(),"stock":stock}));
        variants.push(
            json!({"key":key,"changes":{"schema":"ftx-material-bindings/1","default":selection}}),
        );
    }
    request["variants"] = json!(variants);
    request["cutting_job"]["schema"] = json!("ftx-cutting-job/2");
    request["cutting_job"]["routes"] = json!([{"piece":"lining","stock":"roll"}]);
    request["cutting_job"]["material_routes"] = json!([{"piece":"shell","choices":choices}]);
    request["comparison"]["allow_mixed_protocols"] = json!(true);
    let input = serde_json::to_string(&request).unwrap();
    let output = materials::study(pattern, &input).unwrap();
    let report: Value = serde_json::from_str(&output).unwrap();
    assert_eq!(report["geometry_changed"], false);
    for row in report["alternatives"].as_array().unwrap() {
        assert_eq!(row["cutting_job"]["status"], "available");
        let job = &row["cutting_job"]["result"];
        let shell = job["resolved_routes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|route| route["piece"] == "shell")
            .unwrap();
        assert_eq!(
            shell["stock"],
            if row["key"] == "hide" {
                "finite_stock"
            } else {
                "roll"
            }
        );
        assert_eq!(job["inventory"].as_array().unwrap().len(), 6);
        assert_eq!(
            job["request"]["material_routes"][0]["choices"],
            request["cutting_job"]["material_routes"][0]["choices"]
        );
        let frozen: Value = serde_json::from_str(job["pattern_json"].as_str().unwrap()).unwrap();
        for (after, before) in frozen["pieces"]
            .as_array()
            .unwrap()
            .iter()
            .zip(source["pieces"].as_array().unwrap())
        {
            for field in [
                "id", "cut", "grain", "vertices", "edges", "internal", "drills", "sim",
            ] {
                assert_eq!(
                    after[field], before[field],
                    "unchanged pattern field {field}"
                );
            }
        }
        assert_eq!(frozen["seams"], source["seams"]);
        let replay: Value = serde_json::from_str(
            &ftx_wasm::manufacturing::cut_job(
                job["pattern_json"].as_str().unwrap(),
                job["replay_request_json"].as_str().unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(&replay, job);
    }
    let mut changed = request;
    changed["variants"][1]["changes"]["default"]["record"]["properties"]["width"]["value"] =
        json!(0.11);
    let refused: Value =
        serde_json::from_str(&materials::study(pattern, &changed.to_string()).unwrap()).unwrap();
    let row = refused["alternatives"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["key"] == "linen")
        .unwrap();
    assert_eq!(row["binding"]["status"], "available");
    assert_eq!(row["cutting_job"]["status"], "refused");
    assert!(
        row["cutting_job"]["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("no cutting route declares material snapshot")
    );
    assert_eq!(output, materials::study(pattern, &input).unwrap());
}

#[test]
fn portable_study_rebinds_vapor_layers_and_replays_each_exact_standalone_request() {
    let pattern = include_str!("../../../tests/fixtures/materials/synthetic-sewing-pattern.json");
    let raw = include_str!("../../../examples/vapor-study/leather.json");
    let request: Request = serde_json::from_str(raw).unwrap();
    let expected = serde_json::to_value(
        request
            .evaluate(&Pattern::from_json(pattern).unwrap())
            .unwrap(),
    )
    .unwrap();
    let output = materials::study(pattern, raw).unwrap();
    let report: Value = serde_json::from_str(&output).unwrap();
    assert_eq!(report, expected);
    assert_eq!(output, materials::study(pattern, raw).unwrap());
    assert_eq!(report["comparison"]["frontier"], json!(["chrome-neck"]));
    let rows = report["alternatives"].as_array().unwrap();
    let mut replay: Value = serde_json::from_str(raw).unwrap();
    for (variant, row) in replay["variants"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .zip(rows)
    {
        assert_eq!(variant["key"], row["key"]);
        variant["changes"] = row["frozen_changes"].clone();
        if row["vapor"]["status"] != "available" {
            assert_eq!(row["vapor"]["status"], "refused");
            assert_eq!(row["binding"]["status"], "available");
            continue;
        }
        let vapor = &row["vapor"]["result"];
        let standalone: Value = serde_json::from_str(
            &ftx_wasm::thermal::vapor(vapor["replay_request_json"].as_str().unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(&standalone, vapor);
        assert_eq!(vapor["physical_validation_claim"], false);
    }
    let replayed: Value =
        serde_json::from_str(&materials::study(pattern, &replay.to_string()).unwrap()).unwrap();
    assert_eq!(replayed["alternatives"], report["alternatives"]);
    let original: Value = serde_json::from_str(pattern).unwrap();
    for row in rows {
        assert_eq!(
            row["binding"]["result"]["pattern"]["seams"],
            original["seams"]
        );
        assert_eq!(
            row["binding"]["result"]["pattern"]["pieces"][0]["vertices"],
            original["pieces"][0]["vertices"]
        );
    }
}

#[test]
fn browser_study_selects_qualified_recipes_and_keeps_frozen_replay_and_failures() {
    let pattern = include_str!("../../../tests/fixtures/materials/synthetic-sewing-pattern.json");
    let source: Value = serde_json::from_str(pattern).unwrap();
    let original = source["fabric"]["material"].clone();
    let mut changed = original.clone();
    changed["record"]["name"] = json!("Different lot; identical nominal thickness");
    let mut input = json!({"schema":"ftx-material-study/1","variants":[
        {"key":"original","changes":{"schema":"ftx-material-bindings/1","default":original}},
        {"key":"changed","changes":{"schema":"ftx-material-bindings/1","default":changed}}],
        "sewing_selection":serde_json::from_str::<Value>(include_str!(
            "../../../tests/fixtures/materials/synthetic-sewing-selection.json")).unwrap(),
        "comparison":{"basis":"nominal","required_consumers":["sewing"],
            "objectives":[{"metric":"feed_seconds","direction":"minimize"}]}});
    let typed: Request = serde_json::from_value(input.clone()).unwrap();
    let expected = serde_json::to_value(typed.evaluate(&Pattern::new(source)).unwrap()).unwrap();
    let text = materials::study(pattern, &input.to_string()).unwrap();
    let report: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report, expected);
    assert_eq!(text, materials::study(pattern, &input.to_string()).unwrap());
    let rows = report["alternatives"].as_array().unwrap();
    assert_eq!(rows[0]["sewing"]["status"], "refused");
    assert_eq!(
        rows[0]["sewing_selection"]["result"]["no_feasible_listed_recipe"],
        true
    );
    assert_eq!(rows[1]["sewing"]["status"], "available");
    assert_eq!(
        rows[1]["sewing_selection"]["result"]["selected_choices"]["side"],
        "faster-qualified"
    );
    assert_eq!(rows[1]["metrics"]["feed_seconds"], 2.0);
    assert_eq!(report["comparison"]["frontier"], json!(["original"]));
    for variant in input["variants"].as_array_mut().unwrap() {
        let row = rows
            .iter()
            .find(|row| row["key"] == variant["key"])
            .unwrap();
        variant["changes"] = row["frozen_changes"].clone();
    }
    let replay: Value =
        serde_json::from_str(&materials::study(pattern, &input.to_string()).unwrap()).unwrap();
    assert_eq!(replay["alternatives"], report["alternatives"]);
}

#[test]
fn file_free_study_expands_complete_heterogeneous_choices_without_pruning_refusals() {
    let source: Value = serde_json::from_str(PATTERN).unwrap();
    let hide = source["fabric"]["material"].clone();
    let mut roll = hide.clone();
    roll["record"]["form"] = json!("woven");
    roll["record"]["properties"]["width"] = json!({"value":0.1,"unit":"m",
        "evidence":"estimate","source":"test","method":"Synthetic roll width"});
    let choices = json!([{"key":"hide","selection":hide},{"key":"roll","selection":roll}]);
    let input = json!({"schema":"ftx-material-study/1","combinations":{"groups":[
        {"key":"main","target":{"scope":"default"},"choices":choices},
        {"key":"pocket","target":{"scope":"pieces","ids":["pocket"]},"choices":choices}]},
        "finite_stock":serde_json::from_str::<Value>(STOCK).unwrap(),
        "cutting":{"copies":1,"gap_mm":2.0,"edge_margin_mm":2.0,"end_margin_mm":2.0}});
    let typed: Request = serde_json::from_value(input.clone()).unwrap();
    let expected = serde_json::to_value(typed.evaluate(&Pattern::new(source)).unwrap()).unwrap();
    let raw = input.to_string();
    let report_text = materials::study(PATTERN, &raw).unwrap();
    let result: Value = serde_json::from_str(&report_text).unwrap();
    assert_eq!(result, expected);
    assert_eq!(report_text, materials::study(PATTERN, &raw).unwrap());
    let rows = result["alternatives"].as_array().unwrap();
    assert_eq!(rows.len(), 4);
    assert!(
        rows.iter()
            .all(|row| row["choices"].as_object().unwrap().len() == 2)
    );
    assert!(
        rows.iter()
            .any(|row| row["finite_stock"]["status"] == "available")
    );
    assert!(
        rows.iter()
            .any(|row| row["cutting"]["status"] == "available")
    );
    assert!(rows.iter().any(|row| row["cutting"]["status"] == "refused"));
    let mut excessive = input;
    excessive["combinations"]["groups"][0]["choices"] = Value::Array(
        (0..17)
            .map(|i| json!({"key":format!("choice-{i}"),"selection":{"id":"missing"}}))
            .collect(),
    );
    let refusal = materials::study(PATTERN, &excessive.to_string()).unwrap_err();
    assert_eq!(refusal.code, ftx_core::RefusalCode::BudgetExceeded);
}

#[test]
fn generated_interface_choices_reach_the_real_self_contact_material_authority() {
    let mut source: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-sewing-pattern.json"
    ))
    .unwrap();
    let mechanics: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-scenario-pattern.json"
    ))
    .unwrap();
    let a = mechanics["fabric"]["material"].clone();
    let mut b = a.clone();
    b["id"] = json!("synthetic-second-sheet");
    b["record"]["id"] = json!("synthetic-second-sheet");
    b["record"]["properties"]["areal_density"]["value"] = json!(0.4);
    source["fabric"]["material"] = a.clone();
    let catalog = ftx_pattern::textile_import::catalog().unwrap();
    let hash = |raw: &Value| {
        catalog
            .resolve(&serde_json::from_value(raw.clone()).unwrap())
            .unwrap()
            .sha256()
            .to_owned()
    };
    let choices = json!([{"key":"a","selection":a},{"key":"b","selection":b}]);
    let pair = json!({"materials":[hash(&a),hash(&b)],
        "friction":{"value":0.18,"unit":"1","evidence":"estimate","source":"test",
            "method":"Synthetic pair friction for software integration"},
        "sources":{"test":{"reference":"test:combination-pair","accessed_on":"2026-10-20",
            "note":"Synthetic values only"}},"allow_estimates":true});
    let mut input = json!({"schema":"ftx-material-study/1","combinations":{"groups":[
        {"key":"front","target":{"scope":"pieces","ids":["front"]},"choices":choices},
        {"key":"back","target":{"scope":"pieces","ids":["back"]},"choices":choices}],
        "interfaces":[pair]},"cutting":{"copies":1,"gap_mm":1.0,"edge_margin_mm":0.0,"end_margin_mm":0.0}});
    let options = ftx_drape::run::DrapeOptions {
        self_contact: Some(ftx_drape::run::DrapeOptions::default().contact),
        ..ftx_drape::run::DrapeOptions::default()
    };
    for supplied in [true, false] {
        if !supplied {
            input["combinations"]["interfaces"] = json!([]);
        }
        let result: Value = serde_json::from_str(
            &materials::study(&source.to_string(), &input.to_string()).unwrap(),
        )
        .unwrap();
        for row in result["alternatives"].as_array().unwrap() {
            let mixed = row["choices"]["front"] != row["choices"]["back"];
            let bound = Pattern::new(row["binding"]["result"]["pattern"].clone());
            let resolved = ftx_drape::material::resolve(&bound, &options);
            if mixed && !supplied {
                assert!(
                    resolved
                        .err()
                        .expect("missing pair evidence must refuse")
                        .message
                        .contains("pairwise friction")
                );
            } else {
                let report = resolved.unwrap().report;
                if mixed {
                    assert_eq!(report["self_friction_matrix"][0][1], 0.18);
                    assert_eq!(report["self_friction_matrix"][1][0], 0.18);
                }
                assert_eq!(
                    report["interfaces"].as_array().unwrap().len(),
                    usize::from(mixed)
                );
            }
        }
    }
}

#[test]
fn file_free_stock_study_preserves_production_plans_and_independent_source_refusals() {
    let source: Value = serde_json::from_str(PATTERN).unwrap();
    let hide = source["fabric"]["material"].clone();
    let mut roll = hide.clone();
    roll["record"]["form"] = json!("woven");
    roll["record"]["properties"]["width"] = json!({"value":0.1,"unit":"m",
        "evidence":"estimate","source":"test","method":"Synthetic roll width"});
    let input = json!({"schema":"ftx-material-study/1","variants":[
        {"key":"hide","changes":{"schema":"ftx-material-bindings/1","default":hide}},
        {"key":"roll","changes":{"schema":"ftx-material-bindings/1","default":roll}}],
        "finite_stock":serde_json::from_str::<Value>(STOCK).unwrap(),
        "cutting":{"copies":1,"gap_mm":2.0,"edge_margin_mm":2.0,"end_margin_mm":2.0},
        "comparison":{"basis":"nominal","required_consumers":["finite_stock"],
            "objectives":[{"metric":"used_stock_mass_kg","direction":"minimize"}]}});
    let typed: Request = serde_json::from_value(input.clone()).unwrap();
    let native = typed.evaluate(&Pattern::new(source)).unwrap();
    let text = materials::study(PATTERN, &input.to_string()).unwrap();
    let result: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(result, serde_json::to_value(native).unwrap());
    assert_eq!(text, materials::study(PATTERN, &input.to_string()).unwrap());
    let rows = result["alternatives"].as_array().unwrap();
    let stock_pattern = Pattern::from_json(
        rows[0]["binding"]["result"]["pattern_json"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let inventory: stock::Request = serde_json::from_str(STOCK).unwrap();
    assert_eq!(
        rows[0]["finite_stock"]["result"],
        serde_json::to_value(inventory.build(&stock_pattern).unwrap()).unwrap()
    );
    assert_eq!(rows[0]["cutting"]["status"], "refused");
    assert_eq!(rows[1]["finite_stock"]["status"], "refused");
    assert_eq!(rows[1]["cutting"]["status"], "available");
    assert_eq!(result["comparison"]["frontier"], json!(["hide"]));
    assert_eq!(rows[0]["metrics"]["used_stock_count"], 1);
    assert!(
        (rows[0]["metrics"]["finite_stock_external_cut_area_m2"]
            .as_f64()
            .unwrap()
            - 0.0007)
            .abs()
            < 1e-14
    );

    let mut replay = input;
    replay["finite_stock"] = rows[0]["finite_stock"]["result"]["request"].clone();
    for (variant, row) in replay["variants"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .zip(rows)
    {
        variant["changes"] = row["frozen_changes"].clone();
    }
    let again: Value =
        serde_json::from_str(&materials::study(PATTERN, &replay.to_string()).unwrap()).unwrap();
    assert_eq!(again["alternatives"], result["alternatives"]);
    // Editing an inventory snapshot does not authorize substituting its locked material.
    replay["finite_stock"]["stocks"][0]["material"]["record"]["name"] = json!("Changed lot");
    let refused: Value =
        serde_json::from_str(&materials::study(PATTERN, &replay.to_string()).unwrap()).unwrap();
    assert!(
        refused["alternatives"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["finite_stock"]["status"] == "refused")
    );
    assert_eq!(
        refused["alternatives"][1]["cutting"],
        result["alternatives"][1]["cutting"]
    );
    assert_eq!(refused["comparison"]["frontier"], json!([]));
}

//! Both native study commands reach independent real roll and finite-inventory consumers.
use ftx_export::{
    cutting::{CutPlan, stock},
    material_study::Request,
};
use ftx_pattern::Pattern;
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

const PATTERN: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-stock-pattern.json");
const STOCK: &str = include_str!("../../../tests/fixtures/materials/synthetic-stock-request.json");

fn request() -> Value {
    let pattern: Value = serde_json::from_str(PATTERN).unwrap();
    let hide = pattern["fabric"]["material"].clone();
    let mut roll = hide.clone();
    roll["record"]["form"] = json!("woven");
    roll["record"]["properties"]["width"] = json!({"value":0.1,"unit":"m",
        "evidence":"estimate","source":"test","method":"Synthetic roll width"});
    json!({"schema":"ftx-material-study/1","variants":[
        {"key":"hide","changes":{"schema":"ftx-material-bindings/1","default":hide}},
        {"key":"roll","changes":{"schema":"ftx-material-bindings/1","default":roll}}],
        "finite_stock":serde_json::from_str::<Value>(STOCK).unwrap(),
        "cutting":{"copies":1,"gap_mm":2.0,"edge_margin_mm":2.0,"end_margin_mm":2.0},
        "comparison":{"basis":"nominal","required_consumers":["finite_stock"],
            "objectives":[{"metric":"used_stock_mass_kg","direction":"minimize"}]}})
}

fn run(main: bool, dir: &Path) -> Value {
    let mut command = Command::new(if main {
        env!("CARGO_BIN_EXE_ftx")
    } else {
        env!("CARGO_BIN_EXE_ftx-materials")
    });
    command.current_dir(dir);
    if main {
        command.args(["--json", "materials"]);
    }
    let out = command
        .args(["study", "pattern.json", "study.json"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{} / {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    report.get("result").unwrap_or(&report).clone()
}

#[test]
fn thread_cost_selection_matches_portable_and_both_native_commands() {
    let pattern = include_str!("../../../tests/fixtures/materials/synthetic-sewing-pattern.json");
    let selection = include_str!("../../../examples/sewing-thread-cost-selection.json");
    let selected: Value =
        serde_json::from_str(&ftx_wasm::sew_select(pattern, selection).unwrap()).unwrap();
    assert_eq!(selected["report"]["selected_thread_order_cost"], 10.0);
    assert_eq!(selected["report"]["objective_currency"], "USD");
    assert_eq!(selected["report"]["optimality_proven"], true);
    let source: Value = serde_json::from_str(pattern).unwrap();
    let mut study = json!({"schema":"ftx-material-study/1",
        "variants":[{"key":"original","changes":{"schema":"ftx-material-bindings/1",
            "default":source["fabric"]["material"]}}],
        "sewing_selection":serde_json::from_str::<Value>(selection).unwrap(),
        "comparison":{"basis":"nominal","required_consumers":["sewing_purchasing"],
            "objectives":[{"metric":"thread_observed_order_cost","direction":"minimize"}]}});
    let dir = std::env::temp_dir().join(format!(
        "ftx-thread-cost-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("pattern.json"), pattern).unwrap();
    fs::write(dir.join("selection.json"), selection).unwrap();
    for (binary, prefix) in [
        (
            env!("CARGO_BIN_EXE_ftx"),
            &["--json", "materials", "sew-select"][..],
        ),
        (env!("CARGO_BIN_EXE_ftx-materials"), &["sew-select"][..]),
    ] {
        let output = Command::new(binary)
            .current_dir(&dir)
            .args(prefix)
            .args(["pattern.json", "selection.json"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{} / {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let actual: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(actual.get("result").unwrap_or(&actual), &selected);
    }
    for unpriced in [false, true] {
        if unpriced {
            study["sewing_selection"]["options"]["consumables"]["purchasing"]["quote_book"]["quotes"] =
                json!([]);
        }
        let text = study.to_string();
        fs::write(dir.join("study.json"), &text).unwrap();
        let portable: Value =
            serde_json::from_str(&ftx_wasm::materials::study(pattern, &text).unwrap()).unwrap();
        for main in [false, true] {
            assert_eq!(run(main, &dir), portable);
        }
        let row = &portable["alternatives"][0];
        let search = &row["sewing_selection"]["result"];
        assert_eq!(search["objective_currency"], "USD");
        assert_eq!(search["no_feasible_listed_recipe"], false);
        assert_eq!(search["optimality_proven"], !unpriced);
        assert_eq!(search["unresolved_trials"], if unpriced { 2 } else { 0 });
        if unpriced {
            assert!(row["metrics"]["thread_observed_order_cost"].is_null());
            assert_eq!(row["sewing"]["status"], "refused");
            assert_eq!(portable["comparison"]["frontier"], json!([]));
        } else {
            assert_eq!(row["metrics"]["thread_observed_order_cost"], 10.0);
            assert_eq!(search["selected_thread_order_cost"], 10.0);
            assert_eq!(portable["comparison"]["frontier"], json!(["original"]));
        }
        if let Some(export) = std::env::var_os("FTX_THREAD_COST_REPLAY") {
            let export = std::path::PathBuf::from(export);
            fs::create_dir_all(&export).unwrap();
            fs::write(export.join("pattern.json"), pattern).unwrap();
            fs::write(export.join("selection.json"), selection).unwrap();
            fs::write(export.join("selection-report.json"), selected.to_string()).unwrap();
            let name = if unpriced { "unpriced" } else { "priced" };
            fs::write(export.join(format!("{name}-study.json")), text).unwrap();
            fs::write(
                export.join(format!("{name}-report.json")),
                portable.to_string(),
            )
            .unwrap();
        }
    }
}

#[test]
fn production_thread_orders_follow_cutting_copies_in_native_and_portable_consumers() {
    let mut source: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-sewing-pattern.json"
    ))
    .unwrap();
    source["fabric"]["material"]["record"]["properties"]["width"] = json!({
        "value":0.1,"unit":"m","evidence":"estimate","source":"test",
        "method":"Synthetic stock width for production-copy integration"});
    let material = ftx_pattern::materials::Catalog {
        schema: ftx_pattern::materials::CATALOG_SCHEMA.into(),
        materials: Vec::new(),
    }
    .resolve(&serde_json::from_value(source["fabric"]["material"].clone()).unwrap())
    .unwrap()
    .frozen_selection();
    source["fabric"]["material"] = json!(material);
    let mut selection: Value = serde_json::from_str(include_str!(
        "../../../examples/sewing-thread-cost-selection.json"
    ))
    .unwrap();
    for qualification in selection["options"]["qualifications"]
        .as_array_mut()
        .unwrap()
    {
        qualification["materials"] = json!([material.expected_sha256, material.expected_sha256]);
    }
    selection["options"]["consumables"]["purchasing"]["quote_book"]["quotes"][0]["available_quantity"] =
        json!(16.0);
    let study = json!({"schema":"ftx-material-study/1",
        "variants":[{"key":"batch","changes":{"schema":"ftx-material-bindings/1","default":material}}],
        "cutting":{"copies":2,"gap_mm":2.0,"edge_margin_mm":0.0,"end_margin_mm":0.0},
        "sewing_selection":selection,
        "comparison":{"basis":"nominal","required_consumers":["cutting","sewing_purchasing"],
            "objectives":[{"metric":"thread_observed_order_cost","direction":"minimize"}]}});
    let pattern = source.to_string();
    let study_text = study.to_string();
    let portable: Value =
        serde_json::from_str(&ftx_wasm::materials::study(&pattern, &study_text).unwrap()).unwrap();
    let row = &portable["alternatives"][0];
    assert_eq!(row["cutting"]["status"], "available");
    assert_eq!(
        row["sewing"]["result"]["consumables"]["production_copies"],
        2
    );
    assert_eq!(row["metrics"]["thread_observed_order_cost"], 14.0);
    assert_eq!(row["sewing_selection"]["result"]["optimality_proven"], true);
    assert_eq!(portable["comparison"]["frontier"], json!(["batch"]));
    let selected = &row["sewing_selection"]["result"]["request"];
    let standalone: Value =
        serde_json::from_str(&ftx_wasm::sew_select(&pattern, &selected.to_string()).unwrap())
            .unwrap();
    assert_eq!(
        standalone["report"]["selected_plan"],
        row["sewing"]["result"]
    );
    let dir = std::env::temp_dir().join(format!(
        "ftx-production-thread-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("pattern.json"), pattern).unwrap();
    fs::write(dir.join("study.json"), study_text).unwrap();
    fs::write(dir.join("selection.json"), selected.to_string()).unwrap();
    for main in [false, true] {
        assert_eq!(run(main, &dir), portable);
        let mut command = Command::new(if main {
            env!("CARGO_BIN_EXE_ftx")
        } else {
            env!("CARGO_BIN_EXE_ftx-materials")
        });
        command.current_dir(&dir);
        if main {
            command.args(["--json", "materials"]);
        }
        let output = command
            .args(["sew-select", "pattern.json", "selection.json"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let actual: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(actual.get("result").unwrap_or(&actual), &standalone);
    }
}

#[test]
fn sourced_yarn_mass_and_kilogram_purchasing_replay_in_native_and_portable_studies() {
    let pattern = include_str!("../../../tests/fixtures/materials/synthetic-sewing-pattern.json");
    let source: Value = serde_json::from_str(pattern).unwrap();
    let yarn = ftx_pattern::materials::Catalog::builtin()
        .unwrap()
        .resolve(&ftx_pattern::material_catalog::Selection {
            id: "coats/epic-2994120-2026".into(),
            ..Default::default()
        })
        .unwrap()
        .frozen_selection();
    let yarn_hash = yarn.expected_sha256.as_ref().unwrap();
    let mut selection: Value = serde_json::from_str(include_str!(
        "../../../examples/sewing-thread-cost-selection.json"
    ))
    .unwrap();
    for qualification in selection["options"]["qualifications"]
        .as_array_mut()
        .unwrap()
    {
        for channel in qualification["thread_consumption"]["channels"]
            .as_array_mut()
            .unwrap()
        {
            channel["material"] = json!(yarn);
        }
    }
    selection["options"]["consumables"]["production_copies"] = json!(3);
    let purchasing = &mut selection["options"]["consumables"]["purchasing"];
    purchasing["market"]["require_observed_continuous_length"] = json!(false);
    let offer = &mut purchasing["quote_book"]["quotes"][0];
    offer["offer_id"] = json!("synthetic-kg-epic24");
    offer["material_id"] = json!(yarn.id);
    offer["material_sha256"] = json!(yarn_hash);
    offer["sku"] = json!("synthetic-kg-epic24");
    offer["unit"] = json!("kilogram");
    offer["quantity"] = json!(1.);
    offer["price"] = json!(100.);
    offer["minimum_quantity"] = json!(0.0001);
    offer["order_increment"] = json!(0.0001);
    offer["available_quantity"] = json!(0.0003);
    offer["price_breaks"] = json!([{"minimum_quantity":0.0002,"price":80.}]);
    offer
        .as_object_mut()
        .unwrap()
        .remove("maximum_continuous_quantity");
    offer["source"] = json!("test:synthetic-kg-yarn-quote-not-a-Coats-price");
    let study = json!({"schema":"ftx-material-study/1",
        "variants":[{"key":"three-copy-yarn","changes":{"schema":"ftx-material-bindings/1",
            "default":source["fabric"]["material"]}}],
        "sewing_selection":selection,
        "comparison":{"basis":"nominal","required_consumers":["sewing_consumables","sewing_purchasing"],
            "objectives":[{"metric":"thread_required_mass_kg","direction":"minimize"},
                {"metric":"thread_observed_order_cost","direction":"minimize"}]}});
    let text = study.to_string();
    let portable: Value =
        serde_json::from_str(&ftx_wasm::materials::study(pattern, &text).unwrap()).unwrap();
    let row = &portable["alternatives"][0];
    assert_eq!(row["sewing"]["status"], "available");
    let consumables = &row["sewing"]["result"]["consumables"];
    assert_eq!(consumables["production_copies"], 3);
    assert_eq!(consumables["complete_consumption_coverage"], true);
    assert_eq!(consumables["complete_mass_coverage"], true);
    // Baseline: 40 traversals * (.01 + .02) m plus .5m allowances = 1.7m/copy.
    // Three copies need 5.1 m; published 24 tex is 24e-6 kg/m, not package/cone rounding.
    let expected_mass = 5.1 * 24e-6;
    assert!((consumables["total_required_length_m"].as_f64().unwrap() - 5.1).abs() < 1e-12);
    assert!(
        (consumables["total_required_mass_kg"].as_f64().unwrap() - expected_mass).abs() < 1e-18
    );
    assert!(
        (row["metrics"]["thread_required_mass_kg"].as_f64().unwrap() - expected_mass).abs() < 1e-18
    );
    let original =
        &consumables["materials"][yarn_hash]["record"]["properties"]["yarn_linear_density"];
    assert_eq!(original["unit"], "tex");
    assert_eq!(original["value"], 24.);
    let purchasing = &consumables["purchasing"];
    assert_eq!(purchasing["complete_offer_coverage"], true);
    assert_eq!(purchasing["complete_observed_quantity_coverage"], true);
    assert_eq!(
        purchasing["complete_continuous_length_limit_coverage"],
        false
    );
    let order = &purchasing["material_orders"][0]["least_observed_order"];
    assert_eq!(order["quote"]["unit"], "kilogram");
    assert_eq!(order["quote"]["available_quantity"], 0.0003);
    assert_eq!(order["order_quantity"], 0.0002);
    assert!((order["acquired"].as_f64().unwrap() - 0.0002 / 24e-6).abs() < 1e-12);
    assert!((order["order_cost"].as_f64().unwrap() - 0.016).abs() < 1e-14);
    assert!(
        (row["metrics"]["thread_observed_order_cost"]
            .as_f64()
            .unwrap()
            - 0.016)
            .abs()
            < 1e-14
    );
    assert_eq!(row["sewing_selection"]["result"]["optimality_proven"], true);
    assert_eq!(
        portable["comparison"]["frontier"],
        json!(["three-copy-yarn"])
    );
    let readings = &portable["comparison"]["alternatives"][0]["readings"];
    assert_eq!(readings["thread_required_mass_kg"]["unit"], "kg");
    assert_eq!(
        readings["thread_required_mass_kg"]["value"],
        row["metrics"]["thread_required_mass_kg"]
    );
    assert_eq!(readings["thread_observed_order_cost"]["unit"], "USD");
    let dir = std::env::temp_dir().join(format!(
        "ftx-yarn-mass-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("pattern.json"), pattern).unwrap();
    fs::write(dir.join("study.json"), text).unwrap();
    for main in [false, true] {
        assert_eq!(run(main, &dir), portable);
    }
}

#[test]
fn both_native_commands_expand_design_material_pairs_and_preserve_exact_replay() {
    let pattern = include_str!("../../../tests/fixtures/materials/synthetic-sewing-pattern.json");
    let input = include_str!("../../../examples/design-study.json");
    let typed: Request = serde_json::from_str(input).unwrap();
    let expected = serde_json::to_value(
        typed
            .evaluate(&Pattern::from_json(pattern).unwrap())
            .unwrap(),
    )
    .unwrap();
    let dir = std::env::temp_dir().join(format!(
        "ftx-design-study-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("pattern.json"), pattern).unwrap();
    fs::write(dir.join("study.json"), input).unwrap();
    assert_eq!(expected["schema"], "ftx-design-study-report/1");
    assert_eq!(expected["alternatives"].as_array().unwrap().len(), 8);
    for main in [false, true] {
        let report = run(main, &dir);
        assert_eq!(report, expected);
        for row in report["alternatives"].as_array().unwrap() {
            let design = &row["design"];
            if design["key"] == "unbalanced-seam" {
                assert_eq!(row["binding"]["status"], "refused");
                assert_eq!(design["validation"]["passed"], false);
                assert!(design["altered_pattern_json"].is_string());
                continue;
            }
            let changes: ftx_pattern::material_binding::Changes =
                serde_json::from_value(row["frozen_changes"].clone()).unwrap();
            let altered =
                Pattern::from_json(design["altered_pattern_json"].as_str().unwrap()).unwrap();
            let rebound = changes.apply(&altered).unwrap();
            assert_eq!(
                rebound["pattern_json"],
                row["binding"]["result"]["pattern_json"]
            );
            assert_eq!(row["cutting"]["status"], "available");
            assert_eq!(row["sewing"]["status"], "available");
        }
    }
    assert_eq!(
        fs::read_to_string(dir.join("pattern.json")).unwrap(),
        pattern
    );
    assert_eq!(fs::read_to_string(dir.join("study.json")).unwrap(), input);
}

#[test]
fn both_native_study_commands_preserve_split_purchase_incumbents_and_replay_orders() {
    let pattern = include_str!("../../../examples/mixed-cutting/pattern.json");
    let mut input: Value = serde_json::from_str(include_str!(
        "../../../examples/mixed-cutting/split-orders-study.json"
    ))
    .unwrap();
    let dir = std::env::temp_dir().join(format!(
        "ftx-split-study-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("pattern.json"), pattern).unwrap();
    for budget in [1, 10000] {
        input["cutting_job"]["purchasing"]["market"]["split_orders"]["max_search_nodes"] =
            json!(budget);
        let text = input.to_string();
        fs::write(dir.join("study.json"), &text).unwrap();
        let typed: Request = serde_json::from_value(input.clone()).unwrap();
        let expected = serde_json::to_value(
            typed
                .evaluate(&Pattern::from_json(pattern).unwrap())
                .unwrap(),
        )
        .unwrap();
        let job = &expected["alternatives"][0]["cutting_job"]["result"];
        fs::write(
            dir.join("job-pattern.json"),
            job["pattern_json"].as_str().unwrap(),
        )
        .unwrap();
        fs::write(
            dir.join("job-request.json"),
            job["replay_request_json"].as_str().unwrap(),
        )
        .unwrap();
        assert_eq!(
            expected["comparison"]["frontier"],
            json!(["mixed-shell-and-lining"])
        );
        let price = &job["purchasing"];
        assert_eq!(price["schema"], "ftx-cutting-job-procurement/1");
        assert_eq!(price["complete_offer_coverage"], true);
        assert_eq!(price["order_allocation_optimality_proven"], budget > 1);
        assert_eq!(
            price["least_observed_independent_order_total"].is_null(),
            budget == 1
        );
        let expected_cost = if budget == 1 { 8.0 } else { 7.09 };
        assert!(
            (price["selected_observed_order_total"].as_f64().unwrap() - expected_cost).abs()
                < 1e-12
        );
        for main in [false, true] {
            assert_eq!(run(main, &dir), expected);
            let mut command = Command::new(if main {
                env!("CARGO_BIN_EXE_ftx")
            } else {
                env!("CARGO_BIN_EXE_ftx-materials")
            });
            command.current_dir(&dir);
            if main {
                command.args(["--json", "materials"]);
            }
            let output = command
                .args(["cut-job", "job-pattern.json", "job-request.json"])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let replay: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(replay.get("result").unwrap_or(&replay), job);
        }
        assert_eq!(fs::read_to_string(dir.join("study.json")).unwrap(), text);
    }
    input["cutting_job"]["purchasing"]["finite_bindings"] = json!([]);
    fs::write(dir.join("study.json"), input.to_string()).unwrap();
    for main in [false, true] {
        let incomplete = run(main, &dir);
        assert_eq!(incomplete["comparison"]["frontier"], json!([]));
        let price = &incomplete["alternatives"][0]["cutting_job"]["result"]["purchasing"];
        assert_eq!(price["complete_offer_coverage"], false);
        assert!(price["selected_observed_order_total"].is_null());
        assert_eq!(price["roll"]["result"]["complete_offer_coverage"], true);
    }
    assert_eq!(
        fs::read_to_string(dir.join("pattern.json")).unwrap(),
        pattern
    );
}

#[test]
fn both_study_commands_and_cut_job_replay_allocate_the_complete_mixed_inventory() {
    let pattern = include_str!("../../../examples/mixed-cutting/pattern.json");
    let input = include_str!("../../../examples/mixed-cutting/study.json");
    let request: Request = serde_json::from_str(input).unwrap();
    let expected = serde_json::to_value(
        request
            .evaluate(&Pattern::from_json(pattern).unwrap())
            .unwrap(),
    )
    .unwrap();
    let dir = std::env::temp_dir().join(format!(
        "ftx-mixed-cutting-consumer-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("pattern.json"), pattern).unwrap();
    fs::write(dir.join("study.json"), input).unwrap();
    let job = &expected["alternatives"][0]["cutting_job"]["result"];
    assert_eq!(job["complete_inventory"], true);
    assert_eq!(job["inventory"].as_array().unwrap().len(), 6);
    assert_eq!(job["roll"]["source_piece_ids"], json!(["lining"]));
    assert_eq!(job["finite_stock"]["source_piece_ids"], json!(["shell"]));
    let replay_pattern = job["pattern_json"].as_str().unwrap();
    let replay_request = job["replay_request_json"].as_str().unwrap();
    fs::write(dir.join("job-pattern.json"), replay_pattern).unwrap();
    fs::write(dir.join("job-request.json"), replay_request).unwrap();
    for main in [false, true] {
        assert_eq!(run(main, &dir), expected);
        let mut command = Command::new(if main {
            env!("CARGO_BIN_EXE_ftx")
        } else {
            env!("CARGO_BIN_EXE_ftx-materials")
        });
        command.current_dir(&dir);
        if main {
            command.args(["--json", "materials"]);
        }
        let output = command
            .args(["cut-job", "job-pattern.json", "job-request.json"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let replayed: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(replayed.get("result").unwrap_or(&replayed), job);
    }
    assert_eq!(
        fs::read_to_string(dir.join("pattern.json")).unwrap(),
        pattern
    );
    assert_eq!(fs::read_to_string(dir.join("study.json")).unwrap(), input);
    assert_eq!(
        fs::read_to_string(dir.join("job-pattern.json")).unwrap(),
        replay_pattern
    );
    assert_eq!(
        fs::read_to_string(dir.join("job-request.json")).unwrap(),
        replay_request
    );
}

#[test]
fn both_study_commands_follow_exact_material_routes_and_replay_each_selected_job() {
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
    let expected = serde_json::to_value(
        serde_json::from_value::<Request>(request)
            .unwrap()
            .evaluate(&Pattern::from_json(pattern).unwrap())
            .unwrap(),
    )
    .unwrap();
    let dir = std::env::temp_dir().join(format!(
        "ftx-material-routes-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("pattern.json"), pattern).unwrap();
    fs::write(dir.join("study.json"), &input).unwrap();
    for main in [false, true] {
        assert_eq!(run(main, &dir), expected);
        for row in expected["alternatives"].as_array().unwrap() {
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
            let frozen: Value =
                serde_json::from_str(job["pattern_json"].as_str().unwrap()).unwrap();
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
            let basename = row["key"].as_str().unwrap();
            let pattern_path = dir.join(format!("{basename}-pattern.json"));
            let request_path = dir.join(format!("{basename}-job.json"));
            if !main {
                fs::write(&pattern_path, job["pattern_json"].as_str().unwrap()).unwrap();
                fs::write(&request_path, job["replay_request_json"].as_str().unwrap()).unwrap();
            }
            let mut command = Command::new(if main {
                env!("CARGO_BIN_EXE_ftx")
            } else {
                env!("CARGO_BIN_EXE_ftx-materials")
            });
            if main {
                command.args(["--json", "materials"]);
            }
            let output = command
                .arg("cut-job")
                .arg(pattern_path)
                .arg(request_path)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let replay: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(replay.get("result").unwrap_or(&replay), job);
        }
    }
    assert_eq!(
        fs::read_to_string(dir.join("pattern.json")).unwrap(),
        pattern
    );
    assert_eq!(fs::read_to_string(dir.join("study.json")).unwrap(), input);
}

#[test]
fn both_study_commands_recompute_measured_vapor_transport_from_piece_substitutions() {
    let pattern = include_str!("../../../tests/fixtures/materials/synthetic-sewing-pattern.json");
    let input = include_str!("../../../examples/vapor-study/leather.json");
    let request: Request = serde_json::from_str(input).unwrap();
    let expected = serde_json::to_value(
        request
            .evaluate(&Pattern::from_json(pattern).unwrap())
            .unwrap(),
    )
    .unwrap();
    let dir = std::env::temp_dir().join(format!(
        "ftx-vapor-study-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("pattern.json"), pattern).unwrap();
    fs::write(dir.join("study.json"), input).unwrap();
    for main in [false, true] {
        let report = run(main, &dir);
        assert_eq!(report, expected);
        let rows = report["alternatives"].as_array().unwrap();
        assert_eq!(rows[0]["vapor"]["status"], "available");
        assert_eq!(rows[1]["vapor"]["status"], "available");
        assert_eq!(rows[2]["vapor"]["status"], "refused");
        assert_eq!(rows[2]["binding"]["status"], "available");
        assert_eq!(report["comparison"]["frontier"], json!(["chrome-neck"]));
        assert_eq!(
            rows[0]["vapor"]["result"]["layers"][0]["observation"]["evidence"],
            "measured"
        );
        assert_eq!(report["geometry_changed"], false);
        assert_eq!(report["physical_validation_claim"], false);
    }
    assert_eq!(
        fs::read_to_string(dir.join("pattern.json")).unwrap(),
        pattern
    );
    assert_eq!(fs::read_to_string(dir.join("study.json")).unwrap(), input);
}

#[test]
fn both_study_binaries_select_qualified_recipes_per_material_without_stale_reuse() {
    let pattern = include_str!("../../../tests/fixtures/materials/synthetic-sewing-pattern.json");
    let source: Value = serde_json::from_str(pattern).unwrap();
    let original = source["fabric"]["material"].clone();
    let mut changed = original.clone();
    changed["record"]["name"] = json!("Different lot with the same nominal thickness");
    let input = json!({"schema":"ftx-material-study/1",
        "variants":[
            {"key":"original","changes":{"schema":"ftx-material-bindings/1","default":original}},
            {"key":"changed","changes":{"schema":"ftx-material-bindings/1","default":changed}}],
        "sewing_selection":serde_json::from_str::<Value>(include_str!(
            "../../../tests/fixtures/materials/synthetic-sewing-selection.json")).unwrap(),
        "comparison":{"basis":"nominal","required_consumers":["sewing"],
            "objectives":[{"metric":"feed_seconds","direction":"minimize"}]}});
    let typed: Request = serde_json::from_value(input.clone()).unwrap();
    let expected = serde_json::to_value(typed.evaluate(&Pattern::new(source)).unwrap()).unwrap();
    let dir = std::env::temp_dir().join(format!(
        "ftx-study-recipes-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("pattern.json"), pattern).unwrap();
    fs::write(dir.join("study.json"), input.to_string()).unwrap();
    for main in [false, true] {
        let report = run(main, &dir);
        assert_eq!(report, expected);
        let changed = &report["alternatives"][0];
        let original = &report["alternatives"][1];
        assert_eq!(changed["sewing"]["status"], "refused");
        assert_eq!(
            changed["sewing_selection"]["result"]["no_feasible_listed_recipe"],
            true
        );
        assert!(changed["metrics"]["feed_seconds"].is_null());
        assert_eq!(original["sewing"]["status"], "available");
        assert_eq!(
            original["sewing_selection"]["result"]["selected_choices"]["side"],
            "faster-qualified"
        );
        assert_eq!(original["metrics"]["feed_seconds"], 2.0);
        assert_eq!(report["comparison"]["frontier"], json!(["original"]));
    }
    assert_eq!(
        fs::read_to_string(dir.join("pattern.json")).unwrap(),
        pattern
    );
}

#[test]
fn both_study_commands_expand_mixed_material_choices_through_actual_planners() {
    let mut input = request();
    let choices: Vec<Value> = input["variants"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| json!({"key":v["key"],"selection":v["changes"]["default"]}))
        .collect();
    input.as_object_mut().unwrap().remove("variants");
    input["combinations"] = json!({"groups":[
        {"key":"main","target":{"scope":"default"},"choices":choices},
        {"key":"pocket","target":{"scope":"pieces","ids":["pocket"]},"choices":choices}
    ]});
    let typed: Request = serde_json::from_value(input.clone()).unwrap();
    let expected = serde_json::to_value(
        typed
            .evaluate(&Pattern::from_json(PATTERN).unwrap())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(expected["alternatives"].as_array().unwrap().len(), 4);
    assert!(
        expected["alternatives"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| {
                row["choices"].as_object().unwrap().len() == 2
                    && row["binding"]["status"] == "available"
            })
    );
    let dir = std::env::temp_dir().join(format!(
        "ftx-combination-study-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("pattern.json"), PATTERN).unwrap();
    let original = input.to_string();
    fs::write(dir.join("study.json"), &original).unwrap();
    for main in [false, true] {
        assert_eq!(run(main, &dir), expected);
    }
    assert_eq!(
        fs::read_to_string(dir.join("pattern.json")).unwrap(),
        PATTERN
    );
    assert_eq!(
        fs::read_to_string(dir.join("study.json")).unwrap(),
        original
    );
}

#[test]
fn both_study_binaries_compare_real_stock_without_losing_independent_roll_results() {
    let dir = std::env::temp_dir().join(format!(
        "ftx-stock-study-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("pattern.json"), PATTERN).unwrap();
    let input = request();
    fs::write(dir.join("study.json"), input.to_string()).unwrap();
    let typed: Request = serde_json::from_value(input.clone()).unwrap();
    let expected = serde_json::to_value(
        typed
            .evaluate(&Pattern::from_json(PATTERN).unwrap())
            .unwrap(),
    )
    .unwrap();
    let hide = &expected["alternatives"][0];
    let roll = &expected["alternatives"][1];
    let hide_pattern =
        Pattern::from_json(hide["binding"]["result"]["pattern_json"].as_str().unwrap()).unwrap();
    let roll_pattern =
        Pattern::from_json(roll["binding"]["result"]["pattern_json"].as_str().unwrap()).unwrap();
    let inventory: stock::Request = serde_json::from_str(STOCK).unwrap();
    assert_eq!(hide["finite_stock"]["status"], "available", "{hide}");
    assert_eq!(
        hide["finite_stock"]["result"],
        serde_json::to_value(inventory.build(&hide_pattern).unwrap()).unwrap()
    );
    assert_eq!(hide["cutting"]["status"], "refused");
    assert_eq!(roll["finite_stock"]["status"], "refused");
    assert_eq!(roll["cutting"]["status"], "available", "{roll}");
    assert_eq!(
        roll["cutting"]["result"],
        serde_json::to_value(
            CutPlan::build(&roll_pattern, typed.cutting.as_ref().unwrap()).unwrap()
        )
        .unwrap()
    );
    assert_eq!(expected["comparison"]["frontier"], json!(["hide"]));
    assert_eq!(expected["comparison"]["alternatives"][1]["eligible"], false);
    assert_eq!(
        hide["metrics"]["used_stock_mass_kg"],
        hide["finite_stock"]["result"]["markers"][0]["stock_mass_kg"]
    );
    assert_eq!(expected["hardware_execution_authorized"], false);
    for main in [false, true] {
        assert_eq!(run(main, &dir), expected);
    }
    // Replay source locks while retaining the same physical inventory, without consuming it.
    let mut replay = input;
    replay["finite_stock"] = hide["finite_stock"]["result"]["request"].clone();
    for (variant, row) in replay["variants"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .zip(expected["alternatives"].as_array().unwrap())
    {
        variant["changes"] = row["frozen_changes"].clone();
    }
    fs::write(dir.join("study.json"), replay.to_string()).unwrap();
    for main in [false, true] {
        let actual = run(main, &dir);
        assert_eq!(actual["alternatives"], expected["alternatives"]);
        assert_eq!(actual["comparison"]["frontier"], json!(["hide"]));
    }
    assert_eq!(
        fs::read_to_string(dir.join("pattern.json")).unwrap(),
        PATTERN
    );
}

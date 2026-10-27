//! Real shared sewing plans feed material choices; synthetic coefficients are not measurements.

use super::{close, fixture, purchasing, quote};
use ftx_core::{Refusal, RefusalCode};
use ftx_export::material_study::{Request, matrix::Case};
use ftx_pattern::{
    Pattern,
    material_catalog::Selection,
    materials::{CATALOG_SCHEMA, Catalog},
};
use serde_json::{Value, json};

fn frozen(selection: &Value) -> Selection {
    Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: Vec::new(),
    }
    .resolve(&serde_json::from_value(selection.clone()).unwrap())
    .unwrap()
    .frozen_selection()
}

fn variant(key: &str, material: &Value) -> Value {
    json!({"key":key,"changes":{"schema":"ftx-material-bindings/1","default":material}})
}

fn study() -> (Value, Value) {
    let (mut pattern, mut options, thread) = fixture();
    for (name, value, unit) in [("width", 0.1, "m"), ("areal_density", 0.2, "kg/m2")] {
        pattern["fabric"]["material"]["record"]["properties"][name] = json!({
            "value":value,"unit":unit,"evidence":"estimate","source":"test",
            "method":"Synthetic manufacturing property, not a measurement"});
    }
    let cloth = frozen(&pattern["fabric"]["material"]);
    options["qualifications"][0]["materials"] =
        json!([cloth.expected_sha256, cloth.expected_sha256]);
    let mut thread_quote = quote(&thread, "thread-cone", "thread-a");
    thread_quote["currency"] = json!("EUR");
    purchasing(&mut options, vec![thread_quote]);
    options["consumables"]["purchasing"]["market"]["currency"] = json!("EUR");
    let mut cloth_market = options["consumables"]["purchasing"]["market"].clone();
    cloth_market["currency"] = json!("USD");
    let request = json!({"schema":"ftx-material-study/1",
        "variants":[variant("base", &json!(cloth))],
        "cutting":{"copies":1,"gap_mm":2.0,"edge_margin_mm":0.0,"end_margin_mm":0.0},
        "purchasing":{"quotes":{"schema":"ftx-textile-quotes/1","quotes":[quote(&cloth,"cloth-roll","cloth-a")]},"market":cloth_market},
        "sewing":options,
        "comparison":{"basis":"nominal","required_consumers":["sewing_consumables","sewing_purchasing"],
            "objectives":[{"metric":"thread_required_length_m","direction":"minimize"},
                {"metric":"thread_observed_order_cost","direction":"minimize"}]}
    });
    (pattern, request)
}

fn run(pattern: &Value, request: &Value) -> Value {
    serde_json::to_value(
        serde_json::from_value::<Request>(request.clone())
            .unwrap()
            .evaluate(&Pattern::new(pattern.clone()))
            .unwrap(),
    )
    .unwrap()
}

fn row<'a>(report: &'a Value, key: &str) -> &'a Value {
    report["alternatives"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["key"] == key)
        .unwrap()
}

fn finite_inventory(material: &Value, copies: usize) -> Value {
    json!({"schema":"ftx-finite-stock-request/1","stocks":[{
        "id":"synthetic-remnant","material":material,
        "outline_mm":[[0,0],[200,0],[200,300],[0,300]],"grain_mm":[[0,0],[0,10]],
        "geometry_source":{"reference":"test:production-remnant","accessed_on":"2026-10-26",
            "note":"Synthetic stock geometry; no measured inventory"}}],
        "options":{"copies":copies,"gap_mm":2.0,"boundary_margin_mm":2.0,
            "step_mm":2.0,"work_limit":2000000}})
}

#[test]
fn every_cutting_route_drives_complete_thread_orders_before_fixed_or_searched_recipe_evaluation() {
    for route in ["roll", "finite", "mixed"] {
        for search in [false, true] {
            let (pattern, mut request) = study();
            if search {
                use_recipe_search(&mut request);
                request["sewing_selection"]["schema"] = json!("ftx-sewing-selection/3");
                request["sewing_selection"]["objective"] =
                    json!("minimize_thread_observed_order_cost");
            }
            let options = if search {
                &mut request["sewing_selection"]["options"]
            } else {
                &mut request["sewing"]
            };
            let offer = &mut options["consumables"]["purchasing"]["quote_book"]["quotes"][0];
            offer["available_quantity"] = json!(16.0);
            offer["price_breaks"] = json!([
                {"minimum_quantity":4.0,"price":7.0},
                {"minimum_quantity":8.0,"price":4.0}
            ]);
            request["cutting"]["copies"] = json!(2);
            let finite = finite_inventory(&request["variants"][0]["changes"]["default"], 2);
            let consumer = match route {
                "roll" => "cutting",
                "finite" => {
                    request.as_object_mut().unwrap().remove("cutting");
                    request.as_object_mut().unwrap().remove("purchasing");
                    request["finite_stock"] = finite;
                    "finite_stock"
                }
                _ => {
                    let roll = request.as_object_mut().unwrap().remove("cutting").unwrap();
                    request.as_object_mut().unwrap().remove("purchasing");
                    request["cutting_job"] = json!({"schema":"ftx-cutting-job/1",
                        "routes":[{"piece":"front","stock":"roll"},{"piece":"back","stock":"finite_stock"}],
                        "roll":roll,"finite_stock":finite});
                    "cutting_job"
                }
            };
            let result = run(&pattern, &request);
            let actual = row(&result, "base");
            assert_eq!(
                actual[consumer]["status"], "available",
                "{route}/{search}: {}",
                actual[consumer]
            );
            assert_eq!(
                actual["sewing"]["status"], "available",
                "{route}/{search}: {}",
                actual["sewing"]
            );
            let plan = &actual["sewing"]["result"];
            assert_eq!(plan["consumables"]["production_copies"], 2);
            assert_eq!(plan["options"]["consumables"]["production_copies"], 2);
            assert_eq!(plan["stitch_intervals"], 90);
            close(plan["feed_seconds"].as_f64().unwrap(), 9.0);
            close(
                actual["metrics"]["thread_required_length_m"]
                    .as_f64()
                    .unwrap(),
                7.4,
            );
            close(
                actual["metrics"]["thread_observed_order_cost"]
                    .as_f64()
                    .unwrap(),
                16.0,
            );
            assert_eq!(result["comparison"]["frontier"], json!(["base"]));
            let replay = super::build(
                actual["binding"]["result"]["pattern"].clone(),
                plan["options"].clone(),
            )
            .unwrap();
            assert_eq!(plan, &serde_json::to_value(replay).unwrap());
            if search {
                assert_eq!(
                    actual["sewing_selection"]["result"]["request"]["options"]["consumables"]["production_copies"],
                    2
                );
                assert_eq!(
                    actual["sewing_selection"]["result"]["optimality_proven"],
                    true
                );
            }

            // An explicitly contradictory count refuses the sewing consumer, preserving real stock planning.
            let options = if search {
                &mut request["sewing_selection"]["options"]
            } else {
                &mut request["sewing"]
            };
            options["consumables"]["production_copies"] = json!(1);
            let mismatch = run(&pattern, &request);
            let refused = row(&mismatch, "base");
            assert_eq!(refused[consumer], actual[consumer]);
            assert_eq!(refused["sewing"]["status"], "refused");
            assert!(
                refused["sewing"]["refusal"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("production_copies")
            );
            assert!(refused["metrics"]["thread_required_length_m"].is_null());
            assert_eq!(mismatch["comparison"]["frontier"], json!([]));
            if search {
                assert_eq!(refused["sewing_selection"]["status"], "refused");
            }
        }
    }
}

#[test]
fn conflicting_cut_route_counts_cannot_choose_an_arbitrary_thread_batch_size() {
    let (pattern, mut request) = study();
    request["finite_stock"] = finite_inventory(&request["variants"][0]["changes"]["default"], 2);
    let result = run(&pattern, &request);
    let actual = row(&result, "base");
    assert_eq!(actual["cutting"]["status"], "available");
    assert_eq!(actual["finite_stock"]["status"], "available");
    assert_eq!(actual["sewing"]["status"], "refused");
    assert!(
        actual["sewing"]["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("same valid")
    );

    let roll = request.as_object_mut().unwrap().remove("cutting").unwrap();
    let finite = request
        .as_object_mut()
        .unwrap()
        .remove("finite_stock")
        .unwrap();
    request.as_object_mut().unwrap().remove("purchasing");
    request["cutting_job"] = json!({"schema":"ftx-cutting-job/1",
        "routes":[{"piece":"front","stock":"roll"},{"piece":"back","stock":"finite_stock"}],
        "roll":roll,"finite_stock":finite});
    let result = run(&pattern, &request);
    assert_eq!(row(&result, "base")["cutting_job"]["status"], "refused");
    assert_eq!(row(&result, "base")["sewing"]["status"], "refused");
}

#[test]
fn quantities_and_purchase_metrics_retain_complete_plans_and_their_own_currency() {
    let (pattern, mut request) = study();
    request["comparison"]["objectives"]
        .as_array_mut()
        .unwrap()
        .push(json!({"metric":"observed_order_cost","direction":"minimize"}));
    let result = run(&pattern, &request);
    let actual = row(&result, "base");
    let direct = super::build(
        actual["binding"]["result"]["pattern"].clone(),
        request["sewing"].clone(),
    )
    .unwrap();
    assert_eq!(
        actual["sewing"]["result"],
        serde_json::to_value(&direct).unwrap()
    );
    close(
        actual["metrics"]["thread_required_length_m"]
            .as_f64()
            .unwrap(),
        3.7,
    );
    close(
        actual["metrics"]["thread_observed_order_cost"]
            .as_f64()
            .unwrap(),
        14.0,
    );
    let readings = &row(&result["comparison"], "base")["readings"];
    assert_eq!(readings["thread_required_length_m"]["unit"], "m");
    assert_eq!(
        readings["thread_required_length_m"]["consumer"],
        "sewing_consumables"
    );
    assert_eq!(readings["thread_observed_order_cost"]["unit"], "EUR");
    assert_eq!(
        readings["thread_observed_order_cost"]["consumer"],
        "sewing_purchasing"
    );
    assert_eq!(readings["observed_order_cost"]["unit"], "USD");
    assert_eq!(
        readings["thread_observed_order_cost"]["value"],
        actual["metrics"]["thread_observed_order_cost"]
    );
    assert_eq!(result["comparison"]["frontier"], json!(["base"]));
    assert!(
        readings["thread_required_length_m"]
            .get("reported_bounds")
            .is_none()
    );

    request["comparison"]["constraints"] =
        json!([{"metric":"thread_required_length_m","maximum":3.6}]);
    let constrained = run(&pattern, &request);
    assert_eq!(constrained["comparison"]["frontier"], json!([]));
    assert_eq!(constrained["alternatives"], result["alternatives"]);
}

#[test]
fn absent_thread_consumption_or_offers_cannot_supply_a_partial_comparison_metric() {
    let (pattern, original) = study();
    let baseline = run(&pattern, &original);
    for mutation in 0..3 {
        let mut request = original.clone();
        match mutation {
            0 => {
                request["sewing"]["qualifications"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("thread_consumption");
            }
            1 => {
                request["sewing"]["qualifications"][0]["thread_consumption"]["channels_complete"] =
                    json!(false)
            }
            _ => {
                request["sewing"]["consumables"]["purchasing"]["quote_book"]["quotes"][0]["available"] =
                    json!(false)
            }
        }
        let result = run(&pattern, &request);
        let actual = row(&result, "base");
        assert_eq!(actual["cutting"], row(&baseline, "base")["cutting"]);
        assert_eq!(actual["purchasing"], row(&baseline, "base")["purchasing"]);
        assert_eq!(actual["sewing"]["status"], "available");
        assert!(actual["metrics"]["thread_observed_order_cost"].is_null());
        assert_eq!(result["comparison"]["frontier"], json!([]));
        assert!(
            row(&result["comparison"], "base")["readings"]["thread_observed_order_cost"].is_null()
        );
        if mutation < 2 {
            assert!(actual["metrics"]["thread_required_length_m"].is_null());
            assert!(
                actual["sewing"]["result"]["consumables"]["purchasing"]["material_orders"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
        } else {
            close(
                actual["metrics"]["thread_required_length_m"]
                    .as_f64()
                    .unwrap(),
                3.7,
            );
            request["comparison"] = json!({"basis":"nominal","required_consumers":["sewing_consumables"],
                "objectives":[{"metric":"thread_required_length_m","direction":"minimize"}]});
            assert_eq!(
                run(&pattern, &request)["comparison"]["frontier"],
                json!(["base"])
            );
        }
    }
}

#[test]
fn material_substitution_loses_unmatched_consumption_without_erasing_cutting() {
    let (pattern, mut request) = study();
    let mut changed = pattern["fabric"]["material"].clone();
    changed["id"] = json!("another-sheet");
    changed["record"]["id"] = json!("another-sheet");
    let changed = frozen(&changed);
    request["variants"]
        .as_array_mut()
        .unwrap()
        .push(variant("unqualified", &json!(changed)));
    request["purchasing"]["quotes"]["quotes"]
        .as_array_mut()
        .unwrap()
        .push(quote(&changed, "another-cloth-roll", "cloth-b"));
    let result = run(&pattern, &request);
    let unknown = row(&result, "unqualified");
    assert_eq!(unknown["cutting"]["status"], "available");
    assert_eq!(
        unknown["purchasing"]["result"]["complete_offer_coverage"],
        true
    );
    assert_eq!(unknown["sewing"]["status"], "available");
    assert_eq!(
        unknown["metrics"]["stock_area_m2"],
        row(&result, "base")["metrics"]["stock_area_m2"]
    );
    assert!(unknown["metrics"]["thread_required_length_m"].is_null());
    assert!(unknown["metrics"]["thread_observed_order_cost"].is_null());
    assert_eq!(
        unknown["sewing"]["result"]["consumables"]["missing_operations"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(result["comparison"]["frontier"], json!(["base"]));
}

fn use_recipe_search(request: &mut Value) {
    let mut options = request.as_object_mut().unwrap().remove("sewing").unwrap();
    options["require_qualification"] = json!(true);
    let mut fast = options["qualifications"][0].clone();
    fast["id"] = json!("fast-qualified-recipe");
    fast["needle"] = json!("fast-needle");
    fast["thread"] = json!("fast-thread");
    fast["thread_consumption"]["channels"][0]["metres_per_traversal"] = json!(0.04);
    fast["thread_consumption"]["channels"][1]["metres_per_traversal"] = json!(0.08);
    options["qualifications"].as_array_mut().unwrap().push(fast);
    options["consumables"]["purchasing"]["quote_book"]["quotes"][0]["available_quantity"] =
        json!(8.0);
    options["consumables"]["purchasing"]["quote_book"]["quotes"][0]["maximum_continuous_quantity"] =
        json!(3.0);
    let alternatives = ["join", "reinforce"].map(|operation| {
        json!({"operation":operation,
        "recipes":[{"id":"faster","needle":"fast-needle","thread":"fast-thread",
            "max_stitch_mm":5.0,"stitches_per_minute":1000.0}]})
    });
    request["sewing_selection"] = json!({"schema":"ftx-sewing-selection/1","objective":"minimize_feed_seconds",
        "options":options,"max_plan_evaluations":4,
        "alternatives":alternatives});
}

#[test]
fn thread_study_metrics_follow_the_actual_selected_recipe_and_ignore_unused_source_models() {
    let (pattern, mut request) = study();
    use_recipe_search(&mut request);
    let result = run(&pattern, &request);
    let actual = row(&result, "base");
    let typed: Request = serde_json::from_value(request.clone()).unwrap();
    let direct = typed
        .sewing_selection
        .unwrap()
        .select(&Pattern::new(
            actual["binding"]["result"]["pattern"].clone(),
        ))
        .unwrap();
    assert_eq!(
        actual["sewing"]["result"],
        serde_json::to_value(direct.selected_plan).unwrap()
    );
    assert_eq!(
        actual["sewing_selection"]["result"]["selected_choices"],
        json!({"join":"faster","reinforce":"faster"})
    );
    close(
        actual["metrics"]["thread_required_length_m"]
            .as_f64()
            .unwrap(),
        7.0,
    );
    close(
        actual["metrics"]["thread_observed_order_cost"]
            .as_f64()
            .unwrap(),
        28.0,
    );
    let protocol = row(&result["comparison"], "base")["input_protocol_sha256"].clone();
    assert!(protocol.is_string());
    request["sewing_selection"]["options"]["qualifications"][0]["method"] =
        json!("Unused process method");
    request["sewing_selection"]["options"]["qualifications"][0]["thread_consumption"]["method"] =
        json!("Unused consumption method");
    let changed = run(&pattern, &request);
    assert_eq!(
        row(&changed["comparison"], "base")["input_protocol_sha256"],
        protocol
    );
    assert_eq!(row(&changed, "base")["metrics"], actual["metrics"]);
}

#[test]
fn thread_quantity_and_cost_protocols_require_the_actual_independent_consumption_and_process_methods()
 {
    let (pattern, mut original) = study();
    let mut other = pattern["fabric"]["material"].clone();
    other["id"] = json!("other-qualified-sheet");
    other["record"]["id"] = json!("other-qualified-sheet");
    let other = frozen(&other);
    original["variants"]
        .as_array_mut()
        .unwrap()
        .push(variant("different", &json!(other)));
    let mut qualification = original["sewing"]["qualifications"][0].clone();
    qualification["id"] = json!("other-stack-qualification");
    qualification["materials"] = json!([other.expected_sha256, other.expected_sha256]);
    qualification["thread_consumption"]["channels"][0]["metres_per_traversal"] = json!(0.02);
    qualification["thread_consumption"]["channels"][1]["metres_per_traversal"] = json!(0.04);
    original["sewing"]["qualifications"]
        .as_array_mut()
        .unwrap()
        .push(qualification);
    original["sewing"]["consumables"]["purchasing"]["quote_book"]["quotes"][0]["available_quantity"] =
        json!(8.0);
    original["sewing"]["consumables"]["purchasing"]["quote_book"]["quotes"][0]["maximum_continuous_quantity"] =
        json!(3.0);
    assert_eq!(
        run(&pattern, &original)["comparison"]["frontier"],
        json!(["base"])
    );
    for mutation in 0..7 {
        let mut request = original.clone();
        let q = &mut request["sewing"]["qualifications"][1];
        match mutation {
            0 => q["thread_consumption"]["method"] = json!("Different consumption measurement"),
            1 => {
                q["thread_consumption"]["conditions"]["scope"] =
                    json!("Different acquisition conditioning")
            }
            2 => q["thread_consumption"]["evidence"] = json!("measured"),
            3 => q["thread_consumption"]["channels"][0]["id"] = json!("another-source-channel"),
            4 => q["method"] = json!("Different process qualification method"),
            5 => q["conditions"] = json!({"conditioning":"Different process acquisition"}),
            _ => q["evidence"] = json!("measured"),
        }
        let result = run(&pattern, &request);
        assert_eq!(
            result["comparison"]["frontier"],
            json!(["base", "different"]),
            "mutation {mutation}"
        );
        assert_eq!(
            result["comparison"]["incomparable"]
                .as_array()
                .unwrap()
                .len(),
            1,
            "mutation {mutation}"
        );
        request["comparison"]["allow_mixed_protocols"] = json!(true);
        assert_eq!(
            run(&pattern, &request)["comparison"]["frontier"],
            json!(["base"]),
            "mutation {mutation}"
        );
    }
}

#[test]
fn no_thread_quantity_or_price_interval_is_invented_from_nominal_consumption() {
    let (pattern, original) = study();
    for metric in ["thread_required_length_m", "thread_observed_order_cost"] {
        let mut request = original.clone();
        request["comparison"] = json!({"basis":"reported_bounds","objectives":[{"metric":metric,"direction":"minimize"}]});
        let refusal = serde_json::from_value::<Request>(request)
            .unwrap()
            .evaluate(&Pattern::new(pattern.clone()))
            .unwrap_err();
        assert_eq!(refusal.code, RefusalCode::InvalidDocument);
        assert!(refusal.message.contains("reported_bounds"));
    }
}

#[test]
fn thread_mass_comparison_uses_resolved_density_overrides_and_preserves_missing_mass() {
    let database = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: Vec::new(),
    };
    let locked = |mut selection: Selection| {
        selection.expected_sha256 = None;
        selection.expected_sha256 =
            Some(database.resolve(&selection).unwrap().sha256().to_string());
        selection
    };
    for embedded_density in [false, true] {
        let (pattern, mut request) = study();
        request["comparison"] = json!({"basis":"nominal",
            "objectives":[{"metric":"thread_required_mass_kg","direction":"minimize"}]});
        request["sewing"]["consumables"] = json!({});
        let mut other = pattern["fabric"]["material"].clone();
        other["id"] = json!("other-qualified-sheet");
        other["record"]["id"] = json!("other-qualified-sheet");
        let other = frozen(&other);
        request["variants"]
            .as_array_mut()
            .unwrap()
            .push(variant("different", &json!(other)));
        let mut qualification = request["sewing"]["qualifications"][0].clone();
        qualification["id"] = json!("other-stack-qualification");
        qualification["materials"] = json!([other.expected_sha256, other.expected_sha256]);
        request["sewing"]["qualifications"]
            .as_array_mut()
            .unwrap()
            .push(qualification);

        let mut source = super::yarn("same-authored-thread");
        source.allow_estimates = true;
        let quantity = |value, state| {
            serde_json::from_value(json!({
                "value":value,"unit":"tex","evidence":"estimate","source":"fixture",
                "method":"Synthetic density protocol; not a textile measurement",
                "conditions":{"conditioning":state}
            }))
            .unwrap()
        };
        if embedded_density {
            source.record.as_mut().unwrap().properties.insert(
                "yarn_linear_density".into(),
                quantity(10.0, "original unused state"),
            );
        }
        for (index, value, state) in [
            (0, 20.0, "dry source state"),
            (1, 40.0, "other source state"),
        ] {
            let mut selection = source.clone();
            selection
                .overrides
                .insert("yarn_linear_density".into(), quantity(value, state));
            let selection = locked(selection);
            for channel in
                request["sewing"]["qualifications"][index]["thread_consumption"]["channels"]
                    .as_array_mut()
                    .unwrap()
            {
                channel["material"] = json!(selection);
            }
        }
        let result = run(&pattern, &request);
        for key in ["base", "different"] {
            assert_eq!(
                row(&result, key)["sewing"]["status"],
                "available",
                "{}",
                row(&result, key)["sewing"]
            );
        }
        close(
            row(&result, "base")["metrics"]["thread_required_mass_kg"]
                .as_f64()
                .unwrap(),
            3.7 * 20e-6,
        );
        close(
            row(&result, "different")["metrics"]["thread_required_mass_kg"]
                .as_f64()
                .unwrap(),
            3.7 * 40e-6,
        );
        assert_ne!(
            row(&result["comparison"], "base")["input_protocol_sha256"],
            row(&result["comparison"], "different")["input_protocol_sha256"]
        );
        assert_eq!(
            result["comparison"]["frontier"],
            json!(["base", "different"])
        );
        assert_eq!(
            result["comparison"]["incomparable"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        request["comparison"]["allow_mixed_protocols"] = json!(true);
        assert_eq!(
            run(&pattern, &request)["comparison"]["frontier"],
            json!(["base"])
        );

        source.record.as_mut().unwrap().properties.clear();
        let missing = locked(source);
        for channel in request["sewing"]["qualifications"][1]["thread_consumption"]["channels"]
            .as_array_mut()
            .unwrap()
        {
            channel["material"] = json!(missing);
        }
        let result = run(&pattern, &request);
        let missing = row(&result, "different");
        assert_eq!(missing["sewing"]["status"], "available");
        close(
            missing["metrics"]["thread_required_length_m"]
                .as_f64()
                .unwrap(),
            3.7,
        );
        assert!(missing["metrics"]["thread_required_mass_kg"].is_null());
        assert_eq!(result["comparison"]["frontier"], json!(["base"]));
        assert_eq!(row(&result["comparison"], "different")["eligible"], false);
    }
}

#[test]
fn all_expanded_material_design_and_recipe_rows_reserve_thread_purchasing_before_callbacks() {
    let (pattern, original) = study();
    for mode in ["materials", "designs", "recipes"] {
        let mut request = original.clone();
        if mode == "recipes" {
            use_recipe_search(&mut request);
        }
        let options_key = if mode == "recipes" {
            "sewing_selection"
        } else {
            "sewing"
        };
        let options = if mode == "recipes" {
            &mut request[options_key]["options"]
        } else {
            &mut request[options_key]
        };
        let quote = options["consumables"]["purchasing"]["quote_book"]["quotes"][0].clone();
        options["consumables"]["purchasing"]["quote_book"]["quotes"] =
            json!(vec![quote; if mode == "recipes" { 1000 } else { 4096 }]);
        let variants = if mode == "designs" { 2 } else { 17 };
        let material = request["variants"][0]["changes"]["default"].clone();
        request["variants"] = json!(
            (0..variants)
                .map(|i| variant(&format!("variant-{i:02}"), &material))
                .collect::<Vec<_>>()
        );
        if mode == "designs" {
            request["schema"] = json!("ftx-design-study/1");
            request["designs"] = json!(
                (0..8)
                    .map(|i| json!({"key":format!("design-{i}"),"operations":[
                {"op":"slash_spread","piece":"front","axis":"x","at_mm":5.0,"add_mm":1.0}]}))
                    .collect::<Vec<_>>()
            );
        }
        let typed: Request = serde_json::from_value(request).unwrap();
        let mut callbacks = 0;
        let refusal = typed
            .evaluate_with(
                &Pattern::new(pattern.clone()),
                &json!({"synthetic":"unused callback"}),
                |_, _| {
                    callbacks += 1;
                    Ok::<Value, Refusal>(json!({}))
                },
            )
            .unwrap_err();
        assert_eq!(callbacks, 0, "{mode}");
        assert_eq!(refusal.code, RefusalCode::BudgetExceeded, "{mode}");
        assert!(
            refusal.message.contains("thread-purchasing"),
            "{mode}: {}",
            refusal.message
        );
    }
}

#[test]
fn scenario_cases_reuse_one_thread_manufacturing_result_and_keep_failed_physics_independent() {
    let (pattern, request) = study();
    let typed: Request = serde_json::from_value(request).unwrap();
    let cases: Vec<Case> = serde_json::from_value(json!([
        {"key":"case-a","scenario":{"synthetic":"unused-a"}},
        {"key":"case-b","scenario":{"synthetic":"unused-b"}}
    ]))
    .unwrap();
    let mut callbacks = 0;
    let result = typed
        .evaluate_matrix_with(&Pattern::new(pattern), &cases, 2, |_, _, _| {
            callbacks += 1;
            Err::<Value, _>(Refusal::new(
                RefusalCode::NotConverged,
                "Independent synthetic scenario refusal",
            ))
        })
        .unwrap();
    assert_eq!(callbacks, 2);
    let report = serde_json::to_value(result).unwrap();
    close(
        row(&report["manufacturing"], "base")["metrics"]["thread_required_length_m"]
            .as_f64()
            .unwrap(),
        3.7,
    );
    close(
        row(&report["manufacturing"], "base")["metrics"]["thread_observed_order_cost"]
            .as_f64()
            .unwrap(),
        14.0,
    );
    assert_eq!(report["comparison"]["frontier"], json!(["base"]));
    for case in report["cases"].as_array().unwrap() {
        assert_eq!(case["comparison"]["frontier"], json!(["base"]));
        assert_eq!(
            row(&case["comparison"], "base")["readings"]["thread_observed_order_cost"]["unit"],
            "EUR"
        );
    }
}

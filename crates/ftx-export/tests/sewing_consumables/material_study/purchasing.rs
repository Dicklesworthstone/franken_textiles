//! A complete declared merchandise objective composes the actual independent consumer receipts.

use super::{close, finite_inventory, frozen, quote, row, run, study, variant};
use ftx_export::cutting::{job, procurement};
use ftx_pattern::Pattern;
use serde_json::{Value, json};

fn combined() -> (Value, Value) {
    let (pattern, mut request) = study();
    request.as_object_mut().unwrap().remove("comparison");
    request["cut_and_sew_purchasing"] = json!(true);
    let mut yarn =
        request["sewing"]["consumables"]["purchasing"]["quote_book"]["quotes"][0].clone();
    yarn["currency"] = json!("USD");
    yarn["available_quantity"] = json!(32.0);
    yarn["maximum_continuous_quantity"] = json!(8.0);
    request["purchasing"]["quotes"]["quotes"]
        .as_array_mut()
        .unwrap()
        .push(yarn);
    request["sewing"]["consumables"]["purchasing"]["market"] =
        request["purchasing"]["market"].clone();
    same_book(&mut request);
    (pattern, request)
}

fn same_book(request: &mut Value) {
    let book = if request["cutting_job"].is_object() {
        request["cutting_job"]["purchasing"]["quotes"].clone()
    } else {
        request["purchasing"]["quotes"].clone()
    };
    let options = if request["sewing_selection"].is_object() {
        &mut request["sewing_selection"]["options"]
    } else {
        &mut request["sewing"]
    };
    options["consumables"]["purchasing"]["quote_book"] = book;
}

fn receipt<'a>(report: &'a Value, key: &str) -> &'a Value {
    let actual = row(report, key);
    assert_eq!(
        actual["cut_and_sew_purchasing"]["status"], "available",
        "{actual}"
    );
    &actual["cut_and_sew_purchasing"]["result"]
}

#[test]
fn split_thread_purchases_feed_the_actual_complete_cut_and_sew_decision() {
    let (pattern, mut request) = combined();
    request["cutting"]["copies"] = json!(2);
    let first = &mut request["purchasing"]["quotes"]["quotes"][1];
    first["available_quantity"] = json!(4.0);
    let mut second = first.clone();
    second["offer_id"] = json!("thread-cone-two");
    second["sku"] = json!("independent-thread-stock");
    request["purchasing"]["quotes"]["quotes"]
        .as_array_mut()
        .unwrap()
        .push(second);
    same_book(&mut request);
    request["sewing"]["consumables"]["purchasing"]["market"]["split_orders"] =
        json!({"schema":"ftx-thread-order-allocation/1","max_search_nodes":10000});
    request["comparison"] = json!({"basis":"nominal","required_consumers":["sewing_purchasing","cut_and_sew_purchasing"],
        "objectives":[{"metric":"thread_observed_order_cost","direction":"minimize"},
            {"metric":"cut_and_sew_selected_order_cost","direction":"minimize"}]});
    let report = run(&pattern, &request);
    let combined = receipt(&report, "base");
    close(
        combined["thread_selected_order_cost"].as_f64().unwrap(),
        28.0,
    );
    close(combined["selected_order_cost"].as_f64().unwrap(), 38.0);
    assert_eq!(
        combined["thread_offer_ids"],
        json!(["thread-cone", "thread-cone-two"])
    );
    assert_eq!(combined["order_allocation_optimality_proven"], true);
    assert_eq!(report["comparison"]["frontier"], json!(["base"]));
    close(
        row(&report, "base")["metrics"]["thread_observed_order_cost"]
            .as_f64()
            .unwrap(),
        28.0,
    );
    let selected = &row(&report, "base")["sewing"]["result"];
    let replay = super::super::build(
        row(&report, "base")["binding"]["result"]["pattern"].clone(),
        selected["options"].clone(),
    )
    .unwrap();
    assert_eq!(selected, &serde_json::to_value(replay).unwrap());
}

#[test]
fn complete_orders_share_the_batch_and_keep_actual_minimum_increment_and_tier_rounding() {
    for (copies, thread_cost, total, multicut) in [
        (1, 14.0, 24.0, false),
        (2, 16.0, 26.0, false),
        (3, 24.0, 34.0, true),
    ] {
        let (pattern, mut request) = combined();
        request["cutting"]["copies"] = json!(copies);
        request["purchasing"]["quotes"]["quotes"][1]["price_breaks"] = json!([
            {"minimum_quantity":4.0,"price":7.0},{"minimum_quantity":8.0,"price":4.0}]);
        if multicut {
            request["cutting"]["maximum_marker_length_m"] = json!(0.13);
        }
        same_book(&mut request);
        let report = run(&pattern, &request);
        let complete = receipt(&report, "base");
        assert_eq!(complete["schema"], "ftx-cut-and-sew-purchasing/1");
        assert_eq!(complete["production_copies"], copies);
        assert_eq!(complete["fabric_consumer"], "cutting");
        assert_eq!(complete["market"]["currency"], "USD");
        assert_eq!(complete["order_allocation_optimality_proven"], true);
        assert_eq!(complete["complete_observed_quantity_coverage"], true);
        assert_eq!(complete["execution_authorized"], false);
        assert_eq!(complete["fabric_offer_ids"], json!(["cloth-roll"]));
        assert_eq!(complete["thread_offer_ids"], json!(["thread-cone"]));
        close(
            complete["fabric_selected_order_cost"].as_f64().unwrap(),
            10.0,
        );
        close(
            complete["thread_selected_order_cost"].as_f64().unwrap(),
            thread_cost,
        );
        close(complete["selected_order_cost"].as_f64().unwrap(), total);
        close(
            row(&report, "base")["metrics"]["cut_and_sew_selected_order_cost"]
                .as_f64()
                .unwrap(),
            total,
        );
        let mut legacy = request.clone();
        legacy
            .as_object_mut()
            .unwrap()
            .remove("cut_and_sew_purchasing");
        let legacy = run(&pattern, &legacy);
        let actual = row(&report, "base");
        let old = row(&legacy, "base");
        assert!(old.get("cut_and_sew_purchasing").is_none());
        assert!(
            old["metrics"]
                .get("cut_and_sew_selected_order_cost")
                .is_none()
        );
        for consumer in ["cutting", "purchasing", "sewing"] {
            assert_eq!(actual[consumer], old[consumer]);
        }
        if multicut {
            assert_eq!(
                actual["purchasing"]["result"]["schema"],
                "ftx-cut-procurement/2"
            );
        }
    }
}

#[test]
fn missing_refused_unpriced_and_incoherent_sides_never_publish_a_partial_total() {
    for mutation in 0..8 {
        let (pattern, mut request) = combined();
        let baseline = run(&pattern, &request);
        receipt(&baseline, "base");
        match mutation {
            0 => {
                request.as_object_mut().unwrap().remove("purchasing");
            }
            1 => {
                request["sewing"]["consumables"]
                    .as_object_mut()
                    .unwrap()
                    .remove("purchasing");
            }
            2 => {
                request["purchasing"]["quotes"]["quotes"][0]["available"] = json!(false);
                same_book(&mut request);
            }
            3 => {
                request["purchasing"]["quotes"]["quotes"][1]["available"] = json!(false);
                same_book(&mut request);
            }
            4 => {
                request["sewing"]["consumables"]["purchasing"]["market"]["max_age_days"] = json!(1)
            }
            5 => {
                request["sewing"]["consumables"]["purchasing"]["quote_book"]["quotes"][0]["source"] =
                    json!("test:different-immutable-history")
            }
            6 => request["sewing"]["consumables"]["production_copies"] = json!(2),
            _ => {
                request["sewing"]["qualifications"][0]["thread_consumption"]["channels_complete"] =
                    json!(false)
            }
        }
        let report = run(&pattern, &request);
        let actual = row(&report, "base");
        assert_eq!(
            actual["cut_and_sew_purchasing"]["status"], "refused",
            "mutation {mutation}: {actual}"
        );
        assert!(actual["metrics"]["cut_and_sew_selected_order_cost"].is_null());
        assert_eq!(actual["cutting"], row(&baseline, "base")["cutting"]);
        if matches!(mutation, 1 | 4..=7) {
            assert_eq!(actual["purchasing"], row(&baseline, "base")["purchasing"]);
        }
        if matches!(mutation, 4 | 5) {
            assert_eq!(actual["sewing"]["status"], "available");
            assert!(actual["metrics"]["thread_observed_order_cost"].is_number());
            assert!(actual["metrics"]["least_observed_independent_order_total"].is_number());
        }
    }
}

#[test]
fn cross_consumer_stock_aliases_follow_current_transitive_base_and_unused_tier_identity() {
    for transitive in [false, true] {
        let (pattern, mut request) = combined();
        let baseline = run(&pattern, &request);
        receipt(&baseline, "base");
        if transitive {
            let mut bridge = request["purchasing"]["quotes"]["quotes"][0].clone();
            bridge["offer_id"] = json!("unused-current-bridge");
            bridge["sku"] = json!("cloth-a");
            bridge["available"] = json!(false);
            bridge["price_breaks"] = json!([{"minimum_quantity":4.0,"price":7.0,"sku":"thread-a"}]);
            request["purchasing"]["quotes"]["quotes"]
                .as_array_mut()
                .unwrap()
                .push(bridge);
        } else {
            request["purchasing"]["quotes"]["quotes"][1]["sku"] = json!("cloth-a");
        }
        same_book(&mut request);
        let report = run(&pattern, &request);
        let actual = row(&report, "base");
        assert_eq!(
            actual["cut_and_sew_purchasing"]["status"], "refused",
            "{actual}"
        );
        assert!(
            actual["cut_and_sew_purchasing"]["refusal"]["message"]
                .as_str()
                .unwrap()
                .contains("stock alias")
        );
        assert!(actual["metrics"]["cut_and_sew_selected_order_cost"].is_null());
        assert!(actual["metrics"]["thread_observed_order_cost"].is_number());
        assert!(actual["metrics"]["least_observed_independent_order_total"].is_number());
        if transitive {
            // A later observation replaces that offer's old aliases. A future bridge is invisible.
            let quotes = request["purchasing"]["quotes"]["quotes"]
                .as_array_mut()
                .unwrap();
            quotes[2]["observed_on"] = json!("2026-10-22");
            let mut replacement = quotes[2].clone();
            replacement["observed_on"] = json!("2026-10-23");
            replacement["price_breaks"] = json!([]);
            quotes.push(replacement);
            let mut future = quotes[2].clone();
            future["offer_id"] = json!("future-bridge");
            future["observed_on"] = json!("2026-10-24");
            quotes.push(future);
            same_book(&mut request);
            let current = run(&pattern, &request);
            close(
                receipt(&current, "base")["selected_order_cost"]
                    .as_f64()
                    .unwrap(),
                24.0,
            );
        }
    }
}

#[test]
fn bounded_complete_fabric_allocation_remains_a_selected_total_without_a_minimum_claim() {
    let (pattern, mut request) = combined();
    let mut expensive = request["purchasing"]["quotes"]["quotes"][0].clone();
    expensive["offer_id"] = json!("other-roll");
    expensive["supplier"] = json!("another-synthetic-supplier");
    expensive["price"] = json!(20.0);
    request["purchasing"]["quotes"]["quotes"]
        .as_array_mut()
        .unwrap()
        .push(expensive);
    request["purchasing"]["market"]["split_orders"] =
        json!({"schema":"ftx-cut-order-allocation/1","max_search_nodes":1});
    same_book(&mut request);
    request["comparison"] = json!({"basis":"nominal","required_consumers":["cut_and_sew_purchasing"],
        "objectives":[{"metric":"cut_and_sew_selected_order_cost","direction":"minimize"}]});
    let report = run(&pattern, &request);
    let complete = receipt(&report, "base");
    assert_eq!(complete["fabric_order_allocation_optimality_proven"], false);
    assert_eq!(complete["order_allocation_optimality_proven"], false);
    assert_eq!(complete["market"].get("split_orders"), None);
    close(complete["selected_order_cost"].as_f64().unwrap(), 24.0);
    assert!(row(&report, "base")["metrics"]["least_observed_independent_order_total"].is_null());
    assert_eq!(report["comparison"]["frontier"], json!(["base"]));
    request["purchasing"]["market"]["split_orders"]["max_search_nodes"] = json!(10000);
    let report = run(&pattern, &request);
    assert_eq!(
        receipt(&report, "base")["order_allocation_optimality_proven"],
        true
    );
}

#[test]
fn complete_mixed_stock_job_includes_used_finite_items_rolls_and_selected_thread_orders() {
    let (pattern, mut request) = combined();
    let material = request["variants"][0]["changes"]["default"].clone();
    let roll = request.as_object_mut().unwrap().remove("cutting").unwrap();
    let input = request
        .as_object_mut()
        .unwrap()
        .remove("purchasing")
        .unwrap();
    request["cutting_job"] = json!({"schema":"ftx-cutting-job/1",
        "routes":[{"piece":"front","stock":"roll"},{"piece":"back","stock":"finite_stock"}],
        "roll":roll,"finite_stock":finite_inventory(&material, 1)});
    let bound = super::frozen(&pattern["fabric"]["material"]);
    let mut source = pattern.clone();
    source["fabric"]["material"] = json!(bound);
    let job: job::Request = serde_json::from_value(request["cutting_job"].clone()).unwrap();
    let unpriced = job.build(&Pattern::new(source)).unwrap();
    let identities =
        procurement::finite::identities(unpriced.finite_stock.as_ref().unwrap()).unwrap();
    let used = identities.iter().find(|identity| identity.used).unwrap();
    let mut finite = input["quotes"]["quotes"][0].clone();
    finite["offer_id"] = json!("finite-item");
    finite["sku"] = json!("finite-item-stock");
    finite["price"] = json!(5.0);
    finite["quantity"] = json!(used.gross_area_m2);
    finite["minimum_quantity"] = json!(used.gross_area_m2);
    finite["order_increment"] = json!(used.gross_area_m2);
    finite["available_quantity"] = json!(used.gross_area_m2);
    finite["unit"] = json!("square_meter");
    finite["price_breaks"] = json!([]);
    finite
        .as_object_mut()
        .unwrap()
        .remove("maximum_continuous_quantity");
    request["cutting_job"]["purchasing"] = input;
    request["cutting_job"]["purchasing"]["market"]["require_observed_continuous_length"] =
        json!(false);
    request["sewing"]["consumables"]["purchasing"]["market"]["require_observed_continuous_length"] =
        json!(false);
    request["cutting_job"]["purchasing"]["quotes"]["quotes"]
        .as_array_mut()
        .unwrap()
        .push(finite);
    request["cutting_job"]["purchasing"]["finite_bindings"] = json!([{
        "stock_id":used.stock_id,"stock_sha256":used.stock_sha256,"offer_ids":["finite-item"]}]);
    same_book(&mut request);
    let report = run(&pattern, &request);
    let complete = receipt(&report, "base");
    assert_eq!(complete["fabric_consumer"], "cutting_job");
    assert_eq!(
        complete["fabric_offer_ids"],
        json!(["cloth-roll", "finite-item"])
    );
    close(
        complete["fabric_selected_order_cost"].as_f64().unwrap(),
        15.0,
    );
    close(complete["selected_order_cost"].as_f64().unwrap(), 29.0);
    request["cutting_job"]["purchasing"]["quotes"]["quotes"][2]["available"] = json!(false);
    same_book(&mut request);
    let missing = run(&pattern, &request);
    assert_eq!(row(&missing, "base")["cutting_job"]["status"], "available");
    assert_eq!(
        row(&missing, "base")["cut_and_sew_purchasing"]["status"],
        "refused"
    );
    assert!(row(&missing, "base")["metrics"]["cut_and_sew_selected_order_cost"].is_null());
    assert_eq!(row(&missing, "base")["sewing"]["status"], "available");
}

#[test]
fn complete_cost_reverses_the_cheapest_fabric_ranking_through_actual_thread_consumption() {
    let (pattern, mut request) = combined();
    let mut other = pattern["fabric"]["material"].clone();
    other["id"] = json!("lower-thread-sheet");
    other["record"]["id"] = json!("lower-thread-sheet");
    let other = frozen(&other);
    request["variants"]
        .as_array_mut()
        .unwrap()
        .push(variant("lower-thread", &json!(other)));
    let mut qualification = request["sewing"]["qualifications"][0].clone();
    qualification["id"] = json!("lower-thread-qualified-stack");
    qualification["materials"] = json!([other.expected_sha256, other.expected_sha256]);
    qualification["thread_consumption"]["channels"][0]["metres_per_traversal"] = json!(0.001);
    qualification["thread_consumption"]["channels"][1]["metres_per_traversal"] = json!(0.002);
    request["sewing"]["qualifications"]
        .as_array_mut()
        .unwrap()
        .push(qualification);
    let mut fabric = quote(&other, "lower-thread-roll", "lower-thread-stock");
    fabric["price"] = json!(12.0);
    request["purchasing"]["quotes"]["quotes"]
        .as_array_mut()
        .unwrap()
        .push(fabric);
    same_book(&mut request);
    request["comparison"] = json!({"basis":"nominal","required_consumers":["cut_and_sew_purchasing"],
        "objectives":[{"metric":"cut_and_sew_selected_order_cost","direction":"minimize"}]});
    let report = run(&pattern, &request);
    close(
        receipt(&report, "base")["selected_order_cost"]
            .as_f64()
            .unwrap(),
        24.0,
    );
    close(
        receipt(&report, "lower-thread")["selected_order_cost"]
            .as_f64()
            .unwrap(),
        22.0,
    );
    assert_eq!(report["comparison"]["frontier"], json!(["lower-thread"]));
    request["comparison"]["objectives"][0]["metric"] = json!("selected_observed_order_cost");
    assert_eq!(
        run(&pattern, &request)["comparison"]["frontier"],
        json!(["base"])
    );
}

#[test]
fn common_history_alias_work_is_reserved_before_any_material_or_scenario_evaluation() {
    let (pattern, mut request) = combined();
    let offer = request["purchasing"]["quotes"]["quotes"][0].clone();
    request["purchasing"]["quotes"]["quotes"] = json!(vec![offer; 4096]);
    same_book(&mut request);
    let material = request["variants"][0]["changes"]["default"].clone();
    request["variants"] = json!(
        (0..8)
            .map(|i| variant(&format!("case-{i}"), &material))
            .collect::<Vec<_>>()
    );
    let typed: ftx_export::material_study::Request = serde_json::from_value(request).unwrap();
    assert!(
        ftx_export::sewing::consumables::purchasing_work_bound(typed.sewing.as_ref().unwrap())
            .unwrap()
            * 8
            <= 200000
    );
    let mut callbacks = 0;
    let refusal = typed
        .evaluate_with(&Pattern::new(pattern), &json!({}), |_, _| {
            callbacks += 1;
            Ok::<Value, ftx_core::Refusal>(json!({}))
        })
        .unwrap_err();
    assert_eq!(callbacks, 0);
    assert_eq!(refusal.code, ftx_core::RefusalCode::BudgetExceeded);
    assert!(refusal.message.contains("thread-purchasing/composite"));
}

fn deadline_study() -> (Value, Value) {
    let (pattern, mut request) = combined();
    super::use_recipe_search(&mut request);
    let selection = &mut request["sewing_selection"];
    selection["schema"] = json!("ftx-sewing-selection/3");
    selection["objective"] = json!("minimize_thread_observed_order_cost");
    selection["completion_deadline_milliseconds"] = json!(7000);
    crate::selection::completion_timing(&mut selection["options"]);
    same_book(&mut request);
    (pattern, request)
}

#[test]
fn complete_cut_and_sew_orders_follow_the_deadline_feasible_recipe_and_keep_independent_results() {
    let (pattern, mut request) = deadline_study();
    let report = run(&pattern, &request);
    let selected = row(&report, "base");
    assert_eq!(selected["sewing"]["status"], "available");
    assert_eq!(
        selected["sewing_selection"]["result"]["selected_choices"],
        json!({"join":"faster","reinforce":"template"})
    );
    assert_eq!(
        selected["sewing_selection"]["result"]["selected_thread_order_cost"],
        21.0
    );
    assert_eq!(
        selected["sewing_selection"]["result"]["optimality_proven"],
        true
    );
    let complete = receipt(&report, "base");
    close(
        complete["fabric_selected_order_cost"].as_f64().unwrap(),
        10.0,
    );
    close(
        complete["thread_selected_order_cost"].as_f64().unwrap(),
        21.0,
    );
    close(complete["selected_order_cost"].as_f64().unwrap(), 31.0);
    close(
        selected["metrics"]["cut_and_sew_selected_order_cost"]
            .as_f64()
            .unwrap(),
        31.0,
    );
    let typed: ftx_export::material_study::Request =
        serde_json::from_value(request.clone()).unwrap();
    let direct = typed
        .sewing_selection
        .unwrap()
        .select(&Pattern::new(
            selected["binding"]["result"]["pattern"].clone(),
        ))
        .unwrap();
    assert_eq!(
        selected["sewing"]["result"],
        serde_json::to_value(direct.selected_plan).unwrap()
    );
    assert_eq!(
        selected["sewing_selection"]["result"]["trials"],
        serde_json::to_value(direct.trials).unwrap()
    );

    request["sewing_selection"]["completion_deadline_milliseconds"] = json!(3999);
    let missed = run(&pattern, &request);
    let refused = row(&missed, "base");
    assert_eq!(refused["sewing"]["status"], "refused");
    assert_eq!(
        refused["sewing_selection"]["result"]["no_feasible_listed_recipe"],
        true
    );
    assert_eq!(refused["cut_and_sew_purchasing"]["status"], "refused");
    assert!(refused["metrics"]["cut_and_sew_selected_order_cost"].is_null());
    assert!(selected["metrics"]["stock_mass_kg"].as_f64().unwrap() > 0.0);
    assert_eq!(
        refused["metrics"]["stock_mass_kg"],
        selected["metrics"]["stock_mass_kg"]
    );
    assert_eq!(refused["cutting"], selected["cutting"]);
    assert_eq!(refused["purchasing"], selected["purchasing"]);
}

#[test]
fn deadline_and_candidate_domain_belong_to_sewing_and_thread_comparison_protocols() {
    for (consumer, metric) in [
        ("sewing", "feed_seconds"),
        ("sewing_purchasing", "thread_observed_order_cost"),
    ] {
        let (pattern, mut request) = deadline_study();
        request["comparison"] = json!({"basis":"nominal","required_consumers":[consumer],
            "objectives":[{"metric":metric,"direction":"minimize"}]});
        let original = run(&pattern, &request);
        assert_eq!(original["comparison"]["frontier"], json!(["base"]));
        let protocol = row(&original["comparison"], "base")["input_protocol_sha256"].clone();
        assert!(protocol.is_string());
        let metrics = row(&original, "base")["metrics"].clone();
        let choices =
            row(&original, "base")["sewing_selection"]["result"]["selected_choices"].clone();
        for field in ["deadline", "unused_candidate", "work_limit"] {
            let mut changed = request.clone();
            let selection = &mut changed["sewing_selection"];
            match field {
                "deadline" => selection["completion_deadline_milliseconds"] = json!(7100),
                "unused_candidate" => {
                    selection["alternatives"][1]["recipes"][0]["id"] = json!("same-unused-recipe")
                }
                _ => selection["max_plan_evaluations"] = json!(5),
            }
            let changed = run(&pattern, &changed);
            assert_eq!(row(&changed, "base")["metrics"], metrics);
            assert_eq!(
                row(&changed, "base")["sewing_selection"]["result"]["selected_choices"],
                choices
            );
            assert_ne!(
                row(&changed["comparison"], "base")["input_protocol_sha256"],
                protocol,
                "{consumer}: {field} must retain the complete constrained search protocol"
            );
        }
    }
}

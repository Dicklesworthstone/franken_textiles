//! Independent analytic water limits around canonical material networks and shared balance receipts.
use super::*;
use ftx_pattern::material_vapor::wet_film::{State, WetFilm, scalar_equivalents};

const EVAPORATED: &str = "motion_wet_film_evaporated_mass_kg";
const LATENT: &str = "motion_wet_film_body_reservoir_latent_energy_j";
const UNMET: &str = "motion_wet_film_unmet_demand_mass_kg";
const REMAINING: &str = "motion_wet_film_remaining_liquid_mass_kg";

fn inputs(integration: &str) -> (Request, Value) {
    let (mut request, mut descriptor) = moving_vapor_inputs();
    let body = descriptor["vapor_coverage_body_mesh_sha256"].clone();
    let policy = &mut descriptor["scenario"]["motion"]["vapor_coverage_history"];
    policy["integration"] = json!(integration);
    policy["wet_film"] = json!({"schema":"ftx-body-wet-film/1",
        "body_mesh_sha256":body,"source":"local:synthetic-water-acquisition",
        "basis":"Artificial body liquid and constant supply for numerical comparison only",
        "initial_liquid_mass_kg":[16e-6],"supply_rate_kg_s":[2e-6]});
    request.comparison = Some(
        serde_json::from_value(json!({"basis":"nominal",
        "objectives":[{"metric":EVAPORATED,"direction":"maximize"}],
        "constraints":[{"metric":LATENT,"minimum":0.},
            {"metric":UNMET,"minimum":0.},{"metric":REMAINING,"minimum":0.}],
        "required_consumers":["motion_wet_film"]}))
        .unwrap(),
    );
    (request, descriptor)
}

fn rates(result: &Value, count: usize) -> Vec<f64> {
    let mut rates = vec![0.; count];
    for region in result["coverage"]["regions"].as_array().unwrap() {
        rates[region["body_triangle"].as_u64().unwrap() as usize] +=
            region["vapor"]["potential_mass_flow_kg_s"]
                .as_f64()
                .unwrap();
    }
    rates
}

fn receipts(simulation: &mut Value, descriptor: &Value) {
    let history = &mut simulation["motion"]["result"]["vapor_coverage_history"]["result"];
    let initial = &history["initial"]["result"]["request"];
    let count = initial["map"]["body_triangles"].as_array().unwrap().len();
    let policy: WetFilm = serde_json::from_value(
        descriptor["scenario"]["motion"]["vapor_coverage_history"]["wet_film"].clone(),
    )
    .unwrap();
    let boundaries = initial["conditions"]["boundaries"].as_array().unwrap();
    let latent = (0..count)
        .map(|index| {
            boundaries[if boundaries.len() == 1 { 0 } else { index }]["latent_heat_j_kg"]
                .as_f64()
                .unwrap()
        })
        .collect::<Vec<_>>();
    let body_hash = digest(&json!({"positions_m":initial["map"]["body_positions_m"],
        "triangles":initial["map"]["body_triangles"]}));
    let mut state = State::new(&policy, &body_hash, &latent).unwrap();
    let mut vertices = initial["source"]["positions_m"].as_array().unwrap().len()
        + initial["map"]["body_positions_m"].as_array().unwrap().len();
    for row in history["request"]["samples"].as_array().unwrap() {
        vertices += row["positions_m"].as_array().unwrap().len()
            + row["body_positions_m"].as_array().unwrap().len();
    }
    let trapezoid = history["integration"] == "composite_trapezoid";
    let mut previous = if trapezoid {
        rates(&history["initial"]["result"], count)
    } else {
        vec![0.; count]
    };
    for row in history["samples"].as_array_mut().unwrap() {
        let current = rates(&row["result"], count);
        let dt = row["dt_s"].as_f64().unwrap();
        let (next, receipt) = state
            .advance(&current, trapezoid.then_some(previous.as_slice()), dt)
            .unwrap();
        row["wet_film"] = receipt;
        state = next;
        previous = current;
    }
    history["wet_film"] = state.report().unwrap();
    history["retained_vertex_samples"] = json!(vertices);
    history["retained_wet_film_scalar_equivalents"] =
        json!(scalar_equivalents(count) * (history["samples"].as_array().unwrap().len() + 1));
    for flag in [
        "storage_required",
        "dry_thermal_properties_required",
        "latent_heat_coupled",
        "physical_validation_claim",
        "comfort_claim",
    ] {
        history[flag] = json!(false);
    }
}

fn simulation(pattern: &Pattern, descriptor: &Value) -> Value {
    let mut result = synthetic_moving_vapor(pattern, descriptor);
    receipts(&mut result, descriptor);
    result
}

fn evaluate(request: &Request, descriptor: &Value) -> Value {
    json!(
        request
            .evaluate_with(
                &Pattern::from_json(PATTERN).unwrap(),
                descriptor,
                |_, pattern| Ok(simulation(pattern, descriptor))
            )
            .unwrap()
    )
}

fn close(value: &Value, expected: f64) {
    let actual = value.as_f64().unwrap();
    assert!(
        (actual - expected).abs() <= 1e-12 * expected.abs().max(1e-10),
        "{actual} != {expected}"
    );
}

#[test]
fn finite_water_objectives_use_modeled_supply_limited_outputs_under_both_quadratures() {
    for integration in ["right_endpoint", "composite_trapezoid"] {
        let (request, descriptor) = inputs(integration);
        let report = evaluate(&request, &descriptor);
        assert_eq!(report["comparison"]["frontier"], json!(["a"]), "{report}");
        assert_eq!(report["comparison"]["motion_required"], true);
        for (index, candidate) in report["comparison"]["alternatives"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            // Independent area/time answer: areas 0.5, 0.75 and 1 m²; pressure drop 1000 Pa;
            // film sum 5 plus material Ret 10 or 20; latent heat 2.5 MJ/kg.
            let weighted_area = if integration == "right_endpoint" {
                0.875
            } else {
                0.75
            };
            let demand: f64 =
                weighted_area * 1000. / (if index == 0 { 15. } else { 25. }) / 2_500_000.;
            let evaporated = demand.min(18e-6);
            let expected = [
                (EVAPORATED, evaporated, "kg"),
                (LATENT, evaporated * 2_500_000., "J"),
                (UNMET, demand - evaporated, "kg"),
                (REMAINING, 18e-6 - evaporated, "kg"),
            ];
            assert_eq!(candidate["eligible"], true, "{candidate}");
            for (metric, value, unit) in expected {
                close(&candidate["readings"][metric]["value"], value);
                assert_eq!(candidate["readings"][metric]["unit"], unit);
                assert_eq!(candidate["readings"][metric]["consumer"], "motion_wet_film");
                assert!(candidate["readings"][metric]["reported_bounds"].is_null());
            }
            let result = &report["simulations"][index]["evaluation"]["result"];
            assert_eq!(result["surface"]["status"], "refused");
            assert_eq!(result["coverage"]["status"], "refused");
            assert_eq!(
                report["manufacturing"]["alternatives"][index]["cutting"]["status"],
                "available"
            );
        }
        // Inclusive limits use the actual admitted number; tightening it excludes that row.
        let maximum = report["comparison"]["alternatives"][0]["readings"][EVAPORATED]["value"]
            .as_f64()
            .unwrap();
        let mut limited = request.clone();
        limited.comparison.as_mut().unwrap().constraints = serde_json::from_value(json!([
            {"metric":EVAPORATED,"minimum":maximum,"maximum":maximum}]))
        .unwrap();
        assert_eq!(
            evaluate(&limited, &descriptor)["comparison"]["frontier"],
            json!(["a"])
        );
        limited.comparison.as_mut().unwrap().constraints[0].minimum = Some(maximum * 1.01);
        limited.comparison.as_mut().unwrap().constraints[0].maximum = None;
        assert_eq!(
            evaluate(&limited, &descriptor)["comparison"]["frontier"],
            json!([])
        );
    }
}

#[test]
fn exhausted_supply_ties_evaporation_even_when_potential_transfer_differs() {
    let (mut request, mut descriptor) = inputs("right_endpoint");
    descriptor["scenario"]["motion"]["vapor_coverage_history"]["wet_film"]["initial_liquid_mass_kg"] =
        json!([1e-6]);
    descriptor["scenario"]["motion"]["vapor_coverage_history"]["wet_film"]["supply_rate_kg_s"] =
        json!([0.]);
    let result = evaluate(&request, &descriptor);
    assert_eq!(result["comparison"]["frontier"], json!(["a", "b"]));
    for row in result["comparison"]["alternatives"].as_array().unwrap() {
        close(&row["readings"][EVAPORATED]["value"], 1e-6);
        close(&row["readings"][REMAINING]["value"], 0.);
    }
    request.comparison.as_mut().unwrap().objectives = serde_json::from_value(json!([
        {"metric":"motion_covered_vapor_potential_mass_kg","direction":"maximize"}]))
    .unwrap();
    assert_eq!(
        evaluate(&request, &descriptor)["comparison"]["frontier"],
        json!(["a"])
    );
}

// Divide one covered triangle into two equal-area bodies under the same cloth sheet. The second
// body is dry; its unmet demand cannot draw from the first body's independently owned reservoir.
fn two_body_simulation(pattern: &Pattern, descriptor: &Value) -> Value {
    let mut simulation = synthetic_moving_vapor(pattern, descriptor);
    let motion = &mut simulation["motion"]["result"];
    let mut history = std::mem::take(&mut motion["vapor_coverage_history"]["result"]);
    let mut frames: Vec<_> = std::iter::once(history["initial"]["result"].clone())
        .chain(
            history["samples"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| row["result"].clone()),
        )
        .collect();
    let triangles = json!([[0, 1, 3], [0, 3, 2]]);
    for frame in &mut frames {
        let width = frame["request"]["source"]["positions_m"][1][0]
            .as_f64()
            .unwrap();
        let area = width * 0.25;
        let request = &mut frame["request"];
        request["map"]["body_positions_m"]
            .as_array_mut()
            .unwrap()
            .push(json!([0.5 * width, 0.5, 0.]));
        request["map"]["body_triangles"] = triangles.clone();
        let body_hash = digest(
            &json!({"positions_m":request["map"]["body_positions_m"],"triangles":triangles}),
        );
        request["conditions"]["body_mesh_sha256"] = json!(body_hash);
        let vapor = canonical_vapor_column(
            &request["conditions"],
            &request["source"]["materials"][0],
            area,
        );
        frame["body_mesh_sha256"] = json!(body_hash);
        frame["projection"]["regions"] = json!(
            (0..2)
                .map(|body| json!({
            "id":format!("body-{body}-region-0"),"body_triangle":body,"body_fraction":1.,
            "layers":[{"layer":"shell","cloth_triangle":0,"fraction":0.5}]}))
                .collect::<Vec<_>>()
        );
        frame["coverage"]["body"] = json!([{"area_m2":area,"covered_area_m2":area,"uncovered_area_m2":0.},
            {"area_m2":area,"covered_area_m2":area,"uncovered_area_m2":0.}]);
        frame["coverage"]["regions"] = json!(
            (0..2)
                .map(|body| json!({
            "id":format!("body-{body}-region-0"),"body_triangle":body,
            "body_area_m2":area,"layer_area_m2":[area],"vapor":vapor}))
                .collect::<Vec<_>>()
        );
        frame["request_sha256"] = json!(digest(&frame["request"]));
        frame["replay_request_json"] = json!(serde_json::to_string(&frame["request"]).unwrap());
    }
    motion["body_triangles"] = triangles;
    motion["initial_sample"]["body_positions_m"] =
        frames[0]["request"]["map"]["body_positions_m"].clone();
    history["initial"]["body_positions_sha256"] =
        json!(digest(&motion["initial_sample"]["body_positions_m"]));
    history["initial"]["result"] = frames[0].clone();
    history["request"]["initial"] = frames[0]["request"].clone();
    for (index, frame) in frames.into_iter().skip(1).enumerate() {
        let positions = frame["request"]["map"]["body_positions_m"].clone();
        motion["samples"][index]["body_positions_m"] = positions.clone();
        motion["keyframes"][index]["body_collision_sha256"] =
            json!(digest(&json!([positions, motion["body_triangles"]])));
        history["request"]["samples"][index]["body_positions_m"] = positions.clone();
        history["samples"][index]["body_positions_sha256"] = json!(digest(&positions));
        history["samples"][index]["result"] = frame;
    }
    history["request_sha256"] = json!(digest(&history["request"]));
    history["replay_request_json"] = json!(serde_json::to_string(&history["request"]).unwrap());
    motion["vapor_coverage_history"]["result"] = history;
    receipts(&mut simulation, descriptor);
    simulation
}

#[test]
fn finite_water_never_pools_liquid_between_body_triangles_and_checks_each_retained_balance() {
    for integration in ["right_endpoint", "composite_trapezoid"] {
        let (request, mut descriptor) = inputs(integration);
        descriptor["scenario"]["vapor_coverage"]["body_triangle_ids"] = json!([0, 1]);
        let body_hash = digest(
            &json!({"positions_m":[[0.,0.,0.],[1.,0.,0.],[0.,1.,0.],[0.5,0.5,0.]],
            "triangles":[[0,1,3],[0,3,2]]}),
        );
        descriptor["vapor_coverage_body_mesh_sha256"] = json!(body_hash);
        let policy = &mut descriptor["scenario"]["motion"]["vapor_coverage_history"]["wet_film"];
        policy["body_mesh_sha256"] = json!(body_hash);
        policy["initial_liquid_mass_kg"] = json!([1., 0.]);
        policy["supply_rate_kg_s"] = json!([0., 0.]);
        for tamper in [false, true] {
            let report = json!(
                request
                    .evaluate_with(
                        &Pattern::from_json(PATTERN).unwrap(),
                        &descriptor,
                        |key, pattern| {
                            let mut result = two_body_simulation(pattern, &descriptor);
                            if tamper && key == "a" {
                                // Aggregate sums do not detect a permutation of body ownership.
                                result["motion"]["result"]["vapor_coverage_history"]["result"]
                                        ["samples"][0]["wet_film"]["body"]
                                        .as_array_mut()
                                        .unwrap()
                                        .swap(0, 1);
                            }
                            Ok(result)
                        }
                    )
                    .unwrap()
            );
            assert_eq!(
                report["comparison"]["frontier"],
                if tamper { json!(["b"]) } else { json!(["a"]) },
                "{}",
                report["comparison"]
            );
            if !tamper {
                let result = &report["simulations"][0]["evaluation"]["result"]["motion"]["result"]
                    ["vapor_coverage_history"]["result"];
                let demand = result["totals"]["potential_mass_kg"].as_f64().unwrap();
                close(
                    &report["comparison"]["alternatives"][0]["readings"][EVAPORATED]["value"],
                    0.5 * demand,
                );
                close(
                    &report["comparison"]["alternatives"][0]["readings"][UNMET]["value"],
                    0.5 * demand,
                );
                close(&result["wet_film"]["body"][1]["evaporated_mass_kg"], 0.);
                assert!(
                    demand < 1.,
                    "whole-domain liquid exceeds demand, but the dry body still cannot evaporate"
                );
            }
        }
    }
}

#[test]
fn finite_water_refuses_tampered_steps_sources_supply_and_complete_history_receipts() {
    let (request, descriptor) = inputs("composite_trapezoid");
    for fault in [
        "final",
        "step",
        "totals",
        "zero_evaporation",
        "supply",
        "source",
        "body",
        "material",
        "partial",
        "protocol",
        "omitted_sample",
        "scalars",
        "vertices",
    ] {
        let report = json!(request.evaluate_with(&Pattern::from_json(PATTERN).unwrap(), &descriptor,
            |key, pattern| {
                let mut result = simulation(pattern, &descriptor);
                if key == "a" {
                    match fault {
                        "zero_evaporation" => {
                            // An internally balanced dry-source report cannot replace the declared film.
                            let mut wrong = descriptor.clone();
                            let film = &mut wrong["scenario"]["motion"]["vapor_coverage_history"]["wet_film"];
                            film["initial_liquid_mass_kg"] = json!([0.]);
                            film["supply_rate_kg_s"] = json!([0.]);
                            receipts(&mut result, &wrong);
                        }
                        "supply" => {
                            let mut wrong = descriptor.clone();
                            wrong["scenario"]["motion"]["vapor_coverage_history"]["wet_film"]["supply_rate_kg_s"] = json!([1.]);
                            result = simulation(pattern, &wrong);
                        }
                        "material" => {
                            let mut wrong = pattern.raw().clone();
                            wrong["fabric"]["material"]["record"]["properties"]["vapor_resistance"]["value"] = json!(1.);
                            wrong["fabric"]["material"]["expected_sha256"] = Value::Null;
                            result = simulation(&Pattern::new(wrong), &descriptor);
                            result["pattern_sha256"] = json!(digest(pattern.raw()));
                        }
                        _ => {
                            let history = &mut result["motion"]["result"]["vapor_coverage_history"]["result"];
                            match fault {
                                "final" => history["wet_film"] = Value::Null,
                                "step" => history["samples"][0]["wet_film"] = Value::Null,
                                "totals" => history["wet_film"]["totals"]["remaining_liquid_mass_kg"] = json!(1.),
                                "source" => history["wet_film"]["source"] = json!("local:other-source"),
                                "body" => history["wet_film"]["body_mesh_sha256"] = json!("0".repeat(64)),
                                "partial" => history["samples"][0]["result"]["coverage"]["uncovered_area_m2"] = json!(0.1),
                                "protocol" => history["samples"][0]["result"]["coverage"]["regions"][0]["vapor"]["layers"][0]["observation"]["method"] = json!("unrelated test"),
                                "omitted_sample" => { history["samples"].as_array_mut().unwrap().remove(0); }
                                "scalars" => history["retained_wet_film_scalar_equivalents"] = json!(0),
                                "vertices" => history["retained_vertex_samples"] = json!(0),
                                _ => unreachable!(),
                            }
                        }
                    }
                }
                Ok(result)
            }).unwrap());
        assert_eq!(
            report["comparison"]["frontier"],
            json!(["b"]),
            "{fault}: {}",
            report["comparison"]
        );
        assert_eq!(
            report["comparison"]["alternatives"][0]["eligible"], false,
            "{fault}"
        );
        assert!(
            report["comparison"]["alternatives"][0]["readings"]
                .get(EVAPORATED)
                .is_none(),
            "{fault}"
        );
        assert_eq!(
            report["manufacturing"]["alternatives"][0]["cutting"]["status"],
            "available"
        );
    }
}

#[test]
fn missing_film_is_local_and_missing_ret_ranges_preserve_nominal_water_readings() {
    let (mut request, mut descriptor) = inputs("right_endpoint");
    descriptor["scenario"]["motion"]["vapor_coverage_history"]
        .as_object_mut()
        .unwrap()
        .remove("wet_film");
    let run = |request: &Request| {
        json!(
            request
                .evaluate_with(
                    &Pattern::from_json(PATTERN).unwrap(),
                    &descriptor,
                    |_, pattern| Ok(synthetic_moving_vapor(pattern, &descriptor))
                )
                .unwrap()
        )
    };
    let missing = run(&request);
    assert_eq!(missing["comparison"]["frontier"], json!([]));
    assert_eq!(
        missing["simulations"][0]["evaluation"]["status"],
        "available"
    );
    let water_policy = request.comparison.clone().unwrap();
    request.comparison = moving_vapor_inputs().0.comparison;
    assert_eq!(run(&request)["comparison"]["frontier"], json!(["a"]));
    request.comparison = Some(water_policy);
    request.comparison.as_mut().unwrap().basis =
        ftx_export::material_study::comparison::Basis::ReportedBounds;
    let (_, complete_descriptor) = inputs("right_endpoint");
    let bounded = evaluate(&request, &complete_descriptor);
    assert_eq!(bounded["comparison"]["frontier"], json!([]));
    for row in bounded["comparison"]["alternatives"].as_array().unwrap() {
        assert_eq!(row["eligible"], false);
        assert!(row["readings"][EVAPORATED]["value"].is_number());
        assert!(row["readings"][EVAPORATED]["reported_bounds"].is_null());
    }
    assert_eq!(
        bounded["simulations"][0]["evaluation"]["status"],
        "available"
    );
}

#[test]
fn finite_water_keeps_consumed_ret_protocols_and_can_be_required_for_manufacturing() {
    let (mut request, descriptor) = inputs("right_endpoint");
    request.variants[1]
        .changes
        .default
        .as_mut()
        .unwrap()
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("vapor_resistance")
        .unwrap()
        .method = "different conditioned experiment".into();
    let report = evaluate(&request, &descriptor);
    assert_eq!(report["comparison"]["frontier"], json!(["a", "b"]));
    assert_eq!(
        report["comparison"]["incomparable"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    request.comparison.as_mut().unwrap().allow_mixed_protocols = true;
    assert_eq!(
        evaluate(&request, &descriptor)["comparison"]["frontier"],
        json!(["a"])
    );
    request.comparison = Some(
        serde_json::from_value(json!({"basis":"nominal",
        "objectives":[{"metric":"stock_area_m2","direction":"minimize"}],
        "required_consumers":["motion_wet_film"]}))
        .unwrap(),
    );
    let report = json!(
        request
            .evaluate_with(
                &Pattern::from_json(PATTERN).unwrap(),
                &descriptor,
                |key, pattern| {
                    let mut result = simulation(pattern, &descriptor);
                    if key == "a" {
                        result["motion"]["result"]["vapor_coverage_history"]["result"]["wet_film"] =
                            Value::Null;
                    }
                    Ok(result)
                }
            )
            .unwrap()
    );
    assert_eq!(report["comparison"]["frontier"], json!(["b"]));
    assert!(
        report["comparison"]["alternatives"][0]["readings"]["stock_area_m2"]["value"].is_number()
    );
}

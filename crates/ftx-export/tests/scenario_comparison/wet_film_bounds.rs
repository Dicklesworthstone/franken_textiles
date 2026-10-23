//! Conditional source-Ret ranges pass through the same conserved finite-water reducer.
//! These synthetic histories test decision mathematics, not acquired wearer measurements.
use super::*;
use ftx_export::material_study::{comparison::Basis, matrix::Case};

const EVAPORATED: &str = "motion_wet_film_evaporated_mass_kg";
const LATENT: &str = "motion_wet_film_body_reservoir_latent_energy_j";
const UNMET: &str = "motion_wet_film_unmet_demand_mass_kg";
const REMAINING: &str = "motion_wet_film_remaining_liquid_mass_kg";
const SUPPLIED: &str = "motion_wet_film_supplied_mass_kg";

fn inputs(initial: f64, supply: f64, integration: &str) -> (Request, Value) {
    let (mut request, mut descriptor) = body_water_inputs(initial, supply);
    descriptor["scenario"]["motion"]["vapor_coverage_history"]["integration"] = json!(integration);
    request.comparison = Some(
        serde_json::from_value(json!({
            "basis":"reported_bounds",
            "objectives":[{"metric":EVAPORATED,"direction":"maximize"}],
            "constraints":[
                {"metric":LATENT,"minimum":0.},
                {"metric":UNMET,"minimum":0.},
                {"metric":REMAINING,"minimum":0.},
                {"metric":SUPPLIED,"minimum":0.}],
            "required_consumers":["motion_wet_film"]
        }))
        .unwrap(),
    );
    (request, descriptor)
}

fn readings(report: &Value, alternative: usize) -> &Value {
    &report["comparison"]["alternatives"][alternative]["readings"]
}

fn history(report: &Value, alternative: usize) -> &Value {
    &report["simulations"][alternative]["evaluation"]["result"]["motion"]["result"]["vapor_coverage_history"]
        ["result"]
}

fn range(report: &Value, alternative: usize, metric: &str) -> [f64; 2] {
    let reading = &readings(report, alternative)[metric];
    [0, 1].map(|index| {
        reading["reported_bounds"][index]
            .as_f64()
            .unwrap_or_else(|| panic!("missing {metric} range: {}", report["comparison"]))
    })
}

fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= expected.abs().max(1e-12) * 1e-12,
        "actual {actual:?}, expected {expected:?}"
    );
}

fn close_range(actual: [f64; 2], expected: [f64; 2]) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        close(actual, expected);
    }
}

#[test]
fn source_ranges_cross_depletion_and_keep_overlap_unresolved_in_both_quadratures() {
    for integration in ["right_endpoint", "composite_trapezoid"] {
        let initial = 20e-6;
        let (mut request, descriptor) = inputs(initial, 0., integration);
        bound_vapor_variants(&mut request, [Some([5., 25.]), Some([18., 22.])]);
        let report = body_water_comparison(&request, &descriptor);
        assert_eq!(
            report["comparison"]["frontier"],
            json!(["a", "b"]),
            "{}",
            report["comparison"]
        );
        // Initial, middle and final body areas are 0.5, 0.75 and 1 m². The Ret endpoints
        // straddle depletion; film resistance is 5 m²Pa/W and pressure difference is 1000 Pa.
        let area_time = if integration == "right_endpoint" {
            0.875
        } else {
            0.75
        };
        let demand = [30., 10.].map(|resistance| area_time * 1000. / resistance / 2_500_000.);
        let evaporated = demand.map(|mass: f64| mass.min(initial));
        close_range(range(&report, 0, EVAPORATED), evaporated);
        close_range(
            range(&report, 0, LATENT),
            evaporated.map(|mass| mass * 2_500_000.),
        );
        close_range(
            range(&report, 0, REMAINING),
            [initial - evaporated[1], initial - evaporated[0]],
        );
        close_range(
            range(&report, 0, UNMET),
            [demand[0] - evaporated[0], demand[1] - evaporated[1]],
        );
        assert_eq!(range(&report, 0, SUPPLIED), [0., 0.]);
        for (metric, field) in [
            (EVAPORATED, "evaporated_mass_kg"),
            (LATENT, "body_reservoir_latent_energy_j"),
            (REMAINING, "remaining_liquid_mass_kg"),
            (UNMET, "unmet_demand_mass_kg"),
            (SUPPLIED, "supplied_mass_kg"),
        ] {
            let nominal = readings(&report, 0)[metric]["value"].as_f64().unwrap();
            assert_eq!(
                readings(&report, 0)[metric]["value"],
                history(&report, 0)["wet_film"]["totals"][field]
            );
            let [lower, upper] = range(&report, 0, metric);
            assert!(
                lower <= nominal && nominal <= upper,
                "{metric}: {lower} <= {nominal} <= {upper}"
            );
        }
        request.comparison.as_mut().unwrap().basis = Basis::Nominal;
        let nominal = body_water_comparison(&request, &descriptor);
        assert_eq!(nominal["comparison"]["frontier"], json!(["a"]));
        assert_eq!(report["simulations"], nominal["simulations"]);
        assert_eq!(
            history(&report, 0)["wet_film"]["uncertainty_propagated"],
            false
        );
    }
}

#[test]
fn every_source_endpoint_exhausts_the_same_prescribed_supply_and_ties() {
    for integration in ["right_endpoint", "composite_trapezoid"] {
        let (mut request, descriptor) = inputs(1e-6, 2e-6, integration);
        bound_vapor_variants(&mut request, [Some([8., 12.]), Some([18., 22.])]);
        let report = body_water_comparison(&request, &descriptor);
        assert_eq!(
            report["comparison"]["frontier"],
            json!(["a", "b"]),
            "{}",
            report["comparison"]
        );
        for alternative in 0..2 {
            close_range(range(&report, alternative, EVAPORATED), [3e-6, 3e-6]);
            close_range(range(&report, alternative, LATENT), [7.5, 7.5]);
            close_range(range(&report, alternative, SUPPLIED), [2e-6, 2e-6]);
            assert_eq!(range(&report, alternative, REMAINING), [0., 0.]);
        }
        assert_eq!(range(&report, 0, EVAPORATED), range(&report, 1, EVAPORATED));
    }
}

#[test]
fn inclusive_constraints_apply_to_the_complete_conditional_range() {
    for integration in ["right_endpoint", "composite_trapezoid"] {
        let (mut request, descriptor) = inputs(1e-3, 0., integration);
        bound_vapor_variants(&mut request, [Some([8., 12.]), Some([18., 22.])]);
        let report = body_water_comparison(&request, &descriptor);
        let [lower, upper] = range(&report, 0, EVAPORATED);
        let nominal = readings(&report, 0)[EVAPORATED]["value"].as_f64().unwrap();
        request.comparison.as_mut().unwrap().constraints = serde_json::from_value(json!([
            {"metric":EVAPORATED,"minimum":lower,"maximum":upper}
        ]))
        .unwrap();
        let inclusive = body_water_comparison(&request, &descriptor);
        assert_eq!(inclusive["comparison"]["frontier"], json!(["a"]));
        request.comparison.as_mut().unwrap().constraints[0].minimum = Some((lower + nominal) / 2.);
        let bounded = body_water_comparison(&request, &descriptor);
        assert_eq!(bounded["comparison"]["frontier"], json!([]));
        assert_eq!(
            bounded["manufacturing"]["alternatives"][0]["cutting"]["status"],
            "available"
        );
        request.comparison.as_mut().unwrap().basis = Basis::Nominal;
        assert_eq!(
            body_water_comparison(&request, &descriptor)["comparison"]["frontier"],
            json!(["a"])
        );
    }
}

#[test]
fn missing_ret_ranges_keep_nominal_water_and_independent_supplied_mass_available() {
    for integration in ["right_endpoint", "composite_trapezoid"] {
        let (mut request, descriptor) = inputs(1e-3, 2e-6, integration);
        bound_vapor_variants(&mut request, [None, Some([18., 22.])]);
        let report = body_water_comparison(&request, &descriptor);
        assert_eq!(report["comparison"]["frontier"], json!(["b"]));
        assert_eq!(report["comparison"]["alternatives"][0]["eligible"], false);
        assert!(readings(&report, 0)[EVAPORATED]["value"].as_f64().unwrap() > 0.);
        assert!(readings(&report, 0)[EVAPORATED]["reported_bounds"].is_null());
        assert_eq!(
            report["manufacturing"]["alternatives"][0]["cutting"]["status"],
            "available"
        );
        assert_eq!(
            history(&report, 0)["wet_film"]["schema"],
            "ftx-body-wet-film-report/1"
        );
        request.comparison.as_mut().unwrap().basis = Basis::Nominal;
        assert_eq!(
            body_water_comparison(&request, &descriptor)["comparison"]["frontier"],
            json!(["a"])
        );

        bound_vapor_variants(&mut request, [None, None]);
        request.comparison = Some(
            serde_json::from_value(json!({"basis":"reported_bounds",
            "objectives":[{"metric":SUPPLIED,"direction":"minimize"}],
            "required_consumers":["motion_wet_film"]}))
            .unwrap(),
        );
        let supplied = body_water_comparison(&request, &descriptor);
        assert_eq!(supplied["comparison"]["frontier"], json!(["a", "b"]));
        for alternative in 0..2 {
            close_range(range(&supplied, alternative, SUPPLIED), [2e-6, 2e-6]);
        }
    }
}

#[test]
fn later_supply_can_reverse_interval_evaporation_without_reversing_cumulative_bounds() {
    for integration in ["right_endpoint", "composite_trapezoid"] {
        let (mut request, descriptor) = inputs(10e-6, 4e-6, integration);
        bound_vapor_variants(&mut request, [Some([5., 25.]), Some([5., 25.])]);
        let report = body_water_comparison(&request, &descriptor);
        let mut endpoint_request = request.clone();
        endpoint_request.comparison.as_mut().unwrap().basis = Basis::Nominal;
        for (variant, resistance) in endpoint_request.variants.iter_mut().zip([5., 25.]) {
            let selection = variant.changes.default.as_mut().unwrap();
            selection.expected_sha256 = None;
            selection
                .record
                .as_mut()
                .unwrap()
                .properties
                .get_mut("vapor_resistance")
                .unwrap()
                .value = resistance;
        }
        let endpoints = body_water_comparison(&endpoint_request, &descriptor);
        let high = history(&endpoints, 0);
        let low = history(&endpoints, 1);
        // The high-demand path exhausts initial liquid in interval one and can only consume
        // 2 mg of later supply. The low-demand path still owns carried-over liquid.
        assert!(
            high["samples"][1]["wet_film"]["totals"]["evaporated_mass_kg"]
                .as_f64()
                .unwrap()
                < low["samples"][1]["wet_film"]["totals"]["evaporated_mass_kg"]
                    .as_f64()
                    .unwrap()
        );
        for (metric, field, reverse) in [
            (EVAPORATED, "evaporated_mass_kg", false),
            (LATENT, "body_reservoir_latent_energy_j", false),
            (UNMET, "unmet_demand_mass_kg", false),
            (REMAINING, "remaining_liquid_mass_kg", true),
            (SUPPLIED, "supplied_mass_kg", false),
        ] {
            let mut expected =
                [low, high].map(|history| history["wet_film"]["totals"][field].as_f64().unwrap());
            if reverse {
                expected.reverse();
            }
            close_range(range(&report, 0, metric), expected);
        }
        close(range(&report, 0, EVAPORATED)[1], 14e-6);
        for sample in history(&report, 0)["samples"].as_array().unwrap() {
            assert!(sample["wet_film"].get("reported_bounds").is_none());
        }
    }
}

#[test]
fn named_cases_combine_conditional_wet_preference_with_a_supply_limited_tie() {
    for integration in ["right_endpoint", "composite_trapezoid"] {
        let (mut request, wet) = inputs(1e-3, 0., integration);
        let (_, limited) = inputs(1e-6, 2e-6, integration);
        bound_vapor_variants(&mut request, [Some([8., 12.]), Some([18., 22.])]);
        let mut cases = vec![
            Case {
                key: "water_available".into(),
                scenario: wet,
                comparison: None,
            },
            Case {
                key: "supply_limited".into(),
                scenario: limited,
                comparison: None,
            },
        ];
        let run = |cases: &[Case]| {
            serde_json::to_value(
                request
                    .evaluate_matrix_with(
                        &Pattern::from_json(PATTERN).unwrap(),
                        cases,
                        4,
                        |index, _, pattern| {
                            Ok(synthetic_body_water(pattern, &cases[index].scenario))
                        },
                    )
                    .unwrap(),
            )
            .unwrap()
        };
        let report = run(&cases);
        assert_eq!(
            report["comparison"]["frontier"],
            json!(["a"]),
            "{}",
            report["comparison"]
        );
        assert_eq!(
            report["cases"][1]["comparison"]["frontier"],
            json!(["a", "b"])
        );
        assert_eq!(
            report["comparison"]["alternatives"][0]["cases"]["water_available"]["readings"],
            report["cases"][0]["comparison"]["alternatives"][0]["readings"]
        );
        let mut infeasible = request.comparison.clone().unwrap();
        infeasible
            .constraints
            .push(serde_json::from_value(json!({"metric":EVAPORATED,"minimum":4e-6})).unwrap());
        cases[1].comparison = Some(infeasible);
        let excluded = run(&cases);
        assert_eq!(excluded["comparison"]["frontier"], json!([]));
        assert_eq!(excluded["cases"][0]["comparison"]["frontier"], json!(["a"]));
        assert_eq!(
            excluded["manufacturing"]["alternatives"][0]["cutting"]["status"],
            "available"
        );
    }
}

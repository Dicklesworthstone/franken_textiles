//! Actual sewn geometry and projected coverage through the portable study authority.
use super::*;

#[test]
fn production_surface_and_coverage_ranges_admit_signed_decisions_and_frozen_replay() {
    let form = form();
    let body = DrapeBody {
        frames: &Form,
        collision: (&form.vertices, &form.triangles),
        display: (&form.vertices, &form.triangles),
        arm_deg: 35.,
    };
    let mut request = request();
    request.variants.truncate(2);
    for (variant, value) in request.variants.iter_mut().zip([0.1, 0.4]) {
        let material = variant
            .changes
            .default
            .as_mut()
            .unwrap()
            .record
            .as_mut()
            .unwrap();
        material.properties.insert(
            "thermal_resistance".into(),
            serde_json::from_value(json!({
            "value":value,"bounds":[value * 0.9,value * 1.1],"unit":"m2*K/W","source":"test",
            "evidence":"estimate","method":"Synthetic steady heat range integration"}))
            .unwrap(),
        );
    }
    request.comparison = Some(serde_json::from_value(json!({"basis":"reported_bounds",
        "objectives":[{"metric":"independent_surface_power_w","direction":"minimize"},
            {"metric":"body_coverage_power_w","direction":"minimize"}],
        "constraints":[{"metric":"max_absolute_edge_strain_pct","basis":"nominal","maximum":100.}]})).unwrap());
    let mut scenario: Value = serde_json::from_str(SCENARIO).unwrap();
    // An explicit sewing-only numerical fixture, without a gravity-settled or fit claim.
    scenario["options"]["sew_steps"] = json!(120);
    scenario["options"]["drape_steps"] = json!(0);
    let evaluate = |request: &Request, scenario: &Value| -> Value {
        serde_json::from_str(
            &on_body(
                &body,
                PATTERN,
                &serde_json::to_string(request).unwrap(),
                &scenario.to_string(),
                |_| {},
            )
            .unwrap(),
        )
        .unwrap()
    };
    for (reverse, convection) in [(false, false), (true, false), (false, true), (true, true)] {
        let mut scenario = scenario.clone();
        for path in ["/surface/boundaries/0", "/coverage/conditions/boundaries/0"] {
            let boundary = scenario.pointer_mut(path).unwrap();
            boundary["inside_c"] = json!(if reverse { 10. } else { 30. });
            boundary["outside_c"] = json!(if reverse { 30. } else { 10. });
        }
        if convection {
            let conditions = &mut scenario["coverage"]["conditions"];
            let boundary = conditions["boundaries"][0].clone();
            conditions["boundaries"][0]["outside_resistance_m2_k_w"] = json!(0.);
            let exterior: ftx_drape::thermal::steady::exterior::Exterior =
                serde_json::from_value(json!({
                    "air_c":boundary["outside_c"],"convection_w_m2_k":5.,
                    "surface_temperature_range_c":[-20.,80.],
                    "convection_source":{"reference":"test:portable-covered-convection",
                        "accessed_on":"2026-11-05","note":"Synthetic boundary; no empirical garment claim"},
                    "convection_evidence":"estimate","allow_estimates":true,
                    "convection_conditions":"Prescribed constant coefficient on the sewn fixture's outer cloth"
                }))
                .unwrap();
            conditions["exterior_exchange"] = json!([exterior]);
            conditions["bare_boundaries"] = json!([{
                "inside_c":boundary["inside_c"],"outside_c":boundary["outside_c"],
                "inside_resistance_m2_k_w":0.2,"outside_resistance_m2_k_w":0.4
            }]);
        }
        let result = evaluate(&request, &scenario);
        assert_eq!(
            result["comparison"]["frontier"],
            json!([if reverse { "a" } else { "b" }]),
            "{}",
            result["comparison"]
        );
        for (candidate, simulation) in result["comparison"]["alternatives"]
            .as_array()
            .unwrap()
            .iter()
            .zip(result["simulations"].as_array().unwrap())
        {
            assert_eq!(candidate["eligible"], true, "{candidate}");
            let outcome = &simulation["evaluation"]["result"];
            assert_eq!(outcome["drape"]["settled"], true);
            assert_eq!(outcome["metrics"]["simulation_gating_clear"], true);
            assert_eq!(
                outcome["coverage"]["result"]["body_mesh_sha256"],
                result["scenario"]["coverage_body_mesh_sha256"]
            );
            for (metric, consumer) in [
                ("independent_surface_power_w", "surface"),
                ("body_coverage_power_w", "coverage"),
            ] {
                let reading = &candidate["readings"][metric];
                let steady = &outcome[consumer]["result"]["steady"];
                assert_eq!(reading["value"], steady["power_w"]);
                assert_eq!(
                    reading["reported_bounds"],
                    steady["reported_bounds"]["power_w"]
                );
                let [lo, hi]: [f64; 2] =
                    serde_json::from_value(reading["reported_bounds"].clone()).unwrap();
                let nominal = reading["value"].as_f64().unwrap();
                assert!(lo < nominal && nominal < hi);
                assert_eq!(nominal < 0., reverse);
            }
            assert!(
                candidate["readings"]["max_absolute_edge_strain_pct"]["reported_bounds"].is_null()
            );
            assert!(
                outcome["coverage"]["result"]["steady"]["regions"]
                    .as_array()
                    .is_some_and(|regions| !regions.is_empty())
            );
            if convection {
                let heat = &outcome["coverage"]["result"];
                let steady = &heat["steady"];
                assert_eq!(steady["bounds_unavailable"], json!([]));
                for field in ["boundaries", "exterior_exchange", "bare_boundaries"] {
                    assert_eq!(
                        heat["request"]["conditions"][field],
                        scenario["coverage"]["conditions"][field]
                    );
                }
                let regions = steady["regions"].as_array().unwrap();
                let exchanges = heat["exterior_exchange"]["regions"].as_array().unwrap();
                assert_eq!(regions.len(), exchanges.len());
                for (region, exchange) in regions.iter().zip(exchanges) {
                    assert_eq!(region["conduction"], exchange["conduction"]);
                    assert!(region["conduction"]["reported_bounds"].is_object());
                    assert!(region["reported_bounds"]["power_w"].is_array());
                    assert_eq!(exchange["radiation_enabled"], false);
                    assert_eq!(exchange["radiative_flux_w_m2"].as_f64(), Some(0.));
                    let outer_area = region["layer_area_m2"]
                        .as_array()
                        .unwrap()
                        .last()
                        .unwrap()
                        .as_f64()
                        .unwrap();
                    let convective = exchange["convective_flux_w_m2"].as_f64().unwrap();
                    assert!(
                        (region["power_w"].as_f64().unwrap() - outer_area * convective).abs()
                            < 1e-10
                    );
                }
                let convective = heat["exterior_exchange"]["convective_power_w"]
                    .as_f64()
                    .unwrap();
                let power = steady["power_w"].as_f64().unwrap();
                let bare = steady["bare_power_w"].as_f64().unwrap();
                assert!((power - bare - convective).abs() < 1e-10);
                let replay: Value = serde_json::from_str(
                    &ftx_wasm::thermal::steady_coverage(&heat["request"].to_string()).unwrap(),
                )
                .unwrap();
                assert_eq!(&replay, heat);
            }
        }
        let mut frozen = request.clone();
        for (variant, row) in frozen
            .variants
            .iter_mut()
            .zip(result["manufacturing"]["alternatives"].as_array().unwrap())
        {
            variant.changes = serde_json::from_value(row["frozen_changes"].clone()).unwrap();
        }
        let replay = evaluate(&frozen, &scenario);
        assert_eq!(replay["simulations"], result["simulations"]);
        assert_eq!(
            replay["comparison"]["alternatives"],
            result["comparison"]["alternatives"]
        );
        assert_eq!(
            replay["comparison"]["frontier"],
            result["comparison"]["frontier"]
        );
    }
}

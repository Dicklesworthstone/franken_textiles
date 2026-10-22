//! Analytic series-network checks and preserved published Ret; no measured-stack/comfort claim.

use ftx_core::RefusalCode;
use ftx_pattern::{
    material_catalog::Selection,
    material_thermal::ThermalLaw,
    material_vapor::{Layer, MODEL, Request},
    materials::{Catalog, Evidence, Form},
};
use serde_json::json;

fn empty_catalog() -> Catalog {
    Catalog::from_json(r#"{"schema":"ftx-textile-catalog/1","materials":[]}"#).unwrap()
}

fn synthetic_selection(id: &str, resistance: f64) -> Selection {
    serde_json::from_value(json!({
        "id": id, "allow_estimates": true,
        "record": {
            "id": id, "name": format!("Synthetic vapor layer {id}"), "form": "woven",
            "properties": {"vapor_resistance": {
                "value": resistance, "unit": "m2*Pa/W", "evidence": "estimate",
                "source": "analytic", "method": "synthetic fixed resistance",
                "conditions": {"conditioning": "synthetic isothermal reference"}
            }},
            "sources": {"analytic": {
                "reference": "local:synthetic-vapor-network", "accessed_on": "2026-10-21",
                "note": "Analytic test only; not a measurement of any textile"
            }}
        }
    }))
    .unwrap()
}

fn request() -> Request {
    serde_json::from_value(json!({
        "schema": "ftx-vapor-stack/1",
        "layers": [
            {"key": "lining", "selection": synthetic_selection("a", 2.)},
            {"key": "shell", "selection": synthetic_selection("b", 3.)}
        ],
        "area_m2": 0.5, "inside_vapor_pressure_pa": 2000., "outside_vapor_pressure_pa": 1000.,
        "inside_film_resistance_m2_pa_w": 1., "outside_film_resistance_m2_pa_w": 4.,
        "latent_heat_j_kg": 2_500_000.
    }))
    .unwrap()
}

fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 1e-12 * expected.abs().max(1e-15),
        "{actual} != {expected}"
    );
}

#[test]
fn common_area_series_network_obeys_pressure_heat_and_mass_balances() {
    let input = request();
    let before = serde_json::to_value(&input).unwrap();
    let report = input.evaluate(&empty_catalog()).unwrap();
    close(report.layer_resistance_m2_pa_w, 5.);
    close(report.total_resistance_m2_pa_w, 10.);
    close(report.pressure_difference_pa, 1000.);
    close(report.potential_heat_flux_w_m2, 100.);
    close(report.potential_heat_flow_w, 50.);
    close(report.potential_mass_flux_kg_m2_s, 0.00004);
    close(report.potential_mass_flow_kg_s, 0.00002);
    close(report.inside_film_pressure_drop_pa, 100.);
    close(report.outside_film_pressure_drop_pa, 400.);
    for (layer, inner, outer, drop) in [
        (&report.layers[0], 1900., 1700., 200.),
        (&report.layers[1], 1700., 1400., 300.),
    ] {
        close(layer.inside_pressure_pa, inner);
        close(layer.outside_pressure_pa, outer);
        close(layer.pressure_drop_pa, drop);
        close(
            layer.inside_pressure_pa - layer.outside_pressure_pa,
            layer.pressure_drop_pa,
        );
        close(
            layer.pressure_drop_pa / layer.observation.value,
            report.potential_heat_flux_w_m2,
        );
    }
    close(
        report.inside_film_pressure_drop_pa
            + report.outside_film_pressure_drop_pa
            + report
                .layers
                .iter()
                .map(|l| l.pressure_drop_pa)
                .sum::<f64>(),
        report.pressure_difference_pa,
    );
    assert!(!report.physical_validation_claim);
    assert!(!report.mixed_protocols);
    assert_eq!(report.effective_model, MODEL);
    assert_eq!(serde_json::to_value(&input).unwrap(), before);

    let mut larger = input.clone();
    larger.area_m2 *= 3.;
    larger.latent_heat_j_kg *= 2.;
    let scaled = larger.evaluate(&empty_catalog()).unwrap();
    close(
        scaled.potential_heat_flux_w_m2,
        report.potential_heat_flux_w_m2,
    );
    close(
        scaled.potential_heat_flow_w,
        3. * report.potential_heat_flow_w,
    );
    close(
        scaled.potential_mass_flux_kg_m2_s,
        0.5 * report.potential_mass_flux_kg_m2_s,
    );
    close(
        scaled.potential_mass_flow_kg_s,
        1.5 * report.potential_mass_flow_kg_s,
    );

    let mut reversed = input;
    reversed.layers.reverse();
    let reordered = reversed.evaluate(&empty_catalog()).unwrap();
    close(
        reordered.potential_heat_flux_w_m2,
        report.potential_heat_flux_w_m2,
    );
    close(reordered.layers[0].outside_pressure_pa, 1600.);
    close(reordered.layers[1].inside_pressure_pa, 1600.);

    // A dominant material must not erase the small downstream film pressure by cancellation.
    let mut steep = request();
    steep.layers = vec![Layer {
        key: "dominant".into(),
        selection: synthetic_selection("dominant", 1e14),
    }];
    steep.inside_vapor_pressure_pa = 1000.;
    steep.outside_vapor_pressure_pa = 0.;
    let report = steep.evaluate(&empty_catalog()).unwrap();
    close(report.layers[0].outside_pressure_pa, 4000. / (1e14 + 5.));
    close(
        report.layers[0].inside_pressure_pa,
        1000. - 1000. / (1e14 + 5.),
    );
    close(
        report.layers[0].pressure_drop_pa
            + report.inside_film_pressure_drop_pa
            + report.outside_film_pressure_drop_pa,
        1000.,
    );
}

#[test]
fn constituent_ret_cannot_be_promoted_to_a_sheet_but_characterized_fill_is_supported() {
    for form in [Form::Fiber, Form::Yarn] {
        let mut input = request();
        input.layers[0].selection.record.as_mut().unwrap().form = form;
        let refusal = input.evaluate(&empty_catalog()).unwrap_err();
        assert_eq!(refusal.code, RefusalCode::InvalidDocument);
        assert!(refusal.message.contains("fiber/yarn"));
    }
    for form in [
        Form::Sheet,
        Form::Woven,
        Form::Knit,
        Form::Nonwoven,
        Form::Hide,
        Form::Fill,
        Form::Laminate,
    ] {
        let mut input = request();
        input.layers[0].selection.record.as_mut().unwrap().form = form;
        let result = input.evaluate(&empty_catalog()).unwrap();
        close(result.potential_heat_flow_w, 50.);
        let frozen: Request = serde_json::from_str(&result.replay_request_json).unwrap();
        assert_eq!(
            frozen.layers[0].selection.record.as_ref().unwrap().form,
            form
        );
        close(
            frozen
                .evaluate(&empty_catalog())
                .unwrap()
                .potential_heat_flow_w,
            50.,
        );
    }
}

#[test]
fn built_in_measured_leather_changes_transport_without_inventing_other_capabilities() {
    // Kopitar et al. (2022), Table 5: Cr neck 11.27 and Syn-D-H neck 30.38 m²·Pa/W.
    // Scenario pressures, films, area and latent heat below are explicit synthetic inputs.
    let db = Catalog::builtin().unwrap();
    let mut input = request();
    input.layers.truncate(1);
    input.required_conditions = [
        ("air_temperature_c".into(), "35".into()),
        ("relative_humidity_percent".into(), "40".into()),
        ("air_speed_m_s".into(), "1".into()),
    ]
    .into();
    let mut fluxes = Vec::new();
    for (slug, ret) in [("cr-neck", 11.27), ("syn-d-h-neck", 30.38)] {
        input.layers[0].selection = Selection {
            id: format!("zagreb/leather-2022/{slug}"),
            ..Selection::default()
        };
        let report = input.evaluate(&db).unwrap();
        close(report.potential_heat_flux_w_m2, 1000. / (ret + 5.));
        let layer = &report.layers[0];
        assert_eq!(layer.observation.evidence, Evidence::Measured);
        assert!(layer.observation.method.contains("ISO 11092:2014"));
        assert_eq!(layer.observation.conditions["air_temperature_c"], "35");
        assert_eq!(
            layer.observation.conditions["relative_humidity_percent"],
            "40"
        );
        assert_eq!(layer.observation.conditions["air_speed_m_s"], "1");
        assert!(layer.observation.conditions.contains_key("specimen"));
        assert!(layer.observation.conditions.contains_key("conditioning"));
        assert!(
            layer
                .observation
                .conditions
                .contains_key("reported_standard_deviation")
        );
        assert!(layer.source.reference.contains("10.1177/15280837221077048"));
        let frozen = empty_catalog().resolve(&layer.selection).unwrap();
        assert_eq!(frozen.sha256(), layer.material_sha256);
        assert!(frozen.mechanics().is_err());
        assert!(ThermalLaw::from_material(frozen.material(), false).is_err());
        assert!(!frozen.material().properties.contains_key("specific_heat"));
        let replay: Request = serde_json::from_str(&report.replay_request_json).unwrap();
        let offline = replay.evaluate(&empty_catalog()).unwrap();
        assert_eq!(
            serde_json::to_value(&offline.layers).unwrap(),
            serde_json::to_value(&report.layers).unwrap()
        );
        assert_eq!(
            offline.potential_heat_flux_w_m2.to_bits(),
            report.potential_heat_flux_w_m2.to_bits()
        );
        assert_eq!(
            serde_json::to_value(&report).unwrap(),
            serde_json::to_value(input.evaluate(&db).unwrap()).unwrap()
        );
        fluxes.push(report.potential_heat_flux_w_m2);
    }
    assert!(fluxes[0] > fluxes[1]);
}

#[test]
fn protocols_need_explicit_admission_and_filters_are_never_bypassed() {
    let db = empty_catalog();
    let mut input = request();
    let observation = input.layers[1]
        .selection
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("vapor_resistance")
        .unwrap();
    observation
        .conditions
        .insert("conditioning".into(), "different acquired state".into());
    observation.method = "different published method".into();
    assert!(
        input
            .evaluate(&db)
            .unwrap_err()
            .message
            .contains("allow_mixed_protocols")
    );
    input.allow_mixed_protocols = true;
    let admitted = input.evaluate(&db).unwrap();
    assert!(admitted.mixed_protocols);
    assert_eq!(
        admitted.layers[1].observation.method,
        "different published method"
    );
    input.required_conditions.insert(
        "conditioning".into(),
        "synthetic isothermal reference".into(),
    );
    assert!(
        input
            .evaluate(&db)
            .unwrap_err()
            .message
            .contains("method/conditions")
    );
    input.required_conditions.clear();
    input.required_method = Some("synthetic fixed resistance".into());
    assert!(
        input
            .evaluate(&db)
            .unwrap_err()
            .message
            .contains("method/conditions")
    );
}

#[test]
fn missing_wrong_unit_estimated_nonpositive_or_unsourced_resistance_refuses() {
    let db = empty_catalog();
    let mut input = request();
    input.layers[0].selection.allow_estimates = false;
    assert!(
        input
            .evaluate(&db)
            .unwrap_err()
            .message
            .contains("estimate")
    );
    let mut input = request();
    input.layers[0]
        .selection
        .record
        .as_mut()
        .unwrap()
        .properties
        .remove("vapor_resistance");
    assert!(
        input
            .evaluate(&db)
            .unwrap_err()
            .message
            .contains("missing textile property")
    );
    for value in [0., -1., f64::INFINITY, f64::NAN] {
        let mut input = request();
        input.layers[0]
            .selection
            .record
            .as_mut()
            .unwrap()
            .properties
            .get_mut("vapor_resistance")
            .unwrap()
            .value = value;
        assert!(input.evaluate(&db).is_err());
    }
    for (field, value) in [
        ("unit", "g/(m2*day)"),
        ("source", "missing"),
        ("method", ""),
    ] {
        let mut input = serde_json::to_value(request()).unwrap();
        input["layers"][0]["selection"]["record"]["properties"]["vapor_resistance"][field] =
            json!(value);
        assert!(
            serde_json::from_value::<Request>(input)
                .unwrap()
                .evaluate(&db)
                .is_err()
        );
    }
    let mut input = request();
    input.layers[0]
        .selection
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("vapor_resistance")
        .unwrap()
        .bounds = Some([0., 3.]);
    assert!(input.evaluate(&db).is_err());
}

#[test]
fn frozen_selection_overrides_preserve_sources_and_digest_locks() {
    let db = empty_catalog();
    let mut input = request();
    let mut override_q = input.layers[0]
        .selection
        .record
        .as_ref()
        .unwrap()
        .properties["vapor_resistance"]
        .clone();
    override_q.value = 6.;
    input.layers[0]
        .selection
        .overrides
        .insert("vapor_resistance".into(), override_q);
    let report = input.evaluate(&db).unwrap();
    close(report.total_resistance_m2_pa_w, 14.);
    let mut replay: Request = serde_json::from_str(&report.replay_request_json).unwrap();
    close(
        replay.evaluate(&db).unwrap().potential_heat_flux_w_m2,
        1000. / 14.,
    );
    replay.layers[0]
        .selection
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("vapor_resistance")
        .unwrap()
        .value = 7.;
    assert!(
        replay
            .evaluate(&db)
            .unwrap_err()
            .message
            .contains("expected_sha256")
    );
}

#[test]
fn equal_pressures_have_zero_potential_without_inventing_water_or_dry_heat() {
    let mut input = request();
    input.inside_vapor_pressure_pa = input.outside_vapor_pressure_pa;
    let report = input.evaluate(&empty_catalog()).unwrap();
    assert_eq!(report.potential_heat_flux_w_m2, 0.);
    assert_eq!(report.potential_mass_flow_kg_s, 0.);
    for layer in report.layers {
        assert_eq!(layer.inside_pressure_pa, input.outside_vapor_pressure_pa);
        assert_eq!(layer.outside_pressure_pa, input.outside_vapor_pressure_pa);
        assert_eq!(layer.pressure_drop_pa, 0.);
    }
}

#[test]
fn invalid_boundaries_budget_and_unsupported_fields_refuse() {
    let db = empty_catalog();
    for field in [
        "area_m2",
        "latent_heat_j_kg",
        "inside_film_resistance_m2_pa_w",
        "outside_film_resistance_m2_pa_w",
    ] {
        for value in [0., -1.] {
            let mut input = serde_json::to_value(request()).unwrap();
            input[field] = json!(value);
            assert!(
                serde_json::from_value::<Request>(input)
                    .unwrap()
                    .evaluate(&db)
                    .is_err()
            );
        }
    }
    for pressures in [
        [999., 1000.],
        [-1., -2.],
        [f64::NAN, 0.],
        [1000., f64::INFINITY],
    ] {
        let mut input = request();
        [
            input.inside_vapor_pressure_pa,
            input.outside_vapor_pressure_pa,
        ] = pressures;
        assert!(input.evaluate(&db).is_err());
    }
    let mut input = request();
    input.area_m2 = f64::INFINITY;
    assert!(input.evaluate(&db).is_err());
    input = request();
    input.layers[1].key = input.layers[0].key.clone();
    assert!(input.evaluate(&db).is_err());
    input.layers = vec![input.layers[0].clone(); 129];
    assert_eq!(
        input.evaluate(&db).unwrap_err().code,
        RefusalCode::BudgetExceeded
    );
    input.layers.clear();
    assert!(input.evaluate(&db).is_err());
    let mut invalid = serde_json::to_value(request()).unwrap();
    invalid["sweat_rate_kg_s"] = json!(0.1);
    assert!(serde_json::from_value::<Request>(invalid).is_err());
    let mut invalid = serde_json::to_value(request()).unwrap();
    invalid["temperature_c"] = json!(10.);
    assert!(serde_json::from_value::<Request>(invalid).is_err());
}

#[test]
fn arithmetic_overflow_underflow_and_lost_resistance_do_not_become_results() {
    let db = empty_catalog();
    for (area, latent) in [(f64::MAX, 2_500_000.), (f64::MIN_POSITIVE, f64::MAX)] {
        let mut input = request();
        input.area_m2 = area;
        input.latent_heat_j_kg = latent;
        assert!(input.evaluate(&db).is_err());
    }
    let mut input = request();
    input.layers = vec![Layer {
        key: "large".into(),
        selection: synthetic_selection("large", f64::MAX),
    }];
    input.inside_film_resistance_m2_pa_w = f64::MAX;
    assert!(input.evaluate(&db).is_err());
    input.inside_film_resistance_m2_pa_w = 1.;
    assert!(input.evaluate(&db).is_err()); // Positive outer film cannot disappear in the sum.
}

//! Bounded CLI adapter for the shared dry thermal solver. All material evidence policies survive.

use ftx_core::Refusal;
use ftx_drape::thermal::request::{self, Request as Input};
use ftx_pattern::{material_catalog::Selection, textile_import::catalog};
use serde_json::Value;

use super::{invalid, read};

pub(super) fn point(id: &str, temperature: &str, allow_estimates: bool) -> Result<Value, Refusal> {
    let t: f64 = temperature
        .parse()
        .map_err(|_| invalid("temperature must be in degrees Celsius"))?;
    request::point(
        &Selection {
            id: id.into(),
            allow_estimates,
            ..Selection::default()
        },
        t,
        &catalog()?,
    )
}

fn evaluate(input: Input) -> Result<Value, Refusal> {
    input.evaluate(&catalog()?)
}

pub(super) fn run(path: &str) -> Result<Value, Refusal> {
    evaluate(read(path)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ftx_drape::thermal::{Boundary, DryThermalModel};
    use serde_json::json;

    fn fixture() -> Input {
        serde_json::from_str(include_str!(
            "../../../../../tests/fixtures/materials/synthetic-thermal.json"
        ))
        .unwrap()
    }

    fn exterior_fixture(emissivity: f64) -> Input {
        let mut input: Value = serde_json::from_str(include_str!(
            "../../../../../tests/fixtures/materials/synthetic-exterior-exchange.json"
        ))
        .unwrap();
        input["schema"] = json!("ftx-thermal-transient/1");
        input["cells_per_layer"] = json!(2);
        input["initial_temperature_c"] = json!(10.);
        input["step_seconds"] = json!(0.5);
        input["steps"] = json!(4);
        input["record_every"] = json!(1);
        input["layers"][0]["record"]["properties"]["emissivity"]["value"] = json!(emissivity);
        for (key, value, unit) in [
            ("areal_density", 0.2, "kg/m2"),
            ("specific_heat", 1000., "J/(kg*K)"),
        ] {
            input["layers"][0]["record"]["properties"][key] = json!({"value":value,
                "unit":unit,"evidence":"estimate","source":"synthetic","method":"Synthetic storage",
                "conditions":{"specimen":"synthetic-coupon","conditioning":"synthetic-dry"}});
        }
        serde_json::from_value(input).unwrap()
    }

    #[test]
    fn trace_uses_shared_snapshots_and_area_scaled_energy() {
        let out = evaluate(fixture()).unwrap();
        assert_eq!(out["comfort_claim"], false);
        assert!(out.get("exterior_exchange").is_none());
        assert!(out["trace"][4].get("exterior_exchange").is_none());
        assert_eq!(out["trace"].as_array().unwrap().len(), 5);
        assert_eq!(out["trace"][4]["time_seconds"], 20.0);
        let stored = out["stored_energy_change_j"].as_f64().unwrap();
        let incoming = out["incoming_energy_j"].as_f64().unwrap();
        let outgoing = out["outgoing_energy_j"].as_f64().unwrap();
        assert!((stored - incoming + outgoing).abs() < 1e-8);
        assert_eq!(
            out["model"]["layers"][0]["authored"]["id"],
            "synthetic-insulation"
        );
        let mut bigger = fixture();
        bigger.area_m2 *= 2.0;
        let bigger = evaluate(bigger).unwrap();
        assert_eq!(
            out["trace"][4]["temperature_c"],
            bigger["trace"][4]["temperature_c"]
        );
        assert!((bigger["stored_energy_change_j"].as_f64().unwrap() - 2.0 * stored).abs() < 1e-8);
    }

    #[test]
    fn invalid_budgets_and_missing_evidence_refuse() {
        for mutate in [
            (|i: &mut Input| i.steps = 0) as fn(&mut Input),
            |i| i.steps = 100_001,
            |i| i.record_every = 0,
            |i| i.cells_per_layer = usize::MAX,
            |i| i.area_m2 = f64::NAN,
            |i| i.initial_temperature_c = -274.0,
            |i| i.step_seconds = 0.0,
            |i| i.layers[0].allow_estimates = false,
        ] {
            let mut i = fixture();
            mutate(&mut i);
            assert!(evaluate(i).is_err());
        }
    }

    #[test]
    fn temperature_dependent_fixture_runs_through_the_actual_cli_adapter() {
        let input = || {
            serde_json::from_str::<Input>(include_str!(
                "../../../../../tests/fixtures/materials/synthetic-variable-thermal.json"
            ))
            .unwrap()
        };
        let out = evaluate(input()).unwrap();
        assert_eq!(out["model"]["schema"], "ftx-dry-thermal-model/2");
        assert_eq!(out["model"]["temperature_domain_c"], json!([0.0, 60.0]));
        assert_eq!(out["trace"].as_array().unwrap().len(), 5);
        assert!(
            out["accuracy_note"]
                .as_str()
                .unwrap()
                .contains("No extrapolation")
        );
        let stored = out["stored_energy_change_j"].as_f64().unwrap();
        let incoming = out["incoming_energy_j"].as_f64().unwrap();
        let outgoing = out["outgoing_energy_j"].as_f64().unwrap();
        assert!((stored - incoming + outgoing).abs() < 1e-6);
        let mut invalid = input();
        invalid.outside_c = -10.0;
        assert!(evaluate(invalid).is_err());
    }

    #[test]
    fn exterior_trace_matches_core_and_retains_signed_heat_and_source_conditions() {
        let input = exterior_fixture(0.8);
        let db = catalog().unwrap();
        let model = DryThermalModel::new(
            input
                .layers
                .iter()
                .map(|s| db.resolve(s).unwrap())
                .collect(),
            input.interface_resistance_m2_k_w.clone(),
            input.cells_per_layer,
        )
        .unwrap();
        let b = Boundary {
            inside_c: input.inside_c,
            outside_c: input.outside_c,
            inside_resistance: input.inside_resistance_m2_k_w,
            outside_resistance: input.outside_resistance_m2_k_w,
        };
        let exterior = input.exterior_exchange.as_ref().unwrap();
        let mut temperatures = vec![input.initial_temperature_c; model.cells()];
        let (mut convective, mut radiative) = (0., 0.);
        let out = evaluate(exterior_fixture(0.8)).unwrap();
        for i in 1..=input.steps {
            let next = model
                .advance_exterior(&temperatures, input.step_seconds, b, exterior)
                .unwrap();
            temperatures = next.state.temperature_c;
            convective += input.area_m2 * input.step_seconds * next.exchange.convective_flux_w_m2;
            radiative += input.area_m2 * input.step_seconds * next.exchange.radiative_flux_w_m2;
            assert_eq!(out["trace"][i]["temperature_c"], json!(temperatures));
            assert_eq!(
                out["trace"][i]["exterior_exchange"]["field"],
                json!(next.exchange)
            );
        }
        let exchange = &out["exterior_exchange"];
        assert_eq!(exchange["conditions"], json!(exterior));
        assert_eq!(
            exchange["steady"],
            json!(model.steady_exterior(b, exterior).unwrap().exchange)
        );
        assert!((exchange["convective_energy_j"].as_f64().unwrap() - convective).abs() < 1e-12);
        assert!((exchange["radiative_energy_j"].as_f64().unwrap() - radiative).abs() < 1e-12);
        assert!((out["outgoing_energy_j"].as_f64().unwrap() - convective - radiative).abs() < 1e-7);

        let reflective = evaluate(exterior_fixture(0.05)).unwrap();
        assert_eq!(
            exchange["conditions"],
            reflective["exterior_exchange"]["conditions"]
        );
        assert_ne!(out["model"], reflective["model"]);
        assert_ne!(
            out["trace"][4]["temperature_c"],
            reflective["trace"][4]["temperature_c"]
        );
    }

    #[test]
    fn exterior_adapter_never_falls_back_when_boundary_or_optical_evidence_refuses() {
        let mut input = exterior_fixture(0.8);
        input.outside_resistance_m2_k_w = 0.2;
        assert!(evaluate(input).is_err());
        input = exterior_fixture(0.8);
        input.outside_c += 1.;
        assert!(evaluate(input).is_err());
        input = exterior_fixture(0.8);
        input.exterior_exchange.as_mut().unwrap().allow_estimates = false;
        assert!(evaluate(input).is_err());
        input = exterior_fixture(0.8);
        input.layers[0].allow_estimates = false;
        assert!(evaluate(input).is_err());
    }
}

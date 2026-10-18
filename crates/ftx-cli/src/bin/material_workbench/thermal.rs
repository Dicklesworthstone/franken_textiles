//! Bounded CLI adapter for the shared dry thermal solver. All material evidence policies survive.

use ftx_core::Refusal;
use ftx_drape::thermal::{Boundary, DryThermalModel, steady::exterior::Exterior};
use ftx_pattern::{material_catalog::Selection, textile_import::catalog};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{invalid, read};

pub(super) fn point(id: &str, temperature: &str, allow_estimates: bool) -> Result<Value, Refusal> {
    let t: f64 = temperature
        .parse()
        .map_err(|_| invalid("temperature must be in degrees Celsius"))?;
    let snapshot = catalog()?.resolve(&Selection {
        id: id.into(),
        allow_estimates,
        ..Selection::default()
    })?;
    let law = ftx_pattern::material_thermal::ThermalLaw::from_material(
        snapshot.material(),
        snapshot.allows_estimates(),
    )?;
    let resistance = law.resistance_at(t)?;
    let capacity = law.capacity_at(t)?;
    Ok(json!({"schema":"ftx-thermal-point/1", "temperature_c":t,
        "temperature_domain_c":law.domain(), "resistance_m2_k_w":resistance,
        "areal_heat_capacity_j_m2_k":capacity, "selection":snapshot.frozen_selection(),
        "material":snapshot, "model":"effective dry sheet at the stated material temperature",
        "uncertainty_propagated":false, "comfort_claim":false, "empirical_validation_claim":false}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    schema: String,
    layers: Vec<Selection>,
    interface_resistance_m2_k_w: Vec<f64>,
    cells_per_layer: usize,
    area_m2: f64,
    inside_c: f64,
    outside_c: f64,
    inside_resistance_m2_k_w: f64,
    outside_resistance_m2_k_w: f64,
    #[serde(default)]
    exterior_exchange: Option<Exterior>,
    initial_temperature_c: f64,
    step_seconds: f64,
    steps: usize,
    record_every: usize,
}

fn evaluate(input: Input) -> Result<Value, Refusal> {
    let cells = input.layers.len().checked_mul(input.cells_per_layer);
    if input.schema != "ftx-thermal-transient/1"
        || !input.area_m2.is_finite()
        || input.area_m2 <= 0.0
        || input.steps == 0
        || input.steps > 100_000
        || input.record_every == 0
        || cells
            .and_then(|n| n.checked_mul(input.steps))
            .is_none_or(|n| n > 10_000_000)
        || !input.step_seconds.is_finite()
        || input.step_seconds <= 0.0
        || !(input.steps as f64 * input.step_seconds).is_finite()
    {
        return Err(invalid(
            "invalid thermal schema, area, time or integration budget (at most 10 million cell steps)",
        ));
    }
    let frames = input.steps / input.record_every + 2;
    if cells
        .and_then(|n| n.checked_mul(frames))
        .is_none_or(|n| n > 1_000_000)
    {
        return Err(invalid(
            "thermal trace exceeds one million temperature values; increase record_every",
        ));
    }
    let db = catalog()?;
    let layers = input
        .layers
        .iter()
        .map(|s| db.resolve(s))
        .collect::<Result<Vec<_>, _>>()?;
    let model = DryThermalModel::new(
        layers,
        input.interface_resistance_m2_k_w,
        input.cells_per_layer,
    )?;
    let b = Boundary {
        inside_c: input.inside_c,
        outside_c: input.outside_c,
        inside_resistance: input.inside_resistance_m2_k_w,
        outside_resistance: input.outside_resistance_m2_k_w,
    };
    let (steady, steady_exchange) = match &input.exterior_exchange {
        Some(e) => {
            let solved = model.steady_exterior(b, e)?;
            (solved.state, Some(solved.exchange))
        }
        None => (model.steady(b)?, None),
    };
    let mut temperatures = vec![input.initial_temperature_c; model.cells()];
    let mut trace = vec![json!({"time_seconds":0.0,"temperature_c":temperatures})];
    let mut stored = 0.0;
    let mut incoming = 0.0;
    let mut outgoing = 0.0;
    let mut convective = 0.0;
    let mut radiative = 0.0;
    let mut max_balance_error = 0.0_f64;
    for i in 1..=input.steps {
        let (state, exchange) = match &input.exterior_exchange {
            Some(e) => {
                let next = model.advance_exterior(&temperatures, input.step_seconds, b, e)?;
                (next.state, Some(next.exchange))
            }
            None => (model.advance(&temperatures, input.step_seconds, b)?, None),
        };
        stored += input.area_m2 * state.stored_change_j_m2;
        incoming += input.area_m2 * input.step_seconds * state.inside_flux_w_m2;
        outgoing += input.area_m2 * input.step_seconds * state.outside_flux_w_m2;
        max_balance_error = max_balance_error.max(input.area_m2 * state.balance_error_j_m2.abs());
        let inside_w = input.area_m2 * state.inside_flux_w_m2;
        let outside_w = input.area_m2 * state.outside_flux_w_m2;
        let (convective_w, radiative_w) = match &exchange {
            Some(e) => (
                input.area_m2 * e.convective_flux_w_m2,
                input.area_m2 * e.radiative_flux_w_m2,
            ),
            None => (0., 0.),
        };
        convective += input.step_seconds * convective_w;
        radiative += input.step_seconds * radiative_w;
        if [
            stored,
            incoming,
            outgoing,
            max_balance_error,
            inside_w,
            outside_w,
            convective_w,
            radiative_w,
            convective,
            radiative,
        ]
        .iter()
        .any(|v| !v.is_finite())
        {
            return Err(invalid(
                "thermal area-scaled heat or accumulated energy overflow",
            ));
        }
        temperatures = state.temperature_c;
        if i % input.record_every == 0 || i == input.steps {
            let mut sample = json!({"time_seconds":i as f64*input.step_seconds,"temperature_c":temperatures,
                "inside_heat_w":inside_w,"outside_heat_w":outside_w,
                "stored_energy_change_j":stored});
            if let Some(e) = exchange {
                sample["exterior_exchange"] = json!({"field":e,
                    "convective_heat_w":convective_w,"radiative_heat_w":radiative_w,
                    "convective_energy_j":convective,"radiative_energy_j":radiative});
            }
            trace.push(sample);
        }
    }
    let steady_w = input.area_m2 * steady.inside_flux_w_m2;
    if !steady_w.is_finite() {
        return Err(invalid("steady area-scaled heat overflow"));
    }
    let accuracy_note = if model.temperature_domain().is_some() {
        "Condition-qualified temperature interpolation or published fit; enthalpy-conservative time stepping. No extrapolation. Refine cells_per_layer and step_seconds to check discretization error; this is not garment validation."
    } else {
        "Constant effective layer properties; refine cells_per_layer and step_seconds to check discretization error."
    };
    let mut result = json!({"schema":"ftx-thermal-transient-report/1","model":model.snapshot(),
        "area_m2":input.area_m2,"step_seconds":input.step_seconds,"steps":input.steps,
        "record_every":input.record_every,"inside_c":input.inside_c,"outside_c":input.outside_c,
        "inside_resistance_m2_k_w":input.inside_resistance_m2_k_w,
        "outside_resistance_m2_k_w":input.outside_resistance_m2_k_w,
        "trace":trace,"steady_temperature_c":steady.temperature_c,"steady_heat_w":steady_w,
        "incoming_energy_j":incoming,"outgoing_energy_j":outgoing,"stored_energy_change_j":stored,
        "largest_step_energy_balance_error_j":max_balance_error,
        "comfort_claim":false,"empirical_validation_claim":false,
        "accuracy_note":accuracy_note});
    if let Some(e) = input.exterior_exchange {
        let steady_exchange = steady_exchange.expect("exterior branch solved");
        let convective_w = input.area_m2 * steady_exchange.convective_flux_w_m2;
        let radiative_w = input.area_m2 * steady_exchange.radiative_flux_w_m2;
        if !convective_w.is_finite() || !radiative_w.is_finite() {
            return Err(invalid("steady area-scaled exterior heat overflow"));
        }
        result["exterior_exchange"] = json!({"conditions":e,"steady":steady_exchange,
            "steady_convective_heat_w":convective_w,"steady_radiative_heat_w":radiative_w,
            "convective_energy_j":convective,"radiative_energy_j":radiative});
        result["exterior_model"] = json!(
            "prescribed convection; optional source-qualified opaque-gray large-isothermal-enclosure radiation; no wind correlation or interlayer radiation"
        );
    }
    Ok(result)
}

pub(super) fn run(path: &str) -> Result<Value, Refusal> {
    evaluate(read(path)?)
}

#[cfg(test)]
mod tests {
    use super::*;

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

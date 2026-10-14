//! Bounded CLI adapter for the shared dry thermal solver. All material evidence policies survive.

use ftx_core::Refusal;
use ftx_drape::thermal::{Boundary, DryThermalModel};
use ftx_pattern::{material_catalog::Selection, textile_import::catalog};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{invalid, read};

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
    let steady = model.steady(b)?;
    let mut temperatures = vec![input.initial_temperature_c; model.cells()];
    let mut trace = vec![json!({"time_seconds":0.0,"temperature_c":temperatures})];
    let mut stored = 0.0;
    let mut incoming = 0.0;
    let mut outgoing = 0.0;
    let mut max_balance_error = 0.0_f64;
    for i in 1..=input.steps {
        let state = model.advance(&temperatures, input.step_seconds, b)?;
        stored += input.area_m2 * state.stored_change_j_m2;
        incoming += input.area_m2 * input.step_seconds * state.inside_flux_w_m2;
        outgoing += input.area_m2 * input.step_seconds * state.outside_flux_w_m2;
        max_balance_error = max_balance_error.max(input.area_m2 * state.balance_error_j_m2.abs());
        let inside_w = input.area_m2 * state.inside_flux_w_m2;
        let outside_w = input.area_m2 * state.outside_flux_w_m2;
        if [
            stored,
            incoming,
            outgoing,
            max_balance_error,
            inside_w,
            outside_w,
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
            trace.push(
                json!({"time_seconds":i as f64*input.step_seconds,"temperature_c":temperatures,
                "inside_heat_w":inside_w,"outside_heat_w":outside_w,
                "stored_energy_change_j":stored}),
            );
        }
    }
    let steady_w = input.area_m2 * steady.inside_flux_w_m2;
    if !steady_w.is_finite() {
        return Err(invalid("steady area-scaled heat overflow"));
    }
    Ok(
        json!({"schema":"ftx-thermal-transient-report/1","model":model.snapshot(),
        "area_m2":input.area_m2,"step_seconds":input.step_seconds,"steps":input.steps,
        "record_every":input.record_every,"inside_c":input.inside_c,"outside_c":input.outside_c,
        "inside_resistance_m2_k_w":input.inside_resistance_m2_k_w,
        "outside_resistance_m2_k_w":input.outside_resistance_m2_k_w,
        "trace":trace,"steady_temperature_c":steady.temperature_c,"steady_heat_w":steady_w,
        "incoming_energy_j":incoming,"outgoing_energy_j":outgoing,"stored_energy_change_j":stored,
        "largest_step_energy_balance_error_j":max_balance_error,
        "comfort_claim":false,"empirical_validation_claim":false,
        "accuracy_note":"Constant effective layer properties; refine cells_per_layer and step_seconds to check discretization error."}),
    )
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

    #[test]
    fn trace_uses_shared_snapshots_and_area_scaled_energy() {
        let out = evaluate(fixture()).unwrap();
        assert_eq!(out["comfort_claim"], false);
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
}

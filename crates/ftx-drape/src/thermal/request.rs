//! File-free material point and dry transient requests shared by native and browser consumers.

use super::{Boundary, DryThermalModel, invalid, steady::exterior::Exterior};
use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::{material_catalog::Selection, materials::Catalog};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Evaluate one source-qualified material at a stated temperature without extrapolation.
/// An embedded, locked selection can be replayed against an empty catalog.
///
/// # Errors
/// Unknown or changed material, missing/unadmitted transport or storage evidence, invalid
/// temperature, an empty common characterization domain, or any material-law refusal.
pub fn point(
    selection: &Selection,
    temperature_c: f64,
    catalog: &Catalog,
) -> Result<Value, Refusal> {
    let snapshot = catalog.resolve(selection)?;
    let law = ftx_pattern::material_thermal::ThermalLaw::from_material(
        snapshot.material(),
        snapshot.allows_estimates(),
    )?;
    let resistance = law.resistance_at(temperature_c)?;
    let capacity = law.capacity_at(temperature_c)?;
    Ok(
        json!({"schema":"ftx-thermal-point/1", "temperature_c":temperature_c,
        "temperature_domain_c":law.domain(), "resistance_m2_k_w":resistance,
        "areal_heat_capacity_j_m2_k":capacity, "selection":snapshot.frozen_selection(),
        "material":snapshot, "model":"effective dry sheet at the stated material temperature",
        "uncertainty_propagated":false, "comfort_claim":false, "empirical_validation_claim":false}),
    )
}

/// A fully declared dry layer experiment with the existing `ftx-thermal-transient/1` contract.
/// Material selections and exterior observations retain their individual evidence policies.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Must be `ftx-thermal-transient/1`.
    pub schema: String,
    /// Source-bearing selections, ordered from the inside reservoir to the outside reservoir.
    pub layers: Vec<Selection>,
    /// One explicit contact resistance per adjacent layer pair, in m² K/W.
    pub interface_resistance_m2_k_w: Vec<f64>,
    /// Equal finite volumes per layer; the complete stack is limited to 4096 cells.
    pub cells_per_layer: usize,
    /// Positive common conducting area in m², not inferred wearer coverage.
    pub area_m2: f64,
    /// Inside reservoir temperature in degrees Celsius.
    pub inside_c: f64,
    /// Outside reservoir temperature in degrees Celsius.
    pub outside_c: f64,
    /// Inside film resistance in m² K/W; zero prescribes a surface temperature.
    pub inside_resistance_m2_k_w: f64,
    /// Outside film resistance in m² K/W; zero prescribes a surface temperature.
    pub outside_resistance_m2_k_w: f64,
    /// Optional qualified exterior convection/radiation, replacing the outside film.
    #[serde(default)]
    pub exterior_exchange: Option<Exterior>,
    /// Uniform initial material temperature in degrees Celsius.
    pub initial_temperature_c: f64,
    /// Positive time step in seconds; temporal convergence remains the caller's responsibility.
    pub step_seconds: f64,
    /// Number of steps, at most 100,000 and at most ten million total cell steps.
    pub steps: usize,
    /// Trace interval in steps; the final state is always included.
    pub record_every: usize,
}

impl Request {
    /// Resolve the supplied catalog and advance the shared dry solver atomically.
    /// Frozen selections can replay without a built-in catalog. Signed boundary energy and
    /// integrated material enthalpy are retained; a numerical residual is not physical validation.
    /// `replay_request_json` is the exact source-frozen request text: clients should save that string
    /// unchanged rather than reconstructing hash-bearing selections through another JSON runtime.
    ///
    /// # Errors
    /// Invalid schema, input, work/trace budget, changed sources, missing evidence or consent,
    /// unsupported exterior exchange, temperature extrapolation, failed solve or arithmetic
    /// overflow. No partial history is returned after a failed step.
    pub fn evaluate(self, catalog: &Catalog) -> Result<Value, Refusal> {
        let mut input = self;
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
        // Reject an impossible model before expanding source-bearing records for every layer.
        // This is the same admission rule as DryThermalModel::new, including its refusal text.
        if input.layers.is_empty()
            || input.cells_per_layer == 0
            || cells.is_none_or(|n| n > 4096)
            || input.interface_resistance_m2_k_w.len() != input.layers.len() - 1
        {
            return Err(invalid(
                "thermal stack needs layers, 1..4096 cells and exactly one resistance per interface",
            ));
        }
        let layers = input
            .layers
            .iter()
            .map(|s| catalog.resolve(s))
            .collect::<Result<Vec<_>, _>>()?;
        input.layers = layers
            .iter()
            .map(|layer| layer.frozen_selection())
            .collect();
        let replay_request_json =
            serde_json::to_string(&input).map_err(|e| invalid(e.to_string()))?;
        if replay_request_json.len() > 64 * 1024 * 1024 {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "frozen thermal replay request exceeds the 64 MiB input limit",
            ));
        }
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
            max_balance_error =
                max_balance_error.max(input.area_m2 * state.balance_error_j_m2.abs());
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
            "replay_request_json":replay_request_json,
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
}

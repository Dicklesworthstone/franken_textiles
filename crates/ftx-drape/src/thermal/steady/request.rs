//! Portable, clock-free workbench request shared by native and browser entry points.

use super::{Boundary, MAX_LAYERS, SteadyConductionModel, invalid, positive};
use ftx_core::{Refusal, receipt::sha256_hex};
use ftx_pattern::{material_catalog::Selection, materials::Catalog};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// A fully declared equal-area steady experiment; unknown fields or implicit interfaces refuse.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Must be `ftx-thermal-steady/1`.
    pub schema: String,
    /// Source-bearing material selections, in inside-to-outside order.
    pub layers: Vec<Selection>,
    /// One explicitly declared contact resistance per adjacent pair, in m² K/W.
    pub interface_resistance_m2_k_w: Vec<f64>,
    /// Common, positive conducting area in m²; not inferred wearer coverage.
    pub area_m2: f64,
    /// Inside reservoir temperature, degrees Celsius.
    pub inside_c: f64,
    /// Outside reservoir temperature, degrees Celsius.
    pub outside_c: f64,
    /// Inside film resistance in m² K/W; zero prescribes the material face temperature.
    pub inside_resistance_m2_k_w: f64,
    /// Outside film resistance in m² K/W; zero prescribes the material face temperature.
    pub outside_resistance_m2_k_w: f64,
}

impl Request {
    /// Resolve the shared catalog, solve all layers atomically and retain a runnable frozen request.
    /// An embedded, digest-locked selection replays even with an empty catalog. Output residuals
    /// measure the numerical equations only, not experimental uncertainty or physical accuracy.
    ///
    /// # Errors
    /// Wrong schema/budget, invalid area/boundaries, failed material resolution, unavailable or
    /// out-of-domain transport, unconverged equations, or nonrepresentable total power.
    pub fn evaluate(&self, catalog: &Catalog) -> Result<Value, Refusal> {
        if self.schema != "ftx-thermal-steady/1"
            || self.layers.is_empty()
            || self.layers.len() > MAX_LAYERS
            || self.interface_resistance_m2_k_w.len() != self.layers.len() - 1
        {
            return Err(invalid(
                "invalid steady schema, layer budget or interface count",
            ));
        }
        positive("steady conducting area", self.area_m2)?;
        let layers = self
            .layers
            .iter()
            .map(|s| catalog.resolve(s))
            .collect::<Result<_, _>>()?;
        let model = SteadyConductionModel::new(layers, self.interface_resistance_m2_k_w.clone())?;
        let state = model.solve(Boundary {
            inside_c: self.inside_c,
            outside_c: self.outside_c,
            inside_resistance: self.inside_resistance_m2_k_w,
            outside_resistance: self.outside_resistance_m2_k_w,
        })?;
        let power = self.area_m2 * state.flux_w_m2;
        if !power.is_finite() || (state.flux_w_m2 != 0.0 && power == 0.0) {
            return Err(invalid("steady total power overflow or underflow"));
        }
        let mut frozen = self.clone();
        frozen.layers = model.layers.iter().map(|m| m.frozen_selection()).collect();
        let request = serde_json::to_value(&frozen).map_err(|e| invalid(e.to_string()))?;
        let encoded = serde_json::to_vec(&request).map_err(|e| invalid(e.to_string()))?;
        Ok(
            json!({"schema":"ftx-thermal-steady-result/1", "request":request,
            "request_sha256":sha256_hex(&encoded), "state":state, "power_w":power,
            "temperature_domains_c":model.laws.iter().map(|law| law.domain()).collect::<Vec<_>>(),
            "model":"integrated steady 1D Fourier conduction; fixed geometry and conditioning; equal area",
            "storage_required":false, "uncertainty_propagated":false,
            "comfort_claim":false, "empirical_validation_claim":false,
            "not_simulated":["time-dependent heat storage","moisture transport","evaporation",
                "human thermoregulation","wind-dependent films","compression-dependent loft",
                "inferred air gaps","radiative exchange","thermal bridges"]}),
        )
    }
}

//! Portable, clock-free workbench request shared by native and browser entry points.

use super::exterior::Exterior;
use super::{Boundary, MAX_LAYERS, SteadyConductionModel, invalid, positive};
use crate::thermal::microclimate::MicroclimateExterior;
use ftx_core::{Refusal, receipt::sha256_hex};
use ftx_pattern::{material_catalog::Selection, materials::Catalog};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Explicit reservoirs and films, not inferred skin temperatures or weather conditions.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Film {
    /// Inside reservoir temperature in degrees Celsius.
    pub inside_c: f64,
    /// Outside reservoir temperature in degrees Celsius.
    pub outside_c: f64,
    /// Inside film resistance in m² K/W; zero prescribes the material face temperature.
    pub inside_resistance_m2_k_w: f64,
    /// Outside film resistance in m² K/W; zero prescribes the material face temperature.
    pub outside_resistance_m2_k_w: f64,
}

impl Film {
    fn boundary(self) -> Boundary {
        Boundary {
            inside_c: self.inside_c,
            outside_c: self.outside_c,
            inside_resistance: self.inside_resistance_m2_k_w,
            outside_resistance: self.outside_resistance_m2_k_w,
        }
    }
}

/// Steady conditions on retained drape triangles, independent of transient storage readiness.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceConditions {
    /// Must be `ftx-steady-surface-conditions/1`.
    pub schema: String,
    /// One boundary broadcasts uniformly; otherwise exactly one per frozen material column.
    pub boundaries: Vec<Film>,
    /// Required for multiple boundaries so substitution cannot silently reorder their meaning.
    /// Optional for a uniform boundary; frozen replay always binds the exact model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_sha256: Option<String>,
    /// Optional replacement of the outside film. One entry broadcasts, otherwise one per column
    /// with a bound model identity. Emissivity is read from each column's own outermost material.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exterior_exchange: Option<Vec<Exterior>>,
    /// Optional validity-gated moist-air Nusselt correlation replacement of the outside film
    /// (`Evidence::Estimate`; mutually exclusive with `exterior_exchange`). One entry broadcasts,
    /// otherwise one per column with a bound model identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub microclimate_exterior: Option<Vec<MicroclimateExterior>>,
}

impl SurfaceConditions {
    /// Calculate local face temperatures and signed patch powers on saved solver geometry.
    /// Restores frozen observations, evidence policy and owners without a catalog lookup.
    ///
    /// # Errors
    /// Unsupported schema/boundaries, unavailable or changed drape, stale column assignments,
    /// invalid geometry, out-of-domain transport or any exact steady solver refusal.
    pub fn evaluate(&self, report: &Value) -> Result<Value, Refusal> {
        if self.schema != "ftx-steady-surface-conditions/1"
            || self.boundaries.is_empty()
            || self.boundaries.len() > 65_536
            || self
                .exterior_exchange
                .as_ref()
                .is_some_and(|e| e.is_empty() || e.len() > 65_536)
            || self
                .microclimate_exterior
                .as_ref()
                .is_some_and(|e| e.is_empty() || e.len() > 65_536)
        {
            return Err(invalid("invalid steady surface schema or boundary count"));
        }
        if self.exterior_exchange.is_some() && self.microclimate_exterior.is_some() {
            return Err(invalid(
                "supply either exterior_exchange or microclimate_exterior, not both",
            ));
        }
        let (surface, positions) = super::surface::SteadySurface::from_drape_report(report)?;
        let frame = &report["steady_thermal_surface"];
        let model_sha256 = frame["model"]["sha256"]
            .as_str()
            .ok_or_else(|| invalid("missing steady model identity"))?;
        if self
            .model_sha256
            .as_deref()
            .is_some_and(|s| s != model_sha256)
            || ((self.boundaries.len() != 1
                || self
                    .exterior_exchange
                    .as_ref()
                    .is_some_and(|e| e.len() != 1)
                || self
                    .microclimate_exterior
                    .as_ref()
                    .is_some_and(|e| e.len() != 1))
                && self.model_sha256.is_none())
        {
            return Err(invalid(
                "per-column boundaries require the exact model_sha256; use one uniform boundary for substitution",
            ));
        }
        let boundaries = if self.boundaries.len() == 1 {
            vec![self.boundaries[0].boundary(); surface.column_count()]
        } else if self.boundaries.len() == surface.column_count() {
            self.boundaries.iter().map(|b| b.boundary()).collect()
        } else {
            return Err(invalid("one steady boundary per column is required"));
        };
        let (solution, exterior, microclimate) = if let Some(e) = &self.exterior_exchange {
            let e = if e.len() == 1 {
                vec![e[0].clone(); surface.column_count()]
            } else if e.len() == surface.column_count() {
                e.clone()
            } else {
                return Err(invalid("one exterior environment per column is required"));
            };
            let solved = surface.solve_exterior(&positions, &boundaries, &e)?;
            let details = json!({"columns":solved.columns,
                "convective_power_w":solved.convective_power_w,
                "radiative_power_w":solved.radiative_power_w});
            (solved.steady, Some(details), None)
        } else if let Some(mc) = &self.microclimate_exterior {
            if mc.len() != 1 && mc.len() != surface.column_count() {
                return Err(invalid("one exterior environment per column is required"));
            }
            let resolved = mc
                .iter()
                .map(MicroclimateExterior::resolve)
                .collect::<Result<Vec<_>, _>>()?;
            let e = if resolved.len() == 1 {
                vec![resolved[0].exterior.clone(); surface.column_count()]
            } else {
                resolved.iter().map(|r| r.exterior.clone()).collect()
            };
            let receipts: Vec<_> = resolved.iter().map(|r| r.receipt.clone()).collect();
            // Partial never rounds up: only omit "wind-dependent films" when every column uses forced convection.
            let forced = resolved.iter().all(|r| r.forced_convection);
            let mut solved = surface.solve_exterior(&positions, &boundaries, &e)?;
            solved
                .steady
                .exclude_reported_bounds(MICROCLIMATE_BOUNDS_EXCLUSION);
            for column in &mut solved.columns {
                column
                    .conduction
                    .exclude_reported_bounds(MICROCLIMATE_BOUNDS_EXCLUSION);
            }
            let details = json!({"columns":solved.columns,
                "convective_power_w":solved.convective_power_w,
                "radiative_power_w":solved.radiative_power_w});
            (
                solved.steady,
                Some(details),
                Some((json!(receipts), forced)),
            )
        } else {
            (surface.solve(&positions, &boundaries)?, None, None)
        };
        let mut conditions = self.clone();
        conditions.model_sha256 = Some(model_sha256.into());
        // Keep only the source/geometry inputs needed for replay, not unrelated wearer metadata.
        let request = json!({"report":{"material":report["material"],
            "steady_thermal_surface":frame},"conditions":conditions});
        let request_sha256 =
            sha256_hex(&serde_json::to_vec(&request).map_err(|e| invalid(e.to_string()))?);
        let mut result = json!({"schema":"ftx-steady-surface-result/1",
            "drape_steady_frame_sha256":frame["sha256"],"model_sha256":model_sha256,
            "request":request,"request_sha256":request_sha256,
            "positions_m":positions,"patch_topology":frame["model"]["patches"],
            "triangle_instance":frame["triangle_instance"],"instances":frame["instances"],
            "steady":solution,"storage_required":false,"uncertainty_propagated":false,
            "comfort_claim":false,"empirical_validation_claim":false,
            "scope":"independent fixed-characterization cloth columns; not whole-wearer heat loss",
            "not_simulated":["transient heat storage","inferred body coverage","overlapping-sheet coupling",
                "inferred air gaps","compression-dependent loft","moisture transport","evaporation",
                "wind-dependent films","radiative exchange","thermal bridges","human thermoregulation"]});
        if let Some(details) = exterior {
            // Partial never rounds up: only omit "radiative exchange" when every column enables radiation.
            let enabled = self
                .exterior_exchange
                .as_ref()
                .is_some_and(|v| !v.is_empty() && v.iter().all(|e| e.radiation.is_some()))
                || self
                    .microclimate_exterior
                    .as_ref()
                    .is_some_and(|v| !v.is_empty() && v.iter().all(|e| e.radiation.is_some()));
            describe_exterior(&mut result, details, enabled, microclimate);
        }
        Ok(result)
    }
}

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
    /// Optional qualified convection/radiation replacement of the outside film.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exterior_exchange: Option<Exterior>,
    /// Optional validity-gated moist-air Nusselt correlation replacement of the outside film
    /// (`Evidence::Estimate`; mutually exclusive with `exterior_exchange`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub microclimate_exterior: Option<MicroclimateExterior>,
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
        if self.exterior_exchange.is_some() && self.microclimate_exterior.is_some() {
            return Err(invalid(
                "supply either exterior_exchange or microclimate_exterior, not both",
            ));
        }
        positive("steady conducting area", self.area_m2)?;
        let layers = self
            .layers
            .iter()
            .map(|s| catalog.resolve(s))
            .collect::<Result<_, _>>()?;
        let model = SteadyConductionModel::new(layers, self.interface_resistance_m2_k_w.clone())?;
        let boundary = Boundary {
            inside_c: self.inside_c,
            outside_c: self.outside_c,
            inside_resistance: self.inside_resistance_m2_k_w,
            outside_resistance: self.outside_resistance_m2_k_w,
        };
        let (exterior, microclimate) = if let Some(e) = &self.exterior_exchange {
            (Some(model.solve_exterior(boundary, e)?), None)
        } else if let Some(mc) = &self.microclimate_exterior {
            let resolved = mc.resolve()?;
            let mut solved = model.solve_exterior(boundary, &resolved.exterior)?;
            solved
                .conduction
                .exclude_reported_bounds(MICROCLIMATE_BOUNDS_EXCLUSION);
            (
                Some(solved),
                Some((resolved.receipt, resolved.forced_convection)),
            )
        } else {
            (None, None)
        };
        let state = match &exterior {
            Some(e) => e.conduction.clone(),
            None => model.solve(boundary)?,
        };
        let power = self.area_m2 * state.flux_w_m2;
        if !power.is_finite() || (state.flux_w_m2 != 0.0 && power == 0.0) {
            return Err(invalid("steady total power overflow or underflow"));
        }
        let mut frozen = self.clone();
        frozen.layers = model.layers.iter().map(|m| m.frozen_selection()).collect();
        let request = serde_json::to_value(&frozen).map_err(|e| invalid(e.to_string()))?;
        let encoded = serde_json::to_vec(&request).map_err(|e| invalid(e.to_string()))?;
        let mut result = json!({"schema":"ftx-thermal-steady-result/1", "request":request,
            "request_sha256":sha256_hex(&encoded), "state":state, "power_w":power,
            "temperature_domains_c":model.laws.iter().map(|law| law.domain()).collect::<Vec<_>>(),
            "model":"integrated steady 1D Fourier conduction; fixed geometry and conditioning; equal area",
            "storage_required":false, "uncertainty_propagated":false,
            "comfort_claim":false, "empirical_validation_claim":false,
            "not_simulated":["time-dependent heat storage","moisture transport","evaporation",
                "human thermoregulation","wind-dependent films","compression-dependent loft",
                "inferred air gaps","radiative exchange","thermal bridges"]});
        let ranges = state.power_range(self.area_m2);
        result["reported_bounds"] =
            serde_json::to_value(ranges.reported_bounds).map_err(|e| invalid(e.to_string()))?;
        result["bounds_unavailable"] = json!(ranges.bounds_unavailable);
        if let Some(e) = exterior {
            let enabled = e.radiation_enabled;
            describe_exterior(
                &mut result,
                serde_json::to_value(e).map_err(|e| invalid(e.to_string()))?,
                enabled,
                microclimate,
            );
        }
        Ok(result)
    }
}

pub(super) const MICROCLIMATE_BOUNDS_EXCLUSION: &str = "source-reported conduction ranges do not cover correlation-derived microclimate convection or its model-form discrepancy; nominal heat remains available";

pub(super) fn describe_exterior(
    result: &mut Value,
    details: Value,
    radiation_enabled: bool,
    microclimate: Option<(Value, bool)>,
) {
    result["exterior_exchange"] = details;
    let limits = result["not_simulated"]
        .as_array_mut()
        .expect("authored list");
    if radiation_enabled {
        limits.retain(|v| v != "radiative exchange");
    }
    if let Some((receipt, forced_convection)) = microclimate {
        if forced_convection {
            limits.retain(|v| v != "wind-dependent films");
        }
        limits.extend([
            json!("empirical garment-film / wind-tunnel validation"),
            json!("interlayer or finite-view-factor radiation"),
            json!("solar/spectral irradiation"),
        ]);
        result["microclimate_convection"] = receipt;
        result["exterior_model"] = json!(
            "validity-gated moist-air Nusselt correlation convection (Evidence::Estimate; model-form discrepancy allowance retained); optional source-qualified opaque-gray large-isothermal-enclosure radiation; not a measured garment film"
        );
    } else {
        limits.extend([
            json!("interlayer or finite-view-factor radiation"),
            json!("solar/spectral irradiation"),
        ]);
        result["exterior_model"] = json!(
            "prescribed convection; optional source-qualified opaque-gray large-isothermal-enclosure radiation; no wind correlation"
        );
    }
}

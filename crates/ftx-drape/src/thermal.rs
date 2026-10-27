//! Dry one-dimensional transient heat transport through source-bearing textile layers.
//!
//! A finite-volume RC model, not a comfort, weather, loft-compression or moisture model. Each layer's
//! effective properties are constant or follow its condition-qualified temperature observations.
//! Every material comes from the same immutable selection used by mechanics and purchasing.

use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::material_catalog::ResolvedMaterial;
use ftx_pattern::materials::Form;
use serde_json::{Value, json};

pub mod adaptive;
pub mod compression;
pub mod exterior;
mod heating;
mod nonlinear;
pub mod request;
pub mod steady;
pub mod surface;
pub mod vapor;

/// Prescribed temperatures and film resistances. A zero film means a prescribed surface temperature,
/// not a guessed convective coefficient. Resistance is per unit area, in m² K/W.
#[derive(Clone, Copy, Debug)]
pub struct Boundary {
    /// Inner reservoir/surface temperature in degrees Celsius; not a thermoregulating human model.
    pub inside_c: f64,
    /// Outer reservoir/surface temperature in degrees Celsius.
    pub outside_c: f64,
    /// Inner film resistance in m² K/W.
    pub inside_resistance: f64,
    /// Outer film resistance in m² K/W.
    pub outside_resistance: f64,
}

/// A solved state. Positive flux is from the inner side toward the outer side.
#[derive(Clone, Debug)]
pub struct State {
    /// Cell-centre temperatures, in layer order from inside to outside.
    pub temperature_c: Vec<f64>,
    /// Flux entering the stack at its inner boundary, in W/m².
    pub inside_flux_w_m2: f64,
    /// Flux leaving the stack at its outer boundary, in W/m².
    pub outside_flux_w_m2: f64,
    /// Change in stored sensible heat during the step, in J/m²; zero for a steady-state evaluation.
    pub stored_change_j_m2: f64,
    /// Stored change minus the step-integrated net boundary heat, in J/m².
    pub balance_error_j_m2: f64,
}

/// Immutable material and discretization inputs. Refining space/time remains the caller's accuracy
/// responsibility; numerical stability is not an accuracy or experimental-validation claim.
#[derive(Clone, Debug)]
pub struct DryThermalModel {
    layers: Vec<ResolvedMaterial>,
    interfaces: Vec<f64>,
    cells_per_layer: usize,
    resistance: Vec<f64>,
    capacity: Vec<f64>,
    joins: Vec<f64>,
    variable: Option<nonlinear::Variable>,
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

fn positive(name: &str, value: f64) -> Result<f64, Refusal> {
    if !value.is_finite() || value <= 0.0 {
        return Err(invalid(format!("{name} must be finite and positive")));
    }
    Ok(value)
}

fn nonnegative(name: &str, value: f64) -> Result<f64, Refusal> {
    if !value.is_finite() || value < 0.0 {
        return Err(invalid(format!("{name} must be finite and nonnegative")));
    }
    Ok(value)
}

fn temperature(value: f64) -> Result<(), Refusal> {
    if !value.is_finite() || value < -273.15 {
        return Err(invalid(
            "temperature must be finite and not below absolute zero",
        ));
    }
    Ok(())
}

impl DryThermalModel {
    // Re-express this column per unit COVERED BODY area, without modifying a single material
    // observation. For layer i: R' = R * A_body/A_sheet_current, C' = C * A_sheet_ref/A_body.
    // Interfaces and films already refer to covered-body area. Only the coverage adapter uses this
    // temporary solve object; its public receipt retains original materials and explicit mappings.
    fn with_area_factors(&self, resistance: &[f64], capacity: &[f64]) -> Result<Self, Refusal> {
        if resistance.len() != self.layers.len() || capacity.len() != self.layers.len() {
            return Err(invalid("one geometry factor is required per thermal layer"));
        }
        for &value in resistance.iter().chain(capacity) {
            positive("thermal area factor", value)?;
        }
        let mut model = self.clone();
        if let Some(variable) = &mut model.variable {
            variable.scale_areas(resistance, capacity);
        } else {
            for i in 0..self.cells() {
                let layer = i / self.cells_per_layer;
                model.resistance[i] = positive(
                    "scaled cell resistance",
                    self.resistance[i] * resistance[layer],
                )?;
                model.capacity[i] =
                    positive("scaled cell capacity", self.capacity[i] * capacity[layer])?;
            }
            for i in 1..self.cells() {
                let interface = if i % self.cells_per_layer == 0 {
                    self.interfaces[i / self.cells_per_layer - 1]
                } else {
                    0.0
                };
                let path = positive(
                    "scaled thermal path",
                    model.resistance[i - 1] / 2.0 + model.resistance[i] / 2.0 + interface,
                )?;
                model.joins[i - 1] = positive("scaled conductance", 1.0 / path)?;
            }
        }
        Ok(model)
    }

    /// Build a layer stack with equal cells within each layer. Interfaces are declared thermal
    /// contact/air-gap resistances, not inferred from drape, names, nominal loft or blend fractions.
    /// Fill is admitted only when it has separately characterized effective sheet R, mass and heat
    /// capacity. Fiber/yarn records are not distributed sheet layers.
    ///
    /// # Errors
    /// Missing properties or evidence consent, invalid dimensions, non-finite coefficients, or more
    /// than 4096 cells. Every layer needs resistance, areal density and specific heat; no defaults.
    pub fn new(
        layers: Vec<ResolvedMaterial>,
        interfaces: Vec<f64>,
        cells_per_layer: usize,
    ) -> Result<Self, Refusal> {
        let n = layers.len().checked_mul(cells_per_layer);
        if layers.is_empty()
            || cells_per_layer == 0
            || n.is_none_or(|n| n > 4096)
            || interfaces.len() != layers.len() - 1
        {
            return Err(invalid(
                "thermal stack needs layers, 1..4096 cells and exactly one resistance per interface",
            ));
        }
        for &r in &interfaces {
            nonnegative("interface resistance", r)?;
        }
        for layer in &layers {
            layer.material().require_dry_mass_transport()?;
        }
        if layers
            .iter()
            .any(|m| m.material().thermal_response.is_some())
        {
            let variable = nonlinear::Variable::new(&layers, &interfaces, cells_per_layer)?;
            return Ok(Self {
                layers,
                interfaces,
                cells_per_layer,
                variable: Some(variable),
                resistance: Vec::new(),
                capacity: Vec::new(),
                joins: Vec::new(),
            });
        }
        let mut resistance = Vec::new();
        let mut capacity = Vec::new();
        for layer in &layers {
            let material = layer.material();
            if matches!(material.form, Form::Fiber | Form::Yarn) {
                return Err(invalid(
                    "a fiber/yarn record is not a characterized thermal sheet",
                ));
            }
            let r = positive("layer thermal resistance", layer.thermal_resistance()?)?;
            let mass = positive(
                "areal density",
                material.require("areal_density", "kg/m2", layer.allows_estimates())?,
            )?;
            let specific = positive(
                "specific heat",
                material.require("specific_heat", "J/(kg*K)", layer.allows_estimates())?,
            )?;
            let c = positive("areal heat capacity", mass * specific)?;
            let r = positive("cell resistance", r / cells_per_layer as f64)?;
            let c = positive("cell heat capacity", c / cells_per_layer as f64)?;
            resistance.extend(std::iter::repeat_n(r, cells_per_layer));
            capacity.extend(std::iter::repeat_n(c, cells_per_layer));
        }
        let mut joins = Vec::new();
        for i in 1..resistance.len() {
            let interface = if i % cells_per_layer == 0 {
                interfaces[i / cells_per_layer - 1]
            } else {
                0.0
            };
            let r = positive(
                "internal thermal path",
                resistance[i - 1] / 2.0 + resistance[i] / 2.0 + interface,
            )?;
            joins.push(positive("internal conductance", 1.0 / r)?);
        }
        Ok(Self {
            layers,
            interfaces,
            cells_per_layer,
            resistance,
            capacity,
            joins,
            variable: None,
        })
    }

    /// Number of cell-centre temperatures required by [`Self::advance`].
    #[must_use]
    pub fn cells(&self) -> usize {
        self.layers.len() * self.cells_per_layer
    }

    /// Common characterized temperature interval for state AND reservoirs, or None for legacy
    /// constants. This deliberately conservative scope never extrapolates a constituent curve.
    #[must_use]
    pub fn temperature_domain(&self) -> Option<[f64; 2]> {
        self.variable.as_ref().map(nonlinear::Variable::domain)
    }

    /// Validate an initial material temperature without performing a step.
    ///
    /// # Errors
    /// Non-finite temperature, below absolute zero, or outside the common characterized interval.
    pub fn check_temperature(&self, value: f64) -> Result<(), Refusal> {
        match &self.variable {
            Some(law) => law.check(value),
            None => temperature(value),
        }
    }

    /// Source-complete model description. Serialize outside the numerical time-stepping loop.
    #[must_use]
    pub fn snapshot(&self) -> Value {
        if let Some(variable) = &self.variable {
            let polynomial = self.layers.iter().any(|m| {
                m.material().thermal_response.as_ref().is_some_and(|r| {
                    [&r.conductivity, &r.specific_heat]
                        .into_iter()
                        .flatten()
                        .any(|s| s.polynomial.is_some())
                })
            });
            let storage = if polynomial {
                "integrated source-qualified specific heat (tabulated or polynomial) times reference mass"
            } else {
                "integrated piecewise-linear specific heat times reference mass"
            };
            return json!({"schema":"ftx-dry-thermal-model/2", "layers":self.layers,
                "interface_resistance_m2_k_w":self.interfaces,"cells_per_layer":self.cells_per_layer,
                "temperature_domain_c":variable.domain(),
                "model":"temperature-dependent dry 1D finite volume; enthalpy backward Euler",
                "transport":"cell-centre conductivity, harmonic half-cell paths; no extrapolation",
                "storage":storage,
                "solver":{"iteration_limit":nonlinear::ITERATIONS,"damping":0.5,
                    "acceptance":"local nonlinear residual and global energy balance"},
                "not_simulated":["human thermoregulation","moisture","evaporation","wind-dependent films",
                    "compression-dependent loft","draped air-gap geometry","radiative exchange","thermal bridges"]});
        }
        json!({"schema":"ftx-dry-thermal-model/1", "layers":self.layers,
            "interface_resistance_m2_k_w":self.interfaces,"cells_per_layer":self.cells_per_layer,
            "cell_resistance_m2_k_w":self.resistance,"cell_capacity_j_m2_k":self.capacity,
            "model":"constant-property dry 1D finite volume; implicit Euler",
            "not_simulated":["human thermoregulation","moisture","evaporation","wind-dependent films",
                "compression-dependent loft","draped air-gap geometry","radiative exchange","thermal bridges"]})
    }

    fn conductances(&self, b: Boundary) -> Result<Vec<f64>, Refusal> {
        temperature(b.inside_c)?;
        temperature(b.outside_c)?;
        nonnegative("inside film resistance", b.inside_resistance)?;
        nonnegative("outside film resistance", b.outside_resistance)?;
        let mut g = Vec::with_capacity(self.cells() + 1);
        let ri = positive(
            "inner thermal path",
            b.inside_resistance + self.resistance[0] / 2.0,
        )?;
        let ro = positive(
            "outer thermal path",
            b.outside_resistance + self.resistance[self.cells() - 1] / 2.0,
        )?;
        g.push(positive("inner conductance", 1.0 / ri)?);
        g.extend(&self.joins);
        g.push(positive("outer conductance", 1.0 / ro)?);
        Ok(g)
    }

    /// Steady solution of this discrete network; half-cell resistances locate cell centres. Constant
    /// properties use a direct solution; temperature curves require converged nonlinear residuals.
    ///
    /// # Errors
    /// Invalid boundary conditions or overflow. This still requires heat-capacity characterization,
    /// because this model is prepared for transients; use [`steady::SteadyConductionModel`] for
    /// transport-only data, including temperature-dependent conductivity without heat capacity.
    pub fn steady(&self, boundary: Boundary) -> Result<State, Refusal> {
        if let Some(variable) = &self.variable {
            return variable.solve(None, 1.0, boundary);
        }
        let g = self.conductances(boundary)?;
        let r = positive("stack resistance", g.iter().map(|v| 1.0 / v).sum())?;
        let flux = (boundary.inside_c - boundary.outside_c) / r;
        if !flux.is_finite() {
            return Err(invalid("steady thermal flux overflow"));
        }
        let mut traversed = 0.0;
        let mut temperature_c = Vec::new();
        for conductance in g.iter().take(self.cells()) {
            traversed += 1.0 / conductance;
            let t = boundary.inside_c - flux * traversed;
            temperature(t)?;
            temperature_c.push(t);
        }
        Ok(State {
            temperature_c,
            inside_flux_w_m2: flux,
            outside_flux_w_m2: flux,
            stored_change_j_m2: 0.0,
            balance_error_j_m2: 0.0,
        })
    }

    /// One conservative backward-Euler step. The caller's state is untouched on failure. Boundary
    /// temperatures/films may change between steps. Source observations remain fixed; their bounded
    /// temperature laws update resistance and integrated sensible heat automatically.
    ///
    /// # Errors
    /// Wrong state size, invalid time/temperature, coefficient overflow or failed energy balance.
    pub fn advance(&self, old: &[f64], seconds: f64, boundary: Boundary) -> Result<State, Refusal> {
        if let Some(variable) = &self.variable {
            return variable.solve(Some(old), seconds, boundary);
        }
        positive("thermal time step", seconds)?;
        if old.len() != self.cells() {
            return Err(invalid("thermal state has the wrong cell count"));
        }
        for &t in old {
            temperature(t)?;
        }
        let g = self.conductances(boundary)?;
        let n = self.cells();
        let mut diag = Vec::with_capacity(n);
        let mut rhs = Vec::with_capacity(n);
        // Solve for increments to avoid subtracting two large C*T/dt terms in nearly steady states.
        for i in 0..n {
            let left = if i == 0 {
                boundary.inside_c
            } else {
                old[i - 1]
            };
            let right = if i + 1 == n {
                boundary.outside_c
            } else {
                old[i + 1]
            };
            diag.push(positive(
                "thermal diagonal",
                self.capacity[i] / seconds + g[i] + g[i + 1],
            )?);
            let force = g[i] * (left - old[i]) + g[i + 1] * (right - old[i]);
            if !force.is_finite() {
                return Err(invalid("thermal load overflow"));
            }
            rhs.push(force);
        }
        // SPD tridiagonal elimination/back substitution, O(cells), deterministic fixed ordering.
        for i in 1..n {
            let ratio = g[i] / diag[i - 1];
            diag[i] = positive("thermal elimination pivot", diag[i] - g[i] * ratio)?;
            rhs[i] += ratio * rhs[i - 1];
        }
        let mut delta = vec![0.0; n];
        for i in (0..n).rev() {
            let right = if i + 1 == n {
                0.0
            } else {
                g[i + 1] * delta[i + 1]
            };
            delta[i] = (rhs[i] + right) / diag[i];
        }
        let mut next = Vec::with_capacity(n);
        let low = old
            .iter()
            .copied()
            .fold(boundary.inside_c.min(boundary.outside_c), f64::min);
        let high = old
            .iter()
            .copied()
            .fold(boundary.inside_c.max(boundary.outside_c), f64::max);
        for i in 0..n {
            let t = old[i] + delta[i];
            temperature(t)?;
            let tol = 128.0 * f64::EPSILON * (1.0 + low.abs() + high.abs());
            if t < low - tol || t > high + tol {
                return Err(invalid("thermal maximum principle failed"));
            }
            next.push(t);
        }
        let inside = g[0] * (boundary.inside_c - next[0]);
        let outside = g[n] * (next[n - 1] - boundary.outside_c);
        let stored: f64 = self
            .capacity
            .iter()
            .enumerate()
            .map(|(i, c)| c * (next[i] - old[i]))
            .sum();
        let net = seconds * (inside - outside);
        let error = stored - net;
        let rounding: f64 = self
            .capacity
            .iter()
            .enumerate()
            .map(|(i, c)| c * (1.0 + next[i].abs() + old[i].abs()))
            .sum();
        let tolerance = 512.0 * f64::EPSILON * rounding + 1e-9 * (1.0 + net.abs());
        if [inside, outside, stored, net, error, rounding, tolerance]
            .iter()
            .any(|x| !x.is_finite())
            || error.abs() > tolerance
        {
            return Err(invalid(
                "thermal energy balance failed or overflowed; review coefficients and time step",
            ));
        }
        Ok(State {
            temperature_c: next,
            inside_flux_w_m2: inside,
            outside_flux_w_m2: outside,
            stored_change_j_m2: stored,
            balance_error_j_m2: error,
        })
    }
}

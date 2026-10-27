//! Prescribed internal energy in the existing conservative thermal step.
//!
//! Adiabatic enthalpy preloading is algebraically the backward-Euler source term:
//! H(next) - H(old) = dt * net_boundary_flux + Q. The temporary heated state also supplies
//! the correct maximum-principle enclosure to the existing conduction and exterior solvers.
//! Every returned storage and residual is recomputed against the original, unmodified state.

use super::exterior::ExteriorStep;
use super::steady::exterior::Exterior;
use super::{Boundary, DryThermalModel, Refusal, State, invalid, nonnegative, positive};

impl DryThermalModel {
    // The explicitly qualified free-liquid consumer adds remaining liquid capacity at the inner
    // cell and supplies signed latent energy. Keep the canonical dry model and its receipt intact.
    pub(in crate::thermal) fn with_inner_liquid_capacity(
        &self,
        capacity: f64,
    ) -> Result<Self, Refusal> {
        nonnegative("remaining liquid capacity", capacity)?;
        let mut model = self.clone();
        if let Some(variable) = &mut model.variable {
            variable.add_inner_liquid_capacity(capacity)?;
            return Ok(model);
        }
        model.capacity[0] = positive("dry plus liquid capacity", model.capacity[0] + capacity)?;
        if capacity > 0. && model.capacity[0] <= self.capacity[0] {
            return Err(invalid(
                "liquid heat capacity is lost in floating-point addition",
            ));
        }
        Ok(model)
    }

    pub(in crate::thermal) fn advance_signed_heat(
        &self,
        old: &[f64],
        seconds: f64,
        boundary: Boundary,
        exterior: Option<&Exterior>,
        heat: &[f64],
    ) -> Result<(State, Option<super::exterior::ExteriorFlux>), Refusal> {
        positive("thermal time step", seconds)?;
        if old.len() != self.cells()
            || heat.len() != self.cells()
            || heat.iter().any(|v| !v.is_finite())
        {
            return Err(invalid(
                "signed liquid heat needs one finite energy and temperature per cell",
            ));
        }
        let heated = old
            .iter()
            .zip(heat)
            .enumerate()
            .map(|(cell, (&t, &q))| {
                self.check_temperature(t)?;
                self.preload_cell(cell, t, q)
            })
            .collect::<Result<Vec<_>, Refusal>>()?;
        let supplied: f64 = heat.iter().sum();
        let (state, exchange) = if let Some(exterior) = exterior {
            let step = self.advance_exterior(&heated, seconds, boundary, exterior)?;
            (step.state, Some(step.exchange))
        } else {
            (self.advance(&heated, seconds, boundary)?, None)
        };
        Ok((
            self.account_heat(old, &heated, seconds, supplied, state)?,
            exchange,
        ))
    }

    /// Advance with one nonnegative internal energy per cell, in J/m² of reference material area.
    /// These are energies delivered during this step, not powers or replacement temperatures.
    /// Their physical origin and distribution must be qualified by the calling consumer.
    /// Integrated material enthalpy is inverted with at most 128 bisections per heated cell.
    /// Zero sources use the original step unchanged. Neither input nor material identity changes.
    ///
    /// # Errors
    /// Wrong cell counts, negative/nonfinite energy, invalid time or material temperatures,
    /// an adiabatic preload outside a characterized domain, or a failed solve/energy balance.
    pub fn advance_with_heat(
        &self,
        old: &[f64],
        seconds: f64,
        boundary: Boundary,
        heat_j_m2: &[f64],
    ) -> Result<State, Refusal> {
        positive("thermal time step", seconds)?;
        let Some((heated, supplied)) = self.prepare_heat(old, heat_j_m2)? else {
            return self.advance(old, seconds, boundary);
        };
        let state = self.advance(&heated, seconds, boundary)?;
        self.account_heat(old, &heated, seconds, supplied, state)
    }

    /// Advance with cell energies and the same source-qualified convection/radiation boundary.
    /// The temporary enthalpy preload extends the face-temperature bracket without extrapolating
    /// material or optical laws. The returned balance includes the exact requested source energy.
    ///
    /// # Errors
    /// As [`Self::advance_with_heat`] and [`Self::advance_exterior`].
    pub fn advance_exterior_with_heat(
        &self,
        old: &[f64],
        seconds: f64,
        boundary: Boundary,
        exterior: &Exterior,
        heat_j_m2: &[f64],
    ) -> Result<ExteriorStep, Refusal> {
        positive("thermal time step", seconds)?;
        let Some((heated, supplied)) = self.prepare_heat(old, heat_j_m2)? else {
            return self.advance_exterior(old, seconds, boundary, exterior);
        };
        let mut step = self.advance_exterior(&heated, seconds, boundary, exterior)?;
        step.state = self.account_heat(old, &heated, seconds, supplied, step.state)?;
        Ok(step)
    }

    pub(in crate::thermal) fn cell_heat(
        &self,
        cell: usize,
        old: f64,
        next: f64,
    ) -> Result<f64, Refusal> {
        match &self.variable {
            Some(variable) => variable.heat(cell, old, next),
            None => {
                let heat = self.capacity[cell] * (next - old);
                if heat.is_finite() {
                    Ok(heat)
                } else {
                    Err(invalid("internal-source sensible heat overflows"))
                }
            }
        }
    }

    pub(in crate::thermal) fn cell_capacity(
        &self,
        cell: usize,
        temperature: f64,
    ) -> Result<f64, Refusal> {
        match &self.variable {
            Some(variable) => variable.capacity(cell, temperature),
            None => Ok(self.capacity[cell]),
        }
    }

    pub(in crate::thermal) fn prepare_heat(
        &self,
        old: &[f64],
        heat: &[f64],
    ) -> Result<Option<(Vec<f64>, f64)>, Refusal> {
        if old.len() != self.cells() || heat.len() != self.cells() {
            return Err(invalid(
                "internal heat needs one energy and temperature per cell",
            ));
        }
        let mut supplied = 0.0;
        for (&temperature, &energy) in old.iter().zip(heat) {
            self.check_temperature(temperature)?;
            supplied += nonnegative("internal cell energy", energy)?;
        }
        if !supplied.is_finite() {
            return Err(invalid("total internal heat overflows"));
        }
        if supplied == 0.0 {
            return Ok(None);
        }
        let heated = old
            .iter()
            .zip(heat)
            .enumerate()
            .map(|(cell, (&temperature, &energy))| self.preload_cell(cell, temperature, energy))
            .collect::<Result<_, _>>()?;
        Ok(Some((heated, supplied)))
    }

    fn preload_cell(&self, cell: usize, old: f64, energy: f64) -> Result<f64, Refusal> {
        if energy == 0.0 {
            return Ok(old);
        }
        let Some(variable) = &self.variable else {
            let heated = old + energy / self.capacity[cell];
            self.check_temperature(heated)?;
            return Ok(heated);
        };
        // Integrated enthalpy is strictly increasing. Cooling uses the lower measured endpoint;
        // neither sign may extrapolate the dry law to manufacture an adiabatic source state.
        let endpoint = variable.domain()[usize::from(energy > 0.0)];
        let available = self.cell_heat(cell, old, endpoint)?;
        if (energy > 0.0 && energy > available) || (energy < 0.0 && energy < available) {
            return Err(invalid(
                "internal heat exceeds the adiabatic enthalpy available inside the characterized temperature domain",
            ));
        }
        if energy == available {
            return Ok(endpoint);
        }
        let mut low = old.min(endpoint);
        let mut high = old.max(endpoint);
        for _ in 0..128 {
            let mid = low / 2.0 + high / 2.0;
            let received = self.cell_heat(cell, old, mid)?;
            let rounding = self
                .cell_capacity(cell, old)?
                .max(self.cell_capacity(cell, mid)?)
                * (1.0 + old.abs() + mid.abs());
            let tolerance = 128.0 * f64::EPSILON * rounding + 1e-12 * (1.0 + energy.abs());
            if !rounding.is_finite() || !tolerance.is_finite() {
                return Err(invalid("internal-heat enthalpy inversion overflows"));
            }
            if (received - energy).abs() <= tolerance {
                self.check_temperature(mid)?;
                return Ok(mid);
            }
            if mid == low || mid == high {
                break;
            }
            if received < energy {
                low = mid;
            } else {
                high = mid;
            }
        }
        Err(invalid(
            "internal-heat enthalpy inverse did not converge within 128 bisections",
        ))
    }

    fn account_heat(
        &self,
        old: &[f64],
        heated: &[f64],
        seconds: f64,
        supplied: f64,
        mut state: State,
    ) -> Result<State, Refusal> {
        let mut stored = 0.0;
        let mut rounding = 0.0;
        for (cell, ((&old, &heated), &next)) in
            old.iter().zip(heated).zip(&state.temperature_c).enumerate()
        {
            stored += self.cell_heat(cell, old, next)?;
            let capacity = self
                .cell_capacity(cell, old)?
                .max(self.cell_capacity(cell, heated)?)
                .max(self.cell_capacity(cell, next)?);
            rounding += capacity * (1.0 + old.abs() + heated.abs() + next.abs());
        }
        let net = seconds * (state.inside_flux_w_m2 - state.outside_flux_w_m2);
        let error = stored - net - supplied;
        let tolerance = 1024.0 * f64::EPSILON * rounding
            + 1e-9 * (1.0 + stored.abs() + net.abs() + supplied.abs());
        if [stored, net, error, rounding, tolerance]
            .iter()
            .any(|value| !value.is_finite())
            || error.abs() > tolerance
        {
            return Err(invalid(
                "thermal energy balance including internal heat failed or overflowed",
            ));
        }
        state.stored_change_j_m2 = stored;
        // Retain preload/inversion roundoff in the public residual; never pretend requested energy
        // was exactly representable by changing the source amount to the realized temperature jump.
        state.balance_error_j_m2 = error;
        Ok(state)
    }
}

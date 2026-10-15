//! Steady transport qualification, independent from transient mass and heat capacity.

use super::{Law, invalid, positive};
use crate::materials::{Form, Material};
use ftx_core::Refusal;

pub(super) fn qualified_scalar(
    material: &Material,
    key: &str,
    unit: &str,
    allow_estimates: bool,
) -> Result<f64, Refusal> {
    let value = positive(material.require(key, unit, allow_estimates)?, key)?;
    if let Some(response) = &material.thermal_response {
        let quantity = &material.properties[key];
        for (field, expected) in [
            ("specimen", &response.specimen),
            ("conditioning", &response.conditioning),
        ] {
            if quantity.conditions.get(field) != Some(expected) {
                return Err(invalid(format!(
                    "{}: {key} must match thermal {field} {expected}",
                    material.id
                )));
            }
        }
    }
    Ok(value)
}

/// Effective through-thickness transport for a specific dry sheet or characterized fill stack.
/// No areal mass or specific heat is needed for steady conduction. Missing storage observations
/// remain missing: this law cannot be used as a transient law or a mechanical material.
#[derive(Clone, Debug)]
pub struct ConductionLaw {
    pub(super) thickness: Option<f64>,
    pub(super) transport: Law,
    pub(super) domain: Option<[f64; 2]>,
}

impl ConductionLaw {
    /// Compile the same sourced transport used by [`super::ThermalLaw`], without inventing storage.
    /// A conductivity table requires matched specimen/conditioning thickness and explicit estimate
    /// consent. Its temperature domain does not inherit an unrelated specific-heat interval.
    ///
    /// # Errors
    /// Invalid record, missing transport/geometry, evidence denial or mismatched conditions.
    pub fn from_material(material: &Material, allow_estimates: bool) -> Result<Self, Refusal> {
        material.validate()?;
        if matches!(material.form, Form::Fiber | Form::Yarn) {
            return Err(invalid(
                "fiber/yarn observations are not a characterized thermal sheet",
            ));
        }
        let scalar = |key, unit| qualified_scalar(material, key, unit, allow_estimates);
        let (thickness, transport) = if let Some(series) = material
            .thermal_response
            .as_ref()
            .and_then(|response| response.conductivity.as_ref())
        {
            (
                Some(scalar("thickness", "m")?),
                Law::Curve(series.compile("W/(m*K)", &material.sources, allow_estimates)?),
            )
        } else if material.properties.contains_key("thermal_resistance") {
            (None, Law::Constant(scalar("thermal_resistance", "m2*K/W")?))
        } else {
            (
                Some(scalar("thickness", "m")?),
                Law::Constant(scalar("thermal_conductivity", "W/(m*K)")?),
            )
        };
        let domain = match &transport {
            Law::Constant(_) => None,
            Law::Curve(curve) => Some(curve.domain()),
        };
        Ok(Self {
            thickness,
            transport,
            domain,
        })
    }

    /// Conductivity validity interval; `None` retains legacy constant-property semantics.
    #[must_use]
    pub fn domain(&self) -> Option<[f64; 2]> {
        self.domain
    }

    /// Check a material temperature, not a reservoir temperature behind a declared film.
    ///
    /// # Errors
    /// Non-finite, below absolute zero or outside the characterized conductivity interval.
    pub fn check_temperature(&self, t: f64) -> Result<(), Refusal> {
        if !t.is_finite() || t < -273.15 || self.domain.is_some_and(|[lo, hi]| t < lo || t > hi) {
            return Err(invalid(format!(
                "temperature {t} outside conduction law validity {:?}",
                self.domain
            )));
        }
        Ok(())
    }

    /// Full-sheet differential resistance in m² K/W at one material temperature.
    ///
    /// # Errors
    /// Out-of-domain temperature or nonphysical/overflowed resistance.
    pub fn resistance_at(&self, t: f64) -> Result<f64, Refusal> {
        self.check_temperature(t)?;
        let value = self.transport.at(t)?;
        positive(
            self.thickness.map_or(value, |thickness| thickness / value),
            "thermal resistance",
        )
    }

    /// Steady Fourier flux in W/m², positive from the first face toward the second.
    /// For conductivity this is `integral(k(T), outside..inside) / thickness`, not conductivity
    /// sampled at the mean temperature. Tabulated and bounded cubic laws use their exact integrals.
    ///
    /// # Errors
    /// Either face outside the interval, non-finite arithmetic or an underflowed nonzero flux.
    pub fn flux_between(&self, inside_c: f64, outside_c: f64) -> Result<f64, Refusal> {
        self.check_temperature(inside_c)?;
        self.check_temperature(outside_c)?;
        let flux = if let Some(thickness) = self.thickness {
            self.transport.integral(outside_c, inside_c)? / thickness
        } else {
            (inside_c - outside_c) / self.transport.at(inside_c)?
        };
        if !flux.is_finite() || (inside_c != outside_c && flux == 0.0) {
            return Err(invalid("conduction flux overflow or underflow"));
        }
        Ok(flux)
    }
}

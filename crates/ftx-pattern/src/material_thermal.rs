//! Condition-qualified thermal observations and conservative, bounded material laws.
//!
//! Tables retain reported means and sample variability. Linear interpolation is a modeling choice,
//! never an additional measurement. Enthalpy integrates that same law, rather than multiplying a
//! temperature change by the heat capacity at one endpoint. No extrapolation or moisture model.

use std::collections::BTreeMap;

use ftx_core::{Refusal, RefusalCode};
use serde::{Deserialize, Serialize};

use crate::materials::{Evidence, Form, Material, Source};

mod polynomial;
pub use polynomial::PolynomialFit;

/// One reported mean; standard deviation is not an uncertainty of the mean or a confidence interval.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThermalObservation {
    /// Specimen temperature in degrees Celsius.
    pub temperature_c: f64,
    /// Reported mean in the containing series' unit.
    pub mean: f64,
    /// Reported sample variability, in the same unit; absent when unreported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub standard_deviation: Option<f64>,
}

/// Observations or one published fit from a single protocol, specimen and conditioning state.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemperatureSeries {
    /// Canonical unit; the original reported values are not replaced by interpolated samples.
    pub unit: String,
    /// Origin of the tabulated observations, independent of the interpolation policy.
    pub evidence: Evidence,
    /// Key in the containing material's source map.
    pub source: String,
    /// Test and aggregation method; identify unreported experimental details explicitly.
    pub method: String,
    /// Test-specific geometry, pressure, direction and other qualifiers; not inferred from a label.
    #[serde(default)]
    pub conditions: BTreeMap<String, String>,
    /// Strictly temperature-ordered observations. Duplicate temperatures are not averaged silently.
    /// Empty only when `polynomial` is supplied; fitted samples never masquerade as observations.
    #[serde(default)]
    pub points: Vec<ThermalObservation>,
    /// Alternative bounded polynomial fit, in this series' canonical unit. Mutually exclusive with
    /// observations, and never classified as `Measured`, even if fitted to measured data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub polynomial: Option<PolynomialFit>,
}

/// Thermal characterization attached to the same material record used by other consumers.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThermalResponse {
    /// Exact characterization/specimen identity, not a fiber family or brand name.
    pub specimen: String,
    /// One conditioning protocol/state. Dried and unconditioned records must remain distinct.
    pub conditioning: String,
    /// Effective through-thickness conductivity in W/(m K), when reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conductivity: Option<TemperatureSeries>,
    /// Specific heat in J/(kg K), when reported. Missing data never borrows another specimen's law.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub specific_heat: Option<TemperatureSeries>,
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

fn positive(value: f64, name: &str) -> Result<f64, Refusal> {
    if !value.is_finite() || value <= 0.0 {
        return Err(invalid(format!("{name} must be finite and positive")));
    }
    Ok(value)
}

impl TemperatureSeries {
    /// Validate observations without admitting an interpolated constitutive law.
    ///
    /// # Errors
    /// Wrong units, missing provenance, nonphysical or unordered data, or more than 4096 samples.
    pub fn validate(&self, unit: &str, sources: &BTreeMap<String, Source>) -> Result<(), Refusal> {
        if self.unit != unit
            || self.method.trim().is_empty()
            || sources.get(&self.source).is_none_or(|source| {
                source.reference.trim().is_empty()
                    || source.accessed_on.trim().is_empty()
                    || source.note.trim().is_empty()
            })
            || self
                .conditions
                .iter()
                .any(|(k, v)| k.trim().is_empty() || v.trim().is_empty())
        {
            return Err(invalid(format!(
                "invalid sourced temperature series ({unit})"
            )));
        }
        if let Some(fit) = &self.polynomial {
            if !self.points.is_empty() || self.evidence == Evidence::Measured {
                return Err(invalid(
                    "a thermal polynomial is a fit, not measured knots; choose exactly one representation",
                ));
            }
            return fit.validate();
        }
        if !(2..=4096).contains(&self.points.len()) {
            return Err(invalid("thermal observations need 2..4096 ordered samples"));
        }
        for point in &self.points {
            if !point.temperature_c.is_finite() || point.temperature_c < -273.15 {
                return Err(invalid("invalid observation temperature"));
            }
            positive(point.mean, "thermal observation mean")?;
            if point
                .standard_deviation
                .is_some_and(|s| !s.is_finite() || s < 0.0)
            {
                return Err(invalid("standard deviation must be finite and nonnegative"));
            }
        }
        for pair in self.points.windows(2) {
            let dt = pair[1].temperature_c - pair[0].temperature_c;
            let slope = (pair[1].mean - pair[0].mean) / dt;
            if !dt.is_finite() || dt <= 0.0 || !slope.is_finite() {
                return Err(invalid(
                    "temperature samples must increase with a finite slope",
                ));
            }
        }
        Ok(())
    }

    /// Compile an immutable bounded law. Both interpolation and polynomial fits require consent.
    ///
    /// # Errors
    /// Invalid observations or missing explicit permission to use an interpolated estimate.
    pub fn compile(
        &self,
        unit: &str,
        sources: &BTreeMap<String, Source>,
        allow_estimates: bool,
    ) -> Result<TemperatureCurve, Refusal> {
        self.validate(unit, sources)?;
        if !allow_estimates {
            return Err(invalid(
                "temperature interpolation or fitting is an estimate; explicit opt-in required",
            ));
        }
        Ok(TemperatureCurve {
            points: self
                .points
                .iter()
                .map(|p| [p.temperature_c, p.mean])
                .collect(),
            polynomial: self.polynomial.clone(),
        })
    }
}

impl ThermalResponse {
    /// Validate independent observations and refuse ambiguous scalar/table representations.
    ///
    /// # Errors
    /// Missing identity/conditioning, empty characterization, invalid series or competing scalars.
    pub fn validate(&self, material: &Material) -> Result<(), Refusal> {
        if self.specimen.trim().is_empty()
            || self.conditioning.trim().is_empty()
            || (self.conductivity.is_none() && self.specific_heat.is_none())
        {
            return Err(invalid(
                "thermal response needs specimen, conditioning and observations",
            ));
        }
        for (series, key, unit) in [
            (&self.conductivity, "thermal_conductivity", "W/(m*K)"),
            (&self.specific_heat, "specific_heat", "J/(kg*K)"),
        ] {
            if let Some(series) = series {
                for (key, expected) in [
                    ("specimen", &self.specimen),
                    ("conditioning", &self.conditioning),
                ] {
                    if series.conditions.get(key).is_some_and(|v| v != expected) {
                        return Err(invalid(format!("thermal series has conflicting {key}")));
                    }
                }
                if material.properties.contains_key(key)
                    || (key == "thermal_conductivity"
                        && material.properties.contains_key("thermal_resistance"))
                {
                    return Err(invalid(format!(
                        "{key}: scalar conflicts with thermal_response"
                    )));
                }
                series.validate(unit, &material.sources)?;
            }
        }
        Ok(())
    }
}

/// Validated positive tabulated or polynomial law, with a closed interval and no extrapolation.
#[derive(Clone, Debug)]
pub struct TemperatureCurve {
    points: Vec<[f64; 2]>,
    polynomial: Option<PolynomialFit>,
}

impl TemperatureCurve {
    /// Inclusive lower and upper characterized temperatures in degrees Celsius.
    #[must_use]
    pub fn domain(&self) -> [f64; 2] {
        if let Some(fit) = &self.polynomial {
            return fit.domain_c;
        }
        [self.points[0][0], self.points[self.points.len() - 1][0]]
    }

    fn check(&self, t: f64) -> Result<(), Refusal> {
        let [lo, hi] = self.domain();
        if !t.is_finite() || t < lo || t > hi {
            return Err(invalid(format!(
                "temperature {t} outside characterized interval [{lo}, {hi}] C"
            )));
        }
        Ok(())
    }

    /// Evaluate the declared interpolant at a temperature inside its characterized interval.
    ///
    /// # Errors
    /// Out-of-domain/non-finite temperature or arithmetic overflow.
    pub fn at(&self, t: f64) -> Result<f64, Refusal> {
        self.check(t)?;
        if let Some(fit) = &self.polynomial {
            return positive(fit.value(t), "fitted thermal value");
        }
        let i = self.points.partition_point(|p| p[0] < t);
        if self.points[i][0] == t {
            return Ok(self.points[i][1]);
        }
        let [a, va] = self.points[i - 1];
        let [b, vb] = self.points[i];
        let w = (t - a) / (b - a);
        positive((1.0 - w) * va + w * vb, "interpolated thermal value")
    }

    /// Signed exact integral of the interpolant between two characterized temperatures.
    /// Segment-local integration avoids subtracting two large absolute enthalpies near equilibrium.
    ///
    /// # Errors
    /// Either endpoint outside the interval, or arithmetic overflow.
    pub fn integral(&self, from: f64, to: f64) -> Result<f64, Refusal> {
        self.check(from)?;
        self.check(to)?;
        if let Some(fit) = &self.polynomial {
            return fit.integral(from, to);
        }
        let lo = from.min(to);
        let hi = from.max(to);
        let mut sum = 0.0;
        for pair in self.points.windows(2) {
            let a = lo.max(pair[0][0]);
            let b = hi.min(pair[1][0]);
            if a < b {
                sum += (b - a) * (self.at(a)? / 2.0 + self.at(b)? / 2.0);
            }
        }
        if !sum.is_finite() {
            return Err(invalid("thermal integral overflow"));
        }
        Ok(if to < from { -sum } else { sum })
    }
}

#[derive(Clone, Debug)]
enum Law {
    Constant(f64),
    Curve(TemperatureCurve),
}

impl Law {
    fn at(&self, t: f64) -> Result<f64, Refusal> {
        match self {
            Self::Constant(value) => Ok(*value),
            Self::Curve(curve) => curve.at(t),
        }
    }
    fn integral(&self, a: f64, b: f64) -> Result<f64, Refusal> {
        match self {
            Self::Constant(value) => Ok(value * (b - a)),
            Self::Curve(curve) => curve.integral(a, b),
        }
    }
}

/// A source-qualified effective dry sheet law; mass always refers to undeformed material area.
#[derive(Clone, Debug)]
pub struct ThermalLaw {
    mass: f64,
    // None means resistance is already measured directly; Some(t) means R(T) = t / k(T).
    thickness: Option<f64>,
    transport: Law,
    specific_heat: Law,
    domain: Option<[f64; 2]>,
}

impl ThermalLaw {
    /// Resolve a complete transient law under the same evidence policy as other material consumers.
    /// Geometry accompanying a table must explicitly name the same specimen and conditioning in
    /// `Quantity.conditions`. This prevents combining dried conductivity with an unrelated mass/loft.
    ///
    /// # Errors
    /// Incomplete/ambiguous properties, evidence denial, mismatched conditions or disjoint domains.
    pub fn from_material(material: &Material, allow_estimates: bool) -> Result<Self, Refusal> {
        material.validate()?;
        if matches!(material.form, Form::Fiber | Form::Yarn) {
            return Err(invalid(
                "fiber/yarn observations are not a characterized thermal sheet",
            ));
        }
        let response = material.thermal_response.as_ref();
        let scalar = |key: &str, unit: &str| -> Result<f64, Refusal> {
            let value = positive(material.require(key, unit, allow_estimates)?, key)?;
            if let Some(response) = response {
                let q = &material.properties[key];
                for (k, expected) in [
                    ("specimen", &response.specimen),
                    ("conditioning", &response.conditioning),
                ] {
                    if q.conditions.get(k) != Some(expected) {
                        return Err(invalid(format!(
                            "{}: {key} must match thermal {k} {expected}",
                            material.id
                        )));
                    }
                }
            }
            Ok(value)
        };
        let mass = scalar("areal_density", "kg/m2")?;
        let (thickness, transport) =
            if let Some(series) = response.and_then(|r| r.conductivity.as_ref()) {
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
        let specific_heat = if let Some(series) = response.and_then(|r| r.specific_heat.as_ref()) {
            Law::Curve(series.compile("J/(kg*K)", &material.sources, allow_estimates)?)
        } else {
            Law::Constant(scalar("specific_heat", "J/(kg*K)")?)
        };
        let mut domain: Option<[f64; 2]> = None;
        for law in [&transport, &specific_heat] {
            if let Law::Curve(curve) = law {
                let [a, b] = curve.domain();
                domain = Some(domain.map_or([a, b], |[lo, hi]| [lo.max(a), hi.min(b)]));
            }
        }
        if domain.is_some_and(|[a, b]| a >= b) {
            return Err(invalid(
                "thermal property intervals have no common nonzero range",
            ));
        }
        Ok(Self {
            mass,
            thickness,
            transport,
            specific_heat,
            domain,
        })
    }

    /// Common valid interval, or None for a legacy constant-property law.
    #[must_use]
    pub fn domain(&self) -> Option<[f64; 2]> {
        self.domain
    }

    /// Validate a temperature against absolute zero and every characterized property interval.
    ///
    /// # Errors
    /// Non-finite, below absolute zero or outside the characterized interval.
    pub fn check_temperature(&self, t: f64) -> Result<(), Refusal> {
        if !t.is_finite() || t < -273.15 || self.domain.is_some_and(|[lo, hi]| t < lo || t > hi) {
            return Err(invalid(format!(
                "temperature {t} outside thermal law validity {:?}",
                self.domain
            )));
        }
        Ok(())
    }

    /// Full sheet resistance in m² K/W at the declared material temperature.
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

    /// Areal tangent heat capacity in J/(m² K), based on reference mass.
    ///
    /// # Errors
    /// Out-of-domain temperature or nonphysical/overflowed capacity.
    pub fn capacity_at(&self, t: f64) -> Result<f64, Refusal> {
        self.check_temperature(t)?;
        positive(self.mass * self.specific_heat.at(t)?, "thermal capacity")
    }

    /// Change in sensible heat per undeformed area, J/m², using the integrated specific-heat law.
    ///
    /// # Errors
    /// Out-of-domain temperatures or arithmetic overflow.
    pub fn sensible_change(&self, from: f64, to: f64) -> Result<f64, Refusal> {
        self.check_temperature(from)?;
        self.check_temperature(to)?;
        let heat = self.mass * self.specific_heat.integral(from, to)?;
        if !heat.is_finite() {
            return Err(invalid("sensible heat overflow"));
        }
        Ok(heat)
    }
}

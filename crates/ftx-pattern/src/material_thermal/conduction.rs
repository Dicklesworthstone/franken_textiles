//! Steady transport qualification, independent from transient mass and heat capacity.

use super::{Law, invalid, positive};
use crate::materials::{Form, Material};
use ftx_core::Refusal;
use serde::Serialize;

/// Conditional constant-conduction endpoints from complete source-reported material ranges.
/// Geometry, reservoir temperatures, films and interfaces are fixed authored inputs. These are
/// not confidence intervals or uncertainty in the garment geometry or boundary conditions.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct ReportedConductionBounds {
    /// Sum of material resistance endpoints after declared area scaling, m² K/W.
    pub layer_resistance_m2_k_w: [f64; 2],
    /// Material, interface and film resistance endpoints, m² K/W.
    pub total_resistance_m2_k_w: [f64; 2],
    /// Signed inside-to-outside flux endpoints in increasing order, W/m².
    pub flux_w_m2: [f64; 2],
    /// Present only for a nonlinear exterior. Total resistance then excludes that boundary.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exterior: Option<ReportedExteriorBounds>,
}

/// Nonlinear boundary receipt; no radiative term is represented as a constant film resistance.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct ReportedExteriorBounds {
    /// Exact endpoint model, separate from constant-film conduction.
    pub model: &'static str,
    /// Meaning of the containing total resistance in this mode.
    pub resistance_basis: &'static str,
    /// Complete interval of the actual selected optical observation.
    pub emissivity: [f64; 2],
    /// Enclosing outer-face temperatures over all resistance/emissivity corners, Celsius.
    pub surface_temperature_c: [f64; 2],
}

/// Fixed exterior inputs after independent source, face and evidence qualification.
#[derive(Clone, Copy, Debug)]
pub struct RadiativeBoundary {
    /// Prescribed air temperature, Celsius.
    pub air_c: f64,
    /// Prescribed large-enclosure radiant temperature, Celsius.
    pub ambient_c: f64,
    /// Fixed nonnegative coefficient per current outer cloth area, W/(m² K).
    pub convection_w_m2_k: f64,
    /// Complete source-reported emissivity endpoints, dimensionless.
    pub emissivity: [f64; 2],
    /// Intersection of the qualified optical and convection surface-temperature domains.
    pub surface_temperature_range_c: [f64; 2],
}

/// Stefan-Boltzmann constant in W/(m² K⁴), rounded from its exact SI expression (NIST CODATA 2022).
pub const STEFAN_BOLTZMANN: f64 = 5.670_374_419e-8;

impl RadiativeBoundary {
    /// Signed convection and factored opaque-gray radiation per outer cloth area.
    /// Source qualification remains the caller's responsibility.
    ///
    /// # Errors
    /// Nonrepresentable arithmetic or a nonphysical temperature/coefficient/emissivity.
    pub fn flux(&self, surface_c: f64, emissivity: f64) -> Result<[f64; 2], Refusal> {
        if [surface_c, self.air_c, self.ambient_c]
            .iter()
            .any(|t| !t.is_finite() || *t < -273.15)
            || !self.convection_w_m2_k.is_finite()
            || self.convection_w_m2_k < 0.
            || !(0.0..=1.0).contains(&emissivity)
        {
            return Err(invalid("invalid radiative endpoint boundary"));
        }
        let convective = self.convection_w_m2_k * (surface_c - self.air_c);
        let radiative = if emissivity == 0. || surface_c == self.ambient_c {
            0.
        } else {
            let a = surface_c + 273.15;
            let b = self.ambient_c + 273.15;
            emissivity * STEFAN_BOLTZMANN * (surface_c - self.ambient_c) * (a + b) * (a * a + b * b)
        };
        if !convective.is_finite()
            || !radiative.is_finite()
            || !(convective + radiative).is_finite()
            || (self.convection_w_m2_k != 0. && surface_c != self.air_c && convective == 0.)
            || (emissivity != 0. && surface_c != self.ambient_c && radiative == 0.)
        {
            return Err(invalid("radiative endpoint flux is unrepresentable"));
        }
        Ok([convective, radiative])
    }

    fn corner(
        &self,
        inside: f64,
        resistance: f64,
        factor: f64,
        emissivity: f64,
    ) -> Result<([f64; 2], [f64; 2]), Refusal> {
        let mut lo = inside;
        let mut hi = inside;
        for t in [
            (self.convection_w_m2_k > 0.).then_some(self.air_c),
            (emissivity > 0.).then_some(self.ambient_c),
        ]
        .into_iter()
        .flatten()
        {
            lo = lo.min(t);
            hi = hi.max(t);
        }
        let physical = [lo, hi];
        let domain = self.surface_temperature_range_c;
        lo = lo.max(domain[0]);
        hi = hi.min(domain[1]);
        if lo > hi {
            return Err(invalid(
                "source-reported radiation endpoints leave the qualified surface domain",
            ));
        }
        let balance = |t| -> Result<(f64, f64), Refusal> {
            let [convective, radiative] = self.flux(t, emissivity)?;
            let q = (convective + radiative) / factor;
            let drop = resistance * q;
            let residual = (t - inside) + drop;
            // G'(T) = 1 + R/f * (h + 4*epsilon*sigma*K^3) >= 1. A small
            // floating evaluation residual therefore bounds a root's temperature error.
            let scale = 1.
                + t.abs()
                + inside.abs()
                + drop.abs()
                + resistance * ((convective.abs() + radiative.abs()) / factor);
            let error = 64. * f64::EPSILON * scale;
            if !residual.is_finite()
                || !error.is_finite()
                || (convective + radiative != 0. && q == 0.)
                || (q != 0. && drop == 0.)
            {
                return Err(invalid("radiative endpoint balance is unrepresentable"));
            }
            Ok((residual, error))
        };
        // Clipped bounds are admissible only when they still bracket the root. Uncertain
        // endpoint signs withhold the range; no source law is evaluated outside its domain.
        if lo > physical[0] {
            let (residual, error) = balance(lo)?;
            if residual + error > 0. {
                return Err(invalid(
                    "source-reported radiation endpoints leave the qualified surface domain",
                ));
            }
        }
        if hi < physical[1] {
            let (residual, error) = balance(hi)?;
            if residual - error < 0. {
                return Err(invalid(
                    "source-reported radiation endpoints leave the qualified surface domain",
                ));
            }
        }
        for _ in 0..128 {
            if lo == hi {
                break;
            }
            let t = lo / 2. + hi / 2.;
            if t == lo || t == hi {
                break;
            }
            let (residual, error) = balance(t)?;
            if residual.abs() <= error {
                let radius = residual.abs() + error;
                lo = lo.max((t - radius).next_down()).max(physical[0]);
                hi = hi.min((t + radius).next_up()).min(physical[1]);
                break;
            }
            if residual > 0. {
                hi = t;
            } else {
                lo = t;
            }
        }
        if lo < domain[0] || hi > domain[1] || hi - lo > 1e-9 * (1. + lo.abs().max(hi.abs())) {
            return Err(invalid(
                "source-reported radiation endpoints leave the qualified surface domain or fail to converge",
            ));
        }
        let outward = |t| -> Result<f64, Refusal> {
            let [c, r] = self.flux(t, emissivity)?;
            let q = (c + r) / factor;
            if !q.is_finite() || (c + r != 0. && q == 0.) {
                return Err(invalid("area-scaled radiative endpoint is unrepresentable"));
            }
            Ok(q)
        };
        let lower = ((inside - hi) / resistance).min(outward(lo)?);
        let upper = ((inside - lo) / resistance).max(outward(hi)?);
        if !lower.is_finite() || !upper.is_finite() || lower > upper {
            return Err(invalid("radiative source interval is unrepresentable"));
        }
        // An exactly isothermal or insulated corner has an exact zero balance.
        let flux = if lower == 0. && upper == 0. {
            [0.; 2]
        } else {
            [lower.next_down(), upper.next_up()]
        };
        if flux.iter().any(|q| !q.is_finite()) {
            return Err(invalid("radiative endpoint enclosure overflows"));
        }
        Ok((flux, [lo, hi]))
    }
}

impl ReportedConductionBounds {
    /// Convert signed flux endpoints to power on the exact positive admitted area.
    ///
    /// # Errors
    /// Invalid area or flux endpoints, overflow or underflow of a nonzero endpoint.
    pub fn power_bounds(&self, area: f64) -> Result<[f64; 2], Refusal> {
        if !area.is_finite()
            || area <= 0.
            || self.flux_w_m2.iter().any(|q| !q.is_finite())
            || self.flux_w_m2[0] > self.flux_w_m2[1]
        {
            return Err(invalid("invalid reported power area or flux endpoints"));
        }
        let power = self.flux_w_m2.map(|q| area * q);
        if power
            .iter()
            .zip(self.flux_w_m2)
            .any(|(p, q)| !p.is_finite() || (q != 0. && *p == 0.))
        {
            return Err(invalid("reported power area scaling is unrepresentable"));
        }
        Ok(power)
    }

    /// Add one complete signed power interval, preserving cancellation and endpoint order.
    /// On failure the existing total remains unchanged; no partial range is admitted.
    ///
    /// # Errors
    /// Nonfinite or reversed intervals, overflow or a nonzero contribution lost to rounding.
    pub fn add_power_bounds(total: &mut [f64; 2], contribution: [f64; 2]) -> Result<(), Refusal> {
        if total.iter().chain(&contribution).any(|v| !v.is_finite())
            || total[0] > total[1]
            || contribution[0] > contribution[1]
        {
            return Err(invalid("invalid reported power accumulation endpoints"));
        }
        let mut next = *total;
        for endpoint in 0..2 {
            let a = total[endpoint];
            let b = contribution[endpoint];
            let sum = a + b;
            if !sum.is_finite() || (b != 0. && sum == a) || (a != 0. && sum == b) {
                return Err(invalid(
                    "reported power accumulation loses a nonzero endpoint",
                ));
            }
            next[endpoint] = sum;
        }
        *total = next;
        Ok(())
    }
}

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
    reported_resistance: Result<[f64; 2], &'static str>,
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
        material.require_dry_mass_transport()?;
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
        let endpoints = |key: &str| {
            material
                .properties
                .get(key)?
                .bounds
                .filter(|[lo, hi]| lo.is_finite() && hi.is_finite() && *lo > 0.0 && *lo <= *hi)
        };
        let reported_resistance = if domain.is_some() {
            Err(
                "temperature-dependent conductivity has no supported source-reported conduction range",
            )
        } else if thickness.is_none() {
            endpoints("thermal_resistance")
                .ok_or("thermal_resistance needs complete positive source-reported endpoints")
        } else if let (Some([tlo, thi]), Some([klo, khi])) =
            (endpoints("thickness"), endpoints("thermal_conductivity"))
        {
            let range = [tlo / khi, thi / klo];
            if range.iter().all(|v| v.is_finite() && *v > 0.) && range[0] <= range[1] {
                Ok(range)
            } else {
                Err(
                    "source-reported thickness/conductivity resistance endpoints are unrepresentable",
                )
            }
        } else {
            Err(
                "thickness and thermal_conductivity both need complete positive source-reported endpoints",
            )
        };
        Ok(Self {
            thickness,
            transport,
            domain,
            reported_resistance,
        })
    }

    /// Complete positive reported endpoints of the exact selected constant transport observation.
    /// The same nominal qualification and unit checks have already admitted this law. Missing
    /// endpoints never become singleton nominal values, and sample variability is not promoted.
    ///
    /// # Errors
    /// Missing/nonpositive endpoints, a temperature-dependent law, or unrepresentable conversion.
    pub fn reported_resistance_bounds(&self) -> Result<[f64; 2], Refusal> {
        self.reported_resistance.map_err(invalid)
    }

    /// Propagate reported constant resistance endpoints through one explicitly qualified network.
    /// Area factors refer each material's resistance to the chosen boundary area. Complete source
    /// intervals enclose all combinations; no statistical independence assumption is required.
    /// This shared authority serves producer reports and decision admission without another solver.
    ///
    /// # Errors
    /// Missing source bounds, unsupported laws, invalid network inputs or lost/overflowed endpoints.
    pub fn reported_stack_bounds(
        laws: &[Self],
        area_factors: &[f64],
        interfaces: &[f64],
        temperatures_c: [f64; 2],
        films: [f64; 2],
    ) -> Result<ReportedConductionBounds, Refusal> {
        if laws.is_empty()
            || laws.len() > 256
            || area_factors.len() != laws.len()
            || interfaces.len() != laws.len() - 1
            || temperatures_c
                .iter()
                .any(|v| !v.is_finite() || *v < -273.15)
            || films
                .iter()
                .chain(interfaces)
                .any(|v| !v.is_finite() || *v < 0.)
            || area_factors.iter().any(|v| !v.is_finite() || *v <= 0.)
        {
            return Err(invalid("invalid reported-conduction network inputs"));
        }
        let add = |total: &mut f64, increment: f64| -> Result<(), Refusal> {
            let next = *total + increment;
            if !next.is_finite()
                || (increment > 0. && next <= *total)
                || (*total > 0. && next == increment)
            {
                return Err(invalid(
                    "reported resistance accumulation loses a positive endpoint",
                ));
            }
            *total = next;
            Ok(())
        };
        let mut layer = [0.; 2];
        let mut total = [0.; 2];
        for value in films {
            for endpoint in &mut total {
                add(endpoint, value)?;
            }
        }
        // Match the nominal constant-law seed's cold-to-hot arithmetic. Exact singleton source
        // intervals must not become inconsistent merely because the positive terms were reordered.
        let reverse = temperatures_c[0] < temperatures_c[1];
        for step in 0..laws.len() {
            let index = if reverse { step } else { laws.len() - 1 - step };
            let range = laws[index]
                .reported_resistance_bounds()
                .map_err(|r| invalid(format!("layer {index}: {}", r.message)))?;
            for endpoint in 0..2 {
                let value = range[endpoint] * area_factors[index];
                if !value.is_finite() || value <= 0. {
                    return Err(invalid(
                        "area-scaled reported resistance is unrepresentable",
                    ));
                }
                add(&mut layer[endpoint], value)?;
                add(&mut total[endpoint], value)?;
            }
            if step + 1 < laws.len() {
                for endpoint in &mut total {
                    add(
                        endpoint,
                        interfaces[if reverse { index } else { index - 1 }],
                    )?;
                }
            }
        }
        let delta = temperatures_c[0] - temperatures_c[1];
        let a = delta / total[0];
        let b = delta / total[1];
        if !delta.is_finite()
            || [a, b]
                .iter()
                .any(|v| !v.is_finite() || (delta != 0. && *v == 0.))
        {
            return Err(invalid(
                "reported conduction flux endpoints are unrepresentable",
            ));
        }
        Ok(ReportedConductionBounds {
            layer_resistance_m2_k_w: layer,
            total_resistance_m2_k_w: total,
            flux_w_m2: [a.min(b), a.max(b)],
            exterior: None,
        })
    }

    /// Complete constant-transport and emissivity intervals under fixed opaque-gray exchange.
    /// Flux and surface temperature are separately monotone in each resistance/emissivity
    /// coordinate. All four corners therefore enclose even opposing or sign-changing exchange.
    /// The total resistance excludes the nonlinear exterior; its separate receipt says so.
    ///
    /// # Errors
    /// Incomplete source intervals, invalid boundary/area inputs, endpoint-domain escape,
    /// unrepresentable arithmetic or an unresolved monotone root.
    pub fn reported_radiation_bounds(
        laws: &[Self],
        area_factors: &[f64],
        interfaces: &[f64],
        inside_c: f64,
        inside_resistance: f64,
        exterior: RadiativeBoundary,
    ) -> Result<ReportedConductionBounds, Refusal> {
        let [elo, ehi] = exterior.emissivity;
        let [lo, hi] = exterior.surface_temperature_range_c;
        if !(0.0..=1.0).contains(&elo)
            || !(elo..=1.0).contains(&ehi)
            || !lo.is_finite()
            || !hi.is_finite()
            || lo < -273.15
            || hi < lo
            || !(hi - lo).is_finite()
            || [exterior.air_c, exterior.ambient_c]
                .iter()
                .any(|t| !t.is_finite() || *t < -273.15)
            || !exterior.convection_w_m2_k.is_finite()
            || exterior.convection_w_m2_k < 0.
        {
            return Err(invalid(
                "invalid source-reported optical endpoints or domain",
            ));
        }
        let mut bounds = Self::reported_stack_bounds(
            laws,
            area_factors,
            interfaces,
            [inside_c, inside_c],
            [inside_resistance, 0.],
        )?;
        let factor = *area_factors.last().expect("validated nonempty layers");
        let mut flux = [f64::INFINITY, f64::NEG_INFINITY];
        let mut temperatures = flux;
        for resistance in bounds.total_resistance_m2_k_w {
            for emissivity in exterior.emissivity {
                let (q, t) = exterior.corner(inside_c, resistance, factor, emissivity)?;
                flux = [flux[0].min(q[0]), flux[1].max(q[1])];
                temperatures = [temperatures[0].min(t[0]), temperatures[1].max(t[1])];
            }
        }
        bounds.flux_w_m2 = flux;
        bounds.exterior = Some(ReportedExteriorBounds {
            model: "opaque_gray_source_endpoints/1",
            resistance_basis: "inside_film_interfaces_and_materials_only",
            emissivity: exterior.emissivity,
            surface_temperature_c: temperatures,
        });
        Ok(bounds)
    }

    /// Conductivity validity interval; `None` retains legacy constant-property semantics.
    #[must_use]
    pub fn domain(&self) -> Option<[f64; 2]> {
        self.domain
    }

    /// Full-sheet resistance for an admitted constant transport law, in m² K/W.
    /// Temperature-series observations retain their domain even when every knot is equal;
    /// a scalar stack cannot silently discard that domain or choose an evaluation temperature.
    ///
    /// # Errors
    /// A temperature-dependent transport law, or nonphysical/overflowed resistance.
    pub fn constant_resistance(&self) -> Result<f64, Refusal> {
        let Law::Constant(value) = self.transport else {
            return Err(invalid(
                "temperature-dependent transport requires the steady conduction consumer with explicit face temperatures; no temperature-independent scalar resistance is available",
            ));
        };
        self.resistance(value)
    }

    fn resistance(&self, transport: f64) -> Result<f64, Refusal> {
        positive(
            self.thickness
                .map_or(transport, |thickness| thickness / transport),
            "thermal resistance",
        )
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
        self.resistance(self.transport.at(t)?)
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

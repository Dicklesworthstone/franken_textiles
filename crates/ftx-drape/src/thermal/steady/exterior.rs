//! Prescribed convection and source-qualified opaque-gray ambient radiation.
//!
//! An exterior face temperature is solved with the SAME integrated Fourier material laws. No wind
//! correlation, optical coefficient, skin property or new material database is inferred here.
//! Radiation: COMSOL Heat Transfer, Surface-to-Ambient Radiation; sigma: NIST CODATA 2022.

use super::{Boundary, ITERATIONS, Shot, SteadyConductionModel, SteadyState, scaled_flux};
use crate::thermal::{invalid, nonnegative, positive, temperature};
use ftx_core::Refusal;
use ftx_pattern::{
    material_catalog::ResolvedMaterial,
    material_optics::Side,
    materials::{Evidence, Source},
};
use serde::{Deserialize, Serialize};

/// Stefan-Boltzmann constant in W/(m² K⁴), rounded from its exact SI expression.
/// NIST CODATA 2022: https://physics.nist.gov/cuu/Constants/Table/allascii.txt
pub const STEFAN_BOLTZMANN: f64 = 5.670_374_419e-8;

/// A single large isothermal radiative enclosure, not a ray-traced scene or a solar spectrum.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AmbientRadiation {
    /// Blackbody-equivalent enclosure temperature in Celsius, independent of air temperature.
    pub ambient_c: f64,
    /// Exact property key in the OUTERMOST selected material in explicit mode.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub emissivity_property: String,
    /// Explicit physical face, matching the observation's `face` condition (e.g. `public`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub exposed_face: String,
    /// Resolve the actual side through the selected material's source-bearing `optical_faces`.
    /// Explicit face/property strings must be omitted in this mode. Covered regions use their
    /// current cloth/body normals; independent columns use their canonical positive outside face.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub automatic_material_face: bool,
    /// Affirmative admission of opaque, diffuse, gray, full-enclosure exchange with unit view factor.
    pub allow_opaque_gray_approximation: bool,
}

/// Qualified prescribed exterior exchange; the convection coefficient is NOT calculated from wind.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exterior {
    /// Prescribed air temperature in Celsius.
    pub air_c: f64,
    /// Nonnegative convection coefficient per CURRENT outer cloth area, W/(m² K).
    pub convection_w_m2_k: f64,
    /// Admitted SURFACE temperature range of the prescribed coefficient, inclusive, Celsius.
    pub surface_temperature_range_c: [f64; 2],
    /// Source/measurement record for this coefficient, not a textile material observation.
    pub convection_source: Source,
    /// Whether the coefficient was measured, published or estimated.
    pub convection_evidence: Evidence,
    /// Air state, geometry, orientation, motion and other applicability/estimation conditions.
    pub convection_conditions: String,
    /// Consent for this boundary's estimated convection only; cannot relax material evidence policy.
    #[serde(default)]
    pub allow_estimates: bool,
    /// Omit explicitly for convection-only exchange. Missing emissivity is never replaced by zero.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radiation: Option<AmbientRadiation>,
}

/// Complete coupled result. Positive flux leaves the inside reservoir toward the environment.
#[derive(Clone, Debug, Serialize)]
pub struct ExteriorState {
    /// Material face temperatures and signed flux per model area (body area for scaled coverage).
    pub conduction: SteadyState,
    /// Solved exposed face temperature, Celsius; not prescribed air or radiant temperature.
    pub surface_c: f64,
    /// Signed convection per current OUTER cloth area, W/m².
    pub convective_flux_w_m2: f64,
    /// Signed radiation per current OUTER cloth area, W/m².
    pub radiative_flux_w_m2: f64,
    /// False means radiation was explicitly omitted, not that a zero emissivity was measured.
    pub radiation_enabled: bool,
    /// Actual observation selected by material orientation, absent for explicit-face mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub material_face: Option<MaterialFace>,
    /// Current outer cloth area / model reference area; one for independent equal-area columns.
    pub outer_area_per_model_area: f64,
    /// Conduction minus the area-scaled sum of convection/radiation, W/m² of model area.
    pub balance_residual_w_m2: f64,
}

/// Source-locked optical selection for the physical face used in this exchange result.
#[derive(Clone, Debug, Serialize)]
pub struct MaterialFace {
    /// Side of the retained source triangle's oriented normal.
    pub side: Side,
    /// Exact physical face label in the optical observation.
    pub face: String,
    /// Property key whose qualified value supplied emissivity.
    pub emissivity_property: String,
    /// Source key for the observation-to-orientation mapping in the selected material.
    pub mapping_source: String,
    /// Immutable material identity, including the mapping and its source evidence.
    pub material_sha256: String,
}

pub(in crate::thermal) struct Exchange<'a> {
    input: &'a Exterior,
    pub(in crate::thermal) emissivity: f64,
    pub(in crate::thermal) domain: [f64; 2],
    pub(in crate::thermal) material_face: Option<MaterialFace>,
}

fn range([lo, hi]: [f64; 2]) -> Result<[f64; 2], Refusal> {
    temperature(lo)?;
    temperature(hi)?;
    if lo > hi || !(hi - lo).is_finite() {
        return Err(invalid("invalid exterior surface temperature range"));
    }
    Ok([lo, hi])
}

impl<'a> Exchange<'a> {
    pub(in crate::thermal) fn on_side(
        input: &'a Exterior,
        outer: &ResolvedMaterial,
        side: Side,
    ) -> Result<Self, Refusal> {
        temperature(input.air_c)?;
        nonnegative("prescribed convection coefficient", input.convection_w_m2_k)?;
        let mut domain = range(input.surface_temperature_range_c)?;
        let s = &input.convection_source;
        if s.reference.trim().is_empty()
            || s.note.trim().is_empty()
            || s.accessed_on.len() != 10
            || input.convection_conditions.trim().is_empty()
            || (input.convection_evidence == Evidence::Estimate && !input.allow_estimates)
        {
            return Err(invalid(
                "exterior convection needs a source, conditions and explicit estimate consent",
            ));
        }
        let mut emissivity = 0.0;
        let mut material_face = None;
        if let Some(r) = &input.radiation {
            temperature(r.ambient_c)?;
            if !r.allow_opaque_gray_approximation {
                return Err(invalid("radiation needs opaque-gray enclosure consent"));
            }
            let m = outer.material();
            let (property, face) = if r.automatic_material_face {
                if !r.emissivity_property.is_empty() || !r.exposed_face.is_empty() {
                    return Err(invalid(
                        "automatic material-face selection cannot also prescribe a face or property",
                    ));
                }
                let selected = m.optical_face(side, outer.allows_estimates())?;
                material_face = Some(MaterialFace {
                    side,
                    face: selected.face.clone(),
                    emissivity_property: selected.emissivity_property.clone(),
                    mapping_source: m
                        .optical_faces
                        .as_ref()
                        .expect("qualified mapping")
                        .source
                        .clone(),
                    material_sha256: outer.sha256().to_owned(),
                });
                (&selected.emissivity_property, &selected.face)
            } else {
                if r.exposed_face.trim().is_empty() || r.emissivity_property.trim().is_empty() {
                    return Err(invalid(
                        "radiation needs an explicit face and emissivity property",
                    ));
                }
                (&r.emissivity_property, &r.exposed_face)
            };
            emissivity = m.require(property, "1", outer.allows_estimates())?;
            let q = &m.properties[property];
            if !(0.0..=1.0).contains(&emissivity)
                || q.bounds.is_some_and(|[lo, hi]| lo < 0.0 || hi > 1.0)
                || q.conditions.get("face") != Some(face)
                || q.conditions.get("spectral_basis").map(String::as_str)
                    != Some("total_hemispherical")
            {
                return Err(invalid(
                    "emissivity needs [0,1] bounds, the exact exposed face and total_hemispherical spectral basis",
                ));
            }
            let transport = m
                .properties
                .get("thermal_resistance")
                .or_else(|| m.properties.get("thermal_conductivity"))
                .or_else(|| m.properties.get("thickness"));
            for field in ["specimen", "conditioning"] {
                let actual = q
                    .conditions
                    .get(field)
                    .filter(|s| !s.trim().is_empty())
                    .ok_or_else(|| invalid(format!("emissivity requires {field}")))?;
                let expected = m
                    .thermal_response
                    .as_ref()
                    .map(|t| {
                        if field == "specimen" {
                            &t.specimen
                        } else {
                            &t.conditioning
                        }
                    })
                    .or_else(|| transport.and_then(|t| t.conditions.get(field)));
                if expected.is_some_and(|v| v != actual) {
                    return Err(invalid(format!("emissivity must match transport {field}")));
                }
            }
            let number = |key: &str| -> Result<f64, Refusal> {
                q.conditions
                    .get(key)
                    .and_then(|s| s.parse::<f64>().ok())
                    .ok_or_else(|| invalid(format!("emissivity requires numeric {key}")))
            };
            let [lo, hi] = range([number("temperature_min_c")?, number("temperature_max_c")?])?;
            // Gray absorptivity is an admitted closure over BOTH relevant blackbody spectra.
            if r.ambient_c < lo || r.ambient_c > hi {
                return Err(invalid(
                    "radiant enclosure lies outside the emissivity temperature qualification",
                ));
            }
            domain = [domain[0].max(lo), domain[1].min(hi)];
        }
        range(domain)?;
        Ok(Self {
            input,
            emissivity,
            domain,
            material_face,
        })
    }

    pub(in crate::thermal) fn flux(&self, surface_c: f64) -> Result<[f64; 2], Refusal> {
        temperature(surface_c)?;
        if surface_c < self.domain[0] || surface_c > self.domain[1] {
            return Err(invalid(
                "surface temperature outside exterior coefficient qualification",
            ));
        }
        let convection = self.input.convection_w_m2_k * (surface_c - self.input.air_c);
        let radiation = if let Some(r) = &self.input.radiation
            && self.emissivity != 0.0
            && surface_c != r.ambient_c
        {
            let a = surface_c + 273.15;
            let b = r.ambient_c + 273.15;
            // Factor the fourth-power difference to retain small signed temperature differences.
            self.emissivity
                * STEFAN_BOLTZMANN
                * (surface_c - r.ambient_c)
                * (a + b)
                * (a * a + b * b)
        } else {
            0.0
        };
        if !convection.is_finite()
            || !radiation.is_finite()
            || !(convection + radiation).is_finite()
        {
            return Err(invalid("exterior heat flux is not representable"));
        }
        Ok([convection, radiation])
    }
}

impl SteadyConductionModel {
    /// Solve convection plus optional material-following radiation at the exposed outer face.
    /// `outside_c` must equal the declared air temperature and `outside_resistance` must be zero:
    /// this mode replaces the outer film rather than silently ignoring or double-counting it.
    /// A supplied coefficient is not inferred from a garment name, wind speed or warmth label.
    ///
    /// # Errors
    /// Unqualified emissivity/face/evidence, invalid boundary, out-of-domain material/optical/film
    /// temperatures, unrepresentable arithmetic or failed original-equation/iteration checks.
    pub fn solve_exterior(
        &self,
        boundary: Boundary,
        exterior: &Exterior,
    ) -> Result<ExteriorState, Refusal> {
        self.solve_exterior_on_side(boundary, exterior, Side::Positive)
    }

    pub(in crate::thermal) fn solve_exterior_on_side(
        &self,
        boundary: Boundary,
        exterior: &Exterior,
        side: Side,
    ) -> Result<ExteriorState, Refusal> {
        temperature(boundary.inside_c)?;
        nonnegative("inside film resistance", boundary.inside_resistance)?;
        if boundary.outside_resistance != 0.0 || boundary.outside_c != exterior.air_c {
            return Err(invalid(
                "exterior exchange replaces the outside film: use zero outside resistance and outside_c equal to air_c",
            ));
        }
        let exchange =
            Exchange::on_side(exterior, self.layers.last().expect("nonempty model"), side)?;
        let factor = *self.area_factors.last().expect("nonempty area factors");
        let outer_area_per_model_area = positive("outer cloth/model area ratio", 1.0 / factor)?;
        let mut enclosure = [boundary.inside_c, boundary.inside_c];
        if exterior.convection_w_m2_k > 0.0 {
            enclosure = [
                enclosure[0].min(exterior.air_c),
                enclosure[1].max(exterior.air_c),
            ];
        }
        if exchange.emissivity > 0.0 {
            let t = exterior
                .radiation
                .as_ref()
                .expect("qualified radiation")
                .ambient_c;
            enclosure = [enclosure[0].min(t), enclosure[1].max(t)];
        }
        let domain = self
            .laws
            .last()
            .expect("nonempty laws")
            .domain()
            .unwrap_or(enclosure);
        let mut low = enclosure[0].max(domain[0]).max(exchange.domain[0]);
        let mut high = enclosure[1].min(domain[1]).min(exchange.domain[1]);
        if low > high || !(high - low).is_finite() {
            return Err(invalid(
                "no qualified exterior surface temperature interval",
            ));
        }
        // Increasing T_surface increases exterior flux and every inward face temperature. Invalid
        // trials bound the root; they NEVER evaluate a law outside its domain or become a solution.
        for step in 0..ITERATIONS {
            let t = match step {
                0 => low,
                1 => high,
                _ => low / 2.0 + high / 2.0,
            };
            let [convective, radiative] = exchange.flux(t)?;
            let q = (convective + radiative) / factor;
            if !q.is_finite() || (convective + radiative != 0.0 && q == 0.0) {
                return Err(invalid("area-scaled exterior flux is not representable"));
            }
            match self.exterior_shot(t, q, boundary, enclosure)? {
                Shot::Valid { faces, residual } => {
                    let b = Boundary {
                        outside_c: t,
                        ..boundary
                    };
                    if residual.abs() <= super::temperature_tolerance(b)
                        && let Ok(mut conduction) = self.finish(faces, q, b)
                    {
                        conduction.exclude_reported_bounds(
                            "source-reported conduction ranges do not cover coupled exterior exchange",
                        );
                        return Ok(ExteriorState {
                            balance_residual_w_m2: conduction.flux_w_m2 - q,
                            conduction,
                            surface_c: t,
                            convective_flux_w_m2: convective,
                            radiative_flux_w_m2: radiative,
                            radiation_enabled: exterior.radiation.is_some(),
                            material_face: exchange.material_face,
                            outer_area_per_model_area,
                        });
                    }
                    if residual > 0.0 {
                        high = t;
                    } else {
                        low = t;
                    }
                }
                Shot::Low => low = t,
                Shot::High => high = t,
            }
        }
        Err(invalid(
            "no residual-qualified exterior solution within material and boundary domains",
        ))
    }

    fn exterior_shot(
        &self,
        outer: f64,
        q: f64,
        b: Boundary,
        enclosure: [f64; 2],
    ) -> Result<Shot, Refusal> {
        let mut t = outer;
        let mut faces = vec![[0.0; 2]; self.laws.len()];
        for i in (0..self.laws.len()).rev() {
            let law = &self.laws[i];
            let domain = law.domain().unwrap_or(enclosure);
            let mut lo = domain[0].max(enclosure[0]);
            let mut hi = domain[1].min(enclosure[1]);
            if t < lo {
                return Ok(Shot::Low);
            }
            if t > hi {
                return Ok(Shot::High);
            }
            let f = self.area_factors[i];
            if q < scaled_flux(law, lo, t, f)? {
                return Ok(Shot::Low);
            }
            if q > scaled_flux(law, hi, t, f)? {
                return Ok(Shot::High);
            }
            let inside = if q == 0.0 {
                t
            } else {
                for _ in 0..ITERATIONS {
                    let mid = lo / 2.0 + hi / 2.0;
                    if mid == lo || mid == hi {
                        break;
                    }
                    let flux = scaled_flux(law, mid, t, f)?;
                    if flux == q {
                        lo = mid;
                        hi = mid;
                        break;
                    }
                    if flux < q {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                if (scaled_flux(law, lo, t, f)? - q).abs() < (scaled_flux(law, hi, t, f)? - q).abs()
                {
                    lo
                } else {
                    hi
                }
            };
            faces[i] = [inside, t];
            t = inside;
            if i > 0 {
                t += q * self.interfaces[i - 1];
                if !t.is_finite() {
                    return Err(invalid("exterior interface temperature overflow"));
                }
            }
        }
        let residual = (t - b.inside_c) + q * b.inside_resistance;
        if !residual.is_finite() {
            return Err(invalid("exterior inner-film temperature overflow"));
        }
        Ok(Shot::Valid { faces, residual })
    }
}

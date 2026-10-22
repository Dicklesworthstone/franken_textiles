//! Area coordinates for the existing inverse experiment, evaluated by the production compiler.

use std::collections::BTreeMap;

use ftx_core::Refusal;
use ftx_pattern::{
    material_response::{
        AREA_MODEL, AreaResponse, Interpolation, StrainDomain, StressCurve, TabulatedMembrane,
    },
    materials::{Evidence, Quantity, Source},
};
use serde::{Deserialize, Serialize};

use super::{family::ParameterDescriptor, invalid};
use crate::Membrane;

/// Explicit treatment of a physical-area law, including identification without prior coefficients.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum Area {
    /// Keep both original coefficients and their evidence unchanged while fitting directionality.
    Preserve {
        /// Admission of estimated fixed coefficients, independently of fitted-output consent.
        #[serde(default)]
        allow_estimates: bool,
        /// Signed trial support for a scalar orthotropic base. Required only for that base;
        /// inherited material support still applies and publication uses actual FIT strains.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scalar_green_strain_range: Option<[[f64; 2]; 3]>,
    },
    /// Append quadratic and log-squared coefficients, in N/m, to the base parameter vector.
    Identify {
        /// Nonnegative, nonempty closed search intervals, not uncertainty or confidence bounds.
        parameter_bounds: [[f64; 2]; 2],
        /// Positive J trial applicability containing one. Required without an existing area law,
        /// otherwise intersects its interval. Accepted publication uses actual FIT-only J support.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        area_ratio_range: Option<[f64; 2]>,
        /// Signed trial support for a scalar orthotropic base; omitted for declared shapes/intervals.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scalar_green_strain_range: Option<[[f64; 2]; 3]>,
    },
}

impl Area {
    pub(super) fn bounds(&self) -> &[[f64; 2]] {
        match self {
            Self::Preserve { .. } => &[],
            Self::Identify {
                parameter_bounds, ..
            } => parameter_bounds,
        }
    }

    pub(super) fn identifies(&self) -> bool {
        matches!(self, Self::Identify { .. })
    }

    pub(super) fn scalar_range(&self) -> Option<[[f64; 2]; 3]> {
        match self {
            Self::Preserve {
                scalar_green_strain_range,
                ..
            }
            | Self::Identify {
                scalar_green_strain_range,
                ..
            } => *scalar_green_strain_range,
        }
    }

    pub(super) fn validate_base(&self, scalar: bool) -> Result<(), Refusal> {
        match (scalar, self.scalar_range()) {
            (true, Some(ranges)) => {
                signed_domain(ranges)?;
            }
            (true, None) => {
                return Err(invalid(
                    "scalar area identification requires explicit signed scalar_green_strain_range; no trial support is inferred",
                ));
            }
            (false, Some(_)) => {
                return Err(invalid(
                    "scalar_green_strain_range must be omitted with declared shapes or independent nonlinear intervals",
                ));
            }
            (false, None) => {}
        }
        Ok(())
    }

    pub(super) fn range(&self, template: Option<&AreaResponse>) -> Result<[f64; 2], Refusal> {
        let authored = match self {
            Self::Identify {
                area_ratio_range, ..
            } => *area_ratio_range,
            Self::Preserve { .. } => None,
        };
        let valid = |[lo, hi]: [f64; 2]| {
            lo.is_finite() && hi.is_finite() && lo > 0.0 && lo <= 1.0 && hi >= 1.0 && hi > lo
        };
        if authored.is_some_and(|range| !valid(range)) {
            return Err(invalid(
                "area identification requires a positive finite J interval containing one",
            ));
        }
        let range = match (authored, template) {
            (Some(range), Some(template)) => [
                range[0].max(template.area_ratio_range[0]),
                range[1].min(template.area_ratio_range[1]),
            ],
            (Some(range), None) => range,
            (None, Some(template)) => template.area_ratio_range,
            (None, None) => {
                return Err(invalid(
                    "fresh area identification requires an explicit area_ratio_range; preserving area requires an existing qualified law",
                ));
            }
        };
        if !valid(range) {
            return Err(invalid(
                "declared and inherited J support need a nonempty positive interval containing one",
            ));
        }
        Ok(range)
    }

    pub(super) fn validate(
        &self,
        template: Option<&AreaResponse>,
        conditions: &BTreeMap<String, String>,
        sources: &BTreeMap<String, Source>,
        check_sources: &[&str],
    ) -> Result<(), Refusal> {
        if conditions.len() > 60
            || !["specimen", "conditioning"].iter().all(|key| {
                conditions
                    .get(*key)
                    .is_some_and(|value| !value.trim().is_empty())
            })
            || conditions.iter().any(|(key, value)| {
                key.trim().is_empty()
                    || value.trim().is_empty()
                    || key.len() > 4096
                    || value.len() > 4096
            })
            || template.is_some_and(|template| {
                template.conditions.iter().any(|(k, v)| {
                    !(self.identifies()
                        && [
                            "fitted_green_strain_range",
                            "fitted_area_ratio_range",
                            "held_out_checks",
                            "fixture_model",
                        ]
                        .contains(&k.as_str()))
                        && conditions.get(k) != Some(v)
                })
            })
        {
            return Err(invalid(
                "area identification must retain the original matched specimen/conditioning and room for FIT-domain metadata",
            ));
        }
        self.range(template)?;
        match self {
            Self::Preserve {
                allow_estimates, ..
            } => {
                let template = template.ok_or_else(|| {
                    invalid("preserving area response requires an existing qualified law")
                })?;
                for q in [&template.quadratic, &template.log_squared] {
                    let reference = sources.get(&q.source).map(|s| s.reference.as_str());
                    if q.conditions.contains_key("fitted_green_strain_range")
                        || q.conditions.contains_key("fitted_area_ratio_range")
                        || reference.is_some_and(|reference| {
                            check_sources.contains(&reference)
                                || [
                                    "local:ftx-strip-calibration/",
                                    "local:ftx-strip-area-calibration/",
                                    "local:ftx-swatch-calibration/",
                                    "local:ftx-nonlinear-swatch-calibration/",
                                    "local:ftx-nonlinear-swatch-area-calibration/",
                                ]
                                .iter()
                                .any(|prefix| reference.starts_with(prefix))
                        })
                    {
                        return Err(invalid(
                            "fixed area coefficients need independently qualified evidence; a previous fitted law or CHECK acquisition cannot be reused as an independent area prior",
                        ));
                    }
                }
                if !allow_estimates
                    && [&template.quadratic, &template.log_squared]
                        .iter()
                        .any(|q| q.evidence == Evidence::Estimate)
                {
                    return Err(invalid(
                        "preserving estimated area coefficients requires area_response.allow_estimates",
                    ));
                }
            }
            Self::Identify {
                parameter_bounds, ..
            } => {
                if parameter_bounds.iter().any(|&[lo, hi]| {
                    !lo.is_finite()
                        || !hi.is_finite()
                        || lo < 0.0
                        || lo >= hi
                        || !(hi - lo).is_finite()
                }) || parameter_bounds.iter().all(|p| p[0] == 0.0)
                    || !(2.0 * (parameter_bounds[0][1] + parameter_bounds[1][1])).is_finite()
                {
                    return Err(invalid(
                        "area bounds must be finite nonnegative nonempty N/m intervals; the entire box must retain a positive representable reference area tangent",
                    ));
                }
            }
        }
        Ok(())
    }

    pub(super) fn attach(
        &self,
        response: &mut TabulatedMembrane,
        template: Option<&AreaResponse>,
        parameters: &[f64],
        source: &str,
        conditions: &BTreeMap<String, String>,
        fitted_range: Option<[f64; 2]>,
    ) -> Result<(), Refusal> {
        let area = if self.identifies() {
            if parameters.len() != 2 {
                return Err(invalid(
                    "area identification requires two coefficient coordinates",
                ));
            }
            let mut support = self.range(template)?;
            if let Some(range) = fitted_range {
                if range[0] < support[0] || range[1] > support[1] {
                    return Err(invalid(
                        "identified area support cannot widen the original J domain",
                    ));
                }
                support = range;
            }
            let method = concat!(
                "Joint bounded finite-strip identification of the physical-area energy and directional response; ",
                "conditional on the declared model, FIT-only support and independent CHECK replay. ",
                "Search bounds are not confidence intervals."
            );
            let quantity = |value| Quantity {
                value,
                unit: "N/m".into(),
                evidence: Evidence::Estimate,
                source: source.into(),
                conditions: conditions.clone(),
                bounds: None,
                method: method.into(),
            };
            AreaResponse {
                model: AREA_MODEL.into(),
                quadratic: quantity(parameters[0]),
                log_squared: quantity(parameters[1]),
                area_ratio_range: support,
                conditions: conditions.clone(),
            }
        } else {
            template
                .ok_or_else(|| {
                    invalid("preserving area response requires an existing qualified law")
                })?
                .clone()
        };
        response.area_response = Some(area);
        Ok(())
    }

    pub(super) fn descriptors(&self) -> Vec<ParameterDescriptor> {
        if self.bounds().is_empty() {
            return Vec::new();
        }
        [
            ("area_quadratic", "Physical-area quadratic coefficient"),
            ("area_log_squared", "Physical-area log-squared coefficient"),
        ]
        .map(|(id, label)| ParameterDescriptor {
            id: id.into(),
            label: label.into(),
            unit: "N/m".into(),
        })
        .to_vec()
    }

    pub(super) fn scope(&self) -> &'static str {
        match self {
            Self::Preserve { .. } => {
                " Physical-area coefficients, J domain and original evidence are preserved, not identified by this fit."
            }
            Self::Identify { .. } => {
                " Two physical-area coefficients are jointly identified within the declared and inherited J domain and actual FIT-only physical-area/strain support. Local excitation is not global uniqueness or empirical textile qualification."
            }
        }
    }
}

fn signed_domain(ranges: [[f64; 2]; 3]) -> Result<StrainDomain, Refusal> {
    let domain = StrainDomain::new(ranges)?;
    if ranges.iter().any(|[lo, hi]| *lo >= 0.0 || *hi <= 0.0) {
        return Err(invalid(
            "scalar area base requires explicit negative and positive support on every Green-strain axis",
        ));
    }
    Ok(domain)
}

pub(super) fn scalar_response(
    parameters: [f64; 4],
    ranges: [[f64; 2]; 3],
    source: &str,
    conditions: &BTreeMap<String, String>,
) -> Result<TabulatedMembrane, Refusal> {
    signed_domain(ranges)?;
    let [warp, weft, nu, shear] = parameters;
    let base = Membrane::orthotropic(warp, weft, nu, shear, 1.0);
    base.validate()?;
    let method = concat!(
        "Exact signed linear Green-strain representation of the scalar orthotropic base; ",
        "physical-area energy is a separate jointly fitted or preserved contribution"
    );
    let curve = |axis: usize| StressCurve {
        points: vec![
            [ranges[axis][0], ranges[axis][0] * base.q[axis][axis]],
            [0.0, 0.0],
            [ranges[axis][1], ranges[axis][1] * base.q[axis][axis]],
        ],
        source: source.into(),
        evidence: Evidence::Estimate,
        method: method.into(),
        conditions: conditions.clone(),
    };
    Ok(TabulatedMembrane {
        model: ftx_pattern::material_response::MODEL.into(),
        interpolation: Interpolation::PiecewiseLinear,
        warp: curve(0),
        weft: curve(1),
        shear: curve(2),
        area_response: None,
        coupling: Quantity {
            value: base.q[0][1],
            unit: "N/m".into(),
            evidence: Evidence::Estimate,
            source: source.into(),
            method: concat!(
                "Normal coupling of the identified orthotropic base, ",
                "excluding the separate physical-area energy"
            )
            .into(),
            conditions: conditions.clone(),
            bounds: None,
        },
    })
}

pub(super) fn scalar_descriptors() -> Vec<ParameterDescriptor> {
    [
        ("warp_stiffness", "Warp / wale base stiffness", "N/m"),
        ("weft_stiffness", "Weft / course base stiffness", "N/m"),
        ("poisson_warp_weft", "Major base Poisson ratio", "1"),
        ("shear_stiffness", "Base shear stiffness", "N/m"),
    ]
    .map(|(id, label, unit)| ParameterDescriptor {
        id: id.into(),
        label: label.into(),
        unit: unit.into(),
    })
    .to_vec()
}

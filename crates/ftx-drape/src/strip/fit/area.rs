//! Area coordinates for the existing inverse experiment, evaluated by the production compiler.

use std::collections::BTreeMap;

use ftx_core::Refusal;
use ftx_pattern::{
    material_response::{AreaResponse, TabulatedMembrane},
    materials::{Evidence, Source},
};
use serde::{Deserialize, Serialize};

use super::{family::ParameterDescriptor, invalid};

/// Explicit treatment of an already declared, source-qualified physical-area law.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum Area {
    /// Keep both original coefficients and their evidence unchanged while fitting directionality.
    Preserve {
        /// Admission of estimated fixed coefficients, independently of fitted-output consent.
        #[serde(default)]
        allow_estimates: bool,
    },
    /// Append quadratic and log-squared coefficients, in N/m, to the base parameter vector.
    Identify {
        /// Nonnegative, nonempty closed search intervals, not uncertainty or confidence bounds.
        parameter_bounds: [[f64; 2]; 2],
    },
}

impl Area {
    pub(super) fn bounds(&self) -> &[[f64; 2]] {
        match self {
            Self::Preserve { .. } => &[],
            Self::Identify { parameter_bounds } => parameter_bounds,
        }
    }

    pub(super) fn identifies(&self) -> bool {
        matches!(self, Self::Identify { .. })
    }

    pub(super) fn validate(
        &self,
        template: &AreaResponse,
        conditions: &BTreeMap<String, String>,
        sources: &BTreeMap<String, Source>,
        check_sources: &[&str],
    ) -> Result<(), Refusal> {
        if conditions.len() > 60
            || template.conditions.iter().any(|(k, v)| {
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
        {
            return Err(invalid(
                "area identification must retain the original matched specimen/conditioning and room for FIT-domain metadata",
            ));
        }
        match self {
            Self::Preserve { allow_estimates } => {
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
            Self::Identify { parameter_bounds } => {
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
        template: &AreaResponse,
        parameters: &[f64],
        source: &str,
        conditions: &BTreeMap<String, String>,
        fitted_range: Option<[f64; 2]>,
    ) -> Result<(), Refusal> {
        let mut area = template.clone();
        if self.identifies() {
            if parameters.len() != 2 {
                return Err(invalid(
                    "area identification requires two coefficient coordinates",
                ));
            }
            if let Some(range) = fitted_range {
                if range[0] < template.area_ratio_range[0]
                    || range[1] > template.area_ratio_range[1]
                {
                    return Err(invalid(
                        "identified area support cannot widen the original J domain",
                    ));
                }
                area.area_ratio_range = range;
            }
            area.conditions = conditions.clone();
            for (q, &value) in [&mut area.quadratic, &mut area.log_squared]
                .into_iter()
                .zip(parameters)
            {
                q.value = value;
                q.unit = "N/m".into();
                q.evidence = Evidence::Estimate;
                q.source = source.into();
                q.conditions = conditions.clone();
                q.bounds = None;
                q.method = "Joint bounded finite-strip identification of the physical-area energy and directional response; conditional on the declared model, FIT-only support and independent CHECK replay. Search bounds are not confidence intervals.".into();
            }
        }
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
                " Two physical-area coefficients are jointly identified within the original J domain and actual FIT-only physical-area/strain support. Local excitation is not global uniqueness or empirical textile qualification."
            }
        }
    }
}

//! Source-qualified area energy in the same physical Green-strain membrane authority.
//!
//! Sperl et al. (2022), §5.1 Eq. (1), DOI 10.1145/3528223.3530167:
//! `W = k1 (J-1)^2 + k2 (log J)^2`, without a one-half prefactor. Here the energy is
//! per reference area and both coefficients are N/m. For an embedded sheet,
//! `J = sqrt(det(I+2E))` is the positive surface-area ratio, not a signed orientation.

use std::collections::BTreeMap;

use ftx_core::{Refusal, det};
use serde::{Deserialize, Serialize};

use super::{Response, TabulatedMembrane, evidence, invalid};
use crate::materials::{Quantity, Source};

/// Explicit physical-area extension; no area penalty is inferred from textile identity.
pub const AREA_MODEL: &str = "area_green_strain/1";

/// Source-condition key binding an identified area response to its actual FIT-only domain.
pub const FITTED_AREA_RANGE_KEY: &str = "fitted_area_ratio_range";

/// Jointly qualified coefficients added to the declared separable equilibrium law.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AreaResponse {
    /// Must be [`AREA_MODEL`].
    pub model: String,
    /// Coefficient of `(J-1)^2`, nonnegative N/m; explicit zero is permitted.
    pub quadratic: Quantity,
    /// Coefficient of `(log J)^2`, nonnegative N/m; explicit zero is permitted.
    pub log_squared: Quantity,
    /// Closed, positive characterized area-ratio interval containing the reference `J=1`.
    /// This intersects the three base curve ranges; it never widens them.
    pub area_ratio_range: [f64; 2],
    /// Shared specimen and conditioning, also present on every base and area observation.
    /// At least `specimen` and `conditioning` are required; other supplied keys match exactly.
    pub conditions: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct CompiledArea {
    quadratic: f64,
    log_squared: f64,
    range: [f64; 2],
}

fn valid_conditions(conditions: &BTreeMap<String, String>) -> bool {
    conditions.len() <= 64
        && conditions.iter().all(|(k, v)| {
            !k.trim().is_empty() && !v.trim().is_empty() && k.len() <= 4096 && v.len() <= 4096
        })
}

impl AreaResponse {
    pub(super) fn compile(
        &self,
        base: &TabulatedMembrane,
        sources: &BTreeMap<String, Source>,
        allow: bool,
    ) -> Result<CompiledArea, Refusal> {
        let [lo, hi] = self.area_ratio_range;
        if self.model != AREA_MODEL
            || !lo.is_finite()
            || !hi.is_finite()
            || lo <= 0.0
            || lo > 1.0
            || hi < 1.0
            || lo >= hi
            || !valid_conditions(&self.conditions)
            || !["specimen", "conditioning"]
                .iter()
                .all(|key| self.conditions.contains_key(*key))
        {
            return Err(invalid(
                "area response needs its model, matched specimen/conditioning and positive finite J bounds containing one",
            ));
        }
        for quantity in [&self.quadratic, &self.log_squared] {
            evidence(
                &quantity.source,
                quantity.evidence,
                &quantity.method,
                sources,
                allow,
            )?;
            if quantity.unit != "N/m"
                || !quantity.value.is_finite()
                || quantity.value < 0.0
                || !valid_conditions(&quantity.conditions)
                || quantity.bounds.is_some_and(|[lo, hi]| {
                    !lo.is_finite()
                        || !hi.is_finite()
                        || lo < 0.0
                        || lo > quantity.value
                        || hi < quantity.value
                })
            {
                return Err(invalid(
                    "area coefficients need sourced nonnegative N/m values and valid bounds/conditions",
                ));
            }
        }
        for conditions in [
            &base.warp.conditions,
            &base.weft.conditions,
            &base.shear.conditions,
            &base.coupling.conditions,
            &self.quadratic.conditions,
            &self.log_squared.conditions,
        ] {
            if self
                .conditions
                .iter()
                .any(|(key, value)| conditions.get(key) != Some(value))
            {
                return Err(invalid(
                    "area and base membrane observations must match every declared specimen/conditioning key",
                ));
            }
        }
        let fitted = self.conditions.contains_key(FITTED_AREA_RANGE_KEY)
            || [&self.quadratic, &self.log_squared].iter().any(|quantity| {
                quantity.conditions.contains_key(FITTED_AREA_RANGE_KEY)
                    || sources.get(&quantity.source).is_some_and(|source| {
                        [
                            "local:ftx-nonlinear-swatch-area-calibration/",
                            "local:ftx-strip-area-calibration/",
                        ]
                        .iter()
                        .any(|prefix| source.reference.starts_with(prefix))
                    })
            });
        if fitted {
            let recorded: [f64; 2] = serde_json::from_str(
                self.conditions.get(FITTED_AREA_RANGE_KEY).ok_or_else(|| {
                    invalid("identified area response is missing its FIT area-ratio domain")
                })?,
            )
            .map_err(|_| invalid("identified area response has invalid FIT area-ratio metadata"))?;
            if recorded != self.area_ratio_range
                || [&self.quadratic, &self.log_squared]
                    .iter()
                    .any(|quantity| quantity.source != base.coupling.source)
            {
                return Err(invalid(
                    "identified area response must retain its exact FIT area-ratio domain and joint membrane source",
                ));
            }
        }
        let compiled = CompiledArea {
            quadratic: self.quadratic.value,
            log_squared: self.log_squared.value,
            range: self.area_ratio_range,
        };
        if !compiled.origin_increment().is_finite() || compiled.origin_increment() <= 0.0 {
            return Err(invalid(
                "area response needs at least one positive coefficient and a representable reference tangent",
            ));
        }
        Ok(compiled)
    }
}

// Return J and a cancellation-resistant J-1. Positive C, not merely det(C)>0, is required.
fn metric(strain: [f64; 3]) -> Option<(f64, f64)> {
    if strain.iter().any(|x| !x.is_finite()) {
        return None;
    }
    let [x, y, z] = strain;
    let a = 1.0 + 2.0 * x;
    let b = 1.0 + 2.0 * y;
    if !a.is_finite() || !b.is_finite() || a <= 0.0 || b <= 0.0 {
        return None;
    }
    let delta = 2.0 * x + 2.0 * y + (2.0 * x) * (2.0 * y) - z * z;
    let determinant = if delta.abs() < 0.5 {
        1.0 + delta
    } else {
        a * b - z * z
    };
    if !determinant.is_finite() || determinant <= 0.0 {
        return None;
    }
    let j = det::sqrt(determinant);
    let minus_one = if delta.abs() < 0.5 {
        delta / (j + 1.0)
    } else {
        j - 1.0
    };
    Some((j, minus_one))
}

/// Positive surface-area ratio from physical `[E11, E22, 2 E12]` using the production metric.
/// Returns `None` for a nonfinite or non-positive-definite metric. This is geometry, not an
/// admission of any material's source qualifications or operating domain.
#[must_use]
pub fn physical_area_ratio(strain: [f64; 3]) -> Option<f64> {
    metric(strain).map(|(j, _)| j)
}

// Unit coefficient columns for linear identification, evaluated by the production constitutive
// authority. These mathematical basis responses do not supply material evidence or a domain.
pub(crate) fn area_response_basis(strain: [f64; 3]) -> Option<[Response; 2]> {
    let evaluate = |quadratic, log_squared| {
        CompiledArea {
            quadratic,
            log_squared,
            range: [f64::MIN_POSITIVE, f64::MAX],
        }
        .evaluate(strain)
    };
    Some([evaluate(1.0, 0.0)?, evaluate(0.0, 1.0)?])
}

// ln(J) near one uses its accurately retained difference. Fixed-order atanh terms avoid
// cancellation in ln(1+tiny) and platform libm; the far branch uses the shared deterministic log.
fn logarithm(j: f64, minus_one: f64) -> f64 {
    if minus_one.abs() >= 0.25 {
        return det::ln(j);
    }
    let y = minus_one / (2.0 + minus_one);
    let square = y * y;
    let mut power = 1.0;
    let mut sum = 1.0;
    for n in 1..16 {
        power *= square;
        sum += power / f64::from(2 * n + 1);
    }
    2.0 * y * sum
}

impl CompiledArea {
    pub(super) fn range(&self) -> [f64; 2] {
        self.range
    }

    pub(super) fn contains(&self, strain: [f64; 3]) -> bool {
        metric(strain).is_some_and(|(j, _)| j >= self.range[0] && j <= self.range[1])
    }

    pub(super) fn origin_increment(&self) -> f64 {
        2.0 * (self.quadratic + self.log_squared)
    }

    pub(super) fn evaluate(&self, strain: [f64; 3]) -> Option<Response> {
        let (j, minus_one) = metric(strain)?;
        if j < self.range[0] || j > self.range[1] {
            return None;
        }
        let log = logarithm(j, minus_one);
        let inverse = 1.0 / j;
        let mut first = 2.0 * self.quadratic * minus_one;
        let mut second = 2.0 * self.quadratic;
        if self.log_squared != 0.0 {
            first += 2.0 * self.log_squared * log * inverse;
            second += 2.0 * self.log_squared * (1.0 - log) * inverse * inverse;
        }
        let gradient = [
            (1.0 + 2.0 * strain[1]) * inverse,
            (1.0 + 2.0 * strain[0]) * inverse,
            -strain[2] * inverse,
        ];
        // H(W) = (W'' - W'/J) grad(J) grad(J)^T + (W'/J) H(det(C))/2.
        let metric_factor = first * inverse;
        let rank_factor = second - metric_factor;
        let mut tangent = [[0.0; 3]; 3];
        for i in 0..3 {
            for k in 0..=i {
                let value = rank_factor * gradient[i] * gradient[k];
                tangent[i][k] = value;
                tangent[k][i] = value;
            }
        }
        tangent[0][1] += 2.0 * metric_factor;
        tangent[1][0] += 2.0 * metric_factor;
        tangent[2][2] -= metric_factor;
        let response = Response {
            energy: self.quadratic * minus_one * minus_one + self.log_squared * log * log,
            stress: gradient.map(|value| first * value),
            tangent,
        };
        (response.energy.is_finite()
            && response.stress.iter().all(|v| v.is_finite())
            && response.tangent.iter().flatten().all(|v| v.is_finite()))
        .then_some(response)
    }
}

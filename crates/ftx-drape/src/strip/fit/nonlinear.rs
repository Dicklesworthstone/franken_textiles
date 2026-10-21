//! Parameterization only: every trial and published response uses the ordinary material compiler.

use std::collections::BTreeMap;

use ftx_core::Refusal;
use ftx_pattern::{
    material_response::{Interpolation, StrainDomain, StressCurve, TabulatedMembrane},
    materials::{Evidence, Quantity},
};
use serde::{Deserialize, Serialize};

use super::{family::ParameterDescriptor, invalid};

/// Declared, independently fitted signed intervals. No odd symmetry, compression branch,
/// strain knot, or tangent continuity is inferred from a fiber name or grip extension.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Nonlinear {
    /// Warp, weft and engineering-shear Green-strain knots, strictly increasing with exact zero
    /// and at least one negative and positive knot. These are declared model coordinates, not
    /// measured local strains; final publication is restricted to actual FIT element support.
    pub strain_knots: [Vec<f64>; 3],
    /// Positive finite [lower, upper] bounds for each consecutive interval's tangent, N/m.
    /// Parameters follow warp intervals, weft intervals, shear intervals, then coupling.
    pub tangent_bounds: [Vec<[f64; 2]>; 3],
    /// Constant dS11/dE22 = dS22/dE11, N/m. The entire box must be stable.
    pub coupling_bounds: [f64; 2],
}

impl Nonlinear {
    pub(super) fn validate(&self) -> Result<(), Refusal> {
        let count = self.bounds().len();
        if !(7..=12).contains(&count) {
            return Err(invalid(
                "nonlinear strip identification requires 7..12 independent signed-interval tangents and coupling parameters",
            ));
        }
        for (axis, (knots, bounds)) in self
            .strain_knots
            .iter()
            .zip(&self.tangent_bounds)
            .enumerate()
        {
            if knots.len() != bounds.len() + 1
                || knots.len() < 3
                || knots.iter().any(|v| !v.is_finite())
                || knots
                    .windows(2)
                    .any(|pair| pair[0] >= pair[1] || !(pair[1] - pair[0]).is_finite())
                || !knots.contains(&0.0)
                || knots[0] >= 0.0
                || knots[knots.len() - 1] <= 0.0
                || bounds.iter().any(|&[lo, hi]| {
                    !lo.is_finite()
                        || !hi.is_finite()
                        || lo <= 0.0
                        || hi <= lo
                        || !(hi - lo).is_finite()
                })
            {
                return Err(invalid(format!(
                    "nonlinear strip axis {axis} requires increasing signed knots with zero and one finite positive nonempty tangent bound per interval"
                )));
            }
        }
        let [lo, hi] = self.coupling_bounds;
        if !lo.is_finite() || !hi.is_finite() || hi <= lo || !(hi - lo).is_finite() {
            return Err(invalid(
                "nonlinear strip coupling bounds must be finite and nonempty",
            ));
        }
        let warp = self.tangent_bounds[0]
            .iter()
            .map(|b| b[0])
            .fold(f64::INFINITY, f64::min);
        let weft = self.tangent_bounds[1]
            .iter()
            .map(|b| b[0])
            .fold(f64::INFINITY, f64::min);
        let stability = (lo.abs().max(hi.abs()) / warp.sqrt()) / weft.sqrt();
        if !stability.is_finite() || stability >= 1.0 {
            return Err(invalid(
                "the entire nonlinear tangent/coupling parameter box must be positive-definite",
            ));
        }
        Ok(())
    }

    pub(super) fn bounds(&self) -> Vec<[f64; 2]> {
        self.tangent_bounds
            .iter()
            .flatten()
            .copied()
            .chain([self.coupling_bounds])
            .collect()
    }

    pub(super) fn domain(&self) -> Result<StrainDomain, Refusal> {
        StrainDomain::new(std::array::from_fn(|axis| {
            let knots = &self.strain_knots[axis];
            [knots[0], knots[knots.len() - 1]]
        }))
    }

    pub(super) fn descriptors(&self) -> Vec<ParameterDescriptor> {
        let mut descriptors = Vec::new();
        for (axis, knots) in ["warp", "weft", "shear"]
            .into_iter()
            .zip(&self.strain_knots)
        {
            for (interval, pair) in knots.windows(2).enumerate() {
                descriptors.push(ParameterDescriptor {
                    id: format!("{axis}_tangent_{interval}"),
                    label: format!("{axis} tangent [{}, {}]", pair[0], pair[1]),
                    unit: "N/m".into(),
                });
            }
        }
        descriptors.push(ParameterDescriptor {
            id: "normal_coupling".into(),
            label: "Normal coupling".into(),
            unit: "N/m".into(),
        });
        descriptors
    }

    pub(super) fn response(
        &self,
        parameters: &[f64],
        crop: Option<[[f64; 2]; 3]>,
        source: &str,
        conditions: &BTreeMap<String, String>,
    ) -> Result<TabulatedMembrane, Refusal> {
        if parameters.len() != self.bounds().len() || parameters.iter().any(|v| !v.is_finite()) {
            return Err(invalid(
                "nonlinear strip parameter count or arithmetic is invalid",
            ));
        }
        let mut curves = Vec::new();
        let mut offset = 0;
        for (axis, knots) in self.strain_knots.iter().enumerate() {
            let origin = knots
                .iter()
                .position(|&v| v == 0.0)
                .ok_or_else(|| invalid("nonlinear strip origin is absent"))?;
            let mut points: Vec<[f64; 2]> = knots.iter().map(|&strain| [strain, 0.0]).collect();
            for i in origin..knots.len() - 1 {
                points[i + 1][1] =
                    points[i][1] + parameters[offset + i] * (knots[i + 1] - knots[i]);
            }
            for i in (0..origin).rev() {
                points[i][1] =
                    points[i + 1][1] - parameters[offset + i] * (knots[i + 1] - knots[i]);
            }
            offset += knots.len() - 1;
            if let Some(ranges) = crop {
                let [lo, hi] = ranges[axis];
                if !(lo < 0.0 && hi > 0.0) || lo < knots[0] || hi > knots[knots.len() - 1] {
                    return Err(invalid(format!(
                        "nonlinear FIT domain on axis {axis} must characterize both signed branches inside declared knots; no compression or shear branch is invented"
                    )));
                }
                let value = |strain: f64| {
                    if let Some(point) = points.iter().find(|point| point[0] == strain) {
                        return point[1];
                    }
                    let j = knots
                        .partition_point(|&k| k <= strain)
                        .saturating_sub(1)
                        .min(knots.len() - 2);
                    points[j][1]
                        + (strain - knots[j]) * (points[j + 1][1] - points[j][1])
                            / (knots[j + 1] - knots[j])
                };
                let mut cropped = vec![[lo, value(lo)]];
                cropped.extend(
                    points
                        .iter()
                        .copied()
                        .filter(|point| point[0] > lo && point[0] < hi),
                );
                cropped.push([hi, value(hi)]);
                points = cropped;
            }
            curves.push(StressCurve {
                points,
                source: source.into(),
                evidence: Evidence::Estimate,
                method: "Bounded inverse clamped-strip force/width identification of signed piecewise-linear second-Piola resultants versus actual element Green strain; independent interval tangents, no inferred symmetry or local strain from grip extension".into(),
                conditions: conditions.clone(),
            });
        }
        let [warp, weft, shear]: [StressCurve; 3] = curves
            .try_into()
            .map_err(|_| invalid("nonlinear strip needs three material axes"))?;
        Ok(TabulatedMembrane {
            model: ftx_pattern::material_response::MODEL.into(),
            interpolation: Interpolation::PiecewiseLinear,
            warp, weft, shear,
            coupling: Quantity {
                value: parameters[offset], unit: "N/m".into(), evidence: Evidence::Estimate,
                source: source.into(), method: "Constant cross derivative identified jointly through finite clamped-strip equilibria".into(),
                conditions: conditions.clone(), bounds: None,
            },
        })
    }
}

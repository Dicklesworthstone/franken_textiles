//! Declared nonlinear shapes, compiled and evaluated by the ordinary membrane authority.
//! Identification changes three amplitudes and normal coupling, never knots or hidden branches.

use std::collections::BTreeMap;

use ftx_core::Refusal;
use ftx_pattern::{
    material_response::{
        CompiledMembrane, Interpolation, StrainDomain, StressCurve, TabulatedMembrane,
    },
    materials::{Evidence, Quantity, Source},
};
use serde::{Deserialize, Serialize};

use super::invalid;

/// Conditional family using supplied signed piecewise-linear shapes.
pub const MODEL: &str = "scaled_piecewise_linear/1";

/// Source-bearing fixed shapes; the fitted scales are dimensionless because ordinates are N/m.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Family {
    /// Must equal [`MODEL`]. No symmetry, missing branch or area penalty is inferred.
    pub model: String,
    /// Declared warp response before multiplication by the first parameter.
    pub warp: StressCurve,
    /// Declared weft response before multiplication by the second parameter.
    pub weft: StressCurve,
    /// Declared signed engineering-shear response before the third parameter.
    pub shear: StressCurve,
    /// Admission of estimated shape-node evidence, independently of acquisition and output consent.
    #[serde(default)]
    pub allow_estimated_shapes: bool,
}

/// Report-owned interpretation of each fitted coordinate, in the same order as parameters/bounds.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParameterDescriptor {
    /// Stable parameter identity.
    pub id: String,
    /// Human-readable physical meaning.
    pub label: String,
    /// Dimensionless amplitude or normal coupling in N/m.
    pub unit: String,
}

pub(super) fn descriptors() -> [ParameterDescriptor; 4] {
    [
        ("warp_scale", "Warp shape scale", "1"),
        ("weft_scale", "Weft shape scale", "1"),
        ("shear_scale", "Shear shape scale", "1"),
        ("normal_coupling", "Normal coupling", "N/m"),
    ]
    .map(|(id, label, unit)| ParameterDescriptor {
        id: id.into(),
        label: label.into(),
        unit: unit.into(),
    })
}

pub(super) struct Prepared {
    family: Family,
    sources: BTreeMap<String, Source>,
    candidate_source: String,
    domain: StrainDomain,
}

impl Prepared {
    pub(super) fn new(
        family: &Family,
        sources: &BTreeMap<String, Source>,
        bounds: [[f64; 2]; 4],
    ) -> Result<Self, Refusal> {
        if family.model != MODEL {
            return Err(invalid("unknown strip response family"));
        }
        if [&family.warp, &family.weft, &family.shear]
            .iter()
            .any(|curve| {
                curve.conditions.contains_key("fitted_green_strain_range")
                    || sources.get(&curve.source).is_some_and(|source| {
                        [
                            "local:ftx-strip-calibration/",
                            "local:ftx-swatch-calibration/",
                            "local:ftx-nonlinear-swatch-calibration/",
                        ]
                        .iter()
                        .any(|prefix| source.reference.starts_with(prefix))
                    })
            })
        {
            return Err(invalid(
                "declared shape curves cannot omit the coupling/support group of a previously fitted law; fitted-shape metadata or calibration source markers require independently qualified shape evidence",
            ));
        }
        if !family.allow_estimated_shapes
            && [&family.warp, &family.weft, &family.shear]
                .iter()
                .any(|curve| curve.evidence == Evidence::Estimate)
        {
            return Err(invalid(
                "estimated nonlinear shape nodes require response_family.allow_estimated_shapes, independently of observation and fitted-output permissions",
            ));
        }
        // A candidate coupling is an internal model coordinate, not an additional observation
        // attributed to a source curve. This source never appears in a published material.
        let mut candidate_source = "strip-family-candidate".to_owned();
        while sources.contains_key(&candidate_source) {
            candidate_source.push('-');
        }
        let mut sources = sources.clone();
        sources.insert(
            candidate_source.clone(),
            Source {
                reference: "local:unpublished-strip-family-candidate".into(),
                accessed_on: "internal candidate; not an acquisition date".into(),
                note: "Unpublished constant-coupling trial in the declared shape family; no measurement or accepted material is asserted.".into(),
            },
        );
        let mut prepared = Self {
            family: family.clone(),
            sources,
            candidate_source,
            domain: StrainDomain::new([[0.0, 0.0]; 3])?,
        };
        let template = prepared.compile([1.0, 1.0, 1.0, 0.0])?;
        prepared.domain = StrainDomain::new(template.strain_range())?;
        // Scaling positive directional tangents is monotone. The production compiler at the
        // smallest scales and largest |coupling| therefore checks the entire declared box.
        let coupling = bounds[3][0].abs().max(bounds[3][1].abs());
        prepared.compile([bounds[0][0], bounds[1][0], bounds[2][0], coupling])?;
        // Also reject nonrepresentable largest ordinates before any costly forward experiment.
        prepared.compile([bounds[0][1], bounds[1][1], bounds[2][1], coupling])?;
        Ok(prepared)
    }

    pub(super) fn domain(&self) -> StrainDomain {
        self.domain
    }

    fn response(&self, parameters: [f64; 4]) -> TabulatedMembrane {
        let curve = |shape: &StressCurve, scale: f64| {
            let mut curve = shape.clone();
            for point in &mut curve.points {
                point[1] *= scale;
            }
            curve
        };
        TabulatedMembrane {
            model: ftx_pattern::material_response::MODEL.into(),
            interpolation: Interpolation::PiecewiseLinear,
            warp: curve(&self.family.warp, parameters[0]),
            weft: curve(&self.family.weft, parameters[1]),
            shear: curve(&self.family.shear, parameters[2]),
            coupling: Quantity {
                value: parameters[3],
                unit: "N/m".into(),
                evidence: Evidence::Estimate,
                source: self.candidate_source.clone(),
                method: "Unpublished signed constant normal-coupling candidate in a fixed-shape inverse experiment.".into(),
                conditions: BTreeMap::new(),
                bounds: None,
            },
        }
    }

    pub(super) fn compile(&self, parameters: [f64; 4]) -> Result<CompiledMembrane, Refusal> {
        self.response(parameters).compile(&self.sources, true)
    }

    pub(super) fn publish(
        &self,
        parameters: [f64; 4],
        domain: StrainDomain,
        source: &str,
        conditions: &BTreeMap<String, String>,
    ) -> Result<TabulatedMembrane, Refusal> {
        let compiled = self.compile(parameters)?;
        let mut response = self.response(parameters);
        for (axis, curve) in [&mut response.warp, &mut response.weft, &mut response.shear]
            .into_iter()
            .enumerate()
        {
            let [lo, hi] = domain.ranges()[axis];
            if !(lo < 0.0 && hi > 0.0) {
                return Err(invalid(format!(
                    "nonlinear axis {axis} lacks both signed sides in the FIT-only strain support; no missing branch is invented"
                )));
            }
            let endpoint = |strain| -> Result<[f64; 2], Refusal> {
                let mut state = [0.0; 3];
                state[axis] = strain;
                Ok([strain, compiled.try_evaluate(state)?.stress[axis]])
            };
            // With piecewise-linear stresses, retaining every interior knot and evaluating the
            // two new endpoints preserves the same law and reference-zero integrated energy.
            let mut points = vec![endpoint(lo)?];
            points.extend(
                curve
                    .points
                    .iter()
                    .copied()
                    .filter(|p| p[0] > lo && p[0] < hi),
            );
            points.push(endpoint(hi)?);
            curve.points = points;
            curve.source = source.into();
            curve.evidence = Evidence::Estimate;
            curve.method = "Amplitude identified through finite-strip equilibria, conditional on the declared signed piecewise-linear shape; knots/relative shape are not independently identified. Published support is restricted to FIT states.".into();
            curve.conditions = conditions.clone();
        }
        response.coupling.source = source.into();
        response.coupling.conditions = conditions.clone();
        response.coupling.method = "Signed constant normal coupling jointly identified with three declared-shape amplitudes through finite-strip equilibria; no area-preserving extension.".into();
        Ok(response)
    }
}

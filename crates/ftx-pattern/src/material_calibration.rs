//! Identify the existing orthotropic sheet law from source-bearing planar swatch tests.
//!
//! Fits nominal force resultants P = F S, S = Q E, with Green strain, exactly the unsoftened
//! membrane used by ftx-drape. A constituent modulus, breaking force, or percent stretch at an
//! unspecified load is not a calibration. Unobserved force components remain absent, not zero.
//! The fit is a model estimate, never a replacement for a measured-material or real-fit claim.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode, linalg::lstsq, receipt::sha256_hex};
use serde::{Deserialize, Serialize};

use crate::material_response::{FITTED_AREA_RANGE_KEY, StrainDomain};
use crate::materials::{Evidence, Form, Material, Quantity, Source};

/// Identify the existing nonlinear membrane directly from finite-strain swatch observations.
pub mod nonlinear;

/// Input schema for reproducible swatch identification.
pub const SCHEMA: &str = "ftx-swatch-calibration/1";
const MAX_SAMPLES: usize = 4096;
const KEYS: [&str; 4] = [
    "warp_stiffness",
    "weft_stiffness",
    "poisson_warp_weft",
    "shear_stiffness",
];

/// Recover a fitted membrane's operating domain from its source-bearing quantities or curves.
/// Each set of four components belongs to one fit, with two more coefficients when area response
/// was jointly identified. Dropping or mixing components or bounds must not turn a calibrated
/// material into an unrestricted law. Independently authored materials retain their contract.
///
/// # Errors
/// A fitted coefficient has missing, inconsistent, nonfinite or reversed domain/source metadata.
pub fn fitted_domain(material: &Material) -> Result<Option<StrainDomain>, Refusal> {
    const KEY: &str = "fitted_green_strain_range";
    let scalar = KEYS.map(|key| {
        (
            key,
            material
                .properties
                .get(key)
                .map(|q| (q.source.as_str(), &q.conditions)),
        )
    });
    let nonlinear = material.membrane_response.as_ref().map(|response| {
        [
            (
                "warp curve",
                Some((response.warp.source.as_str(), &response.warp.conditions)),
            ),
            (
                "weft curve",
                Some((response.weft.source.as_str(), &response.weft.conditions)),
            ),
            (
                "shear curve",
                Some((response.shear.source.as_str(), &response.shear.conditions)),
            ),
            (
                "normal coupling",
                Some((
                    response.coupling.source.as_str(),
                    &response.coupling.conditions,
                )),
            ),
        ]
    });
    if let Some(components) = nonlinear {
        let fitted_area = components.iter().any(|(_, component)| {
            component.is_some_and(|(source, conditions)| {
                conditions.contains_key(FITTED_AREA_RANGE_KEY)
                    || material.sources.get(source).is_some_and(|source| {
                        [
                            "local:ftx-nonlinear-swatch-area-calibration/",
                            "local:ftx-strip-area-calibration/",
                        ]
                        .iter()
                        .any(|prefix| source.reference.starts_with(prefix))
                    })
            })
        });
        if fitted_area {
            let response = material.membrane_response.as_ref().unwrap();
            let area = response.area_response.as_ref().ok_or_else(|| {
                invalid("jointly fitted membrane is missing its identified area response")
            })?;
            let [lo, hi] = area.area_ratio_range;
            if !lo.is_finite() || !hi.is_finite() || lo <= 0.0 || lo >= hi || lo > 1.0 || hi < 1.0 {
                return Err(invalid(
                    "jointly fitted membrane has an invalid area domain",
                ));
            }
            for (name, component) in components.into_iter().chain([
                (
                    "quadratic area coefficient",
                    Some((area.quadratic.source.as_str(), &area.quadratic.conditions)),
                ),
                (
                    "log-squared area coefficient",
                    Some((
                        area.log_squared.source.as_str(),
                        &area.log_squared.conditions,
                    )),
                ),
                (
                    "area response",
                    Some((response.coupling.source.as_str(), &area.conditions)),
                ),
            ]) {
                let (source, conditions) = component.unwrap();
                let range = conditions.get(FITTED_AREA_RANGE_KEY).ok_or_else(|| {
                    invalid(format!(
                        "fitted membrane {name} is missing its physical-area domain"
                    ))
                })?;
                let range: [f64; 2] = serde_json::from_str(range)
                    .map_err(|e| invalid(format!("fitted membrane {name} area domain: {e}")))?;
                if source != response.coupling.source || range != area.area_ratio_range {
                    return Err(invalid(
                        "jointly fitted membrane components must share one source and physical-area domain",
                    ));
                }
            }
        }
    }
    let mut result: Option<StrainDomain> = None;
    for components in std::iter::once(scalar).chain(nonlinear) {
        let fitted = components.iter().any(|(_, component)| {
            component.is_some_and(|(source, conditions)| {
                conditions.contains_key(KEY)
                    || material.sources.get(source).is_some_and(|source| {
                        [
                            "local:ftx-swatch-calibration/",
                            "local:ftx-nonlinear-swatch-calibration/",
                            "local:ftx-strip-calibration/",
                            "local:ftx-cylinder-calibration/",
                            "local:ftx-cylinder-history-calibration/",
                            "local:ftx-nonlinear-swatch-area-calibration/",
                            "local:ftx-strip-area-calibration/",
                        ]
                        .iter()
                        .any(|prefix| source.reference.starts_with(prefix))
                    })
            })
        });
        if !fitted {
            continue;
        }
        let mut identity: Option<(&str, StrainDomain)> = None;
        for (name, component) in components {
            let (source, conditions) =
                component.ok_or_else(|| invalid(format!("fitted membrane is missing {name}")))?;
            let ranges = conditions.get(KEY).ok_or_else(|| {
                invalid(format!(
                    "fitted membrane {name} is missing its Green-strain domain"
                ))
            })?;
            let domain = StrainDomain::new(
                serde_json::from_str(ranges)
                    .map_err(|e| invalid(format!("fitted membrane {name} domain: {e}")))?,
            )?;
            if let Some((first_source, first_domain)) = identity
                && (first_source != source || first_domain != domain)
            {
                return Err(invalid(
                    "fitted membrane components must share one source and Green-strain domain",
                ));
            }
            identity = Some((source, domain));
        }
        if let Some((_, domain)) = identity {
            result = Some(if let Some(prior) = result {
                StrainDomain::new(std::array::from_fn(|axis| {
                    [
                        prior.ranges()[axis][0].max(domain.ranges()[axis][0]),
                        prior.ranges()[axis][1].min(domain.ranges()[axis][1]),
                    ]
                }))?
            } else {
                domain
            });
        }
    }
    if let Some(law) = &material.membrane_relaxation {
        let quantities = law
            .branches
            .iter()
            .flat_map(|branch| {
                [
                    (
                        branch.id.as_str(),
                        "stiffness ratio",
                        &branch.stiffness_ratio,
                    ),
                    (branch.id.as_str(), "recovery time", &branch.relaxation_time),
                ]
            })
            .chain(
                law.orthotropic_branches
                    .iter()
                    .map(|branch| (branch.id.as_str(), "recovery time", &branch.relaxation_time)),
            );
        for (branch, name, quantity) in quantities {
            let fitted = quantity
                .conditions
                .contains_key("cylinder_history_fit_sha256")
                || material
                    .sources
                    .get(&quantity.source)
                    .is_some_and(|source| {
                        source
                            .reference
                            .starts_with("local:ftx-cylinder-history-calibration/")
                    });
            if !fitted {
                continue;
            }
            let ranges = quantity.conditions.get(KEY).ok_or_else(|| {
                invalid(format!(
                    "fitted Maxwell branch {branch} {name} is missing its Green-strain domain"
                ))
            })?;
            let domain = StrainDomain::new(serde_json::from_str(ranges).map_err(|error| {
                invalid(format!("fitted Maxwell branch {branch} domain: {error}"))
            })?)?;
            if !domain.contains([0.0; 3])
                || domain
                    .ranges()
                    .iter()
                    .zip(law.strain_range)
                    .any(|(fitted, acquired)| fitted[0] < acquired[0] || fitted[1] > acquired[1])
            {
                return Err(invalid(
                    "fitted Maxwell support must include the reference and remain inside the original acquired law domain",
                ));
            }
            result = Some(if let Some(prior) = result {
                StrainDomain::new(std::array::from_fn(|axis| {
                    [
                        prior.ranges()[axis][0].max(domain.ranges()[axis][0]),
                        prior.ranges()[axis][1].min(domain.ranges()[axis][1]),
                    ]
                }))?
            } else {
                domain
            });
        }
    }
    Ok(result)
}

/// Held-out observations never participate in identification.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Partition {
    /// Determine the four independent stiffness coefficients.
    Fit,
    /// Evaluate the fitted model without altering its coefficients.
    Check,
}

/// One quasi-static planar test state in the specimen's warp/weft (wale/course) reference frame.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sample {
    /// Unique measurement identity; repetitions need distinct IDs.
    pub id: String,
    /// Deformation gradient, row-major; columns are deformed material axes per unit rest length.
    pub deformation: [[f64; 2]; 2],
    /// First Piola resultants P in N/m of REFERENCE width. None means unmeasured, not traction-free.
    /// For an axis-aligned strip P11 = axial force / undeformed strip width; measure lateral strain.
    pub nominal_n_m: [[Option<f64>; 2]; 2],
    /// Positive per-observation weighting scale in nominal N/m, not automatically a confidence bound.
    pub scale_n_m: f64,
    /// Source key in the input material. All samples must refer to the stated specimen/conditions.
    pub source: String,
    /// Evidence for this observation, independent of the fitted model's estimate classification.
    pub evidence: Evidence,
    /// Training or genuinely held-out observation.
    pub partition: Partition,
}

/// Offline input. Geometry, loads, conditioning and evidence survive in the report for replay.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    /// Must be [`SCHEMA`].
    pub schema: String,
    /// The actual textile specimen, with source map and any separately known physical properties.
    pub material: Material,
    /// Measurement protocol, e.g. fixture, preconditioning, loading rate and frame reconstruction.
    pub method: String,
    /// Shared test conditions. Different humidity/finish/temperature states require different fits.
    pub conditions: BTreeMap<String, String>,
    /// Explicit acquisition/analysis date, not the wall clock, so replay is deterministic.
    pub fitted_on: String,
    /// Observations, in a fixed order; maximum 4096.
    pub samples: Vec<Sample>,
    /// Explicit permission to use estimated observations. Fitted coefficients are ALWAYS estimates.
    #[serde(default)]
    pub allow_estimated_observations: bool,
    /// Permit replacement of previously present stiffness properties, retained in the input receipt.
    #[serde(default)]
    pub replace_existing: bool,
    /// Maximum RMS residual divided by the declared observation scale, for each partition.
    pub max_rms_scaled: f64,
    /// Maximum absolute scaled component residual, for each partition.
    pub max_peak_scaled: f64,
}

/// Misfit against observed components only; no statistical or physical-certification interpretation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Quality {
    /// Number of measured scalar force components, not number of test states.
    pub components: usize,
    /// RMS of (predicted - observed) / scale_n_m.
    pub rms_scaled: f64,
    /// Largest absolute scaled component residual.
    pub peak_scaled: f64,
}

/// Predicted forces and observed residuals for an individual state.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Residual {
    /// Measurement ID from the input.
    pub id: String,
    /// Whether this state participated in the fit.
    pub partition: Partition,
    /// Material-frame strain [E11, E22, 2 E12].
    pub green_strain: [f64; 3],
    /// Predicted nominal force resultants, in N/m of reference width.
    pub predicted_n_m: [[f64; 2]; 2],
    /// Predicted minus observed; unobserved components remain None.
    pub residual_n_m: [[Option<f64>; 2]; 2],
}

/// A replayable fit and a normal Material record, usable by the existing common resolver.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    /// `ftx-swatch-calibration-report/1`.
    pub schema: String,
    /// Exact input, including observations, source identities and previous property values.
    pub input: Input,
    /// SHA-256 of the serialized input; an identity, not evidence that the observations are true.
    pub input_sha256: String,
    /// Existing material type with the four fitted properties and a source pointing to this receipt.
    pub material: Material,
    /// Fitted Q in [E11, E22, gamma12] order, N/m, before gather-zone compression softening.
    pub stiffness_n_m: [[f64; 3]; 3],
    /// Minimum squared QR-equivalent pivot of the column-normalized design Gram matrix.
    pub excitation_pivot: f64,
    /// Residual quality of observations used to fit Q.
    pub fit: Quality,
    /// Held-out quality, or None when no independent observations were supplied.
    pub check: Option<Quality>,
    /// One residual record per input sample, in input order.
    pub residuals: Vec<Residual>,
    /// Supported strain envelope of FIT observations, not an extrapolation guarantee.
    pub fitted_strain_range: [[f64; 2]; 3],
    /// Mechanical properties still missing after identification; never filled with default values.
    pub missing_mechanics: Vec<String>,
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

/// Green strain and a four-column linear map from [Q11, Q22, Q12, G12] to row-major P.
fn design(sample: &Sample) -> Result<([f64; 3], [[f64; 4]; 4]), Refusal> {
    let [[a, b], [c, d]] = sample.deformation;
    let determinant = a * d - b * c;
    let norm2 = a * a + b * b + c * c + d * d;
    if !norm2.is_finite() || determinant <= 1e-12 * norm2 || !determinant.is_finite() {
        return Err(invalid(format!(
            "{}: singular, inverted or non-finite swatch deformation",
            sample.id
        )));
    }
    let e = [
        0.5 * (a * a + c * c - 1.0),
        0.5 * (b * b + d * d - 1.0),
        a * b + c * d,
    ];
    let rows = [
        [a * e[0], 0.0, a * e[1], b * e[2]],
        [0.0, b * e[1], b * e[0], a * e[2]],
        [c * e[0], 0.0, c * e[1], d * e[2]],
        [0.0, d * e[1], d * e[0], c * e[2]],
    ];
    if !rows.iter().flatten().all(|v| v.is_finite()) {
        return Err(invalid("swatch strain/design arithmetic overflow"));
    }
    Ok((e, rows))
}

struct ScaledDesign {
    normalized: Vec<[f64; 4]>,
    scales: [f64; 4],
    pivot: f64,
}

/// Check identification rather than silently taking a rank-deficient minimum-norm solution.
fn scaled_design(rows: &[[f64; 4]]) -> Result<ScaledDesign, Refusal> {
    let scales: [f64; 4] =
        std::array::from_fn(|j| rows.iter().map(|r| r[j] * r[j]).sum::<f64>().sqrt());
    if scales.iter().any(|s| !s.is_finite() || *s <= 0.0) {
        return Err(invalid(
            "swatches do not independently excite warp, weft, coupling and shear",
        ));
    }
    let normalized: Vec<[f64; 4]> = rows
        .iter()
        .map(|r| std::array::from_fn(|j| r[j] / scales[j]))
        .collect();
    let mut l = [[0.0; 4]; 4];
    let mut pivot = 1.0_f64;
    for i in 0..4 {
        for j in 0..=i {
            let mut value: f64 = normalized.iter().map(|r| r[i] * r[j]).sum();
            for k in 0..j {
                value -= l[i][k] * l[j][k];
            }
            if i == j {
                if !value.is_finite() || value <= 1e-8 {
                    return Err(invalid(
                        "rank-deficient or poorly excited swatch fit; add independent biaxial/shear tests",
                    ));
                }
                pivot = pivot.min(value);
                l[i][j] = value.sqrt();
            } else {
                l[i][j] = value / l[j][j];
            }
        }
    }
    Ok(ScaledDesign {
        normalized,
        scales,
        pivot,
    })
}

fn quality(values: &[f64], input: &Input, partition: &str) -> Result<Quality, Refusal> {
    let peak = values.iter().copied().map(f64::abs).fold(0.0, f64::max);
    let rms = (values.iter().map(|v| v * v).sum::<f64>() / values.len() as f64).sqrt();
    if !rms.is_finite()
        || !peak.is_finite()
        || rms > input.max_rms_scaled
        || peak > input.max_peak_scaled
    {
        return Err(invalid(format!(
            "{partition} swatch residuals exceed the declared model budget (RMS {rms}, peak {peak}); do not publish this law as a characterized fit"
        )));
    }
    Ok(Quality {
        components: values.len(),
        rms_scaled: rms,
        peak_scaled: peak,
    })
}

/// Fit the solver's effective orthotropic law, refusing missing excitation, unstable laws and misfit.
/// No mass, thickness, bending, interface friction or thermal property is guessed from composition.
///
/// # Errors
/// Invalid/mixed evidence, missing force observations, non-finite inputs, insufficient independent
/// strain states, non-positive-definite stiffness, source-key conflicts, or excessive fit/check error.
pub fn calibrate(input: &Input) -> Result<Report, Refusal> {
    input.material.validate()?;
    if input
        .material
        .structure
        .contains_key("membrane_plasticity_model")
        || input
            .material
            .structure
            .contains_key("membrane_damage_model")
    {
        return Err(invalid(
            "elastic swatch calibration does not identify permanent membrane history; recharacterize the cyclic law before replacing its elastic tensor",
        ));
    }
    if input.schema != SCHEMA
        || input.method.trim().is_empty()
        || input.fitted_on.trim().is_empty()
        || input.samples.is_empty()
        || input.samples.len() > MAX_SAMPLES
        || !input.max_rms_scaled.is_finite()
        || input.max_rms_scaled <= 0.0
        || !input.max_peak_scaled.is_finite()
        || input.max_peak_scaled <= 0.0
        || matches!(input.material.form, Form::Fiber | Form::Yarn | Form::Fill)
    {
        return Err(invalid(
            "invalid swatch schema, protocol, material form, sample count or residual budget",
        ));
    }
    if !input.replace_existing
        && KEYS
            .iter()
            .any(|k| input.material.properties.contains_key(*k))
    {
        return Err(invalid(
            "calibration would replace existing stiffness evidence; explicit replace_existing is required",
        ));
    }
    let mut ids = BTreeSet::new();
    let mut rows = Vec::new();
    let mut target = Vec::new();
    let mut designs = Vec::new();
    let mut ranges = [[f64::INFINITY, f64::NEG_INFINITY]; 3];
    for sample in &input.samples {
        if sample.id.trim().is_empty()
            || !ids.insert(&sample.id)
            || !input.material.sources.contains_key(&sample.source)
            || (sample.evidence == Evidence::Estimate && !input.allow_estimated_observations)
            || !sample.scale_n_m.is_finite()
            || sample.scale_n_m <= 0.0
            || sample.nominal_n_m.iter().flatten().all(Option::is_none)
        {
            return Err(invalid(format!(
                "{}: missing/duplicate ID, source, admitted evidence, load or weighting scale",
                sample.id
            )));
        }
        let (strain, design) = design(sample)?;
        for (i, observation) in sample.nominal_n_m.iter().flatten().enumerate() {
            if let Some(value) = observation {
                if !value.is_finite() {
                    return Err(invalid(format!("{}: non-finite nominal load", sample.id)));
                }
                if sample.partition == Partition::Fit {
                    let row = design[i].map(|v| v / sample.scale_n_m);
                    let value = value / sample.scale_n_m;
                    if !row.iter().all(|v| v.is_finite()) || !value.is_finite() {
                        return Err(invalid("swatch weighting arithmetic overflow"));
                    }
                    rows.push(row);
                    target.push(value);
                }
            }
        }
        if sample.partition == Partition::Fit {
            for (range, value) in ranges.iter_mut().zip(strain) {
                range[0] = range[0].min(value);
                range[1] = range[1].max(value);
            }
        }
        designs.push((strain, design));
    }
    let ScaledDesign {
        normalized,
        scales,
        pivot,
    } = scaled_design(&rows)?;
    let fitted = lstsq(&normalized, &target);
    let q: [f64; 4] = std::array::from_fn(|j| fitted[j] / scales[j]);
    let values = [
        q[0] - q[2] * (q[2] / q[1]),
        q[1] - q[2] * (q[2] / q[0]),
        q[2] / q[1],
        q[3],
    ];
    if q.iter().any(|v| !v.is_finite())
        || q[0] <= 0.0
        || q[1] <= 0.0
        || q[3] <= 0.0
        || values.iter().any(|v| !v.is_finite())
        || values[0] <= 0.0
        || values[1] <= 0.0
    {
        return Err(invalid(
            "identified swatch law is not positive definite; measurements/model must be reviewed",
        ));
    }
    let mut fit_errors = Vec::new();
    let mut check_errors = Vec::new();
    let mut residuals = Vec::new();
    for (sample, (strain, rows)) in input.samples.iter().zip(designs) {
        let prediction: [f64; 4] = rows.map(|r| r.iter().zip(q).map(|(a, b)| a * b).sum());
        if prediction.iter().any(|v| !v.is_finite()) {
            return Err(invalid("swatch prediction arithmetic overflow"));
        }
        let mut residual = [[None; 2]; 2];
        for i in 0..4 {
            if let Some(observed) = sample.nominal_n_m[i / 2][i % 2] {
                let delta = prediction[i] - observed;
                residual[i / 2][i % 2] = Some(delta);
                let errors = if sample.partition == Partition::Fit {
                    &mut fit_errors
                } else {
                    &mut check_errors
                };
                errors.push(delta / sample.scale_n_m);
            }
        }
        residuals.push(Residual {
            id: sample.id.clone(),
            partition: sample.partition,
            green_strain: strain,
            predicted_n_m: [
                [prediction[0], prediction[1]],
                [prediction[2], prediction[3]],
            ],
            residual_n_m: residual,
        });
    }
    let fit = quality(&fit_errors, input, "fit")?;
    let check = if check_errors.is_empty() {
        None
    } else {
        Some(quality(&check_errors, input, "held-out")?)
    };
    let bytes = serde_json::to_vec(input).map_err(|e| invalid(e.to_string()))?;
    let digest = sha256_hex(&bytes);
    let source_id = format!("swatch-{digest}");
    let mut material = input.material.clone();
    if material.sources.contains_key(&source_id) {
        return Err(invalid("calibration receipt source ID already exists"));
    }
    material.sources.insert(source_id.clone(), Source {
        reference: format!("local:ftx-swatch-calibration/{digest}"),
        accessed_on: input.fitted_on.clone(),
        note: format!("Model identified from source-bearing swatches; exact observations and prior evidence in ftx-swatch-calibration-report/1 input SHA-256 {digest}. Protocol: {}", input.method),
    });
    let mut conditions = input.conditions.clone();
    conditions.insert(
        "fitted_green_strain_range".into(),
        serde_json::to_string(&ranges).map_err(|e| invalid(e.to_string()))?,
    );
    conditions.insert("held_out_checks".into(), check.is_some().to_string());
    for (key, value) in KEYS.into_iter().zip(values) {
        material.properties.insert(key.into(), Quantity {
            value, unit: if key == "poisson_warp_weft" { "1" } else { "N/m" }.into(),
            evidence: Evidence::Estimate, source: source_id.clone(),
            method: "Weighted least squares of nominal P=F*S, S=Q*GreenStrain; effective orthotropic sheet, not fiber modulus. No compression-softening or nonlinear/hysteresis identification.".into(),
            conditions: conditions.clone(), bounds: None,
        });
    }
    material.validate()?;
    let missing_mechanics = material
        .missing_mechanics()
        .into_iter()
        .map(str::to_owned)
        .collect();
    Ok(Report {
        schema: "ftx-swatch-calibration-report/1".into(),
        input: input.clone(),
        input_sha256: digest,
        material,
        stiffness_n_m: [[q[0], q[2], 0.0], [q[2], q[1], 0.0], [0.0, 0.0, q[3]]],
        excitation_pivot: pivot,
        fit,
        check,
        residuals,
        fitted_strain_range: ranges,
        missing_mechanics,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::material_relaxation::{
        Initialization, IntegrationScheme, MaxwellBranch, OrthotropicBranch, OrthotropicStiffness,
        Relaxation,
    };

    const DOMAIN_KEY: &str = "fitted_green_strain_range";
    const BRANCH_DOMAIN: [[f64; 2]; 3] = [[-0.05, 0.12], [-0.12, 0.20], [-0.10, 0.18]];

    fn history_material() -> Material {
        // Synthetic provenance classifications exercise admission; these are not measurements.
        let conditions = BTreeMap::from([
            ("specimen_id".into(), "synthetic-domain-coupon".into()),
            ("conditioning".into(), "synthetic-dry-reference".into()),
        ]);
        let quantity = |value, unit: &str| Quantity {
            value,
            unit: unit.into(),
            evidence: Evidence::Measured,
            source: "acquired".into(),
            method: "Synthetic source-authority fixture, not empirical textile data".into(),
            conditions: conditions.clone(),
            bounds: None,
        };
        Material {
            id: "synthetic-history-domains".into(),
            name: "Synthetic history domain authority".into(),
            form: Form::Woven,
            composition: vec![],
            composition_complete: false,
            structure: BTreeMap::new(),
            properties: BTreeMap::from([
                ("warp_stiffness".into(), quantity(1000.0, "N/m")),
                ("weft_stiffness".into(), quantity(600.0, "N/m")),
                ("poisson_warp_weft".into(), quantity(0.2, "1")),
                ("shear_stiffness".into(), quantity(150.0, "N/m")),
            ]),
            membrane_response: None,
            membrane_relaxation: Some(Relaxation {
                model: "green_maxwell/1".into(),
                branches: vec![MaxwellBranch {
                    id: "proportional".into(),
                    stiffness_ratio: quantity(0.8, "1"),
                    relaxation_time: quantity(1.0, "s"),
                }],
                orthotropic_branches: vec![OrthotropicBranch {
                    id: "directional".into(),
                    stiffness: OrthotropicStiffness {
                        warp: quantity(200.0, "N/m"),
                        weft: quantity(100.0, "N/m"),
                        coupling: quantity(10.0, "N/m"),
                        shear: quantity(40.0, "N/m"),
                    },
                    relaxation_time: quantity(0.2, "s"),
                }],
                initialization: Initialization::StressFreeReference,
                strain_range: [[-0.20, 0.30], [-0.20, 0.30], [-0.30, 0.30]],
                source: "acquired".into(),
                method: "Synthetic source domain, not physical characterization".into(),
                conditions: conditions.clone(),
                parallel_plasticity: None,
                integration: IntegrationScheme::default(),
            }),
            thermal_response: None,
            optical_faces: None,
            sources: [
                ("acquired", "test:synthetic-maxwell-domain"),
                (
                    "equilibrium",
                    "local:ftx-swatch-calibration/synthetic-equilibrium",
                ),
                (
                    "ratio-fit",
                    "local:ftx-cylinder-history-calibration/synthetic-ratio",
                ),
                (
                    "time-fit",
                    "local:ftx-cylinder-history-calibration/synthetic-time",
                ),
                (
                    "directional-fit",
                    "local:ftx-cylinder-history-calibration/synthetic-directional",
                ),
            ]
            .into_iter()
            .map(|(id, reference)| {
                (
                    id.into(),
                    Source {
                        reference: reference.into(),
                        accessed_on: "2026-11-24".into(),
                        note: "Synthetic authority test; no laboratory or supplier claim".into(),
                    },
                )
            })
            .collect(),
        }
    }

    fn history_quantity(material: &mut Material, index: usize) -> &mut Quantity {
        let law = material.membrane_relaxation.as_mut().unwrap();
        match index {
            0 => &mut law.branches[0].stiffness_ratio,
            1 => &mut law.branches[0].relaxation_time,
            2 => &mut law.orthotropic_branches[0].relaxation_time,
            _ => unreachable!(),
        }
    }

    #[test]
    fn cylinder_history_source_or_fit_marker_requires_each_target_domain() {
        let pristine = history_material();
        pristine.validate().unwrap();
        assert_eq!(fitted_domain(&pristine).unwrap(), None);
        for index in 0..3 {
            for source_authority in [true, false] {
                let mut material = pristine.clone();
                let target = history_quantity(&mut material, index);
                if source_authority {
                    target.source = "ratio-fit".into();
                } else {
                    target
                        .conditions
                        .insert("cylinder_history_fit_sha256".into(), "synthetic-fit".into());
                }
                target.evidence = Evidence::Estimate;
                target.conditions.insert(
                    DOMAIN_KEY.into(),
                    serde_json::to_string(&BRANCH_DOMAIN).unwrap(),
                );
                material.validate().unwrap();
                assert_eq!(
                    fitted_domain(&material).unwrap().unwrap().ranges(),
                    BRANCH_DOMAIN
                );

                history_quantity(&mut material, index)
                    .conditions
                    .remove(DOMAIN_KEY);
                assert!(
                    fitted_domain(&material)
                        .unwrap_err()
                        .message
                        .contains("missing its Green-strain domain")
                );
                assert!(material.validate().is_err());
            }
        }
    }

    #[test]
    fn independent_history_fits_intersect_without_rewriting_equilibrium_support() {
        let mut material = history_material();
        let equilibrium_domain = [[-0.10, 0.20], [-0.08, 0.16], [-0.15, 0.15]];
        for key in KEYS {
            let quantity = material.properties.get_mut(key).unwrap();
            quantity.source = "equilibrium".into();
            quantity.conditions.insert(
                DOMAIN_KEY.into(),
                serde_json::to_string(&equilibrium_domain).unwrap(),
            );
        }
        let equilibrium = serde_json::to_value(&material.properties).unwrap();
        let acquired = material.membrane_relaxation.as_ref().unwrap().strain_range;
        for (index, (source, domain)) in [
            ("ratio-fit", BRANCH_DOMAIN),
            ("time-fit", [[-0.04, 0.11], [-0.04, 0.12], [-0.12, 0.12]]),
            (
                "directional-fit",
                [[-0.06, 0.13], [-0.06, 0.10], [-0.08, 0.14]],
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let target = history_quantity(&mut material, index);
            target.source = source.into();
            target.evidence = Evidence::Estimate;
            target
                .conditions
                .insert(DOMAIN_KEY.into(), serde_json::to_string(&domain).unwrap());
        }
        material.validate().unwrap();
        let domain = fitted_domain(&material).unwrap().unwrap();
        assert_eq!(
            domain.ranges(),
            [[-0.04, 0.11], [-0.04, 0.10], [-0.08, 0.12]]
        );
        assert!(domain.contains([0.11, 0.10, 0.12]));
        assert!(!domain.contains([0.110000001, 0.0, 0.0]));
        assert!(!domain.contains([0.0, -0.040000001, 0.0]));
        assert!(!domain.contains([0.0, 0.0, 0.120000001]));
        assert_eq!(
            serde_json::to_value(&material.properties).unwrap(),
            equilibrium
        );
        assert_eq!(
            material.membrane_relaxation.as_ref().unwrap().strain_range,
            acquired
        );
        let replay: Material =
            serde_json::from_slice(&serde_json::to_vec(&material).unwrap()).unwrap();
        assert_eq!(fitted_domain(&replay).unwrap(), Some(domain));
    }

    #[test]
    fn fitted_history_support_cannot_escape_acquisition_or_omit_the_reference() {
        let mut material = history_material();
        let acquired = material.membrane_relaxation.as_ref().unwrap().strain_range;
        let target = history_quantity(&mut material, 1);
        target.source = "time-fit".into();
        target
            .conditions
            .insert(DOMAIN_KEY.into(), serde_json::to_string(&acquired).unwrap());
        material.validate().unwrap();
        assert_eq!(
            fitted_domain(&material).unwrap().unwrap().ranges(),
            acquired
        );
        for malformed in [
            "not-json",
            "[[0,0]]",
            "[[-0.1,0.1],[0.1,-0.1],[-0.1,0.1]]",
            "[[-0.1,1e400],[-0.1,0.1],[-0.1,0.1]]",
            "[[0.001,0.1],[-0.1,0.1],[-0.1,0.1]]",
            "[[-0.21,0.1],[-0.1,0.1],[-0.1,0.1]]",
            "[[-0.1,0.31],[-0.1,0.1],[-0.1,0.1]]",
        ] {
            history_quantity(&mut material, 1)
                .conditions
                .insert(DOMAIN_KEY.into(), malformed.into());
            assert!(fitted_domain(&material).is_err(), "accepted {malformed}");
            assert!(material.validate().is_err(), "accepted {malformed}");
        }
    }

    #[test]
    fn history_evidence_labels_cannot_bypass_support_or_invalid_quantity_bounds() {
        let mut material = history_material();
        let target = history_quantity(&mut material, 1);
        target.source = "time-fit".into();
        target.conditions.insert(
            DOMAIN_KEY.into(),
            serde_json::to_string(&BRANCH_DOMAIN).unwrap(),
        );
        for evidence in [Evidence::Estimate, Evidence::Measured, Evidence::Published] {
            let mut labeled = material.clone();
            history_quantity(&mut labeled, 1).evidence = evidence;
            labeled.validate().unwrap();
            assert_eq!(
                fitted_domain(&labeled).unwrap().unwrap().ranges(),
                BRANCH_DOMAIN
            );
            history_quantity(&mut labeled, 1)
                .conditions
                .remove(DOMAIN_KEY);
            assert!(labeled.validate().is_err());
        }
        let mut encoded = serde_json::to_value(&material).unwrap();
        encoded["membrane_relaxation"]["branches"][0]["relaxation_time"]["evidence"] =
            "model_identified".into();
        assert!(serde_json::from_value::<Material>(encoded).is_err());

        history_quantity(&mut material, 1).bounds = Some([0.1, 0.2]);
        assert!(
            material.validate().is_err(),
            "old bounds cannot qualify the fitted 1 s value"
        );
        history_quantity(&mut material, 1).bounds = None;
        history_quantity(&mut material, 1).evidence = Evidence::Estimate;
        material.validate().unwrap();
        let law = material.membrane_relaxation.as_ref().unwrap();
        let tangent = [[1000.0, 50.0, 0.0], [50.0, 600.0, 0.0], [0.0, 0.0, 150.0]];
        assert!(law.compile(&material.sources, false, tangent).is_err());
        assert!(law.compile(&material.sources, true, tangent).is_ok());
    }
}

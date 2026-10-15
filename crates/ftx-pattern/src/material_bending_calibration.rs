//! Identify the directional panel-bending tensor from source-bearing moment/curvature coupons.
//!
//! Fits moment resultants m = D (k - k0) in the same material-frame engineering-curvature basis
//! used by orthotropic_normal_patch/1: [k_uu, k_vv, 2 k_uv]. D has units N m, while moment
//! resultants are N per unit reference width. Coupon geometry and force-to-moment reduction belong
//! in the acquisition method; this module never infers a solid-plate E t^3 law from thickness.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode, linalg::lstsq, receipt::sha256_hex};
use serde::{Deserialize, Serialize};

use crate::material_bending::{BendingTensor, NORMAL_MODEL, PREFERRED_CURVATURE_PROPERTIES};
use crate::material_calibration::{Partition, Quality};
use crate::material_catalog::normalize;
use crate::materials::{Evidence, Form, Material, Quantity, Source};

/// Portable source-bearing coupon calibration schema.
pub const SCHEMA: &str = "ftx-bending-calibration/1";
const MAX_SAMPLES: usize = 4096;
const KEYS: [&str; 4] = [
    "bending_warp",
    "bending_weft",
    "bending_coupling",
    "bending_twist",
];

/// One material-frame coupon state. Unknown moment components are absent, never zero.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sample {
    /// Unique acquisition/state identity; repeated states need distinct IDs.
    pub id: String,
    /// Normal curvature [k_uu, k_vv, 2 k_uv] in inverse reference metres.
    pub curvature_m_inv: [f64; 3],
    /// Bending moment resultants [M_uu, M_vv, M_uv] in N per unit reference width.
    pub moment_n: [Option<f64>; 3],
    /// Positive residual scale in N; a weighting scale, not automatically measurement uncertainty.
    pub scale_n: f64,
    /// Source key in the input material describing this coupon acquisition.
    pub source: String,
    /// Evidence class of the observation, independent of fitted coefficients always being estimates.
    pub evidence: Evidence,
    /// Training or genuinely held-out observation.
    pub partition: Partition,
}

/// Offline identification problem; raw specimen metadata remains in the returned receipt.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    /// Must equal SCHEMA.
    pub schema: String,
    /// Exact specimen/product/construction record and its source map.
    pub material: Material,
    /// Fixture, coupon dimensions, curvature reconstruction and force-to-moment reduction protocol.
    pub method: String,
    /// Shared conditioning/test state; incompatible conditions require separate fits.
    pub conditions: BTreeMap<String, String>,
    /// Acquisition/analysis date carried into the generated source record.
    pub fitted_on: String,
    /// Moment/curvature observations in deterministic order.
    pub samples: Vec<Sample>,
    /// Explicit permission to use estimated observations or estimated preferred curvature inputs.
    #[serde(default)]
    pub allow_estimated_observations: bool,
    /// Explicit permission to replace existing directional bending coefficients/model.
    #[serde(default)]
    pub replace_existing: bool,
    /// Maximum RMS scaled residual for fit and held-out partitions.
    pub max_rms_scaled: f64,
    /// Maximum absolute scaled residual for fit and held-out partitions.
    pub max_peak_scaled: f64,
}

/// Prediction and residual for one coupon state.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Prediction {
    /// Input sample identity.
    pub id: String,
    /// Whether this coupon state participated in identification or was held out.
    pub partition: Partition,
    /// Curvature minus the admitted preferred curvature, in inverse reference metres.
    pub bending_strain_m_inv: [f64; 3],
    /// Model-predicted moment resultants in N.
    pub predicted_moment_n: [f64; 3],
    /// Predicted minus observed; unobserved components remain absent.
    pub residual_n: [Option<f64>; 3],
}

/// Reproducible fit and ordinary material record consumable by the existing resolver/drape path.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    /// ftx-bending-calibration-report/1.
    pub schema: String,
    /// Complete calibration input, including prior material evidence.
    pub input: Input,
    /// SHA-256 identity of the serialized input; not evidence authenticity.
    pub input_sha256: String,
    /// Ordinary material record carrying the fitted tensor and receipt source.
    pub material: Material,
    /// Fitted D matrix in [k_uu, k_vv, 2 k_uv] order, in N m.
    pub bending_nm: [[f64; 3]; 3],
    /// Preferred curvature subtracted during identification, in inverse metres.
    pub preferred_curvature_m_inv: [f64; 3],
    /// Minimum Cholesky pivot of the normalized fit design.
    pub excitation_pivot: f64,
    /// Residual quality for observations used in the fit.
    pub fit: Quality,
    /// Held-out residual quality, absent when no held-out moments were supplied.
    pub check: Option<Quality>,
    /// Stable predictions and residuals for every input sample.
    pub predictions: Vec<Prediction>,
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

fn quality(errors: &[f64], input: &Input, partition: &str) -> Result<Quality, Refusal> {
    if errors.is_empty() {
        return Err(invalid(format!(
            "{partition} bending calibration has no observed moments"
        )));
    }
    let peak = errors.iter().copied().map(f64::abs).fold(0.0, f64::max);
    let rms = (errors.iter().map(|v| v * v).sum::<f64>() / errors.len() as f64).sqrt();
    if !rms.is_finite()
        || !peak.is_finite()
        || rms > input.max_rms_scaled
        || peak > input.max_peak_scaled
    {
        return Err(invalid(format!(
            "{partition} bending residuals exceed the declared model budget (RMS {rms}, peak {peak})"
        )));
    }
    Ok(Quality {
        components: errors.len(),
        rms_scaled: rms,
        peak_scaled: peak,
    })
}

struct Design {
    rows: Vec<[f64; 4]>,
    target: Vec<f64>,
    scales: [f64; 4],
    pivot: f64,
}

fn design(input: &Input, preferred: [f64; 3]) -> Result<Design, Refusal> {
    let mut rows = Vec::new();
    let mut target = Vec::new();
    let mut ids = BTreeSet::new();
    for s in &input.samples {
        if s.id.trim().is_empty()
            || !ids.insert(&s.id)
            || !input.material.sources.contains_key(&s.source)
            || (s.evidence == Evidence::Estimate && !input.allow_estimated_observations)
            || !s.scale_n.is_finite()
            || s.scale_n <= 0.0
            || s.curvature_m_inv.iter().any(|v| !v.is_finite())
            || s.moment_n.iter().flatten().any(|v| !v.is_finite())
            || s.moment_n.iter().all(Option::is_none)
        {
            return Err(invalid(format!(
                "{}: missing/duplicate ID, source, admitted evidence, moment, finite curvature or scale",
                s.id
            )));
        }
        if s.partition != Partition::Fit {
            continue;
        }
        let k: [f64; 3] = std::array::from_fn(|i| s.curvature_m_inv[i] - preferred[i]);
        let basis = [
            [k[0], 0.0, k[1], 0.0],
            [0.0, k[1], k[0], 0.0],
            [0.0, 0.0, 0.0, k[2]],
        ];
        for (i, observed) in s.moment_n.iter().enumerate() {
            if let Some(value) = observed {
                rows.push(basis[i].map(|v| v / s.scale_n));
                target.push(value / s.scale_n);
            }
        }
    }
    if rows.len() < 4 {
        return Err(invalid(
            "bending fit needs at least four observed moment components with independent curvature excitation",
        ));
    }
    let scales: [f64; 4] =
        std::array::from_fn(|j| rows.iter().map(|r| r[j] * r[j]).sum::<f64>().sqrt());
    if scales.iter().any(|s| !s.is_finite() || *s <= 0.0) {
        return Err(invalid(
            "bending coupons do not independently excite warp, weft, coupling and twist",
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
                        "rank-deficient or poorly excited bending fit; add independent warp/weft/twist coupon states",
                    ));
                }
                pivot = pivot.min(value);
                l[i][j] = value.sqrt();
            } else {
                l[i][j] = value / l[j][j];
            }
        }
    }
    Ok(Design {
        rows: normalized,
        target,
        scales,
        pivot,
    })
}

fn preferred_curvature(input: &Input) -> Result<[f64; 3], Refusal> {
    let normalized = normalize(&input.material)?;
    let present = PREFERRED_CURVATURE_PROPERTIES
        .map(|key| normalized.properties.contains_key(key));
    if present.iter().any(|x| *x) && !present.iter().all(|x| *x) {
        return Err(invalid(
            "preferred bending curvature must be absent or complete before stiffness calibration",
        ));
    }
    if !present[0] {
        return Ok([0.0; 3]);
    }
    let mut out = [0.0; 3];
    for (value, key) in out.iter_mut().zip(PREFERRED_CURVATURE_PROPERTIES) {
        *value = normalized.require(key, "1/m", input.allow_estimated_observations)?;
    }
    Ok(out)
}

/// Fit the exact four-coefficient tensor used by the normal-projected panel model.
pub fn calibrate(input: &Input) -> Result<Report, Refusal> {
    input.material.validate()?;
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
            "invalid bending calibration schema, protocol, material form, sample count or residual budget",
        ));
    }
    let has_prior = KEYS
        .iter()
        .any(|key| input.material.properties.contains_key(*key))
        || input.material.structure.contains_key("bending_model");
    if has_prior && !input.replace_existing {
        return Err(invalid(
            "bending calibration would replace existing tensor/model evidence; explicit replace_existing is required",
        ));
    }

    let preferred = preferred_curvature(input)?;
    let design = design(input, preferred)?;
    let fitted = lstsq(&design.rows, &design.target);
    if fitted.len() < 4 {
        return Err(invalid(
            "bending least-squares solver returned an incomplete tensor",
        ));
    }
    let values: [f64; 4] = std::array::from_fn(|j| fitted[j] / design.scales[j]);
    let law = BendingTensor::new(values[0], values[1], values[2], values[3])?
        .normal_projected();
    let matrix = law.matrix();

    let mut fit_errors = Vec::new();
    let mut check_errors = Vec::new();
    let mut predictions = Vec::new();
    for s in &input.samples {
        let k: [f64; 3] =
            std::array::from_fn(|i| s.curvature_m_inv[i] - preferred[i]);
        let predicted = [
            matrix[0][0] * k[0] + matrix[0][1] * k[1],
            matrix[1][0] * k[0] + matrix[1][1] * k[1],
            matrix[2][2] * k[2],
        ];
        let mut residual = [None; 3];
        for (i, slot) in residual.iter_mut().enumerate() {
            if let Some(observed) = s.moment_n[i] {
                let delta = predicted[i] - observed;
                *slot = Some(delta);
                let errors = if s.partition == Partition::Fit {
                    &mut fit_errors
                } else {
                    &mut check_errors
                };
                errors.push(delta / s.scale_n);
            }
        }
        predictions.push(Prediction {
            id: s.id.clone(),
            partition: s.partition,
            bending_strain_m_inv: k,
            predicted_moment_n: predicted,
            residual_n: residual,
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
    let source_id = format!("bending-fit-{digest}");
    let mut material = input.material.clone();
    if material.sources.contains_key(&source_id) {
        return Err(invalid("bending calibration receipt source ID already exists"));
    }
    material.sources.insert(
        source_id.clone(),
        Source {
            reference: format!("local:ftx-bending-calibration/{digest}"),
            accessed_on: input.fitted_on.clone(),
            note: format!(
                "Directional panel bending identified from source-bearing moment/curvature coupons; exact input retained in ftx-bending-calibration-report/1 SHA-256 {digest}. Protocol: {}",
                input.method
            ),
        },
    );
    material
        .structure
        .insert("bending_model".into(), NORMAL_MODEL.into());
    let mut conditions = input.conditions.clone();
    conditions.insert("held_out_checks".into(), check.is_some().to_string());
    conditions.insert(
        "preferred_curvature_m_inv".into(),
        serde_json::to_string(&preferred).map_err(|e| invalid(e.to_string()))?,
    );
    for (key, value) in KEYS.into_iter().zip(values) {
        material.properties.insert(
            key.into(),
            Quantity {
                value,
                unit: "N*m".into(),
                evidence: Evidence::Estimate,
                source: source_id.clone(),
                method: "Weighted least squares of measured bending moment resultants M=D*(k-k0) in the material-frame normal-curvature basis; no E*t^3 or fiber-mixture inference.".into(),
                conditions: conditions.clone(),
                bounds: None,
            },
        );
    }
    material.validate()?;

    Ok(Report {
        schema: "ftx-bending-calibration-report/1".into(),
        input: input.clone(),
        input_sha256: digest,
        material,
        bending_nm: matrix,
        preferred_curvature_m_inv: preferred,
        excitation_pivot: design.pivot,
        fit,
        check,
        predictions,
    })
}

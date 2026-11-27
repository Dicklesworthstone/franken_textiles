//! Explicit, source-qualified parameter data for the coupled knit shell.
//!
//! The ordinary material selection owns evidence consent and identity. Form::Knit alone never
//! selects a law. Parameters use fixed SI units; these data contain no solver or reference prior.
//! Wale follows the authored grainline, course its positive in-plane normal. Membrane coordinates
//! are [E11, E22, 2*E12], conjugate to second-Piola sheet resultants in N/m. Preferred curvature is
//! physical material [k11, k22, 2*k12], in 1/m, signed by the material-frame surface normal.

use std::collections::BTreeMap;

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use serde::{Deserialize, Serialize};

use crate::material_catalog::ResolvedMaterial;
use crate::materials::{Evidence, Form, Material, Source};

/// Versioned explicit coupled membrane, optional asymmetric bending and optional play-bank model.
pub const MODEL: &str = "coupled_knit_shell/1";
/// Bound on independently qualified frictional play operators.
pub const MAX_HYSTERESIS_MODES: usize = 32;

/// Coupled membrane parameters in fixed, work-conjugate material coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnitMembraneParameters {
    /// Wale origin tangent Q11 in N/m.
    pub q11: f64,
    /// Course origin tangent Q22 in N/m.
    pub q22: f64,
    /// Wale/course origin coupling Q12 in N/m.
    pub q12: f64,
    /// Engineering-shear origin tangent Q66 in N/m.
    pub q66: f64,
    /// Nonnegative quartic wale coefficient in N/m.
    pub c1111: f64,
    /// Nonnegative quartic course coefficient in N/m.
    pub c2222: f64,
    /// Nonnegative quartic normal cross coefficient in N/m.
    pub c1122: f64,
    /// Positive dimensionless wale Green-strain jamming limit.
    pub e1_jam: f64,
    /// Positive dimensionless course Green-strain jamming limit.
    pub e2_jam: f64,
    /// Positive dimensionless engineering-shear jamming limit.
    pub gamma_jam: f64,
    /// Nonnegative jamming barrier coefficient in N/m.
    pub kappa_jam: f64,
}

/// Asymmetric bending parameters, independent of scalar seam rigidity and sheet thickness.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnitBendingParameters {
    /// Positive-curvature wale/course rigidities in N*m.
    pub d_plus: [f64; 2],
    /// Negative-curvature wale/course rigidities in N*m.
    pub d_minus: [f64; 2],
    /// Normal cross rigidity in N*m.
    pub d12: f64,
    /// Positive engineering-twist rigidity in N*m.
    pub d66: f64,
    /// Preferred physical material curvature [k11, k22, 2*k12], in 1/m.
    pub kappa0: [f64; 3],
    /// Positive transition half-width in 1/m.
    pub delta_kappa: f64,
}

/// Prandtl-Ishlinskii play-bank parameters; accepted state is not part of a material record.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnitHysteresisParameters {
    /// Dimensionless thresholds, each strictly greater than huber_eps.
    pub thresholds: Vec<f64>,
    /// Nonnegative mode weights in N/m, in the same order as thresholds.
    pub weights: Vec<f64>,
    /// Positive dimensionless Huber smoothing width.
    pub huber_eps: f64,
}

/// One jointly identified law block with independent provenance and evidence.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnitLaw<T> {
    /// Complete parameter data in this schema's fixed SI units.
    pub parameters: T,
    /// Source key in the containing material.
    pub source: String,
    /// Origin of this block; an estimated prior never becomes measured through selection.
    pub evidence: Evidence,
    /// Acquisition, fitting or explicit estimation method.
    pub method: String,
    /// Required nonempty specimen and conditioning, plus any additional acquisition qualifiers.
    pub conditions: BTreeMap<String, String>,
}

/// Source-bearing explicit selection of the knit constitutive model.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnitResponse {
    /// Must equal MODEL.
    pub model: String,
    /// Required equilibrium membrane law; no coefficient has a default.
    pub membrane: KnitLaw<KnitMembraneParameters>,
    /// Optional asymmetric interior-panel bending; scalar seam rigidity remains independent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bending: Option<KnitLaw<KnitBendingParameters>>,
    /// Optional independently sourced frictional history, initially pristine in each triangle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hysteresis: Option<KnitLaw<KnitHysteresisParameters>>,
}

/// Immutable admitted parameters. Numerical kernels remain in ftx-drape.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CompiledKnit {
    /// Admitted equilibrium membrane parameters.
    pub membrane: KnitMembraneParameters,
    /// Admitted optional physical-curvature bending parameters.
    pub bending: Option<KnitBendingParameters>,
    /// Admitted optional frictional play-bank parameters.
    pub hysteresis: Option<KnitHysteresisParameters>,
    fingerprint: String,
    evidence: Evidence,
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

fn positive_normal_block(q11: f64, q22: f64, q12: f64) -> bool {
    let scale = q11.max(q22).max(q12.abs());
    let a = q11 / scale;
    let b = q22 / scale;
    let c = q12 / scale;
    let schur = b - c * (c / a);
    q11 > 0.0 && q22 > 0.0 && schur.is_finite() && schur > 0.0
}

impl KnitMembraneParameters {
    /// Validate finite parameters, a positive-definite origin tangent and finite jamming scales.
    ///
    /// # Errors
    /// Invalid or unrepresentable constitutive coefficients.
    pub fn validate(&self) -> Result<(), Refusal> {
        let values = [
            self.q11,
            self.q22,
            self.q12,
            self.q66,
            self.c1111,
            self.c2222,
            self.c1122,
            self.e1_jam,
            self.e2_jam,
            self.gamma_jam,
            self.kappa_jam,
        ];
        if values.iter().any(|v| !v.is_finite())
            || !positive_normal_block(self.q11, self.q22, self.q12)
            || self.q66 <= 0.0
            || self.c1111 < 0.0
            || self.c2222 < 0.0
            || self.c1122 < 0.0
            || self.kappa_jam < 0.0
            || [self.e1_jam, self.e2_jam, self.gamma_jam]
                .iter()
                .any(|v| *v <= 0.0 || !v.powi(2).is_finite() || !v.powi(-2).is_finite())
        {
            return Err(invalid(
                "coupled knit membrane requires finite SPD origin Q, nonnegative quartic/barrier coefficients and positive finite jamming scales",
            ));
        }
        self.reference_moduli()?;
        Ok(())
    }

    /// Exact origin tangent as [E_wale, E_course, nu_wale_course, G_engineering], in SI units.
    /// The quartic and chosen jamming barrier have zero Hessian at zero strain.
    ///
    /// # Errors
    /// An origin tangent cannot be represented as stable orthotropic mechanics.
    pub fn reference_moduli(&self) -> Result<[f64; 4], Refusal> {
        let nu12 = self.q12 / self.q22;
        let d = 1.0 - nu12 * (self.q12 / self.q11);
        let values = [self.q11 * d, self.q22 * d, nu12, self.q66];
        if !d.is_finite()
            || d <= 0.0
            || values.iter().any(|v| !v.is_finite())
            || values[0] <= 0.0
            || values[1] <= 0.0
            || values[3] <= 0.0
        {
            return Err(invalid(
                "coupled knit origin tangent cannot form stable orthotropic reference mechanics",
            ));
        }
        Ok(values)
    }
}

impl KnitBendingParameters {
    /// Validate strictly convex finite bending and an explicitly finite preferred curvature.
    ///
    /// # Errors
    /// Nonphysical curvature, transition width or rigidity.
    pub fn validate(&self) -> Result<(), Refusal> {
        let values = [
            self.d_plus[0],
            self.d_plus[1],
            self.d_minus[0],
            self.d_minus[1],
            self.d12,
            self.d66,
            self.kappa0[0],
            self.kappa0[1],
            self.kappa0[2],
            self.delta_kappa,
        ];
        if values.iter().any(|v| !v.is_finite())
            || !positive_normal_block(
                self.d_plus[0].min(self.d_minus[0]),
                self.d_plus[1].min(self.d_minus[1]),
                self.d12,
            )
            || self.d66 <= 0.0
            || self.delta_kappa <= 0.0
        {
            return Err(invalid(
                "asymmetric knit bending requires finite strictly convex positive/negative rigidities and positive transition width",
            ));
        }
        Ok(())
    }
}

impl KnitHysteresisParameters {
    /// Validate a bounded nonempty bank without inventing missing modes, weights or history.
    ///
    /// # Errors
    /// Mode budget, length, finiteness, threshold or weight violations.
    pub fn validate(&self) -> Result<(), Refusal> {
        if self.thresholds.is_empty()
            || self.thresholds.len() > MAX_HYSTERESIS_MODES
            || self.thresholds.len() != self.weights.len()
            || !self.huber_eps.is_finite()
            || self.huber_eps <= 0.0
            || self
                .thresholds
                .iter()
                .zip(&self.weights)
                .any(|(r, w)| !r.is_finite() || *r <= self.huber_eps || !w.is_finite() || *w < 0.0)
        {
            return Err(invalid(
                "knit hysteresis requires 1..=32 matched finite modes, thresholds above positive huber_eps, and nonnegative weights",
            ));
        }
        Ok(())
    }
}

impl<T> KnitLaw<T> {
    fn qualify(
        &self,
        sources: &BTreeMap<String, Source>,
        allow_estimates: bool,
    ) -> Result<(&str, &str), Refusal> {
        let source = sources
            .get(&self.source)
            .ok_or_else(|| invalid("knit law references an absent material source"))?;
        if self.method.trim().is_empty()
            || self.method.len() > 4096
            || self.conditions.len() > 64
            || self.conditions.iter().any(|(k, v)| {
                k.trim().is_empty() || v.trim().is_empty() || k.len() > 128 || v.len() > 4096
            })
            || [&source.reference, &source.accessed_on, &source.note]
                .iter()
                .any(|s| s.trim().is_empty())
        {
            return Err(invalid(
                "knit law requires a complete source, method and bounded acquisition conditions",
            ));
        }
        if self.evidence == Evidence::Estimate && !allow_estimates {
            return Err(invalid(
                "knit law is an estimate; explicit allow_estimates consent is required",
            ));
        }
        let condition = |key| {
            self.conditions
                .get(key)
                .map(String::as_str)
                .ok_or_else(|| invalid(format!("knit law requires explicit {key}")))
        };
        Ok((condition("specimen")?, condition("conditioning")?))
    }
}

impl KnitResponse {
    pub(crate) fn compile(
        &self,
        material: &Material,
        allow_estimates: bool,
    ) -> Result<CompiledKnit, Refusal> {
        if self.model != MODEL || material.form != Form::Knit {
            return Err(invalid(
                "coupled knit response requires model coupled_knit_shell/1 and explicit knit sheet form",
            ));
        }
        if material.membrane_response.is_some()
            || material.membrane_relaxation.is_some()
            || [
                "warp_stiffness",
                "weft_stiffness",
                "poisson_warp_weft",
                "shear_stiffness",
            ]
            .iter()
            .any(|key| material.properties.contains_key(*key))
            || [
                "membrane_model",
                "membrane_plasticity_model",
                "membrane_damage_model",
                "membrane_linearization_model",
                "membrane_linearization_parent_sha256",
                "bending_plasticity_model",
            ]
            .iter()
            .any(|key| material.structure.contains_key(*key))
        {
            return Err(invalid(
                "coupled knit response cannot coexist with another membrane law, Maxwell branches, permanent set or damage",
            ));
        }
        if self.bending.is_some()
            && (material.structure.contains_key("bending_model")
                || [
                    "bending_warp",
                    "bending_weft",
                    "bending_coupling",
                    "bending_twist",
                    "preferred_curvature_warp",
                    "preferred_curvature_weft",
                    "preferred_curvature_twist",
                ]
                .iter()
                .any(|key| material.properties.contains_key(*key)))
        {
            return Err(invalid(
                "asymmetric knit bending cannot coexist with a second panel bending or preferred-curvature declaration",
            ));
        }
        let conditions = self.membrane.qualify(&material.sources, allow_estimates)?;
        self.membrane.parameters.validate()?;
        let mut evidence = self.membrane.evidence;
        let mut combine_evidence = |other| {
            evidence = match (evidence, other) {
                (Evidence::Estimate, _) | (_, Evidence::Estimate) => Evidence::Estimate,
                (Evidence::Published, _) | (_, Evidence::Published) => Evidence::Published,
                _ => Evidence::Measured,
            };
        };
        if let Some(law) = &self.bending {
            if law.qualify(&material.sources, allow_estimates)? != conditions {
                return Err(invalid(
                    "knit bending and membrane must identify the same specimen and conditioning",
                ));
            }
            law.parameters.validate()?;
            combine_evidence(law.evidence);
        }
        if let Some(law) = &self.hysteresis {
            if law.qualify(&material.sources, allow_estimates)? != conditions {
                return Err(invalid(
                    "knit hysteresis and membrane must identify the same specimen and conditioning",
                ));
            }
            law.parameters.validate()?;
            combine_evidence(law.evidence);
        }
        // These observations remain independently sourced and independently required by consumers.
        // A supplied observation must belong to the same material state as the selected law.
        for key in [
            "areal_density",
            "thickness",
            "friction_skin",
            "friction_self",
            "bending_rigidity",
            "bending_warp",
            "bending_weft",
            "bending_coupling",
            "bending_twist",
            "preferred_curvature_warp",
            "preferred_curvature_weft",
            "preferred_curvature_twist",
        ] {
            if let Some(q) = material.properties.get(key)
                && (q.conditions.get("specimen").map(String::as_str) != Some(conditions.0)
                    || q.conditions.get("conditioning").map(String::as_str) != Some(conditions.1))
            {
                return Err(invalid(format!(
                    "knit {key} must identify the selected law's exact specimen and conditioning"
                )));
            }
        }
        let bytes = serde_json::to_vec(&(MODEL, self, &material.sources))
            .map_err(|e| invalid(format!("knit law identity: {e}")))?;
        Ok(CompiledKnit {
            membrane: self.membrane.parameters,
            bending: self.bending.as_ref().map(|law| law.parameters),
            hysteresis: self.hysteresis.as_ref().map(|law| law.parameters.clone()),
            fingerprint: sha256_hex(&bytes),
            evidence,
        })
    }
}

impl CompiledKnit {
    /// Stable identity of the complete law declaration and its retained sources.
    #[must_use]
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    /// Least strong evidence across the selected membrane, bending and hysteresis blocks.
    #[must_use]
    pub fn evidence(&self) -> Evidence {
        self.evidence
    }
}

impl Material {
    /// Compile the explicitly selected knit response under the caller's evidence policy.
    ///
    /// # Errors
    /// Malformed/conflicting data, missing sources or consent, unstable coefficients, or mismatched
    /// specimen and conditioning. This does not supply absent mass, thickness, bending or friction.
    pub fn compiled_knit(&self, allow_estimates: bool) -> Result<Option<CompiledKnit>, Refusal> {
        self.validate()?;
        self.knit_response
            .as_ref()
            .map(|law| law.compile(self, allow_estimates))
            .transpose()
    }
}

impl ResolvedMaterial {
    /// The same immutable knit law for ordinary material binding, native drapes and portable drapes.
    ///
    /// # Errors
    /// Source, evidence, parameter and material-state admission failures.
    pub fn coupled_knit(&self) -> Result<Option<CompiledKnit>, Refusal> {
        self.material().compiled_knit(self.allows_estimates())
    }
}

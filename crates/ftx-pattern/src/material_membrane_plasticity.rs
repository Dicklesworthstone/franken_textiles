//! Bounded, rate-independent permanent membrane strain in the reference material frame.
//!
//! Coordinates are raw Green strain [E11,E22,2E12], conjugate to second-Piola sheet
//! resultants [S11,S22,S12] in N/m. This is an additive, phenomenological Green-strain
//! model with directional box yield and isotropic hardening, not multiplicative finite
//! plasticity, damage, or an inferred textile law. Rigid spatial rotations leave its
//! coordinates unchanged. Geometry and stress-free permanent metrics must remain positive.

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use serde::Serialize;

use crate::material_catalog::ResolvedMaterial;
use crate::material_response::Response;
use crate::material_return_map::{self, Failure, Parameters};
use crate::materials::{Form, Material};

/// Source-qualified identification from complete signed loading histories.
pub mod calibration;

/// Explicit opt-in at `structure.membrane_plasticity_model`.
pub const MODEL: &str = "orthotropic_green_return_map/1";
/// Fixed reference material coordinates; engineering shear is twice E12.
pub const AXES: [&str; 3] = ["warp", "weft", "shear"];
/// All directions require yield, hardening (including explicit zero) and validity bounds.
pub const PROPERTY_FAMILIES: [(&str, &str); 4] = [
    ("membrane_yield", "N/m"),
    ("membrane_hardening", "N/m"),
    ("membrane_strain_limit", "1"),
    ("membrane_accumulation_limit", "1"),
];

const ELASTIC: [(&str, &str); 4] = [
    ("warp_stiffness", "N/m"),
    ("weft_stiffness", "N/m"),
    ("poisson_warp_weft", "1"),
    ("shear_stiffness", "N/m"),
];

/// Source-qualified fraction of irreversible membrane dissipation deposited as sensible heat.
/// The complete observation remains in the selected material snapshot; no default fraction exists.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct PlasticHeatFraction {
    /// Dimensionless fraction in [0,1], excluding elastic and hardening storage.
    pub fraction: f64,
    /// Inclusive characterized material-temperature interval in degrees Celsius.
    pub temperature_range_c: [f64; 2],
}

fn bad(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}
fn domain(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::OutOfRange, message).repair(
        "use characterized raw Green strain and history; do not clamp or reset plastic memory",
    )
}

// C = I+2E, with engineering gamma=2E12. Scaling avoids overflow in the determinant.
fn positive_metric(e: [f64; 3]) -> bool {
    let a = 1.0 + 2.0 * e[0];
    let b = 1.0 + 2.0 * e[1];
    let c = e[2];
    if [a, b, c].iter().any(|v| !v.is_finite()) || a <= 0.0 || b <= 0.0 {
        return false;
    }
    let scale = a.max(b).max(c.abs());
    let schur = b / scale - (c / scale) * ((c / scale) / (a / scale));
    schur.is_finite() && schur > 0.0
}

/// Immutable source-qualified law. Accepted history cannot be imported by deserialization.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PlasticMembrane {
    tensor_n_m: [[f64; 3]; 3],
    yield_n_m: [f64; 3],
    hardening_n_m: [f64; 3],
    strain_limit: [f64; 3],
    accumulation_limit: [f64; 3],
    source_sha256: Option<String>,
}

/// Accepted signed permanent Green strain and accumulated absolute increments.
#[derive(Clone, Debug, PartialEq)]
pub struct Memory {
    law: PlasticMembrane,
    plastic: [f64; 3],
    accumulated: [f64; 3],
}

/// Complete proposed material state; the physical solver commits it only after acceptance.
#[derive(Clone, Debug)]
pub struct Update {
    /// Proposed state, separate from the accepted input history.
    pub memory: Memory,
    /// Nonnegative sum(Y_i*abs(delta_plastic_i)), J/m2 of reference area; no thermal feedback.
    pub dissipated_j_m2: f64,
}

impl PlasticMembrane {
    /// Declare a positive-definite orthotropic tensor, positive directional yield/bounds and
    /// nonnegative hardening in raw Green coordinates. Limits are symmetric total-strain bounds
    /// and bounds on accumulated absolute plastic strain; no yield or history defaults exist.
    ///
    /// # Errors
    /// Invalid/unstable tensor, normal-shear coupling, nonfinite values or nonphysical parameters.
    pub fn new(
        q: [[f64; 3]; 3],
        yield_n_m: [f64; 3],
        hardening_n_m: [f64; 3],
        strain_limit: [f64; 3],
        accumulation_limit: [f64; 3],
    ) -> Result<Self, Refusal> {
        let scale = q[0][0].max(q[1][1]);
        let a = q[0][0] / scale;
        let b = q[1][1] / scale;
        let c = q[0][1] / scale;
        let schur = b - c * (c / a);
        if q.iter().flatten().any(|v| !v.is_finite())
            || q[0][0] <= 0.0
            || q[1][1] <= 0.0
            || q[2][2] <= 0.0
            || q[0][1] != q[1][0]
            || [q[0][2], q[2][0], q[1][2], q[2][1]]
                .iter()
                .any(|v| *v != 0.0)
            || !schur.is_finite()
            || schur <= 0.0
            || yield_n_m
                .iter()
                .chain(&strain_limit)
                .chain(&accumulation_limit)
                .any(|v| !v.is_finite() || *v <= 0.0)
            || hardening_n_m.iter().any(|v| !v.is_finite() || *v < 0.0)
        {
            return Err(bad(
                "membrane plasticity needs a positive orthotropic tensor, positive yield/bounds and nonnegative hardening",
            ));
        }
        Ok(Self {
            tensor_n_m: q,
            yield_n_m,
            hardening_n_m,
            strain_limit,
            accumulation_limit,
            source_sha256: None,
        })
    }

    /// Exact reference elastic tensor to replace, never an additional parallel elastic law.
    #[must_use]
    pub fn tensor(&self) -> [[f64; 3]; 3] {
        self.tensor_n_m
    }

    /// Model, numerical-parameter and optional full source-record identity.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        sha256_hex(&serde_json::to_vec(&(MODEL, self)).expect("validated finite membrane law"))
    }

    /// Whether physical Green strain is finite, inside its declared box and has a positive metric.
    #[must_use]
    pub fn contains(&self, strain: [f64; 3]) -> bool {
        (0..3).all(|i| strain[i].is_finite() && strain[i].abs() <= self.strain_limit[i])
            && positive_metric(strain)
    }

    /// Initialize once with zero permanent/accumulated strain in the unchanged reference metric.
    /// This does not reinterpret current placement as a stress-free or annealed configuration.
    #[must_use]
    pub fn pristine(&self) -> Memory {
        Memory {
            law: self.clone(),
            plastic: [0.0; 3],
            accumulated: [0.0; 3],
        }
    }
}

impl Memory {
    /// Accepted signed permanent [E11,E22,2E12]; these are not engineering extension ratios.
    #[must_use]
    pub fn plastic_strain(&self) -> [f64; 3] {
        self.plastic
    }

    /// Accepted accumulated absolute increments in the same raw Green coordinates.
    #[must_use]
    pub fn accumulated_strain(&self) -> [f64; 3] {
        self.accumulated
    }

    fn evaluate(
        &self,
        strain: [f64; 3],
        incremental: bool,
    ) -> Result<(Response, [f64; 3]), Refusal> {
        if !self.law.contains(strain) || !positive_metric(self.plastic) {
            return Err(domain(
                "membrane Green strain exceeds characterization or has a nonpositive current/permanent metric",
            ));
        }
        let (response, delta) = material_return_map::evaluate(
            Parameters {
                tensor: self.law.tensor_n_m,
                yield_value: self.law.yield_n_m,
                hardening: self.law.hardening_n_m,
                accumulation_limit: self.law.accumulation_limit,
            },
            strain,
            self.plastic,
            self.accumulated,
            incremental,
        )
        .map_err(|failure| {
            domain(match failure {
                Failure::TrialOverflow => "plastic membrane trial overflows",
                Failure::SingularActiveTensor => {
                    "plastic membrane active tensor is numerically singular"
                }
                Failure::NoReturnMap => {
                    "no representable plastic membrane return map satisfies the yield conditions"
                }
                Failure::Accumulation => {
                    "plastic membrane accumulation exceeds characterized history"
                }
                Failure::ResponseOverflow => "plastic membrane response overflows",
            })
        })?;
        if !positive_metric(std::array::from_fn(|i| self.plastic[i] + delta[i])) {
            return Err(domain(
                "proposed permanent Green strain has a nonpositive stress-free metric",
            ));
        }
        Ok((response, delta))
    }

    /// Pure trial evaluation. Incremental energy contains the plastic dissipation and change in
    /// hardening storage; frozen energy contains actual elastic plus total hardening storage.
    /// Stress and tangent are exact derivatives with respect to RAW [E11,E22,2E12].
    ///
    /// # Errors
    /// Invalid current/permanent metric, strain/history limit, singular return map or overflow.
    pub fn response(&self, strain: [f64; 3], incremental: bool) -> Result<Response, Refusal> {
        self.evaluate(strain, incremental)
            .map(|(response, _)| response)
    }

    /// Propose a complete return-map update without modifying this accepted memory.
    ///
    /// # Errors
    /// Any response refusal or unrepresentable accepted storage/history/dissipation.
    pub fn advance(&self, strain: [f64; 3]) -> Result<Update, Refusal> {
        let (_, delta) = self.evaluate(strain, true)?;
        let mut memory = self.clone();
        for i in 0..3 {
            memory.plastic[i] += delta[i];
            memory.accumulated[i] += delta[i].abs();
        }
        let dissipated = (0..3)
            .map(|i| self.law.yield_n_m[i] * delta[i].abs())
            .sum::<f64>();
        if memory
            .plastic
            .iter()
            .chain(&memory.accumulated)
            .any(|v| !v.is_finite())
            || !dissipated.is_finite()
        {
            return Err(domain("plastic membrane accepted update overflows"));
        }
        memory.response(strain, false)?;
        Ok(Update {
            memory,
            dissipated_j_m2: dissipated,
        })
    }
}

impl Material {
    /// Compile optional bounded permanent stretch from the four scalar elastic observations and
    /// twelve directional history quantities, all sharing specimen_id and conditioning.
    /// This path does not call mechanics(), fabricate a joint nonlinear/Maxwell model, or convert
    /// tensile breaking force to a yield resultant. Properties must already use canonical units.
    ///
    /// # Errors
    /// Unknown selector, unsupported combination/form, missing/mixed evidence or invalid law.
    pub fn compiled_membrane_plasticity(
        &self,
        allow_estimates: bool,
    ) -> Result<Option<PlasticMembrane>, Refusal> {
        let Some(model) = self.structure.get("membrane_plasticity_model") else {
            return Ok(None);
        };
        self.validate()?;
        if model != MODEL {
            return Err(bad("unsupported permanent membrane model"));
        }
        if self.membrane_response.is_some() || self.membrane_relaxation.is_some() {
            return Err(bad(
                "permanent membrane stretch cannot combine with nonlinear or Maxwell membrane laws without a qualified joint model",
            ));
        }
        if matches!(self.form, Form::Fiber | Form::Yarn | Form::Fill) {
            return Err(bad(
                "permanent membrane stretch requires a characterized sheet",
            ));
        }
        let mut keys = Vec::new();
        let mut elastic = [0.0; 4];
        for (i, (key, unit)) in ELASTIC.iter().enumerate() {
            elastic[i] = self.require(key, unit, allow_estimates)?;
            keys.push((*key).to_owned());
        }
        let mut values = [[0.0; 3]; 4];
        for (i, (family, unit)) in PROPERTY_FAMILIES.iter().enumerate() {
            for (j, axis) in AXES.iter().enumerate() {
                let key = format!("{family}_{axis}");
                values[i][j] = self.require(&key, unit, allow_estimates)?;
                keys.push(key);
            }
        }
        for identity in ["specimen_id", "conditioning"] {
            let expected = self.properties[&keys[0]]
                .conditions
                .get(identity)
                .filter(|v| !v.trim().is_empty())
                .ok_or_else(|| {
                    bad(format!(
                        "permanent membrane stretch needs {identity} on its observations"
                    ))
                })?;
            if keys
                .iter()
                .any(|key| self.properties[key].conditions.get(identity) != Some(expected))
            {
                return Err(bad(format!(
                    "permanent membrane stretch cannot mix {identity} across elastic/history observations"
                )));
            }
        }
        let [warp, weft, poisson, shear] = elastic;
        // Match Mechanics::origin_tangent and the solver's plane-stress conversion exactly.
        let nu21 = poisson * weft / warp;
        let d = 1.0 - poisson * nu21;
        if warp <= 0.0 || weft <= 0.0 || shear <= 0.0 || !d.is_finite() || d <= 0.0 {
            return Err(bad(
                "permanent membrane stretch requires stable positive elastic stiffness",
            ));
        }
        let q = [
            [warp / d, poisson * weft / d, 0.0],
            [poisson * weft / d, weft / d, 0.0],
            [0.0, 0.0, shear],
        ];
        PlasticMembrane::new(q, values[0], values[1], values[2], values[3]).map(Some)
    }
}

impl ResolvedMaterial {
    /// Resolve the permanent membrane law with the exact full material snapshot bound to history.
    ///
    /// # Errors
    /// As Material::compiled_membrane_plasticity under this selection's evidence policy.
    pub fn membrane_plasticity(&self) -> Result<Option<PlasticMembrane>, Refusal> {
        let mut law = self
            .material()
            .compiled_membrane_plasticity(self.allows_estimates())?;
        if let Some(law) = &mut law {
            law.source_sha256 = Some(self.sha256().to_owned());
        }
        Ok(law)
    }

    /// Resolve heat conversion only when explicitly requested by a thermal consumer. Mechanical
    /// readiness never calls this optional compiler. No active permanent membrane law returns None;
    /// an active law needs its own qualified conversion observation, including explicit zero.
    /// The law and this observation share the full frozen material identity, specimen and conditions.
    ///
    /// # Errors
    /// Invalid permanent law, missing/unapproved conversion evidence, mismatched specimen or
    /// conditioning, wrong work basis, fraction/bounds outside [0,1], or invalid temperature range.
    pub fn membrane_plastic_heat_fraction(&self) -> Result<Option<PlasticHeatFraction>, Refusal> {
        if self.membrane_plasticity()?.is_none() {
            return Ok(None);
        }
        let material = self.material();
        let key = "membrane_plastic_heat_fraction";
        let fraction = material.require(key, "1", self.allows_estimates())?;
        let observation = &material.properties[key];
        if !(0.0..=1.0).contains(&fraction)
            || observation
                .bounds
                .is_some_and(|[lo, hi]| lo < 0. || hi > 1.)
        {
            return Err(bad(
                "membrane plastic heat fraction and bounds must lie in [0,1]",
            ));
        }
        // Compiling the active law above checked this identity across every elastic/history datum.
        let conditions = &material.properties[ELASTIC[0].0].conditions;
        for identity in ["specimen_id", "conditioning"] {
            if observation.conditions.get(identity) != conditions.get(identity) {
                return Err(bad(format!(
                    "membrane plastic heat fraction must match the law's {identity}"
                )));
            }
        }
        if observation.conditions.get("work_basis").map(String::as_str)
            != Some("irreversible_membrane_plastic_dissipation")
        {
            return Err(bad(
                "membrane plastic heat fraction requires irreversible_membrane_plastic_dissipation work basis",
            ));
        }
        let mut temperature_range_c = [0.; 2];
        for (i, name) in ["temperature_min_c", "temperature_max_c"]
            .iter()
            .enumerate()
        {
            temperature_range_c[i] = observation
                .conditions
                .get(*name)
                .and_then(|value| value.parse::<f64>().ok())
                .filter(|value| value.is_finite() && *value >= -273.15)
                .ok_or_else(|| {
                    bad(format!(
                        "membrane plastic heat fraction needs finite {name} >= -273.15"
                    ))
                })?;
        }
        if temperature_range_c[0] > temperature_range_c[1] {
            return Err(bad(
                "membrane plastic heat temperature range must be ordered",
            ));
        }
        Ok(Some(PlasticHeatFraction {
            fraction,
            temperature_range_c,
        }))
    }
}

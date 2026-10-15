//! Rate-independent permanent curvature in the existing normal-projected panel frame.
//!
//! A convex incremental material model, not a claim that fiber names predict crease behavior.
//! For old plastic curvature z and accumulated absolute curvature a, minimize over dz:
//! 1/2 (k-z-dz)^T D (k-z-dz) + sum((Y+H*a)*abs(dz) + H*dz^2/2).
//! k is the departure from the authored preferred covariant curvature [b_uu,b_vv,2b_uv].
//! D and H have units N m, Y has units N, and k/z/a have units 1/m. The potential and
//! dissipation are per REFERENCE area (J/m2). All 27 active/sign sets are bounded work.

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use serde::Serialize;

use crate::material_bending::{BendingTensor, PREFERRED_CURVATURE_PROPERTIES};
use crate::material_catalog::ResolvedMaterial;

/// Explicit opt-in at `structure.bending_plasticity_model`.
pub const MODEL: &str = "orthotropic_curvature_return_map/1";
/// Direction suffixes in engineering curvature order; grain defines warp/wale.
pub const AXES: [&str; 3] = ["warp", "weft", "twist"];
/// Every direction requires yield, hardening (including explicit zero) and validity bounds.
pub const PROPERTY_FAMILIES: [(&str, &str); 4] = [
    ("bending_yield", "N"),
    ("bending_hardening", "N*m"),
    ("bending_curvature_limit", "1/m"),
    ("bending_accumulation_limit", "1/m"),
];

fn bad(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}
fn domain(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::OutOfRange, message)
        .repair("use characterized curvature/history ranges; do not clamp or reset plastic memory")
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
fn product(a: [[f64; 3]; 3], x: [f64; 3]) -> [f64; 3] {
    a.map(|row| dot(row, x))
}

/// A validated immutable law. The optional source digest also distinguishes equally-valued lots.
/// No serialization import can bypass the constructor or replace accepted history.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PlasticBending {
    tensor: BendingTensor,
    yield_n: [f64; 3],
    hardening_nm: [f64; 3],
    curvature_limit_m_inv: [f64; 3],
    accumulation_limit_m_inv: [f64; 3],
    source_sha256: Option<String>,
}

/// Condensed incremental response, or frozen-state storage response.
#[derive(Clone, Copy, Debug)]
pub struct Response {
    /// Potential per reference area, J/m2. Incremental values include plastic dissipation and
    /// the CHANGE in hardening storage, not the previous state's constant hardening energy.
    pub energy: f64,
    /// First derivative with respect to engineering curvature: moments per width, N.
    pub moment_n: [f64; 3],
    /// Consistent condensed tangent, N m, including normal coupling and active-set elimination.
    pub tangent_nm: [[f64; 3]; 3],
}

/// Accepted plastic state. Clones preserve its bound law and history; trial evaluation is pure.
#[derive(Clone, Debug, PartialEq)]
pub struct Memory {
    law: PlasticBending,
    plastic_m_inv: [f64; 3],
    accumulated_m_inv: [f64; 3],
}

/// A proposed complete material update. A consumer commits this only after its solve succeeds.
#[derive(Clone, Debug)]
pub struct Update {
    /// Proposed accepted memory, still owned separately from the input state.
    pub memory: Memory,
    /// Nonnegative rate-independent dissipation Y dot abs(dz), J/m2; NOT thermal feedback.
    pub dissipated_j_m2: f64,
}

impl PlasticBending {
    /// Declare a phenomenological law with symmetric geometric-curvature and accumulated-history
    /// validity bounds. No saturation, extrapolation, default yield or inferred hardening is used.
    /// The tensor must use normal-projected kinematics; scalar seam hinges are not modified.
    ///
    /// # Errors
    /// Invalid kinematics, non-finite/nonpositive yield or bounds, or negative hardening.
    pub fn new(
        tensor: BendingTensor,
        yield_n: [f64; 3],
        hardening_nm: [f64; 3],
        curvature_limit_m_inv: [f64; 3],
        accumulation_limit_m_inv: [f64; 3],
    ) -> Result<Self, Refusal> {
        if !tensor.uses_normal()
            || yield_n
                .iter()
                .chain(&curvature_limit_m_inv)
                .chain(&accumulation_limit_m_inv)
                .any(|v| !v.is_finite() || *v <= 0.0)
            || hardening_nm.iter().any(|v| !v.is_finite() || *v < 0.0)
        {
            return Err(bad(
                "plastic bending needs normal curvature, positive yield/bounds and nonnegative hardening",
            ));
        }
        Ok(Self {
            tensor,
            yield_n,
            hardening_nm,
            curvature_limit_m_inv,
            accumulation_limit_m_inv,
            source_sha256: None,
        })
    }

    /// The exact elastic kinematics/tensor this plastic law must replace, not supplement.
    #[must_use]
    pub fn tensor(&self) -> BendingTensor {
        self.tensor
    }

    /// Portable identity including source record when resolved from the catalog.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        sha256_hex(&serde_json::to_vec(&(MODEL, self)).expect("validated finite bending law"))
    }

    /// Start pristine (zero permanent and accumulated curvature), relative to the authored preferred
    /// shape. This does NOT silently anneal an existing simulation or pre-relax its initial shape.
    #[must_use]
    pub fn pristine(&self) -> Memory {
        Memory {
            law: self.clone(),
            plastic_m_inv: [0.; 3],
            accumulated_m_inv: [0.; 3],
        }
    }
}

// Scaled Cholesky of an active principal block of D+H. Inactive coordinates use identity.
// No diagonal stabilization: ill-conditioned/unrepresentable input is refused, not retuned.
fn inverse_active(d: [[f64; 3]; 3], h: [f64; 3], active: [bool; 3]) -> Option<[[f64; 3]; 3]> {
    let scale = (0..3)
        .filter(|&i| active[i])
        .map(|i| d[i][i] + h[i])
        .fold(0.0, f64::max);
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let mut l = [[0.; 3]; 3];
    for i in 0..3 {
        for j in 0..=i {
            let mut v = if active[i] && active[j] {
                (d[i][j] + if i == j { h[i] } else { 0. }) / scale
            } else if i == j {
                1.
            } else {
                0.
            };
            for k in 0..j {
                v -= l[i][k] * l[j][k];
            }
            if i == j {
                if !v.is_finite() || v <= 64. * f64::EPSILON {
                    return None;
                }
                l[i][j] = v.sqrt();
            } else {
                l[i][j] = v / l[j][j];
            }
        }
    }
    let mut inverse = [[0.; 3]; 3];
    for j in 0..3 {
        if !active[j] {
            continue;
        }
        let mut x = [0.; 3];
        for i in 0..3 {
            let rhs = if i == j { 1. } else { 0. };
            x[i] = (rhs - (0..i).map(|k| l[i][k] * x[k]).sum::<f64>()) / l[i][i];
        }
        for i in (0..3).rev() {
            x[i] = (x[i] - (i + 1..3).map(|k| l[k][i] * x[k]).sum::<f64>()) / l[i][i];
        }
        for i in 0..3 {
            inverse[i][j] = if active[i] { x[i] / scale } else { 0. };
        }
    }
    inverse
        .iter()
        .flatten()
        .all(|v| v.is_finite())
        .then_some(inverse)
}

impl Memory {
    /// Accepted signed permanent curvature, inverse reference metres.
    #[must_use]
    pub fn plastic_curvature(&self) -> [f64; 3] {
        self.plastic_m_inv
    }
    /// Accepted accumulated absolute plastic curvature, inverse reference metres.
    #[must_use]
    pub fn accumulated_curvature(&self) -> [f64; 3] {
        self.accumulated_m_inv
    }

    fn check(&self, k: [f64; 3]) -> Result<(), Refusal> {
        if (0..3).any(|i| !k[i].is_finite() || k[i].abs() > self.law.curvature_limit_m_inv[i]) {
            return Err(domain("bending curvature exceeds the characterized range"));
        }
        Ok(())
    }

    fn evaluate(&self, k: [f64; 3], incremental: bool) -> Result<(Response, [f64; 3]), Refusal> {
        self.check(k)?;
        let d = self.law.tensor.matrix();
        let h = self.law.hardening_nm;
        let e = std::array::from_fn(|i| k[i] - self.plastic_m_inv[i]);
        let trial = product(d, e);
        let limit: [f64; 3] =
            std::array::from_fn(|i| self.law.yield_n[i] + h[i] * self.accumulated_m_inv[i]);
        if e.iter().chain(&trial).chain(&limit).any(|v| !v.is_finite()) {
            return Err(domain("plastic bending trial overflows"));
        }
        let mut delta = [0.; 3];
        let mut tangent = d;
        if incremental && (0..3).any(|i| trial[i].abs() > limit[i]) {
            let mut found = false;
            // Zero/inactive is ordered first: at an exact yield boundary prefer the elastic side.
            for code in 1..27 {
                let mut q = code;
                let sign: [f64; 3] = std::array::from_fn(|_| {
                    let digit = q % 3;
                    q /= 3;
                    match digit {
                        0 => 0.,
                        1 => 1.,
                        _ => -1.,
                    }
                });
                let active = sign.map(|s| s != 0.);
                let inv = inverse_active(d, h, active).ok_or_else(|| {
                    domain("plastic bending active tensor is numerically singular")
                })?;
                let rhs = std::array::from_fn(|i| {
                    if active[i] {
                        trial[i] - sign[i] * limit[i]
                    } else {
                        0.
                    }
                });
                let candidate = product(inv, rhs);
                if (0..3).any(|i| active[i] && candidate[i] * sign[i] <= 0.) {
                    continue;
                }
                let moment = product(d, std::array::from_fn(|i| e[i] - candidate[i]));
                // KKT residuals are checked on the ORIGINAL equations, in moment units.
                let tolerance: [f64; 3] = std::array::from_fn(|i| {
                    256. * f64::EPSILON
                        * (trial[i].abs()
                            + limit[i]
                            + (0..3).map(|j| (d[i][j] * candidate[j]).abs()).sum::<f64>())
                });
                if (0..3).any(|i| {
                    !candidate[i].is_finite()
                        || !moment[i].is_finite()
                        || !tolerance[i].is_finite()
                        || if active[i] {
                            (moment[i] - sign[i] * (limit[i] + h[i] * candidate[i].abs())).abs()
                                > tolerance[i]
                        } else {
                            moment[i].abs() > limit[i] + tolerance[i]
                        }
                }) {
                    continue;
                }
                delta = candidate;
                // D - D_:A (D_AA+H)^-1 D_A:; copy the lower half for exact symmetry.
                for i in 0..3 {
                    for j in 0..=i {
                        let mut correction = 0.;
                        for a in 0..3 {
                            for b in 0..3 {
                                correction += d[i][a] * inv[a][b] * d[b][j];
                            }
                        }
                        tangent[i][j] = d[i][j] - correction;
                        tangent[j][i] = tangent[i][j];
                    }
                }
                found = true;
                break;
            }
            if !found {
                return Err(domain(
                    "no representable plastic bending return map satisfies the yield conditions",
                ));
            }
        }
        if (0..3).any(|i| {
            self.accumulated_m_inv[i] + delta[i].abs() > self.law.accumulation_limit_m_inv[i]
        }) {
            return Err(domain(
                "plastic bending accumulation exceeds characterized history",
            ));
        }
        let elastic = std::array::from_fn(|i| e[i] - delta[i]);
        let moment = product(d, elastic);
        let energy = 0.5 * dot(elastic, moment)
            + if incremental {
                (0..3)
                    .map(|i| limit[i] * delta[i].abs() + 0.5 * h[i] * delta[i] * delta[i])
                    .sum::<f64>()
            } else {
                (0..3)
                    .map(|i| 0.5 * h[i] * self.accumulated_m_inv[i] * self.accumulated_m_inv[i])
                    .sum::<f64>()
            };
        if !energy.is_finite()
            || moment
                .iter()
                .chain(tangent.iter().flatten())
                .any(|v| !v.is_finite())
        {
            return Err(domain("plastic bending response overflows"));
        }
        Ok((
            Response {
                energy,
                moment_n: moment,
                tangent_nm: tangent,
            },
            delta,
        ))
    }

    /// Evaluate without modifying accepted memory. Incremental mode eliminates a PROPOSED plastic
    /// update; frozen mode reports actual elastic plus hardening storage at the existing state.
    ///
    /// # Errors
    /// Outside characterized curvature/history, numerical singularity or arithmetic overflow.
    pub fn response(&self, k: [f64; 3], incremental: bool) -> Result<Response, Refusal> {
        self.evaluate(k, incremental).map(|(r, _)| r)
    }

    /// Propose a complete return-map update. The caller decides whether the step is accepted.
    ///
    /// # Errors
    /// As [`Self::response`], or unrepresentable state/storage/dissipation.
    pub fn advance(&self, k: [f64; 3]) -> Result<Update, Refusal> {
        let (_, delta) = self.evaluate(k, true)?;
        let mut memory = self.clone();
        for i in 0..3 {
            memory.plastic_m_inv[i] += delta[i];
            memory.accumulated_m_inv[i] += delta[i].abs();
        }
        let dissipated = dot(self.law.yield_n, delta.map(f64::abs));
        if memory
            .plastic_m_inv
            .iter()
            .chain(&memory.accumulated_m_inv)
            .any(|v| !v.is_finite())
            || !dissipated.is_finite()
        {
            return Err(domain("plastic bending accepted update overflows"));
        }
        memory.response(k, false)?;
        Ok(Update {
            memory,
            dissipated_j_m2: dissipated,
        })
    }
}

impl ResolvedMaterial {
    /// Resolve permanent-curvature behavior from the SAME source-locked record as all other
    /// consumers. All tensor, yield, hardening, range and preferred-curvature observations must
    /// identify the same nonempty `specimen_id` and `conditioning`; different test methods may coexist.
    ///
    /// # Errors
    /// Unknown model, missing/mismatched specimen evidence, unapproved estimates or invalid values.
    pub fn bending_plasticity(&self) -> Result<Option<PlasticBending>, Refusal> {
        let m = self.material();
        let Some(model) = m.structure.get("bending_plasticity_model") else {
            return Ok(None);
        };
        if model != MODEL {
            return Err(bad("unsupported permanent-curvature model"));
        }
        let tensor = self
            .bending_response()?
            .ok_or_else(|| bad("plastic bending requires an explicit normal-projected tensor"))?;
        let mut keys: Vec<String> = [
            "bending_warp",
            "bending_weft",
            "bending_coupling",
            "bending_twist",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        let mut values = [[0.; 3]; 4];
        for (i, (family, unit)) in PROPERTY_FAMILIES.iter().enumerate() {
            for (j, axis) in AXES.iter().enumerate() {
                let key = format!("{family}_{axis}");
                values[i][j] = m.require(&key, unit, self.allows_estimates())?;
                keys.push(key);
            }
        }
        keys.extend(
            PREFERRED_CURVATURE_PROPERTIES
                .into_iter()
                .filter(|key| m.properties.contains_key(*key))
                .map(str::to_owned),
        );
        for identity in ["specimen_id", "conditioning"] {
            let expected = m.properties[&keys[0]]
                .conditions
                .get(identity)
                .filter(|v| !v.trim().is_empty())
                .ok_or_else(|| {
                    bad(format!(
                        "plastic bending needs {identity} on its observations"
                    ))
                })?;
            if keys
                .iter()
                .any(|key| m.properties[key].conditions.get(identity) != Some(expected))
            {
                return Err(bad(format!(
                    "plastic bending cannot mix {identity} across tensor/history observations"
                )));
            }
        }
        let mut law = PlasticBending::new(tensor, values[0], values[1], values[2], values[3])?;
        law.source_sha256 = Some(self.sha256().to_owned());
        Ok(Some(law))
    }
}

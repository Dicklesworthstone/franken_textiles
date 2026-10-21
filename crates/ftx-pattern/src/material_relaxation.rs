//! Generalized Maxwell branches in material-frame Green strain, in parallel with the existing
//! equilibrium membrane. Branch stiffness is either a multiple of its origin tangent or a separately
//! characterized orthotropic tensor, allowing warp/wale, weft/course and bias to recover differently.
//! Backward Euler eliminates internal strains into an incremental potential; trial evaluations
//! never advance history. This is a phenomenological law, not yarn homogenization or plastic damage.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use serde::{Deserialize, Serialize};

use crate::material_response::Response;
use crate::materials::{Evidence, Quantity, Source};

mod binding;
pub mod calibration;
pub mod heating;

/// Stress-free reference strain or a pre-relaxed start; neither is inferred from a garment pose.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Initialization {
    /// Internal strains start at zero, before the initial geometric deformation.
    StressFreeReference,
    /// Internal strains start at the supplied initial strain (zero transient stress at that pose).
    RelaxedStart,
}

/// A Maxwell spring/dashpot pair. Ratios are relative to EQUILIBRIUM, not instantaneous stiffness.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaxwellBranch {
    /// Unique identity within this law, also the order used in state reports.
    pub id: String,
    /// Positive multiplier of the equilibrium membrane's origin tangent, unit `1`.
    pub stiffness_ratio: Quantity,
    /// Positive relaxation time, unit `s`. The viscosity tensor is tau times branch stiffness.
    pub relaxation_time: Quantity,
}

/// An independently characterized Maxwell tensor in material-frame [E11,E22,2E12] order.
/// The four sheet resultants are in N/m, not bulk moduli. Explicit zero axes permit a branch that
/// relaxes only warp, weft or bias; the complete tensor must be nonzero and positive semidefinite.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrthotropicStiffness {
    /// dS11/dE11, including any normal coupling; NOT an uncoupled Young modulus.
    pub warp: Quantity,
    /// dS22/dE22, including any normal coupling.
    pub weft: Quantity,
    /// dS11/dE22 = dS22/dE11. Signed; zero must be explicit.
    pub coupling: Quantity,
    /// dS12/d(2E12), the engineering-shear resultant stiffness.
    pub shear: Quantity,
}

/// A non-proportional branch; its tensor and time share the law's specimen and conditioning.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrthotropicBranch {
    /// Unique across both proportional and orthotropic branches in the law.
    pub id: String,
    /// Measured/fitted material-frame tensor; no values are inferred from composition or grain.
    pub stiffness: OrthotropicStiffness,
    /// Positive relaxation time in seconds for this tensor; viscosity is tau times stiffness.
    pub relaxation_time: Quantity,
}

/// Source-qualified optional time-dependent response in parallel with a material's equilibrium law.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Relaxation {
    /// Must equal `green_maxwell/1`.
    pub model: String,
    /// Proportional branches, preserving authored order. One to sixteen TOTAL branches are required.
    pub branches: Vec<MaxwellBranch>,
    /// Optional independent directional tensors, after proportional branches in state reports.
    /// Omission preserves the legacy source serialization and proportional arithmetic exactly.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub orthotropic_branches: Vec<OrthotropicBranch>,
    /// Explicit internal-strain initialization for a new simulation, never for a continuation.
    pub initialization: Initialization,
    /// Characterized box for [E11, E22, 2 E12], including zero in its interior. No extrapolation.
    pub strain_range: [[f64; 2]; 3],
    /// Source for the applicability domain and initialization/constitutive interpretation.
    pub source: String,
    /// Method identifying the equilibrium/instantaneous split and strain-domain validity.
    pub method: String,
    /// Conditions shared by the law, not inferred from fiber or brand.
    #[serde(default)]
    pub conditions: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq)]
struct Branch {
    q: [[f64; 3]; 3],
    tau: f64,
    // L such that Q=L L^T. Explicit PSD branches use sums of squares for null-mode energy,
    // preventing cancellation from turning a permitted inactive mode into negative storage.
    energy_factor: Option<[[f64; 3]; 3]>,
}

/// Immutable compiled law. Coefficients are in N/m, time in seconds, memory in Green strain.
#[derive(Clone, Debug, PartialEq)]
pub struct CompiledRelaxation {
    branches: Vec<Branch>,
    origin_tangent: [[f64; 3]; 3],
    range: [[f64; 2]; 3],
    initialization: Initialization,
    fingerprint: String,
    orthotropic_branches: bool,
}

/// Accepted history for one material point. No mutating trial-evaluation API is provided.
#[derive(Clone, Debug, PartialEq)]
pub struct Memory {
    law: CompiledRelaxation,
    viscous: Vec<[f64; 3]>,
}

/// A proposed history update, committed by a solver only after a successful physical step.
#[derive(Clone, Debug)]
pub struct Update {
    /// The proposed next state.
    pub memory: Memory,
    /// Maxwell-spring stored energy at the endpoint, J/m² of reference area.
    pub stored_j_m2: f64,
    /// Backward-Euler endpoint quadrature of viscous dissipation, J/m², nonnegative.
    /// This excludes numerical time-integration dissipation and is not a measured heat source.
    pub dissipated_j_m2: f64,
}

fn bad(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::OutOfRange, message)
}

fn source_ok(id: &str, sources: &BTreeMap<String, Source>) -> bool {
    sources.get(id).is_some_and(|s| {
        [&s.reference, &s.accessed_on, &s.note]
            .iter()
            .all(|v| !v.trim().is_empty())
    })
}

fn quantity(q: &Quantity, unit: &str, sources: &BTreeMap<String, Source>, allow: bool) -> bool {
    q.value.is_finite()
        && q.value > 0.0
        && q.unit == unit
        && !q.method.trim().is_empty()
        && source_ok(&q.source, sources)
        && (allow || q.evidence != Evidence::Estimate)
        && q.bounds.is_none_or(|[a, b]| {
            a.is_finite() && b.is_finite() && a > 0.0 && a <= q.value && b >= q.value
        })
}

impl OrthotropicStiffness {
    fn quantities(&self) -> [&Quantity; 4] {
        [&self.warp, &self.weft, &self.coupling, &self.shear]
    }

    fn matrix(&self) -> [[f64; 3]; 3] {
        [
            [self.warp.value, self.coupling.value, 0.],
            [self.coupling.value, self.weft.value, 0.],
            [0., 0., self.shear.value],
        ]
    }

    fn factor(&self) -> Result<[[f64; 3]; 3], Refusal> {
        let [a, b, c, g] = self.quantities().map(|q| q.value);
        if [a, b, c, g].iter().any(|v| !v.is_finite())
            || a < 0.
            || b < 0.
            || g < 0.
            || (a == 0. && b == 0. && g == 0.)
            || ((a == 0. || b == 0.) && c != 0.)
        {
            return Err(bad(
                "orthotropic Maxwell tensor must be nonzero positive semidefinite",
            ));
        }
        let l11 = a.sqrt();
        let l21 = if a == 0. { 0. } else { c / l11 };
        let remainder = b - l21 * l21;
        if !remainder.is_finite() || remainder < 0. {
            return Err(bad(
                "orthotropic Maxwell normal coupling is unstable or unrepresentable",
            ));
        }
        Ok([
            [l11, 0., 0.],
            [l21, remainder.sqrt(), 0.],
            [0., 0., g.sqrt()],
        ])
    }
}

impl Relaxation {
    // Shared conditions describe one specimen preparation, not acquisition-specific directions.
    fn matching_conditions(&self, actual: &BTreeMap<String, String>) -> Result<(), Refusal> {
        if self
            .conditions
            .iter()
            .any(|(key, value)| actual.get(key) != Some(value))
        {
            return Err(bad(
                "orthotropic Maxwell observations must match the declared specimen and conditions",
            ));
        }
        Ok(())
    }

    /// Check that independently characterized directional branches and the equilibrium response
    /// refer to the same specimen/conditioning. Other consumers need no mechanical observations.
    pub(crate) fn validate_equilibrium(
        &self,
        material: &crate::materials::Material,
    ) -> Result<(), Refusal> {
        if self.orthotropic_branches.is_empty() {
            return Ok(());
        }
        if let Some(response) = &material.membrane_response {
            for conditions in [
                &response.warp.conditions,
                &response.weft.conditions,
                &response.shear.conditions,
                &response.coupling.conditions,
            ] {
                self.matching_conditions(conditions)?;
            }
        } else {
            for key in [
                "warp_stiffness",
                "weft_stiffness",
                "poisson_warp_weft",
                "shear_stiffness",
            ] {
                let q = material.properties.get(key).ok_or_else(|| {
                    bad("orthotropic Maxwell law needs characterized equilibrium response")
                })?;
                self.matching_conditions(&q.conditions)?;
            }
        }
        Ok(())
    }

    /// Validate sources, dimensions, positive parameters and the declared domain.
    ///
    /// # Errors
    /// Invalid model/data, unresolved sources or estimated quantities without explicit admission.
    pub fn validate(&self, sources: &BTreeMap<String, Source>, allow: bool) -> Result<(), Refusal> {
        if self.model != "green_maxwell/1"
            || !(1..=16).contains(
                &self
                    .branches
                    .len()
                    .saturating_add(self.orthotropic_branches.len()),
            )
            || !source_ok(&self.source, sources)
            || self.method.trim().is_empty()
            || self.conditions.len() > 64
            || self
                .conditions
                .iter()
                .any(|(k, v)| k.trim().is_empty() || v.trim().is_empty())
            || self
                .strain_range
                .iter()
                .any(|[a, b]| !a.is_finite() || !b.is_finite() || *a >= 0.0 || *b <= 0.0)
            || self.strain_range[..2].iter().any(|r| r[0] <= -0.5)
        {
            return Err(bad(
                "invalid Maxwell model, source, conditions or Green-strain domain",
            ));
        }
        let mut ids = BTreeSet::new();
        for b in &self.branches {
            if b.id.trim().is_empty()
                || !ids.insert(&b.id)
                || !quantity(&b.stiffness_ratio, "1", sources, allow)
                || !quantity(&b.relaxation_time, "s", sources, allow)
            {
                return Err(bad(
                    "Maxwell branches need distinct IDs and sourced positive ratios/times in 1/s; estimates require explicit opt-in",
                ));
            }
        }
        if !self.orthotropic_branches.is_empty() {
            for key in ["specimen_id", "conditioning"] {
                if !self.conditions.contains_key(key) {
                    return Err(bad(
                        "orthotropic Maxwell laws need explicit specimen_id and conditioning",
                    ));
                }
            }
            // Mixed spectra cannot borrow the proportional part from another specimen either.
            for b in &self.branches {
                self.matching_conditions(&b.stiffness_ratio.conditions)?;
                self.matching_conditions(&b.relaxation_time.conditions)?;
            }
        }
        for b in &self.orthotropic_branches {
            if b.id.trim().is_empty()
                || !ids.insert(&b.id)
                || !quantity(&b.relaxation_time, "s", sources, allow)
            {
                return Err(bad(
                    "orthotropic Maxwell branches need distinct IDs and sourced positive times",
                ));
            }
            self.matching_conditions(&b.relaxation_time.conditions)?;
            for q in b.stiffness.quantities() {
                if !q.value.is_finite()
                    || q.unit != "N/m"
                    || q.method.trim().is_empty()
                    || !source_ok(&q.source, sources)
                    || (!allow && q.evidence == Evidence::Estimate)
                    || q.bounds.is_some_and(|[a, b]| {
                        !a.is_finite() || !b.is_finite() || a > q.value || b < q.value
                    })
                {
                    return Err(bad(
                        "orthotropic Maxwell stiffness needs sourced N/m observations with admitted evidence and valid bounds",
                    ));
                }
                self.matching_conditions(&q.conditions)?;
            }
            b.stiffness.factor()?;
        }
        Ok(())
    }

    /// Compile against the equilibrium membrane's symmetric positive-definite origin tangent.
    /// A nonlinear equilibrium law is allowed; its transient branches are explicitly quadratic.
    ///
    /// # Errors
    /// Invalid schema/evidence, unstable equilibrium tangent or unrepresentable branch stiffness.
    pub fn compile(
        &self,
        sources: &BTreeMap<String, Source>,
        allow: bool,
        q: [[f64; 3]; 3],
    ) -> Result<CompiledRelaxation, Refusal> {
        self.validate(sources, allow)?;
        let scale = q.iter().flatten().map(|x| x.abs()).fold(0.0, f64::max);
        if scale <= 0.0 || q.iter().flatten().any(|x| !x.is_finite()) {
            return Err(bad(
                "Maxwell equilibrium tangent must be finite and positive definite",
            ));
        }
        let mut l = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..=i {
                if q[i][j] != q[j][i] {
                    return Err(bad("Maxwell tangent must be symmetric"));
                }
                let v = q[i][j] / scale - (0..j).map(|k| l[i][k] * l[j][k]).sum::<f64>();
                if i == j {
                    if v <= 0.0 || !v.is_finite() {
                        return Err(bad("Maxwell tangent must be positive definite"));
                    }
                    l[i][j] = v.sqrt();
                } else {
                    l[i][j] = v / l[j][j];
                }
            }
        }
        let mut branches = Vec::new();
        let mut total = q;
        for b in &self.branches {
            let scaled = q.map(|row| row.map(|v| v * b.stiffness_ratio.value));
            for i in 0..3 {
                for j in 0..3 {
                    total[i][j] += scaled[i][j];
                    if !total[i][j].is_finite() || (q[i][j] != 0.0 && scaled[i][j] == 0.0) {
                        return Err(bad("Maxwell stiffness overflows or underflows"));
                    }
                }
            }
            branches.push(Branch {
                q: scaled,
                tau: b.relaxation_time.value,
                energy_factor: None,
            });
        }
        for b in &self.orthotropic_branches {
            let tensor = b.stiffness.matrix();
            for i in 0..3 {
                for j in 0..3 {
                    total[i][j] += tensor[i][j];
                    if !total[i][j].is_finite() {
                        return Err(bad("orthotropic Maxwell instantaneous stiffness overflows"));
                    }
                }
            }
            branches.push(Branch {
                q: tensor,
                tau: b.relaxation_time.value,
                energy_factor: Some(b.stiffness.factor()?),
            });
        }
        let bytes =
            serde_json::to_vec(&(self, sources, allow, q)).map_err(|e| bad(e.to_string()))?;
        Ok(CompiledRelaxation {
            branches,
            origin_tangent: q,
            range: self.strain_range,
            initialization: self.initialization,
            fingerprint: sha256_hex(&bytes),
            orthotropic_branches: !self.orthotropic_branches.is_empty(),
        })
    }
}

impl CompiledRelaxation {
    /// Whether the spectrum contains independently specified directional tensors.
    #[must_use]
    pub fn has_orthotropic_branches(&self) -> bool {
        self.orthotropic_branches
    }

    /// Equilibrium reference tangent against which branch ratios were characterized.
    #[must_use]
    pub fn origin_tangent(&self) -> [[f64; 3]; 3] {
        self.origin_tangent
    }

    /// Source/parameter/tangent identity, not experimental validation.
    #[must_use]
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    /// Is the physical strain within the supplied domain?
    #[must_use]
    pub fn contains(&self, e: [f64; 3]) -> bool {
        e.into_iter()
            .zip(self.range)
            .all(|(x, [a, b])| x.is_finite() && x >= a && x <= b)
    }

    /// Initialize ONCE according to the explicit material declaration. Continue with the resulting
    /// Memory thereafter; reconstructing it every frame would erase creep and relaxation.
    ///
    /// # Errors
    /// Initial material strain outside the characterization domain.
    pub fn initialize(&self, strain: [f64; 3]) -> Result<Memory, Refusal> {
        if !self.contains(strain) {
            return Err(bad("initial strain outside Maxwell domain"));
        }
        let z = match self.initialization {
            Initialization::StressFreeReference => [0.0; 3],
            Initialization::RelaxedStart => strain,
        };
        Ok(Memory {
            law: self.clone(),
            viscous: vec![z; self.branches.len()],
        })
    }
}

// a=tau/(tau+h), b=h/(tau+h), without overflowing tau+h or losing the small coefficient.
fn weights(tau: f64, h: f64) -> (f64, f64) {
    if h <= tau {
        let r = h / tau;
        (1.0 / (1.0 + r), r / (1.0 + r))
    } else {
        let r = tau / h;
        (r / (1.0 + r), 1.0 / (1.0 + r))
    }
}

fn mul(q: &[[f64; 3]; 3], e: [f64; 3]) -> [f64; 3] {
    q.map(|row| row.into_iter().zip(e).map(|(a, b)| a * b).sum())
}

fn factored_work(l: [[f64; 3]; 3], e: [f64; 3]) -> f64 {
    (0..3)
        .map(|j| {
            let v = (0..3).map(|i| l[i][j] * e[i]).sum::<f64>();
            v * v
        })
        .sum()
}

fn step_ok(h: f64) -> Result<(), Refusal> {
    if h.is_finite() && h > 0.0 {
        Ok(())
    } else {
        Err(bad("Maxwell step must be finite and positive"))
    }
}

impl Memory {
    /// Accepted internal strain per branch, in authored order.
    #[must_use]
    pub fn viscous_strains(&self) -> &[[f64; 3]] {
        &self.viscous
    }

    /// Additional branch energy/stress/tangent at strain. `Some(h)` evaluates the condensed
    /// incremental potential; `None` evaluates PHYSICAL spring storage at the accepted history.
    /// Neither changes history. Equilibrium membrane energy is added by the caller exactly once.
    ///
    /// # Errors
    /// Domain violation, invalid step, or arithmetic overflow.
    pub fn response(&self, e: [f64; 3], step: Option<f64>) -> Result<Response, Refusal> {
        if !self.law.contains(e) {
            return Err(bad("strain outside Maxwell characterization domain"));
        }
        if let Some(h) = step {
            step_ok(h)?;
        }
        let mut r = Response {
            energy: 0.0,
            stress: [0.0; 3],
            tangent: [[0.0; 3]; 3],
        };
        for (branch, z) in self.law.branches.iter().zip(&self.viscous) {
            let a = step.map_or(1.0, |h| weights(branch.tau, h).0);
            let d = std::array::from_fn(|j| e[j] - z[j]);
            let stress = mul(&branch.q, d).map(|v| a * v);
            r.energy += 0.5
                * if let Some(l) = branch.energy_factor {
                    a * factored_work(l, d)
                } else {
                    d.into_iter().zip(stress).map(|(x, y)| x * y).sum::<f64>()
                };
            for i in 0..3 {
                r.stress[i] += stress[i];
                for j in 0..3 {
                    r.tangent[i][j] += a * branch.q[i][j];
                }
            }
        }
        if !r.energy.is_finite()
            || r.energy < 0.0
            || r.stress
                .iter()
                .chain(r.tangent.iter().flatten())
                .any(|x| !x.is_finite())
        {
            return Err(bad("Maxwell response arithmetic overflow"));
        }
        Ok(r)
    }

    /// Propose a backward-Euler history update at the ACCEPTED endpoint, without mutating self.
    ///
    /// # Errors
    /// Invalid step/domain, non-finite energy or dissipation. Failed updates leave history intact.
    pub fn advance(&self, e: [f64; 3], h: f64) -> Result<Update, Refusal> {
        self.response(e, Some(h))?;
        let mut memory = self.clone();
        let mut dissipated = 0.0;
        for ((branch, old), new) in self
            .law
            .branches
            .iter()
            .zip(&self.viscous)
            .zip(&mut memory.viscous)
        {
            let (a, b) = weights(branch.tau, h);
            let d = std::array::from_fn(|j| e[j] - old[j]);
            let stress = mul(&branch.q, d);
            let work: f64 = if let Some(l) = branch.energy_factor {
                factored_work(l, d)
            } else {
                d.into_iter().zip(stress).map(|(x, y)| x * y).sum()
            };
            dissipated += (a * b) * work;
            *new = std::array::from_fn(|j| a * old[j] + b * e[j]);
        }
        let stored = memory.response(e, None)?.energy;
        if !dissipated.is_finite() || dissipated < 0.0 {
            return Err(bad("Maxwell dissipation overflow"));
        }
        Ok(Update {
            memory,
            stored_j_m2: stored,
            dissipated_j_m2: dissipated,
        })
    }
}

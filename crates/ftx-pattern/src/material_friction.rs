//! Source-bound scalar interfaces and identification from corrected steady sliding forces.
//! The fitted law is pull = mu * normal_load * slip_direction. Static breakaway, fixture drag,
//! adhesion, inertia and gravity are not fitted as Coulomb friction; acquisition must separate them.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode, det::hypot, receipt::sha256_hex};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::material_calibration::{Partition, Quality};
use crate::material_catalog::Selection;
use crate::materials::{Catalog, Evidence, Form, Quantity, Source};

/// Existing fabric.interfaces wire format; shared with the actual drape consumer.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Interface {
    /// Two exact material snapshot hashes, not names; order is irrelevant to the scalar law.
    pub materials: [String; 2],
    /// Source-qualified dimensionless Coulomb coefficient.
    pub friction: Quantity,
    /// Evidence sources, including the calibration receipt when fitted.
    pub sources: BTreeMap<String, Source>,
    /// Permission for THIS interface's estimates, independent of either sheet's policy.
    #[serde(default)]
    pub allow_estimates: bool,
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

fn text(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= 4096
}

fn sources_valid(sources: &BTreeMap<String, Source>) -> bool {
    !sources.is_empty()
        && sources.len() <= 64
        && sources
            .iter()
            .all(|(k, v)| text(k) && text(&v.reference) && text(&v.accessed_on) && text(&v.note))
}

fn conditions_valid(conditions: &BTreeMap<String, String>) -> bool {
    conditions.len() <= 64 && conditions.iter().all(|(k, v)| text(k) && text(v))
}

impl Interface {
    /// Parse the existing interface contract, retaining its field/default/error policy.
    ///
    /// # Errors
    /// Non-object, unknown fields, malformed source/quantity data or non-boolean consent.
    pub fn parse(raw: &Value) -> Result<Self, Refusal> {
        let object = raw
            .as_object()
            .ok_or_else(|| invalid("interface must be an object"))?;
        if object
            .keys()
            .any(|k| !["materials", "friction", "sources", "allow_estimates"].contains(&k.as_str()))
        {
            return Err(invalid("unknown interface field"));
        }
        let materials = serde_json::from_value(raw["materials"].clone())
            .map_err(|e| invalid(format!("interface needs two material snapshot hashes: {e}")))?;
        let friction = serde_json::from_value(raw["friction"].clone())
            .map_err(|e| invalid(format!("interface friction observation: {e}")))?;
        let sources = serde_json::from_value(raw["sources"].clone())
            .map_err(|e| invalid(format!("interface source records: {e}")))?;
        let allow_estimates = match object.get("allow_estimates") {
            None => false,
            Some(v) => v
                .as_bool()
                .ok_or_else(|| invalid("allow_estimates must be boolean"))?,
        };
        Ok(Self {
            materials,
            friction,
            sources,
            allow_estimates,
        })
    }

    /// Source-complete existing drape receipt representation.
    #[must_use]
    pub fn report(&self) -> Value {
        json!({"materials":self.materials, "friction":self.friction,
            "sources":self.sources, "allow_estimates":self.allow_estimates})
    }

    /// Validate the observation without granting permission to consume an estimated coefficient.
    /// Binding may retain valid evidence while a mechanical consumer separately checks admission.
    ///
    /// # Errors
    /// Invalid units, values, bounds or metadata. Assembly separately checks assigned hashes.
    pub fn validate_observation(&self) -> Result<(), Refusal> {
        let q = &self.friction;
        if q.unit != "1"
            || !q.value.is_finite()
            || q.value < 0.0
            || !text(&q.method)
            || !self.sources.contains_key(&q.source)
            || !sources_valid(&self.sources)
            || !conditions_valid(&q.conditions)
            || q.bounds.is_some_and(|[lo, hi]| {
                !lo.is_finite() || !hi.is_finite() || lo < 0.0 || lo > q.value || hi < q.value
            })
        {
            return Err(invalid(
                "interface friction needs finite nonnegative dimensionless observations, valid bounds and complete source/method metadata",
            ));
        }
        Ok(())
    }

    /// Validate observations and consumption consent; assembly separately checks assigned hashes.
    ///
    /// # Errors
    /// Invalid units, values, bounds, metadata or unapproved estimated coefficient.
    pub fn validate(&self) -> Result<(), Refusal> {
        self.validate_observation()?;
        if self.friction.evidence == Evidence::Estimate && !self.allow_estimates {
            return Err(invalid(
                "interface friction estimate needs its own explicit consent",
            ));
        }
        Ok(())
    }
}

/// Explicit physical counterfaces and output destination. No unlike-material mixing rule exists.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Target {
    /// Two distinct sheet snapshots; produces an existing fabric.interfaces entry.
    Pair {
        /// The exact two specimens, with independent material evidence policies.
        materials: Box<[Selection; 2]>,
    },
    /// A sheet tested against itself; only this record's friction_self may be updated.
    SelfContact {
        /// Exact specimen selection; it need not already have complete mechanical coverage.
        material: Box<Selection>,
    },
    /// A sheet tested against a named body surface or surrogate; never inferred from self-contact.
    Body {
        /// Exact specimen selection.
        material: Box<Selection>,
        /// Identification/conditioning of the tested counterface, not a personal measurement.
        counterface: Source,
    },
}

/// Corrected, quasi-static, steady sliding observation in a common two-dimensional tangent frame.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sample {
    /// Actual compressive normal contact load, N; not guessed from nominal sled mass.
    pub normal_n: f64,
    /// Corrected pulling force in N, pointing WITH slip. Remove apparatus drag, gravity and inertia.
    /// Null denotes an unmeasured component, not a zero-force observation.
    pub pull_n: [Option<f64>; 2],
    /// Measured nonzero relative tangent velocity, m/s. Zero-speed/breakaway data are not this law.
    pub velocity_m_s: [f64; 2],
    /// Positive residual weighting scale, N; not automatically a measurement standard deviation.
    pub scale_n: f64,
}

/// Entire acquisitions are assigned to fitting or checking, not interleaved train/check time points.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Experiment {
    /// Distinct acquisition identity.
    pub id: String,
    /// Key in Input.sources, covering both counterfaces, fixture and force corrections.
    pub source: String,
    /// Origin of these observations; synthetic data requires explicit permission.
    pub evidence: Evidence,
    /// Whole-acquisition partition; checks never influence identification.
    pub partition: Partition,
    /// Steady sliding states; no claim that correlated states are independent replicates.
    pub samples: Vec<Sample>,
}

/// Source-bearing steady sliding calibration. No solver constants or generic fabric presets.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    /// Must equal ftx-friction-calibration/1.
    pub schema: String,
    /// Physical surfaces and exact destination of the identified property.
    pub target: Target,
    /// Fixture, loaded faces, grain alignment, conditioning and force-reduction protocol.
    pub method: String,
    /// Shared specimen/test conditions, not an automatic condition-dependent constitutive law.
    pub conditions: BTreeMap<String, String>,
    /// Explicit acquisition/analysis date; no wall-clock dependency in replay.
    pub fitted_on: String,
    /// Acquisition sources. Material sources stay with the frozen selections.
    pub sources: BTreeMap<String, Source>,
    /// At most 128 experiments and 4096 total samples, evaluated in authored order.
    pub experiments: Vec<Experiment>,
    /// Admission of estimated INPUT forces. Does not grant permission to simulate a fitted estimate.
    #[serde(default)]
    pub allow_estimated_observations: bool,
    /// Permit replacing an existing friction_self/friction_skin value; preserve it in the receipt.
    #[serde(default)]
    pub replace_existing: bool,
    /// Separate fit/check RMS residual budget, divided by each observation's scale_n.
    pub max_rms_scaled: f64,
    /// Separate fit/check maximum absolute scaled component residual budget.
    pub max_peak_scaled: f64,
}

/// One model prediction in the original acquisition tangent frame.
#[derive(Clone, Debug, Serialize)]
pub struct Prediction {
    /// Acquisition identity.
    pub experiment: String,
    /// Zero-based sample index within the acquisition.
    pub sample: usize,
    /// Whether the acquisition was used for fitting.
    pub partition: Partition,
    /// Pull predicted by the scalar isotropic law, N.
    pub pull_n: [f64; 2],
    /// Predicted minus observed, N, including transverse errors; unmeasured components stay absent.
    pub residual_n: [Option<f64>; 2],
}

/// Numerical fit plus the complete input and the exact resolved pre-fit specimen records.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// ftx-friction-calibration-report/1.
    pub schema: String,
    /// Authored acquisition, not changed into an empirical-validation claim.
    pub input: Input,
    /// Full locked snapshots used for fitting, so catalog changes cannot silently relabel a fit.
    pub resolved_materials: Vec<Selection>,
    /// SHA-256 of (input, resolved_materials), not a signature authenticating observations.
    pub input_sha256: String,
    /// Fitted dimensionless effective kinetic coefficient; always estimate class.
    pub coefficient: Quantity,
    /// Numerical fit quality; not a confidence interval.
    pub fit: Quality,
    /// Genuinely held-out quality; None explicitly means no independent check was provided.
    pub check: Option<Quality>,
    /// Observed fit-only normal load envelope, N, NOT a runtime load-domain enforcement mechanism.
    pub fit_normal_n_range: [f64; 2],
    /// Observed fit-only speed envelope, m/s; no extrapolation or static-friction qualification.
    pub fit_speed_m_s_range: [f64; 2],
    /// Predictions in input order, with checks clearly distinguished.
    pub predictions: Vec<Prediction>,
}

/// Ready-to-bind existing wire types. Neither output rewrites geometry or unrelated properties.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Binding {
    /// Add the entry to fabric.interfaces with these unchanged specimen selections.
    Interface {
        /// Exact resolved targets, retaining each sheet's original evidence policy.
        materials: Vec<Selection>,
        /// Same interface type consumed by the drape resolver; own consent is separate.
        interface: Box<Interface>,
    },
    /// Bind via the ordinary material selection workflow; all other observations are retained.
    Material {
        /// Locked record containing only the selected friction-property replacement.
        selection: Box<Selection>,
    },
}

fn sample_frame(s: &Sample) -> Result<([f64; 2], f64), Refusal> {
    let speed = hypot(s.velocity_m_s[0], s.velocity_m_s[1]);
    if !s.normal_n.is_finite()
        || s.normal_n <= 0.0
        || !s.scale_n.is_finite()
        || s.scale_n <= 0.0
        || s.pull_n
            .iter()
            .flatten()
            .chain(&s.velocity_m_s)
            .any(|x| !x.is_finite())
        || s.pull_n.iter().all(Option::is_none)
        || !speed.is_finite()
        || speed <= 0.0
    {
        return Err(invalid(
            "friction acquisition requires positive normal load/scale and nonzero finite steady slip",
        ));
    }
    Ok((s.velocity_m_s.map(|v| v / speed), speed))
}

fn quality(errors: &[f64], input: &Input, name: &str) -> Result<Quality, Refusal> {
    let peak = errors.iter().copied().map(f64::abs).fold(0.0, f64::max);
    let rms = (errors.iter().map(|v| v * v).sum::<f64>() / errors.len() as f64).sqrt();
    if !rms.is_finite()
        || !peak.is_finite()
        || rms > input.max_rms_scaled
        || peak > input.max_peak_scaled
    {
        return Err(invalid(format!(
            "{name} friction residual budget exceeded (RMS {rms}, peak {peak}); review anisotropy, force offsets or non-Coulomb behavior"
        )));
    }
    Ok(Quality {
        components: errors.len(),
        rms_scaled: rms,
        peak_scaled: peak,
    })
}

/// Identify one isotropic, speed-independent kinetic coefficient; no static or adhesion intercept.
///
/// # Errors
/// Malformed data/evidence, no fitting states, zero-speed/loading, incompatible force directions,
/// overflow, unsupported material form, identical pair targets or excessive fit/check error.
pub fn calibrate(input: &Input, catalog: &Catalog) -> Result<Report, Refusal> {
    let count = input
        .experiments
        .iter()
        .try_fold(0usize, |n, e| n.checked_add(e.samples.len()));
    if input.schema != "ftx-friction-calibration/1"
        || !text(&input.method)
        || !text(&input.fitted_on)
        || !sources_valid(&input.sources)
        || input.sources.len() >= 64
        || !conditions_valid(&input.conditions)
        || input.conditions.len() > 60
        || input.experiments.is_empty()
        || input.experiments.len() > 128
        || count.is_none_or(|n| n == 0 || n > 4096)
        || !input.max_rms_scaled.is_finite()
        || input.max_rms_scaled <= 0.0
        || !input.max_peak_scaled.is_finite()
        || input.max_peak_scaled <= 0.0
    {
        return Err(invalid(
            "invalid friction acquisition schema, protocol, metadata, budget or residual limits",
        ));
    }
    let targets = match &input.target {
        Target::Pair { materials } => materials.as_slice(),
        Target::SelfContact { material } => std::slice::from_ref(material.as_ref()),
        Target::Body {
            material,
            counterface,
        } => {
            if !text(&counterface.reference)
                || !text(&counterface.accessed_on)
                || !text(&counterface.note)
            {
                return Err(invalid(
                    "body friction requires an identified counterface and conditioning source",
                ));
            }
            std::slice::from_ref(material.as_ref())
        }
    };
    let mut resolved_materials = Vec::new();
    for target in targets {
        let m = catalog.resolve(target)?;
        if matches!(m.material().form, Form::Fiber | Form::Yarn | Form::Fill) {
            return Err(invalid(
                "friction binding requires sheet specimens, not fiber/yarn/loose fill",
            ));
        }
        resolved_materials.push(m.frozen_selection());
    }
    if targets.len() == 2
        && resolved_materials[0].expected_sha256 == resolved_materials[1].expected_sha256
    {
        return Err(invalid(
            "identical pair snapshots require a self_contact target; no duplicate interface diagonal",
        ));
    }
    let mut ids = BTreeSet::new();
    let mut rows = Vec::new();
    let mut normal = [f64::INFINITY, f64::NEG_INFINITY];
    let mut speeds = normal;
    for e in &input.experiments {
        if !text(&e.id)
            || !ids.insert(&e.id)
            || e.samples.is_empty()
            || !input.sources.contains_key(&e.source)
            || (e.evidence == Evidence::Estimate && !input.allow_estimated_observations)
        {
            return Err(invalid(
                "friction experiment needs a unique ID, source, samples and admitted evidence",
            ));
        }
        for s in &e.samples {
            let (direction, speed) = sample_frame(s)?;
            if e.partition == Partition::Fit {
                let load = s.normal_n / s.scale_n;
                if !load.is_finite() || load <= 0.0 {
                    return Err(invalid("friction weighting arithmetic overflow/underflow"));
                }
                for (component, observed) in s.pull_n.iter().enumerate() {
                    if let Some(force) = observed {
                        let x = load * direction[component];
                        let y = force / s.scale_n;
                        if !x.is_finite() || !y.is_finite() {
                            return Err(invalid("friction component weighting overflow"));
                        }
                        rows.push((x, y));
                    }
                }
                normal = [normal[0].min(s.normal_n), normal[1].max(s.normal_n)];
                speeds = [speeds[0].min(speed), speeds[1].max(speed)];
            }
        }
    }
    if rows.is_empty() {
        return Err(invalid(
            "friction identification needs fit observations, not only checks",
        ));
    }
    // Normalize both columns and loads before products; no raw squared force can overflow the fit.
    let xscale = rows.iter().map(|r| r.0.abs()).fold(0.0, f64::max);
    let yscale = rows.iter().map(|r| r.1.abs()).fold(0.0, f64::max);
    if xscale <= 0.0 {
        return Err(invalid(
            "no measured force component observes the slip direction",
        ));
    }
    let mu = if yscale == 0.0 {
        0.0
    } else {
        let mut xx = 0.0;
        let mut xy = 0.0;
        for &(load, pull) in &rows {
            let x = load / xscale;
            xx += x * x;
            xy += x * (pull / yscale);
        }
        (xy / xx) * (yscale / xscale)
    };
    if !mu.is_finite() || mu < 0.0 {
        return Err(invalid(
            "identified friction coefficient is negative/non-finite; check pull/slip convention",
        ));
    }
    let mut errors = Vec::new();
    let mut checks = Vec::new();
    let mut predictions = Vec::new();
    for e in &input.experiments {
        for (sample, s) in e.samples.iter().enumerate() {
            let (direction, _) = sample_frame(s)?;
            let pull = direction.map(|d| mu * s.normal_n * d);
            let residual: [Option<f64>; 2] =
                std::array::from_fn(|j| s.pull_n[j].map(|p| pull[j] - p));
            let scaled = residual.map(|r| r.map(|v| v / s.scale_n));
            if pull
                .iter()
                .chain(residual.iter().flatten())
                .chain(scaled.iter().flatten())
                .any(|v| !v.is_finite())
            {
                return Err(invalid("friction prediction/residual overflow"));
            }
            if e.partition == Partition::Fit {
                errors.extend(scaled.into_iter().flatten());
            } else {
                checks.extend(scaled.into_iter().flatten());
            }
            predictions.push(Prediction {
                experiment: e.id.clone(),
                sample,
                partition: e.partition,
                pull_n: pull,
                residual_n: residual,
            });
        }
    }
    let fit = quality(&errors, input, "fit")?;
    let check = if checks.is_empty() {
        None
    } else {
        Some(quality(&checks, input, "held-out")?)
    };
    let bytes =
        serde_json::to_vec(&(input, &resolved_materials)).map_err(|e| invalid(e.to_string()))?;
    let input_sha256 = sha256_hex(&bytes);
    let mut conditions = input.conditions.clone();
    conditions.insert(
        "fit_normal_n_range".into(),
        serde_json::to_string(&normal).map_err(|e| invalid(e.to_string()))?,
    );
    conditions.insert(
        "fit_speed_m_s_range".into(),
        serde_json::to_string(&speeds).map_err(|e| invalid(e.to_string()))?,
    );
    conditions.insert("held_out_checks".into(), check.is_some().to_string());
    let coefficient = Quantity {
        value: mu, unit: "1".into(), evidence: Evidence::Estimate,
        source: format!("friction-fit-{input_sha256}"),
        method: "Weighted zero-intercept vector fit: corrected steady pull = mu * normal load * slip direction. Isotropic kinetic Coulomb law only; no static breakaway, apparatus drag, adhesion or numerical smoothing fit.".into(),
        conditions, bounds: None,
    };
    Ok(Report {
        schema: "ftx-friction-calibration-report/1".into(),
        input: input.clone(),
        resolved_materials,
        input_sha256,
        coefficient,
        fit,
        check,
        fit_normal_n_range: normal,
        fit_speed_m_s_range: speeds,
        predictions,
    })
}

impl Report {
    /// Build the existing material/interface wire types with separate permission for fitted estimates.
    ///
    /// # Errors
    /// Rebound snapshot, receipt source conflict, implicit replacement of existing sheet friction,
    /// or invalid emitted material. Complete mechanics is intentionally a separate consumer check.
    pub fn binding(&self, catalog: &Catalog, allow_estimates: bool) -> Result<Binding, Refusal> {
        let count = if matches!(self.input.target, Target::Pair { .. }) {
            2
        } else {
            1
        };
        if self.resolved_materials.len() != count {
            return Err(invalid(
                "friction report has the wrong number of resolved counterfaces",
            ));
        }
        let receipt = Source {
            reference: format!("local:ftx-friction-calibration/{}", self.input_sha256),
            accessed_on: self.input.fitted_on.clone(),
            note: format!(
                "Effective kinetic coefficient; raw acquisition and frozen counterfaces retained in receipt SHA-256 {}. Not empirical garment or static-friction validation.",
                self.input_sha256
            ),
        };
        let mut records = self
            .resolved_materials
            .iter()
            .map(|s| catalog.resolve(s))
            .collect::<Result<Vec<_>, _>>()?;
        let key = match &self.input.target {
            Target::Pair { .. } => {
                let mut sources = self.input.sources.clone();
                if sources
                    .insert(self.coefficient.source.clone(), receipt)
                    .is_some()
                {
                    return Err(invalid("friction receipt source conflict"));
                }
                let mut hashes = [
                    records[0].sha256().to_owned(),
                    records[1].sha256().to_owned(),
                ];
                hashes.sort();
                return Ok(Binding::Interface {
                    materials: self.resolved_materials.clone(),
                    interface: Box::new(Interface {
                        materials: hashes,
                        friction: self.coefficient.clone(),
                        sources,
                        allow_estimates,
                    }),
                });
            }
            Target::SelfContact { .. } => "friction_self",
            Target::Body { .. } => "friction_skin",
        };
        let mut record = records.remove(0).authored().clone();
        if record.properties.contains_key(key) && !self.input.replace_existing {
            return Err(invalid(format!(
                "existing {key} requires explicit replace_existing; prior evidence stays in receipt"
            )));
        }
        if record
            .sources
            .insert(self.coefficient.source.clone(), receipt)
            .is_some()
        {
            return Err(invalid("friction receipt would rebind an existing source"));
        }
        record
            .properties
            .insert(key.into(), self.coefficient.clone());
        let selected = catalog.resolve(&Selection {
            id: record.id.clone(),
            record: Some(record),
            allow_estimates,
            ..Selection::default()
        })?;
        Ok(Binding::Material {
            selection: Box::new(selected.frozen_selection()),
        })
    }
}

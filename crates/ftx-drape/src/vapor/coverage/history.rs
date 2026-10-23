//! Bounded moving-coverage potential integrals under fixed prescribed vapor reservoirs.
//! Every accepted position sample reuses the ordinary geometry and canonical Ret evaluator.
//! No water state, latent cooling, unseen motion or empirical garment response is inferred.

use ftx_core::{Refusal, Vec3};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{MAX_BYTES, Request as CoverageRequest, add, bounded, budget, digest, invalid};
use crate::vapor::{BoundsSum, TransferBounds};

/// Portable complete moving-coverage experiment, independent of a body-model installation.
pub const SCHEMA: &str = "ftx-vapor-coverage-history/1";
const MAX_SAMPLES: usize = 512;
const MAX_VERTICES: usize = 2_000_000;

/// Explicit temporal quadrature for prescribed-reservoir potential, not moisture evolution.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Integration {
    /// Hold the newly accepted body/cloth geometry through its preceding mechanical interval.
    RightEndpoint,
    /// Average the two sampled endpoint potentials over each accepted mechanical interval.
    /// This interpolates potential rates, not unobserved geometry or a moisture state.
    CompositeTrapezoid,
}

/// Opt-in integration policy. The scenario's existing vapor-coverage settings remain authoritative.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    /// Must be `ftx-motion-vapor-coverage/1`.
    pub schema: String,
    /// Declared integration rule; no automatic interpolation or adaptive error claim.
    pub integration: Integration,
    /// Total clipping/candidate/allocation operations across initial and accepted samples,
    /// in 1..=20000000. The remaining allowance is enforced inside the existing projection kernel.
    pub max_geometry_operations: usize,
}

impl Policy {
    /// Check the explicit policy before any numerical work or session reservation.
    ///
    /// # Errors
    /// Unknown schema or unsupported work allowance.
    pub fn validate(&self) -> Result<(), Refusal> {
        if self.schema != "ftx-motion-vapor-coverage/1"
            || !(1..=20_000_000).contains(&self.max_geometry_operations)
        {
            return Err(invalid(
                "invalid moving vapor coverage schema or geometry budget",
            ));
        }
        Ok(())
    }
}

/// One accepted mechanical sample. Triangle topology, materials and boundaries stay fixed.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sample {
    /// Strictly increasing seconds since the initial post-drape state, through 60 seconds.
    pub time_s: f64,
    /// All retained cloth vertices, preserving the initial order and count.
    pub positions_m: Vec<Vec3>,
    /// All retained body vertices, preserving the initial order and count.
    pub body_positions_m: Vec<Vec3>,
}

/// Complete frozen source, fixed boundary policy and actual accepted geometry history.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Must equal [`SCHEMA`].
    pub schema: String,
    /// Explicit temporal and bounded-work choices.
    pub policy: Policy,
    /// Time-zero source geometry and fixed material/body-triangle/boundary identities.
    pub initial: CoverageRequest,
    /// One to 512 accepted samples, excluding the separately retained initial state.
    pub samples: Vec<Sample>,
    /// Optional originating motion-path identity. It establishes identity, not authenticity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_sha256: Option<String>,
}

impl Request {
    /// Replay every sampled overlap and integrate the complete potential without hidden state.
    /// A failed sample refuses the whole history; no partial integral is published.
    ///
    /// # Errors
    /// Invalid or stale source, topology/time changes, missing qualified transport, ambiguous
    /// projection, unadmitted source protocols, or exhausted geometry/retention budgets.
    pub fn evaluate(&self) -> Result<Value, Refusal> {
        if self.schema != SCHEMA || self.samples.is_empty() || self.samples.len() > MAX_SAMPLES {
            return Err(invalid(
                "vapor history needs its schema and 1..512 accepted samples",
            ));
        }
        bounded(self)?;
        let mut count = self
            .initial
            .source
            .positions_m
            .len()
            .saturating_add(self.initial.map.body_positions_m.len());
        let mut previous_time = 0.0;
        for sample in &self.samples {
            if !sample.time_s.is_finite()
                || sample.time_s > 60.0
                || sample.time_s - previous_time < 1e-9
                || sample.positions_m.len() != self.initial.source.positions_m.len()
                || sample.body_positions_m.len() != self.initial.map.body_positions_m.len()
            {
                return Err(invalid(
                    "vapor history samples must preserve vertex counts and increasing times through 60 s",
                ));
            }
            previous_time = sample.time_s;
            count = count
                .saturating_add(sample.positions_m.len())
                .saturating_add(sample.body_positions_m.len());
        }
        if count > MAX_VERTICES {
            return Err(budget(
                "vapor history exceeds two million retained vertices",
            ));
        }
        validate_path(self.path_sha256.as_deref())?;
        let mut history = History::new(self.initial.clone(), self.policy.clone())?;
        for sample in &self.samples {
            history.advance(sample.clone())?;
        }
        history.finish(self.path_sha256.clone())
    }
}

fn validate_path(path: Option<&str>) -> Result<(), Refusal> {
    if path.is_some_and(|s| s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit())) {
        return Err(invalid(
            "vapor history path identity must be a SHA-256 digest",
        ));
    }
    Ok(())
}

fn bytes(value: &impl Serialize) -> Result<usize, Refusal> {
    Ok(serde_json::to_vec_pretty(value)
        .map_err(|e| invalid(e.to_string()))?
        .len())
}

fn number(value: &Value, name: &str) -> Result<f64, Refusal> {
    value
        .as_f64()
        .filter(|v| v.is_finite() && *v >= 0.0)
        .ok_or_else(|| invalid(format!("vapor history needs finite nonnegative {name}")))
}

fn integral(rate: f64, duration: f64) -> Result<f64, Refusal> {
    let value = rate * duration;
    if !value.is_finite() || (rate > 0.0 && value <= 0.0) {
        return Err(invalid(
            "vapor history interval integral overflows or loses positive transfer",
        ));
    }
    Ok(value)
}

fn trapezoid(previous_rate: f64, current_rate: f64, duration: f64) -> Result<f64, Refusal> {
    // Scale before adding so two finite large rates need not overflow their intermediate sum.
    // Each endpoint contribution must also survive the existing positive-transfer checks.
    let half_duration = 0.5 * duration;
    let mut value = integral(previous_rate, half_duration)?;
    add(&mut value, integral(current_rate, half_duration)?)?;
    Ok(value)
}

fn operations(report: &Value) -> Result<usize, Refusal> {
    report["projection"]["geometry_operations"]
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(|| invalid("vapor coverage did not retain its geometry work"))
}

/// Private incremental form for the production motion solver. Discarded after any refusal.
pub(crate) struct History {
    request: Request,
    initial: Value,
    rows: Vec<Value>,
    time_s: f64,
    geometry_operations: usize,
    retained_vertices: usize,
    retained_bytes: usize,
    potential_energy_j: f64,
    potential_mass_kg: f64,
    bounds: BoundsSum,
}

impl History {
    pub(crate) fn new(initial: CoverageRequest, policy: Policy) -> Result<Self, Refusal> {
        policy.validate()?;
        let retained_vertices = initial
            .source
            .positions_m
            .len()
            .saturating_add(initial.map.body_positions_m.len());
        if retained_vertices > MAX_VERTICES {
            return Err(budget(
                "vapor history initial geometry exceeds retention budget",
            ));
        }
        let result = initial.evaluate_with_geometry_budget(policy.max_geometry_operations)?;
        let geometry_operations = operations(&result)?;
        // Use the canonical frozen request, including its verified initial body identity.
        let initial: CoverageRequest = serde_json::from_value(result["request"].clone())
            .map_err(|e| invalid(e.to_string()))?;
        let row = json!({"time_s":0.0,
            "positions_sha256":digest(&json!(initial.source.positions_m))?,
            "body_positions_sha256":digest(&json!(initial.map.body_positions_m))?,"result":result});
        let request = Request {
            schema: SCHEMA.into(),
            policy,
            initial,
            samples: Vec::new(),
            path_sha256: None,
        };
        // The final report retains both structured request and escaped exact replay text.
        // Four times the pretty request size conservatively covers JSON string escaping too.
        let retained_bytes = bytes(&request)?
            .saturating_mul(4)
            .saturating_add(bytes(&row)?)
            .saturating_add(16_384);
        if retained_bytes >= MAX_BYTES {
            return Err(budget(
                "vapor history initial report exceeds 64 MiB retention budget",
            ));
        }
        Ok(Self {
            request,
            initial: row,
            rows: Vec::new(),
            time_s: 0.0,
            geometry_operations,
            retained_vertices,
            retained_bytes,
            potential_energy_j: 0.0,
            potential_mass_kg: 0.0,
            bounds: BoundsSum::new(),
        })
    }

    pub(crate) fn advance(&mut self, sample: Sample) -> Result<(), Refusal> {
        if !sample.time_s.is_finite()
            || sample.time_s > 60.0
            || sample.time_s - self.time_s < 1e-9
            || self.rows.len() >= MAX_SAMPLES
        {
            return Err(invalid(
                "vapor history needs ordered accepted times through 60 s and at most 512 samples",
            ));
        }
        let retained_vertices = self
            .retained_vertices
            .saturating_add(sample.positions_m.len())
            .saturating_add(sample.body_positions_m.len());
        let sample_bytes = bytes(&sample)?.saturating_mul(4);
        if retained_vertices > MAX_VERTICES
            || self.retained_bytes.saturating_add(sample_bytes) >= MAX_BYTES
        {
            return Err(budget(
                "vapor history exhausts retained geometry or replay bytes",
            ));
        }
        let current = self
            .request
            .initial
            .with_positions(&sample.positions_m, &sample.body_positions_m)?;
        let remaining = self
            .request
            .policy
            .max_geometry_operations
            .checked_sub(self.geometry_operations)
            .filter(|n| *n != 0)
            .ok_or_else(|| budget("vapor history exhausts aggregate projection work"))?;
        let result = current.evaluate_with_geometry_budget(remaining)?;
        let geometry_operations = self
            .geometry_operations
            .checked_add(operations(&result)?)
            .filter(|&n| n <= self.request.policy.max_geometry_operations)
            .ok_or_else(|| budget("vapor history exhausts aggregate projection work"))?;
        let dt = sample.time_s - self.time_s;
        let integrate = |key: &str, name: &str| {
            let rate = number(&result["coverage"][key], name)?;
            match self.request.policy.integration {
                Integration::RightEndpoint => integral(rate, dt),
                Integration::CompositeTrapezoid => {
                    let previous = self.rows.last().unwrap_or(&self.initial);
                    trapezoid(
                        number(&previous["result"]["coverage"][key], name)?,
                        rate,
                        dt,
                    )
                }
            }
        };
        let energy = integrate("covered_potential_heat_flow_w", "power")?;
        let mass = integrate("covered_potential_mass_flow_kg_s", "mass flow")?;
        let mut total_energy = self.potential_energy_j;
        let mut total_mass = self.potential_mass_kg;
        add(&mut total_energy, energy)?;
        add(&mut total_mass, mass)?;
        let label = format!("sample {}", self.rows.len());
        let mut interval_bounds = BoundsSum::new();
        match self.request.policy.integration {
            Integration::RightEndpoint => interval_bounds.add_scaled(
                TransferBounds::from_json(
                    &result["coverage"]["reported_bounds"],
                    "covered_potential_heat_flow_w",
                    "covered_potential_mass_flow_kg_s",
                ),
                dt,
                [energy, mass],
                &label,
            ),
            Integration::CompositeTrapezoid => {
                let previous = self.rows.last().unwrap_or(&self.initial);
                let half_dt = 0.5 * dt;
                for (coverage, endpoint) in [
                    (&previous["result"]["coverage"], "start"),
                    (&result["coverage"], "end"),
                ] {
                    interval_bounds.add_scaled(
                        TransferBounds::from_json(
                            &coverage["reported_bounds"],
                            "covered_potential_heat_flow_w",
                            "covered_potential_mass_flow_kg_s",
                        ),
                        half_dt,
                        [
                            integral(
                                number(&coverage["covered_potential_heat_flow_w"], "power")?,
                                half_dt,
                            )?,
                            integral(
                                number(&coverage["covered_potential_mass_flow_kg_s"], "mass flow")?,
                                half_dt,
                            )?,
                        ],
                        &format!("{label} {endpoint}"),
                    );
                }
            }
        }
        let mut total_bounds = self.bounds.clone();
        total_bounds.add_scaled(interval_bounds.value, 1., [energy, mass], &label);
        let row = json!({"sample_index":self.rows.len(),"time_s":sample.time_s,"dt_s":dt,
            "positions_sha256":digest(&json!(sample.positions_m))?,
            "body_positions_sha256":digest(&json!(sample.body_positions_m))?,"result":result,
            "potential_energy_j":energy,"potential_mass_kg":mass,
            "reported_bounds":interval_bounds.json("potential_energy_j","potential_mass_kg"),
            "bounds_unavailable":interval_bounds.unavailable});
        let retained_bytes = self
            .retained_bytes
            .saturating_add(sample_bytes)
            .saturating_add(bytes(&row)?);
        if retained_bytes >= MAX_BYTES {
            return Err(budget(
                "vapor history reports exceed 64 MiB retention budget",
            ));
        }
        // Commit only a complete accepted numerical observation; caller discards a failed history.
        self.geometry_operations = geometry_operations;
        self.retained_vertices = retained_vertices;
        self.retained_bytes = retained_bytes;
        self.potential_energy_j = total_energy;
        self.potential_mass_kg = total_mass;
        self.bounds = total_bounds;
        self.time_s = sample.time_s;
        self.request.samples.push(sample);
        self.rows.push(row);
        Ok(())
    }

    pub(crate) fn finish(mut self, path_sha256: Option<String>) -> Result<Value, Refusal> {
        if self.rows.is_empty() {
            return Err(invalid("vapor history has no accepted motion samples"));
        }
        validate_path(path_sha256.as_deref())?;
        self.request.path_sha256 = path_sha256;
        let request = serde_json::to_value(&self.request).map_err(|e| invalid(e.to_string()))?;
        let integration_note = match self.request.policy.integration {
            Integration::RightEndpoint => {
                "Each right-endpoint potential uses newly projected body and cloth geometry with fixed material observations, triangle identities, vapor reservoirs and explicitly supplied interfaces. No unsampled geometry or strain-dependent Ret is inferred."
            }
            Integration::CompositeTrapezoid => {
                "Each composite-trapezoid interval averages its two sampled endpoint potentials, beginning with the initial state. Each endpoint uses newly projected body and cloth geometry with fixed material observations, triangle identities, vapor reservoirs and explicitly supplied interfaces. No unsampled geometry or strain-dependent Ret is inferred."
            }
        };
        let bounds_note = match self.request.policy.integration {
            Integration::RightEndpoint => {
                "Complete source Ret endpoints propagate through each fixed accepted sample and its exact positive duration, then sum without an independence assumption. Time-zero bounds are reported separately and carry no elapsed-time weight. Missing or unrepresentable sampled ranges leave integral ranges unavailable. These conditional ranges do not bound unsampled geometry, temporal quadrature, source SD/CV or physical model error."
            }
            Integration::CompositeTrapezoid => {
                "Complete source Ret endpoints propagate through both sampled endpoints of each interval with half its exact positive duration, then sum without an independence assumption. Time-zero bounds contribute to the first interval. Missing or unrepresentable endpoint ranges leave the affected interval and total ranges unavailable. These conditional ranges do not bound unsampled geometry, temporal quadrature, source SD/CV or physical model error."
            }
        };
        let result = json!({"schema":"ftx-vapor-coverage-history-report/1",
        "request_sha256":digest(&request)?,
        "replay_request_json":serde_json::to_string(&self.request).map_err(|e| invalid(e.to_string()))?,
        "request":request,"path_sha256":self.request.path_sha256,
        "initial_positions_sha256":self.initial["positions_sha256"],
        "initial":self.initial,"samples":self.rows,"duration_s":self.time_s,
        "integration":self.request.policy.integration,"geometry_operations":self.geometry_operations,
        "retained_vertex_samples":self.retained_vertices,
        "totals":{"potential_energy_j":self.potential_energy_j,"potential_mass_kg":self.potential_mass_kg,
            "reported_bounds":self.bounds.json("potential_energy_j","potential_mass_kg"),
            "bounds_unavailable":self.bounds.unavailable},
        "storage_required":false,"dry_thermal_properties_required":false,
        "latent_heat_coupled":false,"physical_validation_claim":false,"comfort_claim":false,
        "notes":[
            integration_note,
            "Integrals include covered body regions only. Uncovered transfer remains unknown; missing or ambiguous samples refuse the complete history instead of contributing zero.",
            bounds_note,
            "Potential joules and kilograms are prescribed-reservoir quadrature, not actual evaporation, sweat supply, stored moisture, latent cooling or an error-bounded transport solution."
        ]});
        bounded(&result)?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interval_arithmetic_retains_subnormal_transfer_or_refuses_its_loss() {
        assert_eq!(integral(0.0, 1e-9).unwrap(), 0.0);
        assert!(integral(f64::MIN_POSITIVE, 1e-9).unwrap() > 0.0);
        assert!(integral(f64::from_bits(1), 1e-9).is_err());
        assert!(integral(f64::MAX, 60.0).is_err());
        assert_eq!(trapezoid(f64::MAX, f64::MAX, 1.0).unwrap(), f64::MAX);
        assert_eq!(trapezoid(2.0, 0.0, 1.0).unwrap(), 1.0);
        assert!(trapezoid(f64::from_bits(1), 0.0, 1e-9).is_err());
        assert!(trapezoid(2.0, f64::from_bits(1), 2.0).is_err());
        let mut total = 1.0;
        assert!(add(&mut total, f64::from_bits(1)).is_err());
        assert_eq!(total, 1.0);
    }
}

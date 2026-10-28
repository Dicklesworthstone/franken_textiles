//! Explicitly qualified memoryless bridge exchange. The steady observation supplies only excess
//! conductance; an independent sourced time-domain law is mandatory before it can enter a history.
//! Geometry/ownership use the steady bridge authority, while qualification uses live transient
//! temperatures. No bridge storage, temperature reset or hidden steady temperature solve occurs.

use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
};

use ftx_core::{Refusal, Vec3};
use ftx_pattern::materials::{Evidence, Source};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{budget, hash, invalid};
use crate::{
    run::DrapeRun,
    thermal::{
        Boundary,
        steady::{
            coverage::bridges::{self as reference, Context, Domain},
            surface::SteadySurface,
        },
        surface::projection::Projection,
    },
};

const MODEL: &str = "qualified_zero_storage_centerline_exchange/1";

fn identity(value: &impl Serialize) -> Result<String, Refusal> {
    // Match the existing steady bridge's canonical JSON-value identity, independent of struct
    // field declaration order. Portable authored JSON and typed requests must bind identically.
    hash(&serde_json::to_value(value).map_err(|e| invalid(e.to_string()))?)
}

/// A separate source's authority to apply one exact excess observation instantaneously over the
/// declared elapsed-time, accepted mechanical interval and actual transient-temperature domains.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Qualification {
    /// Unique temporal observation identity in this request.
    pub id: String,
    /// Exact physical path in the reference request.
    pub path_id: String,
    /// Canonical SHA-256 of the complete reference qualification, including source and domains.
    pub steady_qualification_sha256: String,
    /// Absolute motion elapsed-time domain, seconds. Both ends of every sample must be covered.
    pub time_range_s: [f64; 2],
    /// Qualified duration of each accepted mechanical sample, seconds. Thermal subdivisions do
    /// not make the held mechanical geometry more accurate or extend this domain.
    pub sample_interval_range_s: [f64; 2],
    /// All covered cell, source-preload and reservoir temperatures must lie in this interval, °C.
    pub temperature_range_c: [f64; 2],
    /// Must be true: this law supplies equal boundary exchange and no bridge heat capacity.
    pub zero_storage: bool,
    /// Evidence for the temporal approximation, independent of the steady observation's evidence.
    pub evidence: Evidence,
    /// Independent permission for an estimated temporal law.
    #[serde(default)]
    pub allow_estimates: bool,
    /// Source key for the time-domain and zero-storage qualification.
    pub source: String,
    /// Complete provenance; this model never creates a material measurement.
    pub sources: BTreeMap<String, Source>,
    /// Source's method establishing the bounded instantaneous-exchange approximation.
    pub method: String,
    /// Exact specimen or synthetic mathematical fixture identity.
    pub specimen_id: String,
    /// Conditioning of that specimen and the temporal approximation.
    pub conditioning: String,
    /// Explicit source rationale and limitations for zero storage over these domains.
    pub qualification: String,
}

/// Optional moving-coverage exchange; a bare steady bridge request is never promoted implicitly.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Must be `ftx-motion-thermal-bridges/1`.
    pub schema: String,
    /// Must be `qualified_zero_storage_centerline_exchange/1`.
    pub model: String,
    /// Existing source-locked excess observations and exact retained mesh paths.
    pub reference: reference::Request,
    /// Independently sourced temporal alternatives. Every contributing portion needs one match.
    pub qualifications: Vec<Qualification>,
}

pub(super) struct Authority {
    source: SteadySurface,
    model_sha256: String,
    instances: Vec<String>,
    owners: Vec<usize>,
}

impl Authority {
    pub(super) fn from_run(run: &DrapeRun) -> Result<Self, Refusal> {
        Self::new(
            run.steady_thermal_surface()?,
            run.scene.mesh.instances.clone(),
            run.scene.triangle_owner.clone(),
        )
    }

    pub(super) fn new(
        source: SteadySurface,
        instances: Vec<String>,
        owners: Vec<usize>,
    ) -> Result<Self, Refusal> {
        let model = source.snapshot()?;
        Ok(Self {
            source,
            model_sha256: model["sha256"]
                .as_str()
                .expect("computed source identity")
                .into(),
            instances,
            owners,
        })
    }

    pub(super) fn model_sha256(&self) -> &str {
        &self.model_sha256
    }
}

pub(super) struct Sample {
    pub power_w: f64,
    pub body_power_w: Vec<f64>,
    pub energy_j: f64,
    pub geometry_operations: usize,
    pub report: Value,
}

impl Request {
    /// Validate exact reference bindings and source-complete temporal domains before simulation.
    ///
    /// # Errors
    /// Unsupported models, unbound/duplicate qualifications, missing source metadata, invalid
    /// time/temperature intervals, absent zero-storage consent or oversized input.
    pub fn validate(&self) -> Result<(), Refusal> {
        self.reference.validate()?;
        if self.schema != "ftx-motion-thermal-bridges/1"
            || self.model != MODEL
            || self.qualifications.is_empty()
            || self.qualifications.len() > 512
        {
            return Err(invalid(
                "moving thermal bridges need an explicit bounded zero-storage request",
            ));
        }
        let mut known = BTreeSet::new();
        for path in &self.reference.paths {
            for q in &path.qualifications {
                known.insert((path.id.as_str(), identity(q)?));
            }
        }
        let mut ids = BTreeSet::new();
        for q in &self.qualifications {
            if !reference::text(&q.id)
                || !ids.insert(&q.id)
                || !reference::text(&q.path_id)
                || !reference::hash(&q.steady_qualification_sha256)
                || !known.contains(&(q.path_id.as_str(), q.steady_qualification_sha256.clone()))
                || !reference::range(q.time_range_s, 0.)
                || q.time_range_s[0] == q.time_range_s[1]
                || !reference::range(q.sample_interval_range_s, f64::MIN_POSITIVE)
                || !reference::range(q.temperature_range_c, -273.15)
                || !q.zero_storage
                || [&q.method, &q.specimen_id, &q.conditioning, &q.qualification]
                    .iter()
                    .any(|s| !reference::text(s))
                || q.sources.is_empty()
                || q.sources.len() > 32
                || !q.sources.contains_key(&q.source)
                || q.sources.iter().any(|(id, source)| {
                    !reference::text(id)
                        || [&source.reference, &source.accessed_on, &source.note]
                            .iter()
                            .any(|s| !reference::text(s))
                })
            {
                return Err(invalid(
                    "moving thermal bridge needs a source-qualified zero-storage law bound to an exact reference observation and finite time/temperature domains",
                ));
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn evaluate(
        &self,
        authority: &Authority,
        cloth: &[Vec3],
        projection: &Projection,
        domains: &[Domain],
        boundaries: &[Boundary],
        time_range_s: [f64; 2],
    ) -> Result<Sample, Refusal> {
        if !reference::range(time_range_s, 0.) || time_range_s[0] == time_range_s[1] {
            return Err(invalid(
                "moving bridge needs a positive actual mechanical sample interval",
            ));
        }
        let seconds = time_range_s[1] - time_range_s[0];
        let mut reference_ids = BTreeMap::new();
        for path in &self.reference.paths {
            for q in &path.qualifications {
                reference_ids.insert((path.id.as_str(), q.id.as_str()), identity(q)?);
            }
        }
        let temporal_ids: Vec<_> = self
            .qualifications
            .iter()
            .map(identity)
            .collect::<Result<_, _>>()?;
        let work = Cell::new(0_usize);
        let used_regions = RefCell::new(BTreeSet::new());
        let qualify = |path: &reference::Path,
                       observation: &reference::Qualification,
                       region: usize| {
            work.set(
                work.get()
                    .checked_add(self.qualifications.len())
                    .filter(|n| *n <= 2_000_000)
                    .ok_or_else(|| {
                        budget("moving thermal bridge qualification exceeds two million operations")
                    })?,
            );
            let domain = &domains[region];
            let boundary = boundaries[projection.layout.regions[region].body_triangle];
            let reference_id = &reference_ids[&(path.id.as_str(), observation.id.as_str())];
            let matches: Vec<_> = self
                .qualifications
                .iter()
                .enumerate()
                .filter(|(_, q)| {
                    q.path_id == path.id
                        && &q.steady_qualification_sha256 == reference_id
                        && time_range_s
                            .iter()
                            .all(|&t| reference::contains(q.time_range_s, t))
                        && reference::contains(q.sample_interval_range_s, seconds)
                        && domain
                            .temperature_range_c
                            .iter()
                            .chain([&boundary.inside_c, &boundary.outside_c])
                            .all(|&t| reference::contains(q.temperature_range_c, t))
                })
                .collect();
            if matches.len() != 1 {
                return Err(invalid(
                    "moving thermal bridge needs exactly one temporal qualification covering its exact reference, elapsed time, accepted sample duration and actual transient temperatures",
                ));
            }
            if matches[0].1.evidence == Evidence::Estimate && !matches[0].1.allow_estimates {
                return Err(invalid(
                    "estimated zero-storage bridge laws require independent explicit permission",
                ));
            }
            used_regions.borrow_mut().insert(region);
            Ok(temporal_ids[matches[0].0].clone())
        };
        let excess = self.reference.evaluate_excess(Context {
            source: &authority.source,
            cloth,
            projection,
            domains,
            boundaries,
            instances: &authority.instances,
            owners: &authority.owners,
            model_sha256: &authority.model_sha256,
            additional_qualification: Some(&qualify),
        })?;
        let geometry_operations = excess
            .geometry_operations
            .checked_add(work.get())
            .ok_or_else(|| budget("moving bridge work count overflow"))?;
        let energy_j = reference::product(excess.power_w, seconds)?;
        let region_domains: Vec<_> = domains
            .iter()
            .zip(&projection.layout.regions)
            .enumerate()
            .filter(|(i, _)| used_regions.borrow().contains(i))
            .map(|(_, (domain, region))| {
                json!({"region":region,"layer_area_ratios":domain.layer_area_ratios,
                "temperature_range_c":domain.temperature_range_c})
            })
            .collect();
        let report = json!({
            "schema":"ftx-motion-thermal-bridge-sample/1", "model":MODEL,
            "request_sha256":identity(self)?, "surface_model_sha256":authority.model_sha256,
            "projection_input_sha256":projection.input_sha256, "time_range_s":time_range_s,
            "sample_interval_s":seconds, "region_domains":region_domains,
            "bridge_power_w":excess.power_w, "bridge_energy_j":energy_j,
            "inside_excess_energy_j":energy_j, "outside_excess_energy_j":energy_j,
            "bridge_stored_change_j":0., "excess_balance_residual_j":0.,
            "body_bridge_power_w":excess.body_power_w, "paths":excess.paths,
            "geometry_operations":geometry_operations,
            "ownership_roundoff_length_bound_m":excess.ownership_roundoff_length_bound_m,
            "quadrature":"held_right_endpoint_geometry_exact_constant_reservoir_exchange",
            "temperature_policy":"all held-frame initial, solved and source-preloaded covered cells plus prescribed reservoirs",
            "storage_modeled":false, "empirical_validation_claim":false
        });
        Ok(Sample {
            power_w: excess.power_w,
            body_power_w: excess.body_power_w,
            energy_j,
            geometry_operations,
            report,
        })
    }
}

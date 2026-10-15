//! File-free garment-to-body coverage workbench, shared by native and browser entry points.
use std::collections::{BTreeMap, BTreeSet};

use super::{Boundary, Refusal, SteadyCoverage, SteadySurface, Vec3, digest, invalid};
use crate::thermal::{
    steady::request::Film,
    surface::projection::{self, Settings},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Body geometry and physical layer identities, not a precomputed or trusted overlap receipt.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Map {
    /// Prescribed body boundary vertices in the SAME metre frame as the retained drape.
    pub body_positions_m: Vec<Vec3>,
    /// Outward-wound body boundary triangles; no global watertightness is inferred.
    pub body_triangles: Vec<[usize; 3]>,
    /// Every retained drape instance exactly once. Several sewn pieces can name the same layer.
    pub instance_layers: BTreeMap<String, String>,
    /// Explicit geometric closure, directed interfaces and optional stagnant-gas assumptions.
    pub settings: Settings,
}

/// Reservoir/film conditions on body triangles, never on material-column or viewer-mesh indices.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Conditions {
    /// One uniform boundary, or one per body triangle with an exact body mesh identity.
    pub boundaries: Vec<Film>,
    /// Required for nonuniform boundaries; optional for uniform conditions and populated on replay.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_mesh_sha256: Option<String>,
}

/// Portable material-following coverage request. Every evaluation recomputes geometric overlap.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Must be `ftx-steady-coverage-request/1`.
    pub schema: String,
    /// Saved drape containing `material` and `steady_thermal_surface` source/geometry receipts.
    pub report: Value,
    /// Physical layer identities and body geometry. No projection from an earlier pose is trusted.
    pub map: Map,
    /// Explicit inside/outside reservoirs and films on the prescribed body boundary.
    pub conditions: Conditions,
}

impl Request {
    /// Recompute disjoint body/cloth polygons, then solve exact series paths with one shared catalog
    /// authority. Material or pose changes propagate to transport, overlap and declared gas gaps.
    /// Frozen replay retains only necessary source/geometry inputs, not unrelated wearer metadata.
    ///
    /// # Errors
    /// Unsupported or oversized input, changed source receipts, stale nonuniform boundaries,
    /// incomplete layer assignments, ambiguous geometry, unavailable transport or solver refusal.
    pub fn evaluate(&self) -> Result<Value, Refusal> {
        if self.schema != "ftx-steady-coverage-request/1"
            || self.conditions.boundaries.is_empty()
            || self.conditions.boundaries.len() > 65_536
            || self.map.instance_layers.len() > 65_536
            || self.map.body_positions_m.is_empty()
            || self.map.body_positions_m.len() > 196_608
            || self.map.body_triangles.is_empty()
            || self.map.body_triangles.len() > 65_536
        {
            return Err(invalid(
                "invalid steady coverage request schema or input budget",
            ));
        }
        let frame = &self.report["steady_thermal_surface"];
        let report = json!({"material":self.report["material"],"steady_thermal_surface":frame});
        let mut frozen = Self {
            report,
            schema: self.schema.clone(),
            map: self.map.clone(),
            conditions: self.conditions.clone(),
        };
        bounded(&frozen)?;
        let (source, cloth) = SteadySurface::from_drape_report(&frozen.report)?;
        let instances: Vec<String> = serde_json::from_value(frame["instances"].clone())
            .map_err(|e| invalid(e.to_string()))?;
        let owners: Vec<usize> = serde_json::from_value(frame["triangle_instance"].clone())
            .map_err(|e| invalid(e.to_string()))?;
        let mut names = BTreeSet::new();
        for name in &instances {
            if name.trim().is_empty()
                || !names.insert(name)
                || self
                    .map
                    .instance_layers
                    .get(name)
                    .is_none_or(|s| s.trim().is_empty())
            {
                return Err(invalid(
                    "steady coverage needs one physical layer per unique drape instance",
                ));
            }
        }
        if names.len() != self.map.instance_layers.len() {
            return Err(invalid("steady coverage names an absent drape instance"));
        }
        let cloth_layers = owners
            .iter()
            .map(|&owner| {
                let name = instances
                    .get(owner)
                    .ok_or_else(|| invalid("invalid retained triangle owner"))?;
                Ok(self.map.instance_layers[name].clone())
            })
            .collect::<Result<Vec<_>, Refusal>>()?;
        let body_sha = digest(&json!({"positions_m":self.map.body_positions_m,
            "triangles":self.map.body_triangles}))?;
        if self
            .conditions
            .body_mesh_sha256
            .as_ref()
            .is_some_and(|s| *s != body_sha)
            || (self.conditions.boundaries.len() != 1 && self.conditions.body_mesh_sha256.is_none())
        {
            return Err(invalid(
                "nonuniform coverage boundaries need the exact body_mesh_sha256",
            ));
        }
        let boundary = |f: &Film| Boundary {
            inside_c: f.inside_c,
            outside_c: f.outside_c,
            inside_resistance: f.inside_resistance_m2_k_w,
            outside_resistance: f.outside_resistance_m2_k_w,
        };
        let boundaries = if self.conditions.boundaries.len() == 1 {
            vec![boundary(&self.conditions.boundaries[0]); self.map.body_triangles.len()]
        } else if self.conditions.boundaries.len() == self.map.body_triangles.len() {
            self.conditions.boundaries.iter().map(boundary).collect()
        } else {
            return Err(invalid("coverage requires one boundary per body triangle"));
        };
        let projection = source.project_coverage(
            &cloth,
            &self.map.body_positions_m,
            &self.map.body_triangles,
            &projection::Request {
                schema: "ftx-thermal-projection/1".into(),
                cloth_layers,
                settings: self.map.settings.clone(),
            },
        )?;
        let assembly = SteadyCoverage::new(source, projection.layout.clone())?;
        let solution = assembly.solve(&cloth, &self.map.body_positions_m, &boundaries)?;
        frozen.conditions.body_mesh_sha256 = Some(body_sha.clone());
        let request = serde_json::to_value(frozen).map_err(|e| invalid(e.to_string()))?;
        let result = json!({"schema":"ftx-steady-coverage-result/1",
            "request_sha256":digest(&request)?,"request":request,
            "drape_steady_frame_sha256":frame["sha256"],"body_mesh_sha256":body_sha,
            "assembly":assembly.snapshot()?,"projection":projection,"steady":solution,
            "storage_required":false,"uncertainty_propagated":false,
            "comfort_claim":false,"empirical_validation_claim":false,
            "scope":"local-normal projected coverage with prescribed reservoirs; not whole-wearer heat loss",
            "not_simulated":["transient heat storage","compression-dependent loft","moisture transport",
                "evaporation","wind-dependent films","radiative exchange","thermal bridges","human thermoregulation"]});
        bounded(&result)?;
        Ok(result)
    }
}

fn bounded(value: &impl Serialize) -> Result<(), Refusal> {
    // Include the command's pretty encoding and final newline so successful output is replayable.
    if serde_json::to_vec_pretty(value)
        .map_err(|e| invalid(e.to_string()))?
        .len()
        >= 64 * 1024 * 1024
    {
        return Err(invalid(
            "steady coverage exceeds 64 MiB replay budget; reduce the supplied mesh",
        ));
    }
    Ok(())
}

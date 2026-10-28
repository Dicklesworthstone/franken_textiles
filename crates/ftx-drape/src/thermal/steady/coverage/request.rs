//! File-free garment-to-body coverage workbench, shared by native and browser entry points.
use std::collections::{BTreeMap, BTreeSet};

use super::{Boundary, Refusal, SteadyCoverage, SteadySurface, Vec3, digest, invalid};
use crate::thermal::{
    steady::{exterior::Exterior, request::Film},
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
    /// Optional exterior environment per body triangle, or one uniform environment. Only each
    /// coverage path's outermost selected material is exposed to its ambient radiation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exterior_exchange: Option<Vec<Exterior>>,
    /// Separate bare-body film conditions in exterior mode. Required for any uncovered fraction;
    /// one uniform entry or one per body triangle. Never inherit textile emissivity for bare skin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bare_boundaries: Option<Vec<Film>>,
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
    /// Optional observed centerline excess over this exact source-qualified unbridged network.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thermal_bridges: Option<super::bridges::Request>,
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
        if self.conditions.bare_boundaries.is_some() && self.conditions.exterior_exchange.is_none()
        {
            return Err(invalid(
                "separate bare boundaries require explicit exterior exchange mode",
            ));
        }
        if let Some(bridges) = &self.thermal_bridges {
            bridges.validate()?;
            if self.conditions.exterior_exchange.is_some() {
                return Err(invalid(
                    "qualified centerline excess requires the characterized reservoir films; exterior convection/radiation is not supported by this bridge model",
                ));
            }
        }
        let frame = &self.report["steady_thermal_surface"];
        let report = json!({"material":self.report["material"],"steady_thermal_surface":frame});
        let mut frozen = Self {
            report,
            schema: self.schema.clone(),
            map: self.map.clone(),
            conditions: self.conditions.clone(),
            thermal_bridges: self.thermal_bridges.clone(),
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
            || ((self.conditions.boundaries.len() != 1
                || self
                    .conditions
                    .exterior_exchange
                    .as_ref()
                    .is_some_and(|v| v.len() != 1)
                || self
                    .conditions
                    .bare_boundaries
                    .as_ref()
                    .is_some_and(|v| v.len() != 1))
                && self.conditions.body_mesh_sha256.is_none())
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
        let (mut solution, exterior) = if let Some(e) = &self.conditions.exterior_exchange {
            let e = broadcast(e, boundaries.len())?;
            let bare = self
                .conditions
                .bare_boundaries
                .as_ref()
                .map(|b| broadcast(b, boundaries.len()))
                .transpose()?
                .unwrap_or_default();
            let bare: Vec<_> = bare.iter().map(boundary).collect();
            let result = assembly.solve_exterior(
                &cloth,
                &self.map.body_positions_m,
                &boundaries,
                &e,
                &bare,
            )?;
            let details = json!({"regions":result.regions,
                "convective_power_w":result.convective_power_w,
                "radiative_power_w":result.radiative_power_w,
                "balance_residual_w":result.balance_residual_w,
                "bare_policy":"separate explicitly prescribed films; no inferred skin emissivity"});
            (result.steady, Some(details))
        } else {
            (
                assembly.solve(&cloth, &self.map.body_positions_m, &boundaries)?,
                None,
            )
        };
        let bridges = self
            .thermal_bridges
            .as_ref()
            .map(|policy| {
                let domains: Vec<_> = solution
                    .regions
                    .iter()
                    .map(|region| {
                        let mut temperatures = [f64::INFINITY, f64::NEG_INFINITY];
                        for &t in region.conduction.layer_faces_c.iter().flatten() {
                            temperatures[0] = temperatures[0].min(t);
                            temperatures[1] = temperatures[1].max(t);
                        }
                        super::bridges::Domain {
                            layer_area_ratios: region
                                .layer_area_m2
                                .iter()
                                .map(|a| a / region.body_area_m2)
                                .collect(),
                            temperature_range_c: temperatures,
                        }
                    })
                    .collect();
                policy.evaluate(
                    super::bridges::Context {
                        source: &assembly.source,
                        cloth: &cloth,
                        projection: &projection,
                        domains: &domains,
                        boundaries: &boundaries,
                        instances: &instances,
                        owners: &owners,
                        model_sha256: frame["model"]["sha256"].as_str().ok_or_else(|| {
                            invalid("thermal bridge needs the retained source model identity")
                        })?,
                        additional_qualification: None,
                    },
                    &solution,
                )
            })
            .transpose()?;
        if let Some(bridges) = &bridges {
            solution.baseline_power_w = Some(solution.power_w);
            solution.bridge_power_w = bridges["bridge_power_w"].as_f64();
            solution.power_w = bridges["total_power_w"]
                .as_f64()
                .expect("computed finite power");
            for (field, extra) in solution
                .body
                .iter_mut()
                .zip(bridges["body"].as_array().expect("computed fields"))
            {
                field.baseline_power_w = Some(field.power_w);
                field.bridge_power_w = extra["bridge_power_w"].as_f64();
                field.power_w = extra["total_power_w"]
                    .as_f64()
                    .expect("computed finite field");
            }
        }
        frozen.conditions.body_mesh_sha256 = Some(body_sha.clone());
        let request = serde_json::to_value(frozen).map_err(|e| invalid(e.to_string()))?;
        let mut result = json!({"schema":"ftx-steady-coverage-result/1",
            "request_sha256":digest(&request)?,"request":request,
            "drape_steady_frame_sha256":frame["sha256"],"body_mesh_sha256":body_sha,
            "assembly":assembly.snapshot()?,"projection":projection,"steady":solution,
            "storage_required":false,"uncertainty_propagated":false,
            "comfort_claim":false,"empirical_validation_claim":false,
            "scope":"local-normal projected coverage with prescribed reservoirs; not whole-wearer heat loss",
            "not_simulated":["transient heat storage","compression-dependent loft","moisture transport",
                "evaporation","wind-dependent films","radiative exchange","thermal bridges","human thermoregulation"]});
        if let Some(bridges) = bridges {
            result["thermal_bridges"] = bridges;
            result["not_simulated"]
                .as_array_mut()
                .expect("authored exclusions")
                .retain(|value| value != "thermal bridges");
            result["not_simulated"]
                .as_array_mut()
                .expect("authored exclusions")
                .extend([
                    json!("unqualified seam/closure microgeometry or replacement-area paths"),
                    json!("thermal bridge storage, transient response or exterior radiation"),
                ]);
        }
        if let Some(details) = exterior {
            let enabled = self
                .conditions
                .exterior_exchange
                .as_ref()
                .is_some_and(|v| v.iter().any(|e| e.radiation.is_some()));
            crate::thermal::steady::request::describe_exterior(&mut result, details, enabled);
        }
        bounded(&result)?;
        Ok(result)
    }
}

fn broadcast<T: Clone>(values: &[T], count: usize) -> Result<Vec<T>, Refusal> {
    if values.len() == 1 {
        Ok(vec![values[0].clone(); count])
    } else if values.len() == count {
        Ok(values.to_vec())
    } else {
        Err(invalid(
            "coverage needs one uniform value or one per body triangle",
        ))
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

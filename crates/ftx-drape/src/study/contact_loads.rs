//! Source-bound body barrier loads on a selected union of welded cloth vertices.
//! These are discrete forces, never contact pressure or a physiological comfort model.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, Vec3};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{budget, hash, invalid};
use crate::{
    contact::{Body, Contact},
    run::DrapeRun,
    thermal::surface::io::retained_materials,
};

const MAX_WORK: usize = 64_000_000;

/// Optional body-load scope. No numerical force threshold or contact area is invented.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Must be `ftx-contact-loads/1`.
    pub schema: String,
    /// Exact retained instance IDs; omission selects the whole retained garment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instances: Option<Vec<String>>,
    /// Full cloth plus body vertex evaluations across the initial and all accepted samples.
    pub max_vertex_samples: usize,
    /// Contact candidate derivative evaluations, including inactive candidates and self-contact.
    /// Broad-phase work remains the production contact algorithm.
    pub max_pair_evaluations: usize,
}

impl Request {
    fn validate(&self) -> Result<(), Refusal> {
        if self.schema != "ftx-contact-loads/1"
            || !(1..=MAX_WORK).contains(&self.max_vertex_samples)
            || !(1..=MAX_WORK).contains(&self.max_pair_evaluations)
            || self.instances.as_ref().is_some_and(|ids| {
                ids.is_empty()
                    || ids.len() > 4096
                    || ids.iter().any(|id| id.trim().is_empty() || id.len() > 4096)
                    || ids.iter().collect::<BTreeSet<_>>().len() != ids.len()
            })
        {
            return Err(invalid(
                "contact loads need their schema, positive bounded work allowances and unique exact instance IDs",
            ));
        }
        Ok(())
    }
}

#[derive(Default)]
pub(super) struct Budget {
    vertices: usize,
    pairs: usize,
}

impl Budget {
    pub(super) fn reserve(&mut self, request: &Request) -> Result<(), Refusal> {
        request.validate()?;
        self.vertices = self.vertices.saturating_add(request.max_vertex_samples);
        self.pairs = self.pairs.saturating_add(request.max_pair_evaluations);
        if self.vertices > MAX_WORK || self.pairs > MAX_WORK {
            return Err(budget(
                "aggregate contact-load vertex/pair allowance exhausted",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize)]
struct Reading {
    resultant_n: Vec3,
    resultant_magnitude_n: f64,
    max_vertex_force_n: f64,
    max_force_vertex: usize,
}

fn reading(vertices: &[usize], forces: &[Vec3]) -> Result<Reading, Refusal> {
    if vertices.is_empty() || vertices.len() != forces.len() {
        return Err(invalid(
            "contact loads require a nonempty complete selected force field",
        ));
    }
    let mut result = Reading {
        resultant_n: Vec3::ZERO,
        resultant_magnitude_n: 0.,
        max_vertex_force_n: 0.,
        max_force_vertex: vertices[0],
    };
    for (&vertex, &force) in vertices.iter().zip(forces) {
        let magnitude = force.norm();
        if !force.is_finite() || !magnitude.is_finite() {
            return Err(invalid("contact force magnitude is not representable"));
        }
        result.resultant_n += force;
        if magnitude > result.max_vertex_force_n {
            result.max_vertex_force_n = magnitude;
            result.max_force_vertex = vertex;
        }
    }
    result.resultant_magnitude_n = result.resultant_n.norm();
    if !result.resultant_magnitude_n.is_finite() {
        return Err(invalid("contact force resultant is not representable"));
    }
    Ok(result)
}

#[derive(Clone, Debug, Serialize)]
struct Frame {
    sample_index: usize,
    time_s: f64,
    positions_sha256: String,
    body_state_sha256: String,
    active_policy_sha256: Value,
    candidate_pairs: usize,
    body_force_on_selected_vertices_n: Vec<Vec3>,
    body: Reading,
}

pub(super) struct History {
    request: Request,
    source: Value,
    source_sha256: String,
    contact: Contact,
    selected: Vec<usize>,
    vertex_count: usize,
    frames: Vec<Frame>,
    pairs: usize,
}

impl History {
    pub(super) fn new(request: &Request, run: &DrapeRun) -> Result<Self, Refusal> {
        request.validate()?;
        let scene = &run.scene;
        let contact = scene
            .solver
            .contact
            .as_ref()
            .filter(|c| !c.body.triangles.is_empty())
            .ok_or_else(|| invalid("body contact loads require the production collision body"))?;
        if contact.params.thickness < scene.target_thickness {
            return Err(invalid(
                "complete the physical material-thickness ramp before evaluating body loads",
            ));
        }
        let (records, assignment) = retained_materials(
            &run.report["material"],
            &scene.mesh.instances,
            &scene.triangle_owner,
        )?;
        if assignment.len() != contact.cloth.triangles.len()
            || scene.triangle_owner.len() != assignment.len()
            || assignment.len() > 65_536
        {
            return Err(invalid(
                "contact loads require complete retained triangle ownership",
            ));
        }
        let requested = request
            .instances
            .as_ref()
            .map(|ids| ids.iter().collect::<BTreeSet<_>>());
        let mut instances = BTreeMap::new();
        let mut selected = BTreeSet::new();
        for (i, triangle) in contact.cloth.triangles.iter().enumerate() {
            let instance = &scene.mesh.instances[scene.triangle_owner[i]];
            if requested
                .as_ref()
                .is_some_and(|ids| !ids.contains(instance))
            {
                continue;
            }
            instances.insert(
                instance.clone(),
                records[assignment[i]].sha256().to_string(),
            );
            selected.extend(triangle.iter().copied());
        }
        if selected.is_empty()
            || instances.len() > 4096
            || requested.is_some_and(|ids| ids.len() != instances.len())
        {
            return Err(invalid(
                "contact load scope names an unknown, omitted or collapsed instance",
            ));
        }
        let selected: Vec<usize> = selected.into_iter().collect();
        let source = json!({"vertex_count":scene.solver.x.len(),
            "body_vertex_count":contact.body.vertices.len(),"body_triangles":contact.body.triangles,
            "all_instances":scene.mesh.instances,"triangle_instance":scene.triangle_owner,
            "instances":instances,"selected_vertices":selected,
            "material_report_sha256":hash(&run.report["material"])?});
        let mut history = Self {
            request: request.clone(),
            source_sha256: hash(&source)?,
            source,
            contact: contact.clone(),
            selected,
            vertex_count: scene.solver.x.len(),
            frames: vec![],
            pairs: 0,
        };
        history.advance(&scene.solver.x, &contact.body.vertices, 0.)?;
        Ok(history)
    }

    pub(super) fn advance(
        &mut self,
        positions: &[Vec3],
        body: &[Vec3],
        time_s: f64,
    ) -> Result<(), Refusal> {
        let vertices_per_frame = self.vertex_count + self.contact.body.vertices.len();
        if vertices_per_frame.saturating_mul(self.frames.len() + 1)
            > self.request.max_vertex_samples
        {
            return Err(budget(
                "contact-load vertex-sample budget exhausted; no partial history published",
            ));
        }
        if positions.len() != self.vertex_count
            || body.len() != self.contact.body.vertices.len()
            || !time_s.is_finite()
            || time_s < 0.
            || self
                .frames
                .last()
                .is_some_and(|frame| time_s <= frame.time_s)
            || (self.frames.is_empty() && time_s != 0.)
        {
            return Err(invalid(
                "contact loads need matching geometry and strictly ordered accepted samples",
            ));
        }
        // Retain source-bound surface fields, seam envelopes and self-contact creases. Only the
        // kinematic collider moves; rebuilding Contact from global defaults would lose materials.
        let mut contact = self.contact.clone();
        if contact.body.vertices != body {
            contact.body = Body::new(
                body.to_vec(),
                contact.body.triangles.clone(),
                contact.params.dhat,
            )?;
        }
        let (map, pairs) = contact.reaction_map_bounded(
            positions,
            self.request.max_pair_evaluations.saturating_sub(self.pairs),
        )?;
        let all: Vec<Vec3> = serde_json::from_value(map["body_force_on_cloth_n"].clone())
            .map_err(|_| invalid("production body force field unavailable"))?;
        let forces: Vec<_> = self.selected.iter().map(|&i| all[i]).collect();
        let frame = Frame {
            sample_index: self.frames.len(),
            time_s,
            positions_sha256: hash(&positions)?,
            body_state_sha256: hash(&json!([body, contact.body.triangles]))?,
            active_policy_sha256: map["active_policy_sha256"].clone(),
            candidate_pairs: pairs,
            body: reading(&self.selected, &forces)?,
            body_force_on_selected_vertices_n: forces,
        };
        self.frames.push(frame);
        self.pairs += pairs;
        Ok(())
    }

    pub(super) fn report(&self, motion: bool) -> Value {
        let mut peak_resultant = 0;
        let mut peak_vertex = 0;
        for i in 1..self.frames.len() {
            if self.frames[i].body.resultant_magnitude_n
                > self.frames[peak_resultant].body.resultant_magnitude_n
            {
                peak_resultant = i;
            }
            if self.frames[i].body.max_vertex_force_n
                > self.frames[peak_vertex].body.max_vertex_force_n
            {
                peak_vertex = i;
            }
        }
        let first = &self.frames[0];
        let last = self.frames.last().expect("initial sample admitted");
        json!({"schema":"ftx-contact-loads-report/1","request":self.request,
            "source":self.source,"source_sha256":self.source_sha256,
            "scope":if motion {"accepted_motion_samples"} else {"initial_state"},"unit":"N",
            "initial_positions_sha256":first.positions_sha256,"final_positions_sha256":last.positions_sha256,
            "duration_s":last.time_s,"frames":self.frames,
            "peak_body_resultant_n":self.frames[peak_resultant].body.resultant_magnitude_n,
            "peak_body_resultant_sample":peak_resultant,
            "max_body_vertex_force_n":self.frames[peak_vertex].body.max_vertex_force_n,
            "max_body_vertex_force_sample":peak_vertex,
            "vertex_samples":self.frames.len()*(self.vertex_count+self.contact.body.vertices.len()),
            "pair_evaluations":self.pairs,
            "physical_validation_claim":false,"pressure_claim":false,"comfort_claim":false,
            "continuous_time_bound":false,
            "note":"Negative gradient of production body barrier energy, including mollifiers; excludes self-contact and Coulomb tangential forces. Selected instance vertices form a union, with welded seam vertices counted once and their entire body force retained. Resultants can cancel opposing forces; nodal peaks depend on the mesh and barrier. Sampled forces are not pressure, safe-fit limits, continuous-time bounds or measured comfort."})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn force_resultants_preserve_cancellation_and_lowest_vertex_ties() {
        let result = reading(
            &[2, 4, 8],
            &[Vec3::new(3., 4., 0.), Vec3::new(-3., -4., 0.), Vec3::ZERO],
        )
        .unwrap();
        assert_eq!(result.resultant_magnitude_n, 0.);
        assert_eq!(result.max_vertex_force_n, 5.);
        assert_eq!(result.max_force_vertex, 2);
        assert!(reading(&[0], &[Vec3::new(f64::MAX, f64::MAX, 0.)]).is_err());
    }

    #[test]
    fn malformed_scope_and_aggregate_work_refuse() {
        let mut request = Request {
            schema: "ftx-contact-loads/1".into(),
            instances: None,
            max_vertex_samples: MAX_WORK,
            max_pair_evaluations: 1,
        };
        let mut budget = Budget::default();
        budget.reserve(&request).unwrap();
        assert!(budget.reserve(&request).is_err());
        request.instances = Some(vec!["front".into(), "front".into()]);
        assert!(request.validate().is_err());
    }
}

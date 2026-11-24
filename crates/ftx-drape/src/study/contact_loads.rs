//! Source-bound body barrier loads on a selected union of welded cloth vertices.
//! These are discrete forces, never contact pressure or a physiological comfort model.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, Vec2, Vec3};
use ftx_pattern::material_compression::regions::{
    MAX_REGION_EDGE_TESTS, PreparedSelector, Selector,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{budget, hash, invalid, retained_flat_triangles};
use crate::{
    contact::{Body, Contact},
    run::DrapeRun,
    thermal::surface::io::retained_materials,
};

const MAX_WORK: usize = 64_000_000;

/// Optional body-load scope. No numerical force threshold or contact area is invented.
#[derive(Clone, Debug, Serialize)]
pub struct Request {
    /// `ftx-contact-loads/1` for whole instances, or /2 for fixed flat-material regions.
    pub schema: String,
    /// Version 1 only: exact retained instance IDs; omission selects the whole retained garment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instances: Option<Vec<String>>,
    /// Version 2 only: nonempty disjoint whole-triangle regions within intact pattern instances.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub regions: Option<Vec<Region>>,
    /// Full cloth plus body vertex evaluations across the initial and all accepted samples.
    pub max_vertex_samples: usize,
    /// Contact candidate derivative evaluations, including inactive candidates and self-contact.
    /// Broad-phase work remains the production contact algorithm.
    pub max_pair_evaluations: usize,
}

/// A named body-load scope in one physical instance's mirrored and scaled flat millimetres.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Region {
    /// Unique region name, distinct from the physical instance or material identity.
    pub id: String,
    /// Exact retained physical instance, including its mirror/copy identity.
    pub instance: String,
    /// Convex polygon selecting complete retained triangles by their exact flat centroids.
    pub selector: Selector,
}

impl<'de> Deserialize<'de> for Request {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        fn instances<'de, D: serde::Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Option<Option<Vec<String>>>, D::Error> {
            Option::<Vec<String>>::deserialize(deserializer).map(Some)
        }
        fn regions<'de, D: serde::Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Option<Vec<Region>>, D::Error> {
            Vec::<Region>::deserialize(deserializer).map(Some)
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            schema: String,
            #[serde(default, deserialize_with = "instances")]
            instances: Option<Option<Vec<String>>>,
            #[serde(default, deserialize_with = "regions")]
            regions: Option<Vec<Region>>,
            max_vertex_samples: usize,
            max_pair_evaluations: usize,
        }
        let fields = Fields::deserialize(deserializer)?;
        if (fields.schema == "ftx-contact-loads/2"
            && (fields.instances.is_some() || fields.regions.is_none()))
            || (fields.schema != "ftx-contact-loads/2" && fields.regions.is_some())
        {
            return Err(serde::de::Error::custom(
                "contact loads /2 require regions without instances; /1 accepts only instance scope",
            ));
        }
        Ok(Self {
            schema: fields.schema,
            instances: fields.instances.flatten(),
            regions: fields.regions,
            max_vertex_samples: fields.max_vertex_samples,
            max_pair_evaluations: fields.max_pair_evaluations,
        })
    }
}

impl Request {
    fn validate(&self) -> Result<(), Refusal> {
        let regional = self.schema == "ftx-contact-loads/2";
        if (!regional && self.schema != "ftx-contact-loads/1")
            || (regional && (self.instances.is_some() || self.regions.is_none()))
            || (!regional && self.regions.is_some())
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
                "contact loads need schema-qualified scope, positive bounded work allowances and unique exact instance IDs",
            ));
        }
        if let Some(regions) = &self.regions {
            let mut names = BTreeSet::new();
            if !(1..=256).contains(&regions.len()) {
                return Err(invalid("contact loads need 1..256 declared regions"));
            }
            for region in regions {
                if region.id.trim().is_empty()
                    || region.id.len() > 512
                    || !names.insert(&region.id)
                    || region.instance.trim().is_empty()
                    || region.instance.len() > 4096
                {
                    return Err(invalid(
                        "contact-load regions need unique bounded names and exact physical instances",
                    ));
                }
                PreparedSelector::new(&region.selector)?;
            }
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
struct RegionSource {
    instance: String,
    selector: Selector,
    source_triangles: Vec<usize>,
    selected_vertices: Vec<usize>,
    reference_area_m2: f64,
}

struct Partition {
    selected_triangles: Vec<bool>,
    regions: BTreeMap<String, RegionSource>,
}

fn partition(
    regions: &[Region],
    instances: &[String],
    owners: &[usize],
    faces: &[[usize; 3]],
    triangles: &[[Vec2; 3]],
) -> Result<Partition, Refusal> {
    if triangles.is_empty() || triangles.len() != owners.len() || triangles.len() != faces.len() {
        return Err(invalid(
            "regional contact loads need complete retained flat triangles and ownership",
        ));
    }
    let mut by_instance = BTreeMap::<&str, Vec<usize>>::new();
    for (triangle, &owner) in owners.iter().enumerate() {
        let instance = instances
            .get(owner)
            .ok_or_else(|| invalid("regional contact-load source owner is unavailable"))?;
        by_instance.entry(instance).or_default().push(triangle);
    }
    let mut edge_tests = 0usize;
    let mut selectors = Vec::with_capacity(regions.len());
    for region in regions {
        let owned = by_instance.get(region.instance.as_str()).ok_or_else(|| {
            invalid("regional contact-load scope names an unknown or unretained instance")
        })?;
        edge_tests = owned
            .len()
            .checked_mul(region.selector.polygon_mm.len())
            .and_then(|work| edge_tests.checked_add(work))
            .filter(|&work| work <= MAX_REGION_EDGE_TESTS)
            .ok_or_else(|| budget("regional contact-load selector edge-test budget exhausted"))?;
        selectors.push((region, PreparedSelector::new(&region.selector)?, owned));
    }
    let mut result = Partition {
        selected_triangles: vec![false; triangles.len()],
        regions: BTreeMap::new(),
    };
    for (region, selector, owned) in selectors {
        let mut selected = Vec::new();
        let mut vertices = BTreeSet::new();
        let mut area = 0.;
        for &index in owned {
            if selector.contains_triangle_centroid(triangles[index])? {
                if result.selected_triangles[index] {
                    return Err(invalid(
                        "contact-load regions select the same retained triangle",
                    ));
                }
                result.selected_triangles[index] = true;
                selected.push(index);
                vertices.extend(faces[index]);
                let [a, b, c] = triangles[index];
                area += 0.5 * (b - a).cross(c - a).abs() * 1e-6;
            }
        }
        if selected.is_empty() || !area.is_finite() || area <= 0. {
            return Err(invalid(
                "contact-load region needs nonempty selection with finite positive reference area",
            ));
        }
        result.regions.insert(
            region.id.clone(),
            RegionSource {
                instance: region.instance.clone(),
                selector: region.selector.clone(),
                source_triangles: selected,
                selected_vertices: vertices.into_iter().collect(),
                reference_area_m2: area,
            },
        );
    }
    Ok(result)
}

#[derive(Clone, Debug, Serialize)]
struct Reading {
    resultant_n: Vec3,
    resultant_magnitude_n: f64,
    sum_vertex_force_n: f64,
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
        sum_vertex_force_n: 0.,
        max_vertex_force_n: 0.,
        max_force_vertex: vertices[0],
    };
    for (&vertex, &force) in vertices.iter().zip(forces) {
        let magnitude = force.norm();
        if !force.is_finite() || !magnitude.is_finite() {
            return Err(invalid("contact force magnitude is not representable"));
        }
        result.resultant_n += force;
        // Sum the already assembled nodal magnitudes in source-vertex order. Opposing
        // body loads remain visible without inventing a contact area or dividing by
        // vertex count. Forces meeting at the same welded vertex are still assembled once.
        result.sum_vertex_force_n += magnitude;
        if magnitude > result.max_vertex_force_n {
            result.max_vertex_force_n = magnitude;
            result.max_force_vertex = vertex;
        }
    }
    result.resultant_magnitude_n = result.resultant_n.norm();
    if !result.resultant_magnitude_n.is_finite() || !result.sum_vertex_force_n.is_finite() {
        return Err(invalid("contact force reduction is not representable"));
    }
    Ok(result)
}

#[derive(Clone, Debug, Serialize)]
struct RegionSummary {
    peak_body_resultant_n: f64,
    peak_body_resultant_sample: usize,
    peak_body_sum_vertex_force_n: f64,
    peak_body_sum_vertex_force_sample: usize,
    max_body_vertex_force_n: f64,
    max_body_vertex_force_sample: usize,
    max_force_vertex: usize,
}

impl RegionSummary {
    fn new(reading: &Reading, sample: usize) -> Self {
        Self {
            peak_body_resultant_n: reading.resultant_magnitude_n,
            peak_body_resultant_sample: sample,
            peak_body_sum_vertex_force_n: reading.sum_vertex_force_n,
            peak_body_sum_vertex_force_sample: sample,
            max_body_vertex_force_n: reading.max_vertex_force_n,
            max_body_vertex_force_sample: sample,
            max_force_vertex: reading.max_force_vertex,
        }
    }

    fn observe(&mut self, reading: &Reading, sample: usize) {
        // Strict comparisons retain the earliest accepted sample. `reading` already breaks
        // equal nodal maxima by the lowest welded vertex in this region's sorted union.
        if reading.resultant_magnitude_n > self.peak_body_resultant_n {
            self.peak_body_resultant_n = reading.resultant_magnitude_n;
            self.peak_body_resultant_sample = sample;
        }
        if reading.sum_vertex_force_n > self.peak_body_sum_vertex_force_n {
            self.peak_body_sum_vertex_force_n = reading.sum_vertex_force_n;
            self.peak_body_sum_vertex_force_sample = sample;
        }
        if reading.max_vertex_force_n > self.max_body_vertex_force_n {
            self.max_body_vertex_force_n = reading.max_vertex_force_n;
            self.max_body_vertex_force_sample = sample;
            self.max_force_vertex = reading.max_force_vertex;
        }
    }
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
    region_vertices: BTreeMap<String, Vec<usize>>,
    region_summaries: BTreeMap<String, RegionSummary>,
    rest_triangles_mm: Option<Vec<[Vec2; 3]>>,
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
        let rest_triangles_mm = request
            .regions
            .as_ref()
            .map(|_| retained_flat_triangles(run))
            .transpose()?;
        let partition = request
            .regions
            .as_ref()
            .map(|regions| {
                if scene.solver.model.membranes.len() != contact.cloth.triangles.len()
                    || scene
                        .solver
                        .model
                        .membranes
                        .iter()
                        .zip(&contact.cloth.triangles)
                        .any(|(element, face)| element.v != *face)
                {
                    return Err(invalid(
                        "regional contact-load source differs from retained contact topology",
                    ));
                }
                partition(
                    regions,
                    &scene.mesh.instances,
                    &scene.triangle_owner,
                    &contact.cloth.triangles,
                    rest_triangles_mm.as_ref().expect("regional flat source"),
                )
            })
            .transpose()?;
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
                || partition
                    .as_ref()
                    .is_some_and(|partition| !partition.selected_triangles[i])
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
        let mut source = json!({"vertex_count":scene.solver.x.len(),
            "body_vertex_count":contact.body.vertices.len(),"body_triangles":contact.body.triangles,
            "all_instances":scene.mesh.instances,"triangle_instance":scene.triangle_owner,
            "instances":instances,"selected_vertices":selected,
            "material_report_sha256":hash(&run.report["material"])?});
        let mut region_vertices = BTreeMap::new();
        if let Some(partition) = partition {
            region_vertices = partition
                .regions
                .iter()
                .map(|(id, region)| (id.clone(), region.selected_vertices.clone()))
                .collect();
            source["regions"] = json!(partition.regions);
            source["rest_triangles_sha256"] = json!(hash(
                rest_triangles_mm.as_ref().expect("regional flat source")
            )?);
        }
        let mut history = Self {
            request: request.clone(),
            source_sha256: hash(&source)?,
            source,
            contact: contact.clone(),
            selected,
            region_vertices,
            region_summaries: BTreeMap::new(),
            rest_triangles_mm,
            vertex_count: scene.solver.x.len(),
            frames: vec![],
            pairs: 0,
        };
        history.advance(&scene.solver.x, &contact.body.vertices, 0., None)?;
        Ok(history)
    }

    pub(super) fn rest_triangles_mm(&self) -> Option<&[[Vec2; 3]]> {
        self.rest_triangles_mm.as_deref()
    }

    pub(super) fn advance(
        &mut self,
        positions: &[Vec3],
        body: &[Vec3],
        time_s: f64,
        kappas: Option<(f64, Option<f64>)>,
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
        // kinematic collider and any adapted barrier stiffness move; rebuilding Contact from global
        // defaults would lose materials.
        let mut contact = self.contact.clone();
        if let Some((body_kappa, self_kappa)) = kappas {
            if !body_kappa.is_finite()
                || body_kappa <= 0.0
                || self_kappa.is_some_and(|k| !k.is_finite() || k <= 0.0)
            {
                return Err(invalid(
                    "contact loads require finite positive adapted barrier stiffness",
                ));
            }
            contact.restore_kappas(body_kappa, self_kappa);
        }
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
        let mut region_summaries = self.region_summaries.clone();
        for (id, vertices) in &self.region_vertices {
            let region_forces: Vec<_> = vertices.iter().map(|&i| all[i]).collect();
            let reading = reading(vertices, &region_forces)?;
            region_summaries
                .entry(id.clone())
                .or_insert_with(|| RegionSummary::new(&reading, self.frames.len()))
                .observe(&reading, self.frames.len());
        }
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
        self.region_summaries = region_summaries;
        self.pairs += pairs;
        Ok(())
    }

    pub(super) fn report(&self, motion: bool) -> Value {
        let mut peak_resultant = 0;
        let mut peak_sum = 0;
        let mut peak_vertex = 0;
        for i in 1..self.frames.len() {
            if self.frames[i].body.resultant_magnitude_n
                > self.frames[peak_resultant].body.resultant_magnitude_n
            {
                peak_resultant = i;
            }
            if self.frames[i].body.sum_vertex_force_n
                > self.frames[peak_sum].body.sum_vertex_force_n
            {
                peak_sum = i;
            }
            if self.frames[i].body.max_vertex_force_n
                > self.frames[peak_vertex].body.max_vertex_force_n
            {
                peak_vertex = i;
            }
        }
        let first = &self.frames[0];
        let last = self.frames.last().expect("initial sample admitted");
        let mut report = json!({"schema":if self.rest_triangles_mm.is_some() {
                "ftx-contact-loads-report/2"
            } else {"ftx-contact-loads-report/1"},"request":self.request,
            "source":self.source,"source_sha256":self.source_sha256,
            "scope":if motion {"accepted_motion_samples"} else {"initial_state"},"unit":"N",
            "initial_positions_sha256":first.positions_sha256,"final_positions_sha256":last.positions_sha256,
            "duration_s":last.time_s,"frames":self.frames,
            "peak_body_resultant_n":self.frames[peak_resultant].body.resultant_magnitude_n,
            "peak_body_resultant_sample":peak_resultant,
            "peak_body_sum_vertex_force_n":self.frames[peak_sum].body.sum_vertex_force_n,
            "peak_body_sum_vertex_force_sample":peak_sum,
            "max_body_vertex_force_n":self.frames[peak_vertex].body.max_vertex_force_n,
            "max_body_vertex_force_sample":peak_vertex,
            "vertex_samples":self.frames.len()*(self.vertex_count+self.contact.body.vertices.len()),
            "pair_evaluations":self.pairs,
            "physical_validation_claim":false,"pressure_claim":false,"comfort_claim":false,
            "continuous_time_bound":false,
            "note":"Negative gradient of production body barrier energy, including mollifiers; excludes self-contact and Coulomb tangential forces. Selected instance vertices form a union, with welded seam vertices counted once and their entire body force retained. Resultants can cancel opposing forces; the sum of nodal magnitudes retains opposite-vertex loads without averaging by vertex count. Both nodal peaks and summed magnitudes depend on the mesh and barrier. Sampled forces are not pressure, safe-fit limits, continuous-time bounds or measured comfort."});
        if self.rest_triangles_mm.is_some() {
            report["regions"] = json!(self.region_summaries);
            report["max_force_vertex"] = json!(self.frames[peak_vertex].body.max_force_vertex);
            report["note"] = json!(
                "Negative gradient of production body barrier energy on the union of vertices of selected flat whole triangles. Regions retain the entire assembled force at shared welded vertices; regional totals are not additive, while the global union counts each vertex once. Region membership stays fixed through motion and is reevaluated on each new pattern mesh. Excludes self-contact and Coulomb tangential forces. Results depend on mesh and barrier policy, cover accepted samples only, and establish no pressure, comfort, safe-fit, continuous-time or physical validation claim."
            );
        }
        report
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
        assert_eq!(result.sum_vertex_force_n, 10.);
        assert_eq!(result.max_vertex_force_n, 5.);
        assert_eq!(result.max_force_vertex, 2);
        assert!(reading(&[0], &[Vec3::new(f64::MAX, f64::MAX, 0.)]).is_err());
    }

    #[test]
    fn summed_load_preserves_rotation_scaling_and_distributed_opposing_forces() {
        let forces = [Vec3::new(3., 4., 0.), Vec3::new(-3., -4., 0.)];
        let original = reading(&[2, 8], &forces).unwrap();
        // A signed permutation is an exact rigid rotation: no tolerances can hide a
        // component-wise absolute sum being substituted for the Euclidean magnitude.
        let rotated = forces.map(|f| Vec3::new(f.z, f.x, f.y));
        assert_eq!(reading(&[2, 8], &rotated).unwrap().sum_vertex_force_n, 10.);
        let scaled = forces.map(|f| f * 3.);
        assert_eq!(reading(&[2, 8], &scaled).unwrap().sum_vertex_force_n, 30.);

        // Spreading the same parallel nodal forces over more vertices changes the
        // maximum per-vertex reading, but neither the resultant nor the magnitude sum.
        let distributed = reading(
            &[0, 1, 2, 3],
            &[
                Vec3::new(1.5, 2., 0.),
                Vec3::new(1.5, 2., 0.),
                Vec3::new(-1.5, -2., 0.),
                Vec3::new(-1.5, -2., 0.),
            ],
        )
        .unwrap();
        assert_eq!(distributed.resultant_n, original.resultant_n);
        assert_eq!(distributed.sum_vertex_force_n, original.sum_vertex_force_n);
        assert_eq!(distributed.max_vertex_force_n, 2.5);
        assert_eq!(reading(&[4], &[Vec3::ZERO]).unwrap().sum_vertex_force_n, 0.);
    }

    #[test]
    fn malformed_scope_and_aggregate_work_refuse() {
        let mut request = Request {
            schema: "ftx-contact-loads/1".into(),
            instances: None,
            regions: None,
            max_vertex_samples: MAX_WORK,
            max_pair_evaluations: 1,
        };
        let mut budget = Budget::default();
        budget.reserve(&request).unwrap();
        assert!(budget.reserve(&request).is_err());
        request.instances = Some(vec!["front".into(), "front".into()]);
        assert!(request.validate().is_err());
    }

    fn region(id: &str, left: f64, right: f64) -> Region {
        Region {
            id: id.into(),
            instance: "panel".into(),
            selector: Selector {
                kind: "whole_triangle_centroid".into(),
                polygon_mm: vec![
                    Vec2::new(left, -1.),
                    Vec2::new(right, -1.),
                    Vec2::new(right, 4.),
                    Vec2::new(left, 4.),
                ],
            },
        }
    }

    #[test]
    fn regional_scope_preserves_shared_welded_forces_without_adding_regional_totals() {
        let triangles = [
            [[0., 0.], [3., 0.], [0., 3.]].map(Vec2::from),
            [[3., 0.], [6., 0.], [3., 3.]].map(Vec2::from),
        ];
        let faces = [[0, 1, 2], [1, 3, 2]];
        let result = partition(
            &[region("left", -1., 2.), region("right", 3., 7.)],
            &["panel".into()],
            &[0, 0],
            &faces,
            &triangles,
        )
        .unwrap();
        assert_eq!(result.selected_triangles, vec![true, true]);
        assert_eq!(result.regions["left"].source_triangles, [0]);
        assert_eq!(result.regions["right"].source_triangles, [1]);
        assert_eq!(result.regions["left"].selected_vertices, [0, 1, 2]);
        assert_eq!(result.regions["right"].selected_vertices, [1, 2, 3]);
        assert_eq!(result.regions["left"].reference_area_m2, 4.5e-6);
        let all = [
            Vec3::new(5., 0., 0.),
            Vec3::new(-5., 0., 0.),
            Vec3::new(0., 0., 2.),
            Vec3::ZERO,
        ];
        let left = reading(&[0, 1, 2], &all[..3]).unwrap();
        let right = reading(&[1, 2, 3], &all[1..]).unwrap();
        let union = reading(&[0, 1, 2, 3], &all).unwrap();
        assert_eq!(left.sum_vertex_force_n, 12.);
        assert_eq!(right.sum_vertex_force_n, 7.);
        assert_eq!(union.sum_vertex_force_n, 12.);
        assert_eq!(union.resultant_n, Vec3::new(0., 0., 2.));
        assert_ne!(
            union.sum_vertex_force_n,
            left.sum_vertex_force_n + right.sum_vertex_force_n
        );
        assert_eq!(left.max_force_vertex, 0);
        assert_eq!(right.max_force_vertex, 1);
    }

    #[test]
    fn regional_scope_refuses_overlap_empty_and_unretained_owners_and_reselects_new_meshes() {
        let triangles = [
            [[0., 0.], [3., 0.], [0., 3.]].map(Vec2::from),
            [[3., 0.], [6., 0.], [3., 3.]].map(Vec2::from),
        ];
        let faces = [[0, 1, 2], [1, 3, 2]];
        let names = ["panel".into()];
        for selected in [
            vec![region("one", -1., 2.), region("two", 0., 3.)],
            vec![region("empty", 7., 8.)],
        ] {
            assert!(partition(&selected, &names, &[0, 0], &faces, &triangles).is_err());
        }
        let mut unknown = region("unknown", -1., 7.);
        unknown.instance = "unretained".into();
        assert!(partition(&[unknown], &names, &[0, 0], &faces, &triangles).is_err());
        let selected = [region("fixed", -1., 2.)];
        assert_eq!(
            partition(&selected, &names, &[0, 0], &faces, &triangles)
                .unwrap()
                .regions["fixed"]
                .source_triangles,
            [0]
        );
        // The polygon stays fixed in source millimetres; a fresh altered mesh changes membership.
        let moved = triangles.map(|triangle| triangle.map(|p| p - Vec2::new(3., 0.)));
        assert_eq!(
            partition(&selected, &names, &[0, 0], &faces, &moved)
                .unwrap()
                .regions["fixed"]
                .source_triangles,
            [1]
        );
    }

    #[test]
    fn regional_peaks_keep_opposing_loads_and_earliest_sample_then_lowest_vertex_ties() {
        let first = reading(
            &[2, 4, 8],
            &[
                Vec3::new(3., 4., 0.),
                Vec3::new(-3., -4., 0.),
                Vec3::new(0., 0., 2.),
            ],
        )
        .unwrap();
        let second = reading(&[2, 4, 8], &[Vec3::ZERO, Vec3::ZERO, Vec3::new(0., 0., 6.)]).unwrap();
        let third = reading(&[2, 4, 8], &[Vec3::new(0., 0., 6.), Vec3::ZERO, Vec3::ZERO]).unwrap();
        let mut summary = RegionSummary::new(&first, 0);
        summary.observe(&second, 1);
        summary.observe(&third, 2);
        assert_eq!(summary.peak_body_sum_vertex_force_n, 12.);
        assert_eq!(summary.peak_body_sum_vertex_force_sample, 0);
        assert_eq!(summary.peak_body_resultant_n, 6.);
        assert_eq!(summary.peak_body_resultant_sample, 1);
        assert_eq!(summary.max_body_vertex_force_n, 6.);
        assert_eq!(summary.max_body_vertex_force_sample, 1);
        assert_eq!(summary.max_force_vertex, 8);
        let zero = reading(&[2, 4, 8], &[Vec3::ZERO; 3]).unwrap();
        assert_eq!(RegionSummary::new(&zero, 0).max_force_vertex, 2);
    }

    #[test]
    fn regional_request_schemas_are_exclusive_and_scope_is_bounded() {
        let value = json!({"schema":"ftx-contact-loads/2",
            "regions":[region("waist", -1., 2.)],
            "max_vertex_samples":100,"max_pair_evaluations":100});
        let request: Request = serde_json::from_value(value.clone()).unwrap();
        request.validate().unwrap();
        assert_eq!(serde_json::to_value(&request).unwrap(), value);
        for (key, changed) in [
            ("instances", Value::Null),
            ("instances", json!(["panel"])),
            ("regions", Value::Null),
            ("unexpected", json!(true)),
            ("schema", json!("ftx-contact-loads/1")),
        ] {
            let mut mixed = value.clone();
            mixed[key] = changed;
            assert!(serde_json::from_value::<Request>(mixed).is_err(), "{key}");
        }
        let mut empty = request.clone();
        empty.regions = Some(vec![]);
        assert!(empty.validate().is_err());
        let mut duplicate = request.clone();
        duplicate
            .regions
            .as_mut()
            .unwrap()
            .push(region("waist", 3., 7.));
        assert!(duplicate.validate().is_err());
        let mut legacy = value;
        legacy["schema"] = json!("ftx-contact-loads/1");
        legacy.as_object_mut().unwrap().remove("regions");
        let legacy: Request = serde_json::from_value(legacy).unwrap();
        legacy.validate().unwrap();
        assert!(
            serde_json::to_value(legacy)
                .unwrap()
                .get("regions")
                .is_none()
        );
    }
}

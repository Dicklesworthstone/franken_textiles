//! Qualified centerline excess conductance over the unchanged, source-locked coverage network.
//! The observation is the difference from that unbridged reference under the same reservoir films,
//! not a second full seam footprint. It supplies no transient storage or seam microgeometry.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode, Vec2, Vec3};
use ftx_pattern::materials::{Evidence, Quantity, Source};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Boundary, CoverageSolution, SteadySurface, digest, invalid};
use crate::thermal::surface::projection::{Hit, Projection};

const MODEL: &str = "qualified_excess_centerline_conductance/1";
const BASIS: &str = "excess_over_locked_unbridged_stack_same_reservoir_films";
// Dimensionless barycentric/edge-parameter roundoff, not a physical seam width or fitting tolerance.
const ROUND: f64 = 512. * f64::EPSILON;

/// One independently qualified observation of the excess over an exact unbridged reference stack.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Qualification {
    /// Unique observation identity within this path.
    pub id: String,
    /// Existing steady surface model hash: binds source selections and reference mesh topology.
    pub surface_model_sha256: String,
    /// Exact normalized material identities, in inside-to-outside order at the centerline.
    pub material_sha256: Vec<String>,
    /// Exact reference interfaces per covered body area, m² K/W; no inferred gap correction.
    pub interface_resistance_m2_k_w: Vec<f64>,
    /// Source-qualified current cloth/body area-ratio range for each reference layer.
    pub layer_area_ratio_ranges: Vec<[f64; 2]>,
    /// The reference test's prescribed inner reservoir film, m² K/W.
    pub inside_resistance_m2_k_w: f64,
    /// The reference test's prescribed outer reservoir film, m² K/W.
    pub outside_resistance_m2_k_w: f64,
    /// Both reservoir temperatures and all source-path faces must remain in this interval, °C.
    pub temperature_range_c: [f64; 2],
    /// Qualified total current centerline length of this named path, metres.
    pub length_range_m: [f64; 2],
    /// Nonnegative excess in W/(m*K), with method, source, specimen and conditioning.
    pub excess_linear_conductance: Quantity,
    /// Sources for the observation and its complete declared applicability domain.
    pub sources: BTreeMap<String, Source>,
    /// Independent permission for estimated bridge observations; never changes material consent.
    #[serde(default)]
    pub allow_estimates: bool,
    /// Exactly `excess_over_locked_unbridged_stack_same_reservoir_films`.
    pub reference_basis: String,
    /// Source's basis for the reference subtraction, geometry domains and additive centerline model.
    pub qualification: String,
}

/// Explicit physical seam or closure centerline on the retained solver mesh, not inferred from a label.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Path {
    /// Unique physical feature identity; repeated/overlapping edges are forbidden across features.
    pub id: String,
    /// Exact retained drape instance carrying this centerline.
    pub instance: String,
    /// Connected, nonrepeating mesh-edge path in the retained solver vertex order.
    pub vertices: Vec<usize>,
    /// Alternative exact source/stack qualifications. Every contributing interval needs one match.
    pub qualifications: Vec<Qualification>,
}

/// Optional extension of the existing steady coverage request; no independent heat solver.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Exactly `ftx-steady-thermal-bridges/1`.
    pub schema: String,
    /// Exactly `qualified_excess_centerline_conductance/1`.
    pub model: String,
    /// Explicit disjoint centerlines, each with its own source-qualified reference stack.
    pub paths: Vec<Path>,
}

fn text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 4096
}
fn hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|c| c.is_ascii_hexdigit())
}
fn range([lo, hi]: [f64; 2], floor: f64) -> bool {
    lo.is_finite() && hi.is_finite() && lo >= floor && hi >= lo
}
fn contains(domain: [f64; 2], value: f64) -> bool {
    value.is_finite() && value >= domain[0] && value <= domain[1]
}

impl Qualification {
    fn validate(&self) -> Result<(), Refusal> {
        let q = &self.excess_linear_conductance;
        if !text(&self.id)
            || !hash(&self.surface_model_sha256)
            || self.material_sha256.is_empty()
            || self.material_sha256.len() > 64
            || self.material_sha256.iter().any(|s| !hash(s))
            || self.interface_resistance_m2_k_w.len() + 1 != self.material_sha256.len()
            || self.layer_area_ratio_ranges.len() != self.material_sha256.len()
            || self
                .interface_resistance_m2_k_w
                .iter()
                .any(|v| !v.is_finite() || *v < 0.)
            || self
                .layer_area_ratio_ranges
                .iter()
                .any(|r| !range(*r, f64::MIN_POSITIVE))
            || !range(self.temperature_range_c, -273.15)
            || !range(self.length_range_m, f64::MIN_POSITIVE)
            || [
                self.inside_resistance_m2_k_w,
                self.outside_resistance_m2_k_w,
            ]
            .iter()
            .any(|v| !v.is_finite() || *v < 0.)
            || q.unit != "W/(m*K)"
            || !q.value.is_finite()
            || q.value < 0.
            || !text(&q.method)
            || ["specimen_id", "conditioning"]
                .iter()
                .any(|key| q.conditions.get(*key).is_none_or(|value| !text(value)))
            || q.bounds
                .is_some_and(|r| !range(r, 0.) || !contains(r, q.value))
            || self.sources.is_empty()
            || self.sources.len() > 32
            || !self.sources.contains_key(&q.source)
            || self.sources.iter().any(|(id, s)| {
                !text(id)
                    || [&s.reference, &s.accessed_on, &s.note]
                        .iter()
                        .any(|v| !text(v))
            })
            || self.reference_basis != BASIS
            || !text(&self.qualification)
        {
            return Err(invalid(
                "thermal bridge needs a complete source-qualified excess observation, exact reference stack and finite applicability domains",
            ));
        }
        Ok(())
    }
}

#[derive(Default)]
struct Work(usize);
impl Work {
    fn add(&mut self, count: usize) -> Result<(), Refusal> {
        self.0 = self
            .0
            .checked_add(count)
            .filter(|n| *n <= 2_000_000)
            .ok_or_else(|| {
                Refusal::new(
                    RefusalCode::BudgetExceeded,
                    "thermal bridge geometry exceeds two million operations",
                )
            })?;
        Ok(())
    }
}

fn local_vertex(triangle: [usize; 3], vertex: usize) -> Vec2 {
    match triangle
        .iter()
        .position(|&v| v == vertex)
        .expect("admitted mesh edge")
    {
        0 => Vec2::ZERO,
        1 => Vec2::new(1., 0.),
        _ => Vec2::new(0., 1.),
    }
}

// Clip an edge parameter against the existing convex source-triangle footprint. No second body
// projection or guessed seam width is introduced. Shared boundaries are resolved after partitioning.
fn interval(
    a: Vec2,
    b: Vec2,
    polygon: &[Vec2],
    work: &mut Work,
) -> Result<Option<[f64; 2]>, Refusal> {
    work.add(polygon.len())?;
    let mut result = [0_f64, 1_f64];
    for i in 0..polygon.len() {
        let p = polygon[i];
        let edge = polygon[(i + 1) % polygon.len()] - p;
        let c = edge.cross(a - p);
        let d = edge.cross(b - a);
        if !c.is_finite() || !d.is_finite() {
            return Err(invalid("thermal bridge projection overflow"));
        }
        if d.abs() <= ROUND {
            if c < -ROUND {
                return Ok(None);
            }
        } else {
            let t = -c / d;
            if d > 0. {
                result[0] = result[0].max(t);
            } else {
                result[1] = result[1].min(t);
            }
        }
    }
    if result[0] >= result[1] {
        return Ok(None);
    }
    if result[0] < ROUND {
        result[0] = 0.;
    }
    if 1. - result[1] < ROUND {
        result[1] = 1.;
    }
    Ok(Some(result))
}

fn product(a: f64, b: f64) -> Result<f64, Refusal> {
    let value = a * b;
    if !value.is_finite() || (a != 0. && b != 0. && value == 0.) {
        Err(invalid(
            "thermal bridge length or signed power is not representable",
        ))
    } else {
        Ok(value)
    }
}

fn add(a: f64, b: f64) -> Result<f64, Refusal> {
    let sum = a + b;
    if !sum.is_finite()
        || (a != 0.
            && b != 0.
            && a.signum() == b.signum()
            && (sum.abs() <= a.abs() || sum.abs() <= b.abs()))
    {
        Err(invalid(
            "thermal bridge accumulation cannot retain every nonzero length or signed power contribution",
        ))
    } else {
        Ok(sum)
    }
}

struct Portion {
    interval: [f64; 2],
    triangle: usize,
    region: usize,
}

/// Private call context: all geometry, source selections and baseline fields have already passed
/// the canonical steady coverage authority. Public callers enter through `coverage::request`.
pub(super) struct Context<'a> {
    pub source: &'a SteadySurface,
    pub cloth: &'a [Vec3],
    pub projection: &'a Projection,
    pub baseline: &'a CoverageSolution,
    pub boundaries: &'a [Boundary],
    pub instances: &'a [String],
    pub owners: &'a [usize],
    pub model_sha256: &'a str,
}

impl Context<'_> {
    fn qualification<'a>(
        &self,
        path: &'a Path,
        region: usize,
        length: f64,
    ) -> Result<&'a Qualification, Refusal> {
        let topology = &self.projection.layout.regions[region];
        let solution = &self.baseline.regions[region];
        let boundary = self.boundaries[topology.body_triangle];
        let materials: Vec<_> = topology
            .layers
            .iter()
            .map(|layer| {
                let patch = &self.source.patches[layer.cloth_triangle];
                self.source.columns[patch.column].layers[0].sha256()
            })
            .collect();
        let matches: Vec<_> = path
            .qualifications
            .iter()
            .filter(|q| {
                q.surface_model_sha256 == self.model_sha256
                    && q.material_sha256
                        .iter()
                        .map(String::as_str)
                        .eq(materials.iter().copied())
                    && q.interface_resistance_m2_k_w == topology.interface_resistance_m2_k_w
                    && q.inside_resistance_m2_k_w == boundary.inside_resistance
                    && q.outside_resistance_m2_k_w == boundary.outside_resistance
                    && contains(q.temperature_range_c, boundary.inside_c)
                    && contains(q.temperature_range_c, boundary.outside_c)
                    && solution
                        .conduction
                        .layer_faces_c
                        .iter()
                        .flatten()
                        .all(|&t| contains(q.temperature_range_c, t))
                    && contains(q.length_range_m, length)
                    && q.layer_area_ratio_ranges
                        .iter()
                        .zip(&solution.layer_area_m2)
                        .all(|(&domain, &area)| contains(domain, area / solution.body_area_m2))
            })
            .collect();
        if matches.len() != 1 {
            return Err(invalid(
                "thermal bridge needs exactly one qualification matching the current source model, ordered stack, interfaces, films, length, area ratios and temperatures",
            ));
        }
        if matches[0].excess_linear_conductance.evidence == Evidence::Estimate
            && !matches[0].allow_estimates
        {
            return Err(invalid(
                "estimated thermal bridge observations require independent explicit permission",
            ));
        }
        Ok(matches[0])
    }
}

impl Request {
    /// Validate bounded source policies independently of any candidate material's availability.
    ///
    /// # Errors
    /// Unsupported excess model, invalid sources/domains, repeated identities or excessive paths.
    pub fn validate(&self) -> Result<(), Refusal> {
        if self.schema != "ftx-steady-thermal-bridges/1"
            || self.model != MODEL
            || self.paths.is_empty()
            || self.paths.len() > 128
            || self.paths.iter().map(|p| p.vertices.len()).sum::<usize>() > 2048
        {
            return Err(invalid(
                "invalid thermal bridge model, path count or vertex budget",
            ));
        }
        let mut paths = BTreeSet::new();
        for path in &self.paths {
            if !text(&path.id)
                || !paths.insert(&path.id)
                || !text(&path.instance)
                || path.vertices.len() < 2
                || path.qualifications.is_empty()
                || path.qualifications.len() > 64
                || path.vertices.iter().collect::<BTreeSet<_>>().len() != path.vertices.len()
            {
                return Err(invalid(
                    "thermal bridge needs unique physical paths, nonrepeating vertices and bounded qualification alternatives",
                ));
            }
            let mut ids = BTreeSet::new();
            for q in &path.qualifications {
                if !ids.insert(&q.id) {
                    return Err(invalid("duplicate thermal bridge qualification identity"));
                }
                q.validate()?;
            }
        }
        Ok(())
    }

    pub(super) fn evaluate(&self, context: Context<'_>) -> Result<Value, Refusal> {
        self.validate()?;
        let b = context.boundaries[0];
        if context.boundaries.iter().any(|v| {
            v.inside_c != b.inside_c
                || v.outside_c != b.outside_c
                || v.inside_resistance != b.inside_resistance
                || v.outside_resistance != b.outside_resistance
        }) {
            return Err(invalid(
                "this thermal bridge model requires uniform prescribed reservoir temperatures and films",
            ));
        }
        let mut work = Work::default();
        work.add(context.source.patches.len() * 3)?;
        let mut hits: Vec<Vec<(usize, &Hit)>> = vec![Vec::new(); context.source.patches.len()];
        let region_ids: BTreeMap<_, _> = context
            .projection
            .layout
            .regions
            .iter()
            .enumerate()
            .map(|(i, r)| (r.id.as_str(), i))
            .collect();
        for footprint in &context.projection.footprints {
            let region = region_ids[footprint.region.as_str()];
            for hit in &footprint.hits {
                hits[hit.cloth_triangle].push((region, hit));
            }
        }
        let mut edges: BTreeMap<[usize; 2], Vec<usize>> = BTreeMap::new();
        for (i, patch) in context.source.patches.iter().enumerate() {
            for mut edge in [
                [patch.vertices[0], patch.vertices[1]],
                [patch.vertices[1], patch.vertices[2]],
                [patch.vertices[2], patch.vertices[0]],
            ] {
                edge.sort_unstable();
                edges.entry(edge).or_default().push(i);
            }
        }
        let mut used = BTreeSet::new();
        let mut physical_segments: Vec<[Vec3; 2]> = Vec::new();
        let mut body_power = vec![0_f64; context.baseline.body.len()];
        let mut path_reports = Vec::new();
        let mut roundoff_length_bound = 0_f64;
        for path in &self.paths {
            let owner = context
                .instances
                .iter()
                .position(|s| s == &path.instance)
                .ok_or_else(|| invalid("thermal bridge names an absent retained instance"))?;
            let mut path_edges = Vec::new();
            let mut path_length = 0_f64;
            for pair in path.vertices.windows(2) {
                let edge = [pair[0].min(pair[1]), pair[0].max(pair[1])];
                let triangles: Vec<_> = edges.get(&edge).into_iter().flatten().copied().collect();
                if !triangles.iter().any(|&i| context.owners[i] == owner) || !used.insert(edge) {
                    return Err(invalid(
                        "thermal bridge centerline must follow distinct actual mesh edges of its named instance",
                    ));
                }
                let segment = [context.cloth[edge[0]], context.cloth[edge[1]]];
                let direction = segment[1] - segment[0];
                let length = direction.norm();
                if !length.is_finite() || length <= 0. {
                    return Err(invalid(
                        "thermal bridge has a collapsed or nonfinite current edge",
                    ));
                }
                for previous in &physical_segments {
                    work.add(1)?;
                    let u = previous[1] - previous[0];
                    let offset = previous[0] - segment[0];
                    if direction.cross(u).norm() <= ROUND * length * u.norm()
                        && direction.cross(offset).norm() <= ROUND * length * offset.norm()
                    {
                        let a = offset.dot(direction) / direction.norm2();
                        let z = (previous[1] - segment[0]).dot(direction) / direction.norm2();
                        if a.max(z).min(1.) - a.min(z).max(0.) > ROUND {
                            return Err(invalid(
                                "thermal bridge centerlines overlap in current physical space; excess power cannot be counted twice",
                            ));
                        }
                    }
                }
                physical_segments.push(segment);
                path_length = add(path_length, length)?;
                path_edges.push((edge, triangles, length));
            }
            let mut contributions = Vec::new();
            let mut power = 0_f64;
            let mut qualified = BTreeMap::new();
            let identities: BTreeMap<_, _> = path
                .qualifications
                .iter()
                .map(|q| {
                    Ok((
                        q.id.as_str(),
                        digest(&serde_json::to_value(q).map_err(|e| invalid(e.to_string()))?)?,
                    ))
                })
                .collect::<Result<_, Refusal>>()?;
            for (edge, triangles, length) in path_edges {
                let mut portions = Vec::new();
                let mut cuts = vec![0_f64, 1_f64];
                for triangle in triangles {
                    let vertices = context.source.patches[triangle].vertices;
                    for &(region, hit) in &hits[triangle] {
                        if let Some(interval) = interval(
                            local_vertex(vertices, edge[0]),
                            local_vertex(vertices, edge[1]),
                            &hit.cloth_polygon,
                            &mut work,
                        )? {
                            cuts.extend(interval);
                            portions.push(Portion {
                                interval,
                                triangle,
                                region,
                            });
                        }
                    }
                }
                cuts.sort_by(f64::total_cmp);
                cuts.dedup_by(|a, b| (*a - *b).abs() <= ROUND);
                // All cuts are dimensionless; report the conservative ownership roundoff scale.
                roundoff_length_bound += length * ROUND * cuts.len() as f64;
                for cut in cuts.windows(2) {
                    let midpoint = cut[0] + (cut[1] - cut[0]) * 0.5;
                    let mut candidates = Vec::new();
                    for portion in &portions {
                        work.add(1)?;
                        if midpoint >= portion.interval[0] - ROUND
                            && midpoint <= portion.interval[1] + ROUND
                        {
                            let q = if let Some(&q) = qualified.get(&portion.region) {
                                q
                            } else {
                                work.add(
                                    path.qualifications.len()
                                        * (context.projection.layout.regions[portion.region]
                                            .layers
                                            .len()
                                            + 1),
                                )?;
                                let q = context.qualification(path, portion.region, path_length)?;
                                qualified.insert(portion.region, q);
                                q
                            };
                            candidates.push((portion, q));
                        }
                    }
                    if candidates.is_empty() {
                        return Err(invalid(
                            "thermal bridge centerline is not completely mapped to the declared covered body",
                        ));
                    }
                    let identity = &identities[candidates[0].1.id.as_str()];
                    for (_, q) in &candidates[1..] {
                        if &identities[q.id.as_str()] != identity {
                            return Err(invalid(
                                "shared thermal bridge boundary has incompatible physical stack or source qualifications",
                            ));
                        }
                    }
                    candidates.sort_by_key(|(p, _)| {
                        (
                            context.projection.layout.regions[p.region].body_triangle,
                            p.region,
                            p.triangle,
                        )
                    });
                    let (portion, q) = candidates[0];
                    let region = &context.projection.layout.regions[portion.region];
                    let part_length = product(length, cut[1] - cut[0])?;
                    let conductance = product(q.excess_linear_conductance.value, part_length)?;
                    let part_power = product(conductance, b.inside_c - b.outside_c)?;
                    power = add(power, part_power)?;
                    body_power[region.body_triangle] =
                        add(body_power[region.body_triangle], part_power)?;
                    contributions.push(
                        json!({"edge":edge,"edge_fraction":cut,"source_triangle":portion.triangle,
                        "body_triangle":region.body_triangle,"region":region.id,
                        "qualification_id":q.id,"qualification_sha256":identity,
                        "length_m":part_length,"conductance_w_k":conductance,"power_w":part_power}),
                    );
                }
            }
            if !power.is_finite() {
                return Err(invalid("thermal bridge power accumulation overflow"));
            }
            path_reports.push(
                json!({"id":path.id,"instance":path.instance,"length_m":path_length,
                "power_w":power,"contributions":contributions}),
            );
        }
        let bridge_power = body_power
            .iter()
            .try_fold(0., |sum, &value| add(sum, value))?;
        let total = add(context.baseline.power_w, bridge_power)?;
        if !bridge_power.is_finite() || !total.is_finite() || !roundoff_length_bound.is_finite() {
            return Err(invalid("thermal bridge total power is not representable"));
        }
        let body: Vec<_> = body_power.iter().zip(&context.baseline.body).enumerate().map(|(i, (&q, b))|
            Ok(json!({"body_triangle":i,"baseline_power_w":b.power_w,"bridge_power_w":q,"total_power_w":add(b.power_w,q)?})))
            .collect::<Result<_, Refusal>>()?;
        Ok(
            json!({"schema":"ftx-steady-thermal-bridge-result/1","model":MODEL,
            "surface_model_sha256":context.model_sha256,"baseline_power_w":context.baseline.power_w,
            "bridge_power_w":bridge_power,"total_power_w":total,"inside_excess_power_w":bridge_power,
            "outside_excess_power_w":bridge_power,"excess_balance_residual_w":0.,
            "summation_residual_w":total-context.baseline.power_w-bridge_power,
            "body":body,"paths":path_reports,"geometry_operations":work.0,
            "ownership_roundoff_length_bound_m":roundoff_length_bound,
            "ownership_policy":"partition each current mesh-edge length once; shared boundaries choose lowest body/region/source-triangle only when exact physical qualification agrees",
            "reference_basis":BASIS,"storage_modeled":false,"uncertainty_propagated":false,
            "empirical_validation_claim":false,
            "scope":"source-qualified centerline excess over the identified covered stack; not generic seam microgeometry, a replacement footprint or a full additional seam area"}),
        )
    }
}

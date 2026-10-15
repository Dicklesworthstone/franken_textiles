//! Geometry-derived disjoint coverage by orthogonal projection along each body triangle's normal.
//!
//! This is a declared local-normal transport closure, not a ray-sampled estimate of covered area.
//! Convex clipping partitions each body face exactly up to reported barycentric roundoff. Affine
//! inverse maps allocate actual cloth/reference area; intersecting allocations on a cloth triangle
//! refuse even when their summed fractions are below one. Folded/ambiguous layers are not guessed.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, Vec2, Vec3, receipt::sha256_hex};
use ftx_pattern::material_catalog::ResolvedMaterial;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::coverage::{Coverage, Layout, Portion, Region};
use super::{Patch, Surface, invalid, positive};
use crate::bvh::{QueryBvh, overlaps};

pub(super) mod clip;
pub mod gap;
use clip::{AREA_EPS, Work, area, barycentric, ccw, intersection, split};

// Borrow only retained geometry and material observations. Projection and face-gap evaluation have
// no storage dependency: a steady source must never be adapted by inventing density or heat capacity.
// The original capability snapshot stays in the input hash, preserving transient replay identities.
pub(crate) struct ProjectionSource<'a> {
    pub vertex_count: usize,
    pub patches: &'a [Patch],
    pub layers: Vec<&'a [ResolvedMaterial]>,
    pub snapshot: Value,
}

/// An explicit adjacent-layer resistance or an opt-in, geometry-derived stagnant-gas path.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Interface {
    /// Inner physical-layer identity.
    pub inner: String,
    /// Outer physical-layer identity.
    pub outer: String,
    /// Resistance per covered body area, m² K/W. Zero must also be explicitly declared.
    pub resistance_m2_k_w: f64,
    /// Measurement source or explicit assumption explaining this interface value.
    pub basis: String,
    /// Optional conduction-only gap model. Its resistance is added to the explicitly declared
    /// fixed resistance above. Absent keeps the original constant-interface behavior and receipt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stagnant_gap: Option<gap::Conduction>,
}

/// Controls the geometric closure; all distances use the solver's metre frame.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// Outward normal search horizon. A contributing sheet cut by this horizon is refused.
    pub max_distance_m: f64,
    /// Minimum absolute normal cosine for a contributing cloth triangle, in (0,1].
    pub min_normal_cosine: f64,
    /// Minimum distinguishable separation of different mid-surfaces; positive, in metres.
    pub separation_tolerance_m: f64,
    /// Through-thickness discretization for the existing heat solver.
    pub cells_per_layer: usize,
    /// Directed interface properties for every adjacent pair found by projection.
    pub interfaces: Vec<Interface>,
}

/// Physical sheet identities are authored; triangle correspondence and layer order are geometric.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Must be `ftx-thermal-projection/1`.
    pub schema: String,
    /// One nonempty physical-layer name for every source cloth triangle, in retained order.
    pub cloth_layers: Vec<String>,
    /// Explicit geometry and transport assumptions.
    pub settings: Settings,
}

/// One invertible body-to-cloth subpolygon, retained for spatial review.
#[derive(Clone, Debug, Serialize)]
pub struct Hit {
    /// Source thermal triangle index.
    pub cloth_triangle: usize,
    /// Barycentric coordinates (weights of vertices 1 and 2) on this cloth triangle.
    pub cloth_polygon: Vec<Vec2>,
    /// Minimum/maximum normal distance from the body face to the cloth MID-SURFACE, metres.
    /// These are not surface air gaps: no half-thickness, convection or radiation is inferred.
    pub mid_surface_distance_m: [f64; 2],
}

/// Convex body-area region, with ordered cloth intersections.
#[derive(Clone, Debug, Serialize)]
pub struct Footprint {
    /// Matching layout region ID.
    pub region: String,
    /// Owning body face.
    pub body_triangle: usize,
    /// Weights of body-face vertices 1 and 2; vertex 0 has weight 1-x-y.
    pub body_polygon: Vec<Vec2>,
    /// Inner-to-outer hits in the same order as the corresponding layout region.
    pub hits: Vec<Hit>,
}

/// Geometry-derived layout ready for the existing source-locked coverage consumer.
#[derive(Clone, Debug, Serialize)]
pub struct Projection {
    /// No alternate thermal/material model: this is the existing layout contract.
    pub layout: Layout,
    /// Actual clipped polygons and distance bounds, not merely area fractions.
    pub footprints: Vec<Footprint>,
    /// Exact material/geometry/request identity from which the layout was derived.
    pub input_sha256: String,
    /// Number of BVH candidate triangle pairs inspected.
    pub candidate_pairs: usize,
    /// Bounded clipping/allocation work, including duplicate-area checks.
    pub geometry_operations: usize,
    /// Sum of absolute partition-area residuals, m²; tiny clipping cells may be below resolution.
    pub partition_roundoff_m2: f64,
    /// Sum of projected footprint areas below the 1e-12 barycentric area resolution, m².
    /// Footprints may overlap, so this is a conservative numerical bound, not added coverage.
    pub unresolved_footprint_area_m2: f64,
    /// Body area removed solely to correct <= 2e-10 fractional floating-point overshoot, m².
    pub body_roundoff_adjustment_m2: f64,
    /// Reference cloth area removed by that same explicitly reported numerical correction, m².
    pub cloth_roundoff_adjustment_m2: f64,
    /// Derived surface gaps and numerical resistance bounds, only for opted-in interfaces.
    /// Source observations and modeling consent remain in the hashed projection request.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub gap_interfaces: Vec<gap::Resolved>,
}

#[derive(Clone)]
struct Cell {
    polygon: Vec<Vec2>,
    hits: Vec<usize>,
}
struct Candidate {
    id: usize,
    xy: [Vec2; 3],
    z: [f64; 3],
    polygon: Vec<Vec2>,
}
impl Candidate {
    fn depth(&self, p: Vec2) -> Result<f64, Refusal> {
        let b = barycentric(p, self.xy)?;
        let z = self.z[0] + b.x * (self.z[1] - self.z[0]) + b.y * (self.z[2] - self.z[0]);
        if !z.is_finite() {
            return Err(invalid("projection depth overflow"));
        }
        Ok(z)
    }
    fn depths(&self, polygon: &[Vec2]) -> Result<[f64; 2], Refusal> {
        let mut r = [f64::INFINITY, f64::NEG_INFINITY];
        for &p in polygon {
            let z = self.depth(p)?;
            r = [r[0].min(z), r[1].max(z)];
        }
        Ok(r)
    }
}

fn aabb(points: impl IntoIterator<Item = Vec3>) -> (Vec3, Vec3) {
    points.into_iter().fold(
        (
            Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
            Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
        ),
        |(a, b), v| {
            (
                Vec3::new(a.x.min(v.x), a.y.min(v.y), a.z.min(v.z)),
                Vec3::new(b.x.max(v.x), b.y.max(v.y), b.z.max(v.z)),
            )
        },
    )
}

fn validate_request<'a>(
    source: &ProjectionSource<'_>,
    request: &'a Request,
) -> Result<BTreeMap<(&'a str, &'a str), &'a Interface>, Refusal> {
    let s = &request.settings;
    if request.schema != "ftx-thermal-projection/1"
        || request.cloth_layers.len() != source.patches.len()
        || request.cloth_layers.iter().any(|s| s.trim().is_empty())
        || !s.max_distance_m.is_finite()
        || s.max_distance_m <= 0.0
        || !s.min_normal_cosine.is_finite()
        || s.min_normal_cosine <= 0.0
        || s.min_normal_cosine > 1.0
        || !s.separation_tolerance_m.is_finite()
        || s.separation_tolerance_m <= 0.0
        || s.separation_tolerance_m >= s.max_distance_m
        || s.interfaces.len() > 4096
    {
        return Err(invalid(
            "invalid projection schema, layer identities, geometry controls or interface budget",
        ));
    }
    let names: BTreeSet<_> = request.cloth_layers.iter().map(String::as_str).collect();
    let mut interfaces = BTreeMap::new();
    for i in &s.interfaces {
        if i.inner == i.outer
            || !names.contains(i.inner.as_str())
            || !names.contains(i.outer.as_str())
            || i.basis.trim().is_empty()
            || !i.resistance_m2_k_w.is_finite()
            || i.resistance_m2_k_w < 0.0
            || interfaces
                .insert((i.inner.as_str(), i.outer.as_str()), i)
                .is_some()
        {
            return Err(invalid(
                "invalid, duplicate or unsourced projection interface",
            ));
        }
        if let Some(gap) = &i.stagnant_gap {
            gap.validate()?;
        }
    }
    Ok(interfaces)
}

fn insert_candidate(
    cells: Vec<Cell>,
    c: &Candidate,
    id: usize,
    work: &mut Work,
) -> Result<Vec<Cell>, Refusal> {
    let mut out = Vec::new();
    for cell in cells {
        let mut remaining = cell.polygon;
        for i in 0..c.polygon.len() {
            let (inside, outside) = split(
                &remaining,
                c.polygon[i],
                c.polygon[(i + 1) % c.polygon.len()],
                work,
            )?;
            if area(&outside) > AREA_EPS {
                out.push(Cell {
                    polygon: outside,
                    hits: cell.hits.clone(),
                });
            }
            remaining = inside;
            if area(&remaining) <= AREA_EPS {
                break;
            }
        }
        if area(&remaining) > AREA_EPS {
            let mut hits = cell.hits;
            hits.push(id);
            if hits.len() > 64 {
                return Err(invalid("projection exceeds 64 layers at a body region"));
            }
            out.push(Cell {
                polygon: remaining,
                hits,
            });
        }
        if out.len() > 4096 {
            return Err(invalid(
                "projection exceeds 4096 partitions on one body face",
            ));
        }
    }
    Ok(out)
}

// Geometry-generated fractions only. Authored layout fractions retain Coverage's strict validation.
fn budget(value: f64, used: &mut f64, correction: &mut f64, scale: f64) -> Result<f64, Refusal> {
    positive("projected area fraction", value)?;
    let excess = (*used + value - 1.0).max(0.0);
    if excess > 2e-10 {
        return Err(invalid(
            "projection duplicates body area or reference cloth mass",
        ));
    }
    let accepted = if excess > 0.0 { 1.0 - *used } else { value };
    positive("resolved projected fraction", accepted)?;
    *correction += (value - accepted) * scale;
    *used += accepted;
    Ok(accepted)
}

/// Partition supplied body faces by normal projection onto the actual source cloth geometry.
/// Normals are determined by body winding. The caller supplies a non-self-intersecting outward
/// boundary; this checks edge consistency, not a global watertight-body or human-coverage certificate.
/// Remote/behind-body sheets outside the declared outward prism do not count as coverage.
///
/// # Errors
/// Invalid geometry or settings, grazing contributing sheets, body crossings, cut search horizons,
/// overlapping folds of one layer, unresolved/intersecting layer order, missing interfaces, spatial
/// reuse of a cloth portion, numerical partition failure, or bounded work/output exhaustion.
pub fn project(
    source: &Surface,
    cloth: &[Vec3],
    body: &[Vec3],
    faces: &[[usize; 3]],
    request: &Request,
) -> Result<Projection, Refusal> {
    let input = ProjectionSource {
        vertex_count: source.vertex_count,
        patches: &source.patches,
        layers: source.columns.iter().map(|c| c.layers.as_slice()).collect(),
        snapshot: source.snapshot(),
    };
    let projection = project_source(&input, cloth, body, faces, request)?;
    Coverage::new(source.clone(), projection.layout.clone())?;
    Ok(projection)
}

pub(crate) fn project_source(
    source: &ProjectionSource<'_>,
    cloth: &[Vec3],
    body: &[Vec3],
    faces: &[[usize; 3]],
    request: &Request,
) -> Result<Projection, Refusal> {
    if source.layers.iter().any(|layers| layers.len() != 1) {
        return Err(invalid("projection requires single-layer source triangles"));
    }
    let interfaces = validate_request(source, request)?;
    if cloth.len() != source.vertex_count
        || body.is_empty()
        || body.len() > 196_608
        || faces.is_empty()
        || faces.len() > 65_536
        || cloth.iter().chain(body).any(|p| !p.is_finite())
    {
        return Err(invalid(
            "projection requires all finite cloth/body positions",
        ));
    }
    let input_sha256 = sha256_hex(
        &serde_json::to_vec(&(&source.snapshot, cloth, body, faces, request))
            .map_err(|e| invalid(e.to_string()))?,
    );
    let mut layout = Layout {
        schema: "ftx-thermal-coverage-layout/1".into(),
        mapping_basis: format!(
            "Local-normal geometric projection /1; input {input_sha256}; mid-surface separation is not an air-gap resistance"
        ),
        body_vertex_count: body.len(),
        body_triangles: faces.to_vec(),
        regions: Vec::new(),
        cells_per_layer: request.settings.cells_per_layer,
    };
    if request
        .settings
        .interfaces
        .iter()
        .any(|i| i.stagnant_gap.is_some())
    {
        layout.mapping_basis.push_str("; opted-in interlayer stagnant-gas conduction uses frozen textile face thickness and parallel gap integration; see full projection request and gap diagnostics");
    }
    // Reuse the common topology/allocation contract before indexing the supplied body faces.
    layout.allocations(source.patches.len())?;
    let mut edges = BTreeMap::new();
    for face in faces {
        for i in 0..3 {
            let (a, b) = (face[i], face[(i + 1) % 3]);
            let entry = edges.entry((a.min(b), a.max(b))).or_insert((0, 0));
            entry.0 += 1;
            entry.1 += if a < b { 1 } else { -1 };
            if entry.0 > 2 || (entry.0 == 2 && entry.1 != 0) {
                return Err(invalid(
                    "projection body has inconsistent or nonmanifold edges",
                ));
            }
        }
    }
    let boxes: Vec<_> = source
        .patches
        .iter()
        .map(|p| aabb(p.vertices.map(|i| cloth[i])))
        .collect();
    let mut normals = Vec::new();
    for p in source.patches {
        let [a, b, c] = p.vertices.map(|i| cloth[i]);
        let n = (b - a).cross(c - a);
        normals.push(n / positive("cloth triangle normal", n.norm())?);
    }
    let bvh = QueryBvh::build(&boxes);
    let mut candidates = Vec::new();
    let mut work = Work::default();
    let mut tested = 0;
    let mut partition_roundoff = 0.0;
    let mut unresolved_footprint_area = 0.0;
    let mut body_correction = 0.0;
    let mut cloth_correction = 0.0;
    let mut footprints = Vec::new();
    let mut gap_interfaces = Vec::new();
    let mut allocations: Vec<Vec<(usize, Vec<Vec2>)>> = vec![Vec::new(); source.patches.len()];
    let mut allocated = vec![0.0; source.patches.len()];
    let tri = [Vec2::ZERO, Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0)];
    for (bi, face) in faces.iter().enumerate() {
        let [origin, b, c] = face.map(|i| body[i]);
        let e1 = b - origin;
        let e2 = c - origin;
        let cross = e1.cross(e2);
        let twice_area = positive("body triangle normal", cross.norm())?;
        let n = cross / twice_area;
        let det = twice_area * twice_area;
        if !det.is_finite() || det <= 1e-12 * e1.norm2() * e2.norm2() {
            return Err(invalid("ill-conditioned projection body triangle"));
        }
        let dual1 = e2.cross(n) / twice_area;
        let dual2 = n.cross(e1) / twice_area;
        let settings = &request.settings;
        let prism = aabb(face.iter().flat_map(|&i| {
            [
                body[i] - n * settings.separation_tolerance_m,
                body[i] + n * (settings.max_distance_m + settings.separation_tolerance_m),
            ]
        }));
        if !prism.0.is_finite() || !prism.1.is_finite() {
            return Err(invalid("projection prism overflow"));
        }
        bvh.query(prism, &mut candidates);
        let mut projected = Vec::new();
        for &ci in &candidates {
            tested += 1;
            work.add(1)?;
            if !overlaps(prism, boxes[ci]) {
                continue;
            }
            let xyz = source.patches[ci].vertices.map(|i| cloth[i] - origin);
            let xy = xyz.map(|v| Vec2::new(v.dot(dual1), v.dot(dual2)));
            if xy.iter().any(|p| !p.is_finite()) {
                return Err(invalid("projection coordinates overflow"));
            }
            let cosine = normals[ci].dot(n).abs();
            if cosine == 0.0 {
                continue;
            } // an exactly edge-on triangle has zero projected area
            let polygon = intersection(&tri, &ccw(xy.to_vec()), &mut work)?;
            if area(&polygon) <= AREA_EPS {
                unresolved_footprint_area += area(&polygon).abs() * twice_area;
                continue;
            }
            let candidate = Candidate {
                id: ci,
                xy,
                z: xyz.map(|v| v.dot(n)),
                polygon,
            };
            let [lo, hi] = candidate.depths(&candidate.polygon)?;
            if hi < -settings.separation_tolerance_m || lo > settings.max_distance_m {
                continue;
            }
            if lo < -settings.separation_tolerance_m {
                return Err(invalid(format!(
                    "cloth triangle {ci} crosses projection body face {bi}"
                )));
            }
            if hi > settings.max_distance_m + settings.separation_tolerance_m {
                return Err(invalid(
                    "projection distance horizon cuts a contributing sheet; increase max_distance_m",
                ));
            }
            if cosine < settings.min_normal_cosine {
                return Err(invalid(
                    "grazing cloth/body projection; refine geometry or review normal closure",
                ));
            }
            projected.push(candidate);
        }
        let mut cells = vec![Cell {
            polygon: tri.to_vec(),
            hits: Vec::new(),
        }];
        for (i, candidate) in projected.iter().enumerate() {
            cells = insert_candidate(cells, candidate, i, &mut work)?;
        }
        let total: f64 = cells.iter().map(|c| area(&c.polygon)).sum();
        if !total.is_finite() || (total - 0.5).abs() > 1e-9 {
            return Err(invalid("geometric body partition did not conserve area"));
        }
        partition_roundoff += (total - 0.5).abs() * twice_area;
        let mut covered = 0.0;
        for mut cell in cells {
            if cell.hits.is_empty() {
                continue;
            }
            if layout.regions.len() >= 65_536 {
                return Err(invalid("projection exceeds 65536 covered regions"));
            }
            let centre = cell.polygon.iter().copied().fold(Vec2::ZERO, |a, b| a + b)
                / cell.polygon.len() as f64;
            let mut order = cell
                .hits
                .iter()
                .map(|&i| Ok((i, projected[i].depth(centre)?)))
                .collect::<Result<Vec<_>, Refusal>>()?;
            order.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
            cell.hits = order.iter().map(|p| p.0).collect();
            let id = format!("body-{bi}-region-{}", layout.regions.len());
            let mut region = Region {
                id: id.clone(),
                body_triangle: bi,
                body_fraction: budget(
                    2.0 * area(&cell.polygon),
                    &mut covered,
                    &mut body_correction,
                    twice_area / 2.0,
                )?,
                layers: Vec::new(),
                interface_resistance_m2_k_w: Vec::new(),
            };
            let mut hits = Vec::new();
            let mut names = BTreeSet::new();
            for (j, &hit) in cell.hits.iter().enumerate() {
                let c = &projected[hit];
                let name = &request.cloth_layers[c.id];
                if !names.insert(name) {
                    return Err(invalid(format!(
                        "folded or overlapping physical layer {name} on body face {bi}"
                    )));
                }
                if j > 0 {
                    let prev = &projected[cell.hits[j - 1]];
                    for &v in &cell.polygon {
                        if c.depth(v)? - prev.depth(v)? <= settings.separation_tolerance_m {
                            return Err(invalid(
                                "crossing, touching or unresolved physical-layer order",
                            ));
                        }
                    }
                    let key = (request.cloth_layers[prev.id].as_str(), name.as_str());
                    let interface = interfaces.get(&key).ok_or_else(|| {
                        invalid(format!(
                            "missing explicit projection interface {} -> {}",
                            key.0, key.1
                        ))
                    })?;
                    let mut resistance = interface.resistance_m2_k_w;
                    if let Some(law) = &interface.stagnant_gap {
                        let resolved = gap::resolve(
                            source,
                            cloth,
                            face.map(|i| body[i]),
                            &cell.polygon,
                            [prev.id, c.id],
                            law,
                            &mut work,
                            &id,
                        )?;
                        resistance += resolved.resistance_m2_k_w;
                        gap_interfaces.push(resolved);
                    }
                    if !resistance.is_finite() {
                        return Err(invalid("gap and fixed interface resistance overflow"));
                    }
                    region.interface_resistance_m2_k_w.push(resistance);
                }
                let polygon = ccw(cell
                    .polygon
                    .iter()
                    .map(|&v| barycentric(v, c.xy))
                    .collect::<Result<Vec<_>, _>>()?);
                for (other_face, other) in &allocations[c.id] {
                    work.add(1)?;
                    if *other_face != bi
                        && clip::bounds_overlap(&polygon, other)
                        && area(&intersection(&polygon, other, &mut work)?) > AREA_EPS
                    {
                        return Err(invalid(format!(
                            "body normals reuse the same spatial cloth portion of triangle {}",
                            c.id
                        )));
                    }
                }
                allocations[c.id].push((bi, polygon.clone()));
                region.layers.push(Portion {
                    layer: name.clone(),
                    cloth_triangle: c.id,
                    fraction: budget(
                        2.0 * area(&polygon),
                        &mut allocated[c.id],
                        &mut cloth_correction,
                        source.patches[c.id].reference_area_m2,
                    )?,
                });
                hits.push(Hit {
                    cloth_triangle: c.id,
                    cloth_polygon: polygon,
                    mid_surface_distance_m: c.depths(&cell.polygon)?,
                });
            }
            layout.regions.push(region);
            footprints.push(Footprint {
                region: id,
                body_triangle: bi,
                body_polygon: cell.polygon,
                hits,
            });
        }
    }
    Ok(Projection {
        layout,
        footprints,
        input_sha256,
        candidate_pairs: tested,
        geometry_operations: work.0,
        partition_roundoff_m2: partition_roundoff,
        unresolved_footprint_area_m2: unresolved_footprint_area,
        body_roundoff_adjustment_m2: body_correction,
        cloth_roundoff_adjustment_m2: cloth_correction,
        gap_interfaces,
    })
}

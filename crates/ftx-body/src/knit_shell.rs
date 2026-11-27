//! Uncapped, conforming plane-clipped shells for 3D-to-knit drafting (`FTX-K12`).
//!
//! The generic tube path preserves shared edge intersections and the actual clipping planes.
//! A torso request requires its real landmarks and an already segmented torso tube. Full-body
//! sleeve/armhole extraction is not implemented and refuses instead of returning a horizontal
//! slice of unrelated anatomy. A bounded exact intersection audit checks both the clipped and
//! offset surfaces; local topology checks require an outward-wound annulus. These are geometric
//! checks on the requested discrete shell, not physical cloth clearance or garment-fit evidence.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::intersection::validate_mesh;
use ftx_core::shaping::{
    BoundaryRing, EaseProfile, EaseShellMesh, KnitShellRegion, ShapingSurface,
};
use ftx_core::{Refusal, RefusalCode, Vec3};

use crate::drape::{DEFAULT_POSE_AUDIT_PAIR_BUDGET, SkinMesh, SkinReport};
use crate::landmarks::BodyFrames;

const MAX_SHELL_VERTICES: usize = 250_000;
const MAX_SHELL_TRIANGLES: usize = 500_000;

/// Adapter exposing a supplied body region through `ShapingSurface`.
#[derive(Clone, Debug)]
pub struct AnnyKnitShell<'a> {
    /// Outward-wound source geometry (metres, z-up); anatomical regions must be pre-segmented.
    pub skin: &'a SkinMesh,
    /// Anatomical landmark heights and part frames (millimetres).
    pub frames: &'a BodyFrames,
    /// Canonical SHA-256 digest of the fitted body / report.
    pub source_body_sha256: String,
}

impl ShapingSurface for AnnyKnitShell<'_> {
    fn build_ease_shell(
        &self,
        region: KnitShellRegion,
        ease: &EaseProfile,
    ) -> Result<EaseShellMesh, Refusal> {
        validate_ease_profile(ease)?;
        let (z_low, z_high, offset_mm) = match region {
            KnitShellRegion::Torso => {
                let low = landmark_height_m(self.frames, "waist")?;
                let high = landmark_height_m(self.frames, "snp")?;
                require_torso_source(self.skin, self.frames, low, high)?;
                (low, high, ease.torso_radial_mm)
            }
            KnitShellRegion::FullUpperBody
            | KnitShellRegion::LeftSleeve
            | KnitShellRegion::RightSleeve => {
                return Err(Refusal::new(
                    RefusalCode::InvalidDocument,
                    "anatomical sleeve and full-upper-body knit shells require region segmentation and named wrist/armhole rings; a global height slab cannot supply them",
                )
                .repair("Supply an explicitly segmented tube to the tube shell builder."));
            }
            KnitShellRegion::Tube => {
                if self.skin.vertices.is_empty() {
                    return Err(Refusal::new(
                        RefusalCode::DegenerateGeometry,
                        "skin mesh has zero vertices",
                    ));
                }
                let mut zmin = f64::INFINITY;
                let mut zmax = f64::NEG_INFINITY;
                for v in &self.skin.vertices {
                    Refusal::finite("skin_vertex.z", v.z)?;
                    zmin = zmin.min(v.z);
                    zmax = zmax.max(v.z);
                }
                let span = zmax - zmin;
                Refusal::finite("tube height", span)?;
                if span <= 0.0 {
                    return Err(Refusal::new(
                        RefusalCode::DegenerateGeometry,
                        "a tube shell needs positive source height",
                    ));
                }
                // Retain the generic tube's explicit five-percent end trim, without inventing
                // a minimum anatomical height for short or flat input geometry.
                (zmin + 0.05 * span, zmax - 0.05 * span, ease.torso_radial_mm)
            }
        };
        build_ease_shell_from_mesh(
            &self.skin.vertices,
            &self.skin.triangles,
            z_low,
            z_high,
            offset_mm * 0.001,
            ease.min_clearance_mm * 0.001,
            &self.source_body_sha256,
        )
    }
}

fn landmark_height_m(frames: &BodyFrames, name: &str) -> Result<f64, Refusal> {
    Refusal::finite("body floor height", frames.floor_z)?;
    let height = frames.landmarks.get(name).ok_or_else(|| {
        Refusal::new(
            RefusalCode::InvalidDocument,
            format!("a torso knit shell requires the body's {name} landmark"),
        )
    })?;
    Refusal::finite("body landmark height", *height)?;
    let world_m = (frames.floor_z + height) * 0.001;
    Refusal::finite("body landmark world height", world_m)?;
    Ok(world_m)
}

fn position_key(p: Vec3) -> [u64; 3] {
    [p.x, p.y, p.z].map(|v| if v == 0.0 { 0 } else { v.to_bits() })
}

fn require_torso_source(
    skin: &SkinMesh,
    frames: &BodyFrames,
    low: f64,
    high: f64,
) -> Result<(), Refusal> {
    if frames.torso_points.is_empty() {
        return Err(Refusal::new(
            RefusalCode::InvalidDocument,
            "a torso knit shell needs source anatomical torso membership",
        ));
    }
    let torso: BTreeSet<_> = frames
        .torso_points
        .iter()
        .copied()
        .map(position_key)
        .collect();
    for tri in &skin.triangles {
        let mut points = [Vec3::ZERO; 3];
        for (slot, &index) in points.iter_mut().zip(tri) {
            *slot = *skin.vertices.get(index).ok_or_else(|| {
                Refusal::new(
                    RefusalCode::DegenerateGeometry,
                    "skin mesh triangle index out of bounds",
                )
            })?;
        }
        if points.iter().all(|p| p.z <= low) || points.iter().all(|p| p.z >= high) {
            continue;
        }
        // BodyFrames stores these same original posed vertices multiplied by 1000. Exact
        // membership keeps an arm or a generated cap from being relabelled as torso anatomy.
        if points
            .iter()
            .any(|&p| !torso.contains(&position_key(p * 1000.0)))
        {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                "torso shell input includes retained limb or unclassified vertices; supply an anatomically segmented torso tube",
            ));
        }
    }
    Ok(())
}

/// Validates `EaseProfile` finiteness and explicit opt-in for negative wearing ease.
///
/// # Errors
/// `NonFinite` or `OutOfRange`.
pub fn validate_ease_profile(ease: &EaseProfile) -> Result<(), Refusal> {
    Refusal::finite("torso_radial_mm", ease.torso_radial_mm)?;
    Refusal::finite("limb_radial_mm", ease.limb_radial_mm)?;
    Refusal::finite("min_clearance_mm", ease.min_clearance_mm)?;
    if ease.min_clearance_mm < 0.0 {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            "min_clearance_mm cannot be negative",
        ));
    }
    if (ease.torso_radial_mm < 0.0 || ease.limb_radial_mm < 0.0) && !ease.allow_negative_ease {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            "negative wearing ease requires explicit allow_negative_ease = true",
        )
        .repair(
            "set allow_negative_ease = true for intentionally stretched ribbed/fitted knitwear",
        ));
    }
    Ok(())
}

// Every incident face reuses the same intersection index and interpolation order. The second
// clipping plane may intersect an edge shortened by the first; those endpoints are shared too.
fn clip_polygon(
    polygon: &[usize],
    vertices: &mut Vec<Vec3>,
    intersections: &mut BTreeMap<(u8, usize, usize), usize>,
    plane: u8,
    z: f64,
    above: bool,
) -> Result<Vec<usize>, Refusal> {
    let mut result = Vec::with_capacity(polygon.len() + 1);
    let Some(&last) = polygon.last() else {
        return Ok(result);
    };
    let inside = |p: Vec3| if above { p.z >= z } else { p.z <= z };
    let mut previous = last;
    for &current in polygon {
        let previous_inside = inside(vertices[previous]);
        let current_inside = inside(vertices[current]);
        if previous_inside != current_inside {
            let a = previous.min(current);
            let b = previous.max(current);
            let crossing = if vertices[a].z == z {
                a
            } else if vertices[b].z == z {
                b
            } else if let Some(&index) = intersections.get(&(plane, a, b)) {
                index
            } else {
                if vertices.len() == MAX_SHELL_VERTICES {
                    return Err(Refusal::new(
                        RefusalCode::BudgetExceeded,
                        "plane clipping exceeds the shell vertex work bound",
                    ));
                }
                let p = vertices[a];
                let q = vertices[b];
                let denominator = q.z - p.z;
                let t = (z - p.z) / denominator;
                if !denominator.is_finite() || !t.is_finite() || !(0.0..=1.0).contains(&t) {
                    return Err(Refusal::new(
                        RefusalCode::NonFinite,
                        "a shell clipping intersection is not representable",
                    ));
                }
                let point = Vec3::new((1.0 - t) * p.x + t * q.x, (1.0 - t) * p.y + t * q.y, z);
                if !point.is_finite() {
                    return Err(Refusal::new(
                        RefusalCode::NonFinite,
                        "shell clipping produced a non-finite intersection",
                    ));
                }
                let index = vertices.len();
                vertices.push(point);
                intersections.insert((plane, a, b), index);
                index
            };
            if result.last() != Some(&crossing) {
                result.push(crossing);
            }
        }
        if current_inside && result.last() != Some(&current) {
            result.push(current);
        }
        previous = current;
    }
    if result.len() > 1 && result.first() == result.last() {
        result.pop();
    }
    Ok(result)
}

/// Clips a supplied tube mesh to `[z_low_m, z_high_m]` without adding caps, applies the
/// requested normal offset, and extracts its two ordered open boundary rings.
///
/// Straddling faces are split conformingly; faces wholly on a clipping plane are omitted.
/// The retained surface must be connected with exactly one boundary on each clipping plane.
/// Branched anatomy and pre-existing openings inside the slab require a separate region/ring
/// specification. At the cut rings the offset normal is projected into the plane before
/// normalization, preserving both the authored heights and requested displacement magnitude.
/// Nonnegative ease smaller than `min_clearance_m` refuses rather than increasing the offset.
///
/// # Errors
/// `DegenerateGeometry` for a missing/ambiguous boundary, disconnected or inconsistently wound
/// geometry, a non-manifold vertex, a handle, self-intersection or an inverted/collapsed offset
/// triangle. Inward winding returns `InwardWound`. Non-representable arithmetic refuses;
/// inward ease beyond the conservative local edge guard returns `OutOfRange`. Mesh growth and
/// exact intersection work have explicit bounds and return `BudgetExceeded` on exhaustion.
pub fn build_ease_shell_from_mesh(
    vertices: &[Vec3],
    triangles: &[[usize; 3]],
    z_low_m: f64,
    z_high_m: f64,
    radial_offset_m: f64,
    min_clearance_m: f64,
    source_body_sha256: &str,
) -> Result<EaseShellMesh, Refusal> {
    Refusal::finite("z_low_m", z_low_m)?;
    Refusal::finite("z_high_m", z_high_m)?;
    Refusal::finite("radial_offset_m", radial_offset_m)?;
    Refusal::finite("min_clearance_m", min_clearance_m)?;
    if min_clearance_m < 0.0 {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            "min_clearance_m cannot be negative",
        ));
    }
    if radial_offset_m >= 0.0 && radial_offset_m < min_clearance_m {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            "requested radial ease is below min_clearance_m; the offset will not be increased silently",
        ));
    }
    if z_high_m <= z_low_m {
        return Err(Refusal::new(
            RefusalCode::DegenerateGeometry,
            format!("clipping slab [{z_low_m}, {z_high_m}] m is empty"),
        ));
    }
    if vertices.len() > MAX_SHELL_VERTICES || triangles.len() > MAX_SHELL_TRIANGLES {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "knit shell exceeds the vertex or triangle work bound",
        ));
    }
    validate_mesh(vertices, triangles)?;
    let mut clipped_vertices = vertices.to_vec();
    let mut intersections = BTreeMap::new();
    let mut kept_tris = Vec::new();
    for &[a, b, c] in triangles {
        if a >= vertices.len() || b >= vertices.len() || c >= vertices.len() {
            return Err(Refusal::new(
                RefusalCode::DegenerateGeometry,
                "skin mesh triangle index out of bounds",
            ));
        }
        let corners = [vertices[a], vertices[b], vertices[c]];
        if corners.iter().all(|p| p.z <= z_low_m) || corners.iter().all(|p| p.z >= z_high_m) {
            continue;
        }
        let lower = clip_polygon(
            &[a, b, c],
            &mut clipped_vertices,
            &mut intersections,
            0,
            z_low_m,
            true,
        )?;
        let polygon = clip_polygon(
            &lower,
            &mut clipped_vertices,
            &mut intersections,
            1,
            z_high_m,
            false,
        )?;
        // Sutherland-Hodgman clipping keeps this polygon convex and retains face winding.
        for i in 1..polygon.len().saturating_sub(1) {
            if kept_tris.len() == MAX_SHELL_TRIANGLES {
                return Err(Refusal::new(
                    RefusalCode::BudgetExceeded,
                    "plane clipping exceeds the shell triangle work bound",
                ));
            }
            kept_tris.push([polygon[0], polygon[i], polygon[i + 1]]);
        }
    }
    if kept_tris.len() < 4 {
        return Err(Refusal::new(
            RefusalCode::DegenerateGeometry,
            "plane clipping left fewer than 4 triangles",
        ));
    }

    let mut remap = BTreeMap::new();
    let mut compact_verts = Vec::new();
    let mut compact_tris = Vec::with_capacity(kept_tris.len());
    for tri in kept_tris {
        let mut indices = [0u32; 3];
        for (slot, original) in indices.iter_mut().zip(tri) {
            let index = if let Some(&index) = remap.get(&original) {
                index
            } else {
                let index = u32::try_from(compact_verts.len()).map_err(|_| {
                    Refusal::new(
                        RefusalCode::BudgetExceeded,
                        "shell vertex count exceeds u32",
                    )
                })?;
                compact_verts.push(clipped_vertices[original]);
                remap.insert(original, index);
                index
            };
            *slot = index;
        }
        compact_tris.push(indices);
    }
    audit_open_shell(&compact_verts, &compact_tris)?;

    let n = compact_verts.len();
    let mut normals = vec![Vec3::ZERO; n];
    let mut min_edge = vec![f64::INFINITY; n];
    for &[ia, ib, ic] in &compact_tris {
        let (a, b, c) = (ia as usize, ib as usize, ic as usize);
        let va = compact_verts[a];
        let vb = compact_verts[b];
        let vc = compact_verts[c];
        let n_tri = (vb - va).cross(vc - va);
        if !n_tri.norm2().is_finite() {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "clipped shell triangle area overflows",
            ));
        }
        if n_tri.norm2() == 0.0 {
            return Err(Refusal::new(
                RefusalCode::DegenerateGeometry,
                "clipped shell contains a collapsed triangle",
            ));
        }
        normals[a] += n_tri;
        normals[b] += n_tri;
        normals[c] += n_tri;
        let lab = (vb - va).norm();
        let lbc = (vc - vb).norm();
        let lca = (va - vc).norm();
        min_edge[a] = min_edge[a].min(lab).min(lca);
        min_edge[b] = min_edge[b].min(lab).min(lbc);
        min_edge[c] = min_edge[c].min(lbc).min(lca);
    }

    let effective_offset = radial_offset_m;
    let mut offset_verts = Vec::with_capacity(n);
    for i in 0..n {
        if compact_verts[i].z == z_low_m || compact_verts[i].z == z_high_m {
            normals[i].z = 0.0;
        }
        let nn = normals[i].norm();
        if !nn.is_finite() {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "shell offset normal overflows",
            ));
        }
        if nn == 0.0 && effective_offset != 0.0 {
            return Err(Refusal::new(
                RefusalCode::DegenerateGeometry,
                "shell vertex has no usable normal for the requested offset",
            ));
        }
        let dir = if nn > 0.0 {
            normals[i] / nn
        } else {
            Vec3::ZERO
        };
        // This retained local edge threshold is a conservative admission guard, not a
        // self-collision certificate. Never silently change the requested negative ease.
        if effective_offset < 0.0
            && min_edge[i].is_finite()
            && effective_offset < -0.35 * min_edge[i]
        {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!(
                    "requested inward ease {} m exceeds the conservative local limit {} m at shell vertex {i}",
                    -effective_offset,
                    0.35 * min_edge[i],
                ),
            )
            .repair("Reduce the negative radial ease, or supply a shell whose local edges support the requested offset."));
        }
        let p = compact_verts[i] + dir * effective_offset;
        if !p.is_finite() {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "shell offset position overflows",
            ));
        }
        offset_verts.push(p);
    }

    for &[ia, ib, ic] in &compact_tris {
        let (a, b, c) = (ia as usize, ib as usize, ic as usize);
        let n_old =
            (compact_verts[b] - compact_verts[a]).cross(compact_verts[c] - compact_verts[a]);
        let n_new = (offset_verts[b] - offset_verts[a]).cross(offset_verts[c] - offset_verts[a]);
        let area2 = n_new.norm2();
        let orientation = n_old.dot(n_new);
        if !area2.is_finite() || !orientation.is_finite() {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "offset shell triangle geometry overflows",
            ));
        }
        if area2 == 0.0 || orientation <= 0.0 {
            return Err(Refusal::new(
                RefusalCode::DegenerateGeometry,
                "ease offset inverted or collapsed a shell triangle; reduce inward/excessive ease",
            ));
        }
    }

    let mut seen_half_edges = BTreeMap::new();
    let mut directed_edges = BTreeSet::new();
    let mut adjacent = vec![Vec::new(); compact_tris.len()];
    for (face, &[a, b, c]) in compact_tris.iter().enumerate() {
        for (u, v) in [(a, b), (b, c), (c, a)] {
            if seen_half_edges.insert((u, v), face).is_some() {
                return Err(Refusal::new(
                    RefusalCode::DegenerateGeometry,
                    format!(
                        "non-manifold or inconsistently wound mesh shares directed half-edge ({u}, {v})"
                    ),
                ));
            }
            if let Some(&other) = seen_half_edges.get(&(v, u)) {
                adjacent[face].push(other);
                adjacent[other].push(face);
            }
            if !directed_edges.remove(&(v, u)) {
                directed_edges.insert((u, v));
            }
        }
    }
    let mut reached = vec![false; compact_tris.len()];
    let mut pending = vec![0usize];
    reached[0] = true;
    while let Some(face) = pending.pop() {
        for &other in &adjacent[face] {
            if !reached[other] {
                reached[other] = true;
                pending.push(other);
            }
        }
    }
    if reached.iter().any(|&seen| !seen) {
        return Err(Refusal::new(
            RefusalCode::DegenerateGeometry,
            "clipped shell has disconnected surface components",
        ));
    }

    let mut next_map = BTreeMap::new();
    let mut incoming = BTreeSet::new();
    for &(u, v) in &directed_edges {
        if next_map.insert(u, v).is_some() || !incoming.insert(v) {
            return Err(Refusal::new(
                RefusalCode::DegenerateGeometry,
                "clipped shell has a branching or pinched boundary",
            ));
        }
    }
    validate_vertex_links(n, &compact_tris, &next_map)?;

    let mut visited = BTreeSet::new();
    let mut boundary_rings = Vec::new();
    for &start in next_map.keys() {
        if visited.contains(&start) {
            continue;
        }
        let mut ring = Vec::new();
        let mut cur = start;
        loop {
            if !visited.insert(cur) {
                return Err(Refusal::new(
                    RefusalCode::DegenerateGeometry,
                    "clipped shell boundary does not close as a simple loop",
                ));
            }
            ring.push(cur);
            cur = *next_map.get(&cur).ok_or_else(|| {
                Refusal::new(
                    RefusalCode::DegenerateGeometry,
                    "clipped shell has an open boundary chain",
                )
            })?;
            if cur == start {
                break;
            }
        }
        if ring.len() < 3 {
            return Err(Refusal::new(
                RefusalCode::DegenerateGeometry,
                "clipped shell boundary has fewer than 3 vertices",
            ));
        }
        let is_start = ring.iter().all(|&v| compact_verts[v as usize].z == z_low_m);
        let is_end = ring
            .iter()
            .all(|&v| compact_verts[v as usize].z == z_high_m);
        if !is_start && !is_end {
            return Err(Refusal::new(
                RefusalCode::DegenerateGeometry,
                "shell has an opening away from its clipping planes; declare anatomical rings before shaping",
            ));
        }
        let origin = compact_verts[ring[0] as usize];
        let oriented_area = ring
            .iter()
            .zip(ring.iter().cycle().skip(1))
            .map(|(&a, &b)| {
                (compact_verts[a as usize] - origin)
                    .cross(compact_verts[b as usize] - origin)
                    .z
            })
            .sum::<f64>();
        Refusal::finite("cut ring oriented area", oriented_area)?;
        if oriented_area == 0.0 {
            return Err(Refusal::new(
                RefusalCode::DegenerateGeometry,
                "cut-plane ring has zero enclosed area",
            ));
        }
        if (is_start && oriented_area < 0.0) || (is_end && oriented_area > 0.0) {
            return Err(Refusal::new(
                RefusalCode::InwardWound,
                "tube shell is wound inward",
            ));
        }
        let mut perimeter_m = 0.0;
        for i in 0..ring.len() {
            let p = offset_verts[ring[i] as usize];
            let q = offset_verts[ring[(i + 1) % ring.len()] as usize];
            perimeter_m += (q - p).norm();
        }
        Refusal::finite("shell boundary perimeter", perimeter_m)?;
        if perimeter_m <= 0.0 {
            return Err(Refusal::new(
                RefusalCode::DegenerateGeometry,
                "shell boundary has zero perimeter",
            ));
        }
        boundary_rings.push(BoundaryRing {
            name: if is_start { "start_ring_0" } else { "top_ring" }.into(),
            is_start,
            vertices: ring,
            perimeter_m,
        });
    }
    if boundary_rings.len() != 2 || boundary_rings.iter().filter(|r| r.is_start).count() != 1 {
        return Err(Refusal::new(
            RefusalCode::DegenerateGeometry,
            "a tube shell requires exactly one lower and one upper boundary ring",
        ));
    }
    let edge_count = (seen_half_edges.len() + directed_edges.len()) / 2;
    if compact_verts.len() + compact_tris.len() != edge_count {
        return Err(Refusal::new(
            RefusalCode::DegenerateGeometry,
            "a tube shell must be an annulus without handles",
        ));
    }
    if effective_offset != 0.0 {
        audit_open_shell(&offset_verts, &compact_tris)?;
    }
    boundary_rings.sort_by_key(|ring| !ring.is_start);

    Ok(EaseShellMesh {
        vertices_m: offset_verts.into_iter().map(|v| [v.x, v.y, v.z]).collect(),
        triangles: compact_tris,
        boundary_rings,
        source_body_sha256: source_body_sha256.to_string(),
    })
}

fn validate_vertex_links(
    vertex_count: usize,
    triangles: &[[u32; 3]],
    boundary_next: &BTreeMap<u32, u32>,
) -> Result<(), Refusal> {
    let bad = |message| Refusal::new(RefusalCode::DegenerateGeometry, message);
    let mut links = vec![Vec::new(); vertex_count];
    for &[a, b, c] in triangles {
        links[a as usize].push((b, c));
        links[b as usize].push((c, a));
        links[c as usize].push((a, b));
    }
    // Edge incidence alone cannot exclude two surface fans pinched at one vertex.
    // Every link must be one cycle (interior) or one path (boundary).
    for (v, edges) in links.iter().enumerate() {
        let mut neighbors: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        for &(a, b) in edges {
            neighbors.entry(a).or_default().push(b);
            neighbors.entry(b).or_default().push(a);
        }
        let degree_one = neighbors.values().filter(|n| n.len() == 1).count();
        let expected = if boundary_next.contains_key(&(v as u32)) {
            2
        } else {
            0
        };
        if neighbors.is_empty()
            || neighbors.values().any(|n| n.is_empty() || n.len() > 2)
            || degree_one != expected
        {
            return Err(bad("shell has a non-manifold vertex link"));
        }
        let mut reached = BTreeSet::new();
        let mut pending = vec![*neighbors.keys().next().expect("nonempty vertex link")];
        while let Some(neighbor) = pending.pop() {
            if reached.insert(neighbor) {
                pending.extend(&neighbors[&neighbor]);
            }
        }
        if reached.len() != neighbors.len() {
            return Err(bad("shell has disconnected surface fans at one vertex"));
        }
    }
    Ok(())
}

fn audit_open_shell(vertices: &[Vec3], triangles: &[[u32; 3]]) -> Result<(), Refusal> {
    // The canonical BVH/predicate audit reads only positions and triangle indices; the
    // closed-body report is not consulted or published for this open surface.
    let mesh = SkinMesh {
        vertices: vertices.to_vec(),
        triangles: triangles.iter().map(|t| t.map(|v| v as usize)).collect(),
        source_vertex: Vec::new(),
        trim_roots: Vec::new(),
        report: SkinReport {
            shells_dropped: 0,
            triangles_dropped: 0,
            volume: 0.0,
            area: 0.0,
            min_triangle_area: 0.0,
        },
    };
    let audit = mesh.audit_self_intersections(DEFAULT_POSE_AUDIT_PAIR_BUDGET)?;
    if let Some(pair) = audit.first_intersecting_pair {
        return Err(Refusal::new(
            RefusalCode::DegenerateGeometry,
            format!("knit shell triangles {} and {} intersect", pair[0], pair[1]),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::landmarks::Frame;
    use ftx_core::det;

    fn square_tube(levels: &[f64]) -> (Vec<Vec3>, Vec<[usize; 3]>) {
        let mut vertices = Vec::new();
        for &z in levels {
            for (x, y) in [(-0.1, -0.1), (0.1, -0.1), (0.1, 0.1), (-0.1, 0.1)] {
                vertices.push(Vec3::new(x, y, z));
            }
        }
        let mut faces = Vec::new();
        for layer in 0..levels.len() - 1 {
            for side in 0..4 {
                let a = 4 * layer + side;
                let b = 4 * layer + (side + 1) % 4;
                faces.push([a, b, b + 4]);
                faces.push([a, b + 4, a + 4]);
            }
        }
        (vertices, faces)
    }

    fn area(shell: &EaseShellMesh) -> f64 {
        let vertices: Vec<_> = shell
            .vertices_m
            .iter()
            .map(|p| Vec3::new(p[0], p[1], p[2]))
            .collect();
        shell
            .triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| vertices[i as usize]);
                0.5 * (b - a).cross(c - a).norm()
            })
            .sum()
    }

    #[test]
    fn offset_preserves_declared_displacement_and_cut_planes_on_a_taper() {
        let (mut vertices, triangles) = square_tube(&[0.0, 1.0]);
        for p in &mut vertices {
            p.x *= 1.0 + p.z;
            p.y *= 1.0 + p.z;
        }
        let source = build_ease_shell_from_mesh(
            &vertices,
            &triangles,
            0.25,
            0.75,
            0.0,
            0.0,
            "synthetic-taper",
        )
        .unwrap();
        for offset in [0.01, -0.001] {
            let shell = build_ease_shell_from_mesh(
                &vertices,
                &triangles,
                0.25,
                0.75,
                offset,
                0.0,
                "synthetic-taper",
            )
            .unwrap();
            assert_eq!(source.triangles, shell.triangles);
            for (p, q) in source.vertices_m.iter().zip(&shell.vertices_m) {
                let displacement = Vec3::new(p[0] - q[0], p[1] - q[1], p[2] - q[2]).norm();
                assert!((displacement - offset.abs()).abs() < 1e-14);
            }
            for (ring, height) in shell.boundary_rings.iter().zip([0.25, 0.75]) {
                assert!(
                    ring.vertices
                        .iter()
                        .all(|&v| shell.vertices_m[v as usize][2] == height)
                );
            }
        }
        let refusal = build_ease_shell_from_mesh(
            &vertices,
            &triangles,
            0.25,
            0.75,
            0.001,
            0.002,
            "synthetic-taper",
        )
        .unwrap_err();
        assert_eq!(refusal.code, RefusalCode::OutOfRange);
        assert!(refusal.message.contains("will not be increased"));
    }

    #[test]
    fn exact_audit_refuses_crossed_surfaces_and_ring_winding_refuses_inward_tubes() {
        let (vertices, triangles) = square_tube(&[0.0, 0.5, 1.0]);
        let inward: Vec<_> = triangles.iter().map(|&[a, b, c]| [a, c, b]).collect();
        assert_eq!(
            build_ease_shell_from_mesh(&vertices, &inward, 0.01, 0.99, 0.0, 0.0, "inward")
                .unwrap_err()
                .code,
            RefusalCode::InwardWound,
        );
        let mut crossed = vertices;
        for p in &mut crossed[4..8] {
            p.x = -p.x;
            p.y = -p.y;
        }
        let refusal =
            build_ease_shell_from_mesh(&crossed, &triangles, 0.01, 0.99, 0.0, 0.0, "crossed")
                .unwrap_err();
        assert_eq!(refusal.code, RefusalCode::DegenerateGeometry);
        assert!(refusal.message.contains("intersect"));
    }

    #[test]
    fn interior_vertex_must_have_one_surface_fan() {
        // Two closed tetrahedral surface fans meet only at vertex 0. Every edge still has
        // exactly two incident faces; the local surface at their shared vertex is not a disk.
        let triangles = [
            [0, 1, 2],
            [0, 3, 1],
            [0, 2, 3],
            [1, 3, 2],
            [0, 4, 5],
            [0, 6, 4],
            [0, 5, 6],
            [4, 6, 5],
        ];
        let refusal = validate_vertex_links(7, &triangles, &BTreeMap::new()).unwrap_err();
        assert_eq!(refusal.code, RefusalCode::DegenerateGeometry);
        assert!(refusal.message.contains("disconnected surface fans"));
    }

    #[test]
    fn straddling_faces_share_exact_plane_boundaries_and_preserve_area() {
        let (vertices, triangles) = square_tube(&[0.0, 1.0]);
        let shell =
            build_ease_shell_from_mesh(&vertices, &triangles, 0.2, 0.8, 0.0, 0.0, "synthetic")
                .unwrap();
        assert_eq!(shell.boundary_rings.len(), 2);
        assert_eq!(shell.vertices_m.len(), 16);
        assert_eq!(shell.triangles.len(), 16);
        assert!(shell.vertices_m.iter().all(|p| p[2] == 0.2 || p[2] == 0.8));
        assert!((area(&shell) - 4.0 * 0.2 * 0.6).abs() < 1e-14);
        for (ring, z) in shell.boundary_rings.iter().zip([0.2, 0.8]) {
            assert!(
                ring.vertices
                    .iter()
                    .all(|&v| shell.vertices_m[v as usize][2] == z)
            );
            assert!((ring.perimeter_m - 0.8).abs() < 1e-14);
        }
        // Closed rings, one edge-connected surface and the exact source side area jointly check
        // the shared diagonal/vertical crossings; separate per-face duplicates cannot pass.
        let again =
            build_ease_shell_from_mesh(&vertices, &triangles, 0.2, 0.8, 0.0, 0.0, "synthetic")
                .unwrap();
        assert_eq!(shell, again);
    }

    #[test]
    fn clipping_at_existing_vertices_has_no_duplicate_or_collapsed_faces() {
        let (vertices, triangles) = square_tube(&[0.0, 0.2, 0.8, 1.0]);
        let shell =
            build_ease_shell_from_mesh(&vertices, &triangles, 0.2, 0.8, 0.0, 0.0, "synthetic")
                .unwrap();
        assert_eq!(shell.vertices_m.len(), 8);
        assert_eq!(shell.triangles.len(), 8);
        assert!((area(&shell) - 0.48).abs() < 1e-14);
    }

    #[test]
    fn tapered_shell_clipping_retains_the_analytic_cross_sections() {
        let (mut vertices, triangles) = square_tube(&[0.0, 1.0]);
        for p in &mut vertices {
            p.x *= 1.0 + p.z;
            p.y *= 1.0 + p.z;
        }
        let shell =
            build_ease_shell_from_mesh(&vertices, &triangles, 0.25, 0.75, 0.0, 0.0, "synthetic")
                .unwrap();
        assert!((shell.boundary_rings[0].perimeter_m - 1.0).abs() < 1e-14);
        assert!((shell.boundary_rings[1].perimeter_m - 1.4).abs() < 1e-14);
        let expected = 4.0 * 0.3 * (0.5_f64 * 0.5 + 0.05 * 0.05).sqrt();
        assert!((area(&shell) - expected).abs() < 1e-14);
    }

    #[test]
    fn inward_ease_is_applied_exactly_or_refused() {
        let (vertices, triangles) = square_tube(&[0.0, 1.0]);
        let original =
            build_ease_shell_from_mesh(&vertices, &triangles, 0.2, 0.8, 0.0, 0.0, "synthetic")
                .unwrap();
        let inward =
            build_ease_shell_from_mesh(&vertices, &triangles, 0.2, 0.8, -0.001, 0.0, "synthetic")
                .unwrap();
        assert_eq!(original.triangles, inward.triangles);
        for (p, q) in original.vertices_m.iter().zip(&inward.vertices_m) {
            let displacement = Vec3::new(p[0] - q[0], p[1] - q[1], p[2] - q[2]).norm();
            assert!((displacement - 0.001).abs() < 1e-14);
        }
        let error =
            build_ease_shell_from_mesh(&vertices, &triangles, 0.2, 0.8, -0.1, 0.0, "synthetic")
                .unwrap_err();
        assert_eq!(error.code, RefusalCode::OutOfRange);
        assert!(error.message.contains("inward ease"));
    }

    #[test]
    fn disconnected_or_undeclared_open_regions_refuse() {
        let (mut vertices, mut triangles) = square_tube(&[0.0, 1.0]);
        let (other, faces) = square_tube(&[0.0, 1.0]);
        let base = vertices.len();
        vertices.extend(other.into_iter().map(|p| p + Vec3::new(1.0, 0.0, 0.0)));
        triangles.extend(faces.into_iter().map(|t| t.map(|i| i + base)));
        assert_eq!(
            build_ease_shell_from_mesh(&vertices, &triangles, 0.2, 0.8, 0.0, 0.0, "synthetic")
                .unwrap_err()
                .code,
            RefusalCode::DegenerateGeometry,
        );
        let (vertices, triangles) = square_tube(&[0.3, 0.7]);
        assert_eq!(
            build_ease_shell_from_mesh(&vertices, &triangles, 0.2, 0.8, 0.0, 0.0, "synthetic")
                .unwrap_err()
                .code,
            RefusalCode::DegenerateGeometry,
        );
    }

    fn frames() -> BodyFrames {
        let axis = Frame {
            origin: Vec3::ZERO,
            up: Vec3::new(0.0, 0.0, 1.0),
            front: Vec3::new(0.0, -1.0, 0.0),
            lat: Vec3::new(1.0, 0.0, 0.0),
        };
        BodyFrames {
            torso: axis,
            arm_r: axis,
            arm_l: axis,
            leg_r: axis,
            leg_l: axis,
            floor_z: -500.0,
            tip_h: 0.0,
            tip_h_l: 0.0,
            upper_arm_radius: 0.0,
            upper_arm_radius_l: 0.0,
            arm_length: 0.0,
            arm_length_l: 0.0,
            landmarks: BTreeMap::new(),
            neck: axis,
            head_bone: Vec3::ZERO,
            shoulder_joint_r: Vec3::ZERO,
            torso_points: Vec::new(),
            leg_points: Vec::new(),
            leg_points_l: Vec::new(),
            leg_landmarks_l: [0.0; 3],
        }
    }

    #[test]
    fn torso_landmarks_are_required_and_use_the_actual_floor() {
        let mut frames = frames();
        assert_eq!(
            landmark_height_m(&frames, "waist").unwrap_err().code,
            RefusalCode::InvalidDocument
        );
        frames.landmarks.insert("waist".into(), 700.0);
        assert_eq!(landmark_height_m(&frames, "waist").unwrap(), 0.2);
        frames.landmarks.insert("waist".into(), f64::NAN);
        assert_eq!(
            landmark_height_m(&frames, "waist").unwrap_err().code,
            RefusalCode::NonFinite
        );
        frames.landmarks.insert("waist".into(), 700.0);
        frames.floor_z = f64::INFINITY;
        assert_eq!(
            landmark_height_m(&frames, "waist").unwrap_err().code,
            RefusalCode::NonFinite
        );
    }

    #[test]
    fn adapter_requires_segmented_torso_and_refuses_invented_limb_regions() {
        let (vertices, mut triangles) = square_tube(&[0.0, 1.0]);
        triangles.extend([[0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7]]);
        let faces: Vec<[u32; 3]> = triangles
            .iter()
            .map(|triangle| triangle.map(|index| u32::try_from(index).unwrap()))
            .collect();
        let skin = crate::drape::skin_mesh(&vertices, &faces).unwrap();
        let mut frames = frames();
        frames.landmarks.insert("waist".into(), 700.0);
        frames.landmarks.insert("snp".into(), 1300.0);
        frames.torso_points = skin.vertices.iter().map(|p| *p * 1000.0).collect();
        let ease = EaseProfile {
            torso_radial_mm: 0.0,
            limb_radial_mm: 0.0,
            min_clearance_mm: 0.0,
            allow_negative_ease: false,
        };
        {
            let adapter = AnnyKnitShell {
                skin: &skin,
                frames: &frames,
                source_body_sha256: "synthetic".into(),
            };
            let torso = adapter
                .build_ease_shell(KnitShellRegion::Torso, &ease)
                .unwrap();
            assert_eq!(torso.boundary_rings.len(), 2);
            assert!((area(&torso) - 0.48).abs() < 1e-14);
            assert!(
                adapter
                    .build_ease_shell(KnitShellRegion::Tube, &ease)
                    .is_ok()
            );
            for region in [
                KnitShellRegion::LeftSleeve,
                KnitShellRegion::RightSleeve,
                KnitShellRegion::FullUpperBody,
            ] {
                assert_eq!(
                    adapter.build_ease_shell(region, &ease).unwrap_err().code,
                    RefusalCode::InvalidDocument,
                );
            }
        }
        frames.torso_points.pop();
        let adapter = AnnyKnitShell {
            skin: &skin,
            frames: &frames,
            source_body_sha256: "synthetic".into(),
        };
        assert_eq!(
            adapter
                .build_ease_shell(KnitShellRegion::Torso, &ease)
                .unwrap_err()
                .code,
            RefusalCode::InvalidDocument,
        );
    }

    #[test]
    fn clips_open_shell_offsets_and_extracts_two_boundary_rings() {
        let n_theta = 20usize;
        let n_z = 10usize;
        let mut verts = Vec::new();
        for iz in 0..=n_z {
            let z = iz as f64 * 0.05;
            for it in 0..n_theta {
                let ang = 2.0 * std::f64::consts::PI * it as f64 / n_theta as f64;
                verts.push(Vec3::new(0.12 * det::cos(ang), 0.12 * det::sin(ang), z));
            }
        }
        let mut tris = Vec::new();
        for iz in 0..n_z {
            for it in 0..n_theta {
                let nxt = (it + 1) % n_theta;
                let a = iz * n_theta + it;
                let b = iz * n_theta + nxt;
                let c = (iz + 1) * n_theta + nxt;
                let d = (iz + 1) * n_theta + it;
                tris.push([a, b, c]);
                tris.push([a, c, d]);
            }
        }
        let shell =
            build_ease_shell_from_mesh(&verts, &tris, 0.10, 0.40, 0.01, 0.002, "body-sha").unwrap();
        assert_eq!(shell.boundary_rings.len(), 2);
        assert!(shell.boundary_rings[0].is_start);
        assert!(!shell.boundary_rings[1].is_start);
        assert!((shell.boundary_rings[0].perimeter_m - 0.81).abs() < 0.03);

        let bad = EaseProfile {
            torso_radial_mm: -5.0,
            limb_radial_mm: 0.0,
            min_clearance_mm: 0.0,
            allow_negative_ease: false,
        };
        assert_eq!(
            validate_ease_profile(&bad).unwrap_err().code,
            RefusalCode::OutOfRange
        );

        let mut nan_verts = verts.clone();
        nan_verts[0].x = f64::NAN;
        assert_eq!(
            build_ease_shell_from_mesh(&nan_verts, &tris, 0.10, 0.40, 0.01, 0.002, "body-sha")
                .unwrap_err()
                .code,
            RefusalCode::NonFinite,
        );
        assert_eq!(
            build_ease_shell_from_mesh(&verts, &tris, 0.10, 0.40, 0.01, -0.001, "body-sha")
                .unwrap_err()
                .code,
            RefusalCode::OutOfRange,
        );

        let bowtie_verts = vec![
            Vec3::new(0.0, 0.0, 0.2),
            Vec3::new(0.1, 0.0, 0.2),
            Vec3::new(0.05, 0.1, 0.2),
            Vec3::new(-0.1, 0.0, 0.2),
            Vec3::new(-0.05, -0.1, 0.2),
            Vec3::new(0.0, 0.05, 0.25),
            Vec3::new(0.0, -0.05, 0.25),
        ];
        let bowtie_tris = vec![[0, 1, 2], [0, 3, 4], [1, 2, 5], [3, 4, 6]];
        assert_eq!(
            build_ease_shell_from_mesh(&bowtie_verts, &bowtie_tris, 0.0, 0.5, 0.0, 0.0, "body-sha")
                .unwrap_err()
                .code,
            RefusalCode::DegenerateGeometry,
        );

        let mut flipped_tris = tris.clone();
        let mid_tri = 4 * 2 * n_theta;
        flipped_tris[mid_tri] = [
            flipped_tris[mid_tri][0],
            flipped_tris[mid_tri][2],
            flipped_tris[mid_tri][1],
        ];
        assert_eq!(
            build_ease_shell_from_mesh(&verts, &flipped_tris, 0.10, 0.40, 0.01, 0.002, "body-sha")
                .unwrap_err()
                .code,
            RefusalCode::DegenerateGeometry,
        );
    }
}

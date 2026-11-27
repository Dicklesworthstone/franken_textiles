//! Open-limb Anny body shell adapter implementing Seam 1 (`ftx_core::shaping::ShapingSurface`)
//! for 3D-to-knit harmonic AutoKnit drafting (`FTX-K12`).
//!
//! Clips posed `SkinMesh` geometry along `BodyFrames` anatomical planes WITHOUT inserting end-caps,
//! offsets along area-weighted outward normals with clearance clamping and degeneracy refusal,
//! and extracts ordered open boundary rings (`t = 0` start hem/cuffs, `t = 1` end neck/top).

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::shaping::{
    BoundaryRing, EaseProfile, EaseShellMesh, KnitShellRegion, ShapingSurface,
};
use ftx_core::{Refusal, RefusalCode, Vec3};

use crate::drape::SkinMesh;
use crate::landmarks::BodyFrames;

/// Adapter exposing a posed Anny [`SkinMesh`] and [`BodyFrames`] through [`ShapingSurface`].
#[derive(Clone, Debug)]
pub struct AnnyKnitShell<'a> {
    /// Posed outward-wound body skin mesh (metres, z-up).
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
        let floor_m = self.frames.floor_z * 0.001;
        let lm_m = |name: &str, fallback_offset_m: f64| -> f64 {
            self.frames
                .landmarks
                .get(name)
                .map_or(floor_m + fallback_offset_m, |&h_mm| floor_m + h_mm * 0.001)
        };
        let z_waist = lm_m("waist", 0.55);
        let z_neck = lm_m("snp", 0.92);
        let (z_low, z_high, offset_mm) = match region {
            KnitShellRegion::Torso | KnitShellRegion::FullUpperBody => {
                (z_waist, z_neck, ease.torso_radial_mm)
            }
            KnitShellRegion::LeftSleeve | KnitShellRegion::RightSleeve => {
                let z_lo = z_waist + 0.15 * (z_neck - z_waist);
                (z_lo, z_neck, ease.limb_radial_mm)
            }
            KnitShellRegion::Tube => {
                let zmin = self
                    .skin
                    .vertices
                    .iter()
                    .map(|v| v.z)
                    .fold(f64::INFINITY, f64::min);
                let zmax = self
                    .skin
                    .vertices
                    .iter()
                    .map(|v| v.z)
                    .fold(f64::NEG_INFINITY, f64::max);
                let span = (zmax - zmin).max(0.05);
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

/// Validates [`EaseProfile`] finiteness and explicit opt-in for negative wearing ease.
///
/// # Errors
/// [`RefusalCode::NonFinite`] or [`RefusalCode::OutOfRange`].
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

/// Clips an outward-wound closed or open triangle mesh to `[z_low_m, z_high_m]` without capping,
/// applies clearance-clamped radial normal offset, and extracts ordered open boundary rings.
///
/// # Errors
/// [`RefusalCode::DegenerateGeometry`] if fewer than 2 boundary rings exist or any offset triangle collapses.
pub fn build_ease_shell_from_mesh(
    vertices: &[Vec3],
    triangles: &[[usize; 3]],
    z_low_m: f64,
    z_high_m: f64,
    radial_offset_m: f64,
    min_clearance_m: f64,
    source_body_sha256: &str,
) -> Result<EaseShellMesh, Refusal> {
    if z_high_m <= z_low_m {
        return Err(Refusal::new(
            RefusalCode::DegenerateGeometry,
            format!("clipping slab [{z_low_m}, {z_high_m}] m is empty"),
        ));
    }
    let mut kept_tris = Vec::new();
    for &[a, b, c] in triangles {
        if a >= vertices.len() || b >= vertices.len() || c >= vertices.len() {
            return Err(Refusal::new(
                RefusalCode::DegenerateGeometry,
                "skin mesh triangle index out of bounds",
            ));
        }
        let cz = (vertices[a].z + vertices[b].z + vertices[c].z) / 3.0;
        if cz >= z_low_m && cz <= z_high_m {
            kept_tris.push([a, b, c]);
        }
    }
    if kept_tris.len() < 4 {
        return Err(Refusal::new(
            RefusalCode::DegenerateGeometry,
            "anatomical plane clip left fewer than 4 triangles",
        ));
    }

    // Compact vertex numbering deterministically
    let mut remap: BTreeMap<usize, u32> = BTreeMap::new();
    let mut compact_verts: Vec<Vec3> = Vec::new();
    let mut compact_tris: Vec<[u32; 3]> = Vec::with_capacity(kept_tris.len());
    for [a, b, c] in kept_tris {
        let mut idx3 = [0u32; 3];
        for (slot, orig) in idx3.iter_mut().zip([a, b, c]) {
            let new_idx = *remap.entry(orig).or_insert_with(|| {
                let id = compact_verts.len() as u32;
                compact_verts.push(vertices[orig]);
                id
            });
            *slot = new_idx;
        }
        compact_tris.push(idx3);
    }

    // Compute area-weighted outward vertex normals and local shortest incident edge length
    let n = compact_verts.len();
    let mut normals = vec![Vec3::ZERO; n];
    let mut min_edge = vec![f64::INFINITY; n];
    for &[ia, ib, ic] in &compact_tris {
        let (a, b, c) = (ia as usize, ib as usize, ic as usize);
        let va = compact_verts[a];
        let vb = compact_verts[b];
        let vc = compact_verts[c];
        let n_tri = (vb - va).cross(vc - va);
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

    let effective_offset = if radial_offset_m >= 0.0 {
        radial_offset_m.max(min_clearance_m)
    } else {
        radial_offset_m
    };
    let mut offset_verts = Vec::with_capacity(n);
    for i in 0..n {
        let nn = normals[i].norm();
        let dir = if nn > 1e-14 {
            normals[i] / nn
        } else {
            Vec3::ZERO
        };
        // Clamp negative inward offset to <= 35% of local shortest edge to prevent fold-over
        let safe_off = if effective_offset < 0.0 && min_edge[i].is_finite() {
            effective_offset.max(-0.35 * min_edge[i])
        } else {
            effective_offset
        };
        let p = compact_verts[i] + dir * safe_off;
        offset_verts.push(p);
    }

    // Verify no triangle flipped or collapsed after offset
    for &[ia, ib, ic] in &compact_tris {
        let (a, b, c) = (ia as usize, ib as usize, ic as usize);
        let n_old =
            (compact_verts[b] - compact_verts[a]).cross(compact_verts[c] - compact_verts[a]);
        let n_new = (offset_verts[b] - offset_verts[a]).cross(offset_verts[c] - offset_verts[a]);
        if n_new.norm2() <= 1e-16 || n_old.dot(n_new) <= 0.0 {
            return Err(Refusal::new(
                RefusalCode::DegenerateGeometry,
                "ease offset inverted or collapsed a shell triangle; reduce inward/excessive ease",
            ));
        }
    }

    // Extract directed boundary half-edges (edges appearing in only one triangle)
    let mut directed_edges: BTreeSet<(u32, u32)> = BTreeSet::new();
    for &[a, b, c] in &compact_tris {
        for (u, v) in [(a, b), (b, c), (c, a)] {
            if !directed_edges.remove(&(v, u)) {
                directed_edges.insert((u, v));
            }
        }
    }
    let mut next_map: BTreeMap<u32, u32> = BTreeMap::new();
    for &(u, v) in &directed_edges {
        next_map.insert(u, v);
    }

    let mut visited: BTreeSet<u32> = BTreeSet::new();
    let mut raw_rings: Vec<(f64, Vec<u32>, f64)> = Vec::new();
    for &start in next_map.keys() {
        if visited.contains(&start) {
            continue;
        }
        let mut loop_verts = Vec::new();
        let mut cur = start;
        let mut closed = false;
        while visited.insert(cur) {
            loop_verts.push(cur);
            if let Some(&nxt) = next_map.get(&cur) {
                if nxt == start {
                    closed = true;
                    break;
                }
                cur = nxt;
            } else {
                break;
            }
        }
        if closed && loop_verts.len() >= 3 {
            let mut perim = 0.0_f64;
            let mut z_sum = 0.0_f64;
            for i in 0..loop_verts.len() {
                let p0 = offset_verts[loop_verts[i] as usize];
                let p1 = offset_verts[loop_verts[(i + 1) % loop_verts.len()] as usize];
                perim += (p1 - p0).norm();
                z_sum += p0.z;
            }
            let mean_z = z_sum / loop_verts.len() as f64;
            raw_rings.push((mean_z, loop_verts, perim));
        }
    }
    if raw_rings.len() < 2 {
        return Err(Refusal::new(
            RefusalCode::DegenerateGeometry,
            format!(
                "clipped knit shell has {} open boundary ring(s); expected >= 2 (hem and neck/top)",
                raw_rings.len()
            ),
        ));
    }

    // Sort rings ascending by mean z: highest ring is end (`t = 1`), lower rings are start (`t = 0`)
    raw_rings.sort_by(|a, b| a.0.total_cmp(&b.0));
    let n_rings = raw_rings.len();
    let boundary_rings = raw_rings
        .into_iter()
        .enumerate()
        .map(|(idx, (_mz, verts, perim))| {
            let is_start = idx + 1 < n_rings;
            BoundaryRing {
                name: if is_start {
                    format!("start_ring_{idx}")
                } else {
                    "top_ring".to_string()
                },
                is_start,
                vertices: verts,
                perimeter_m: perim,
            }
        })
        .collect();

    Ok(EaseShellMesh {
        vertices_m: offset_verts.into_iter().map(|v| [v.x, v.y, v.z]).collect(),
        triangles: compact_tris,
        boundary_rings,
        source_body_sha256: source_body_sha256.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ftx_core::det;

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
        // Offset radius 0.12 + 0.01 = 0.13m -> perimeter ~ 2*pi*0.13 = 0.8168m (polygon chord slightly less)
        assert!((shell.boundary_rings[0].perimeter_m - 0.81).abs() < 0.03);

        // Negative ease refused without opt-in
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
    }
}

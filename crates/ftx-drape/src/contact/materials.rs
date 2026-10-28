//! Feature-local contact coefficients. Broad phases use global upper envelopes; narrow phases use
//! the actual sheet's half-thickness and the explicitly supplied interface coefficient. A welded
//! junction uses the maximum adjacent half-thickness and coefficient, a conservative seam envelope,
//! not a resolved seam cross-section. Unlike-material friction is never inferred by a mixing rule.

use super::*;
use std::collections::{BTreeMap, BTreeSet};

/// Physical surface inputs for one sheet. Full sheet thickness is twice `half_thickness_m`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceMaterial {
    /// Distance from mid-surface to either side, metres.
    pub half_thickness_m: f64,
    /// This sheet against the body surface, dimensionless.
    pub friction_body: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct MaterialField {
    triangles: Vec<[usize; 3]>,
    edges: Vec<[usize; 2]>,
    triangle_ids: Vec<usize>,
    vertices: Vec<Vec<usize>>,
    edge_ids: Vec<Vec<usize>>,
    surfaces: Vec<SurfaceMaterial>,
    friction: Option<Vec<Vec<f64>>>,
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

impl MaterialField {
    fn build(
        cloth: &ClothTopology,
        n: usize,
        triangle_ids: &[usize],
        surfaces: &[SurfaceMaterial],
        friction: Option<&[Vec<f64>]>,
    ) -> Result<Self, Refusal> {
        if surfaces.is_empty()
            || triangle_ids.len() != cloth.triangles.len()
            || triangle_ids.iter().any(|&i| i >= surfaces.len())
        {
            return Err(invalid(
                "contact needs one valid surface identity per triangle",
            ));
        }
        for s in surfaces {
            if !s.half_thickness_m.is_finite()
                || s.half_thickness_m < 0.0
                || !s.friction_body.is_finite()
                || s.friction_body < 0.0
                || !(2.0 * s.half_thickness_m).is_finite()
            {
                return Err(invalid(
                    "contact surface thickness/friction must be finite and nonnegative",
                ));
            }
        }
        if let Some(f) = friction {
            let count = surfaces.len();
            if f.len() != count
                || f.iter().any(|r| r.len() != count)
                || f.iter().flatten().any(|v| !v.is_finite() || *v < 0.0)
                || (0..count).any(|i| (0..i).any(|j| f[i][j] != f[j][i]))
            {
                return Err(invalid(
                    "self-contact requires a complete symmetric nonnegative interface matrix",
                ));
            }
        }
        let mut vertices = vec![BTreeSet::new(); n];
        let mut edges: BTreeMap<[usize; 2], BTreeSet<usize>> = BTreeMap::new();
        for (tri, &id) in cloth.triangles.iter().zip(triangle_ids) {
            for &v in tri {
                vertices
                    .get_mut(v)
                    .ok_or_else(|| invalid("invalid contact vertex"))?
                    .insert(id);
            }
            for k in 0..3 {
                let (a, b) = (tri[k], tri[(k + 1) % 3]);
                edges.entry([a.min(b), a.max(b)]).or_default().insert(id);
            }
        }
        let edge_ids = cloth
            .edges
            .iter()
            .map(|e| {
                edges
                    .get(e)
                    .map(|s| s.iter().copied().collect())
                    .ok_or_else(|| invalid("contact edges differ from their triangles"))
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            triangles: cloth.triangles.clone(),
            edges: cloth.edges.clone(),
            triangle_ids: triangle_ids.to_vec(),
            surfaces: surfaces.to_vec(),
            vertices: vertices
                .into_iter()
                .map(|v| v.into_iter().collect())
                .collect(),
            edge_ids,
            friction: friction.map(<[Vec<f64>]>::to_vec),
        })
    }

    pub(super) fn validate_topology(&self, cloth: &ClothTopology, n: usize) -> Result<(), Refusal> {
        if self.vertices.len() != n
            || self.triangles != cloth.triangles
            || self.edges != cloth.edges
        {
            return Err(invalid(
                "contact topology changed; explicitly rebind surface materials",
            ));
        }
        Ok(())
    }

    fn half(&self, ids: &[usize]) -> f64 {
        ids.iter()
            .map(|&i| self.surfaces[i].half_thickness_m)
            .fold(0.0, f64::max)
    }

    pub(super) fn params(&self, pair: Pair, mut p: ContactParams) -> ContactParams {
        let (a, b): (&[usize], Option<&[usize]>) = match pair {
            Pair::VertexTri { cv, .. } => (&self.vertices[cv], None),
            Pair::TriVertex { ct, .. } => (&self.triangle_ids[ct..ct + 1], None),
            Pair::EdgeEdge { ce, .. } => (&self.edge_ids[ce], None),
            Pair::SelfVertexTri { cv, ct } => {
                (&self.vertices[cv], Some(&self.triangle_ids[ct..ct + 1]))
            }
            Pair::SelfEdgeEdge { a, b } => (&self.edge_ids[a], Some(&self.edge_ids[b])),
        };
        if let Some(b) = b {
            p.thickness = self.half(a) + self.half(b);
            // with_self_contact verifies completeness before this branch can be evaluated.
            let f = self.friction.as_ref().expect("validated interface matrix");
            p.mu = a
                .iter()
                .flat_map(|&i| b.iter().map(move |&j| f[i][j]))
                .fold(0.0, f64::max);
        } else {
            // The global value is also the existing, explicit body-offset ramp cap.
            p.thickness = p.thickness.min(self.half(a));
            p.mu = a
                .iter()
                .map(|&i| self.surfaces[i].friction_body)
                .fold(0.0, f64::max);
        }
        p
    }

    fn max_half(&self) -> f64 {
        self.surfaces
            .iter()
            .map(|s| s.half_thickness_m)
            .fold(0.0, f64::max)
    }
}

impl Contact {
    // Compare the complete source field, including inactive interfaces. Rebinding at the current
    // positions would recompute start creases and discard accepted Coulomb history unnecessarily.
    pub(crate) fn check_source_materials(
        &self,
        vertex_count: usize,
        triangle_ids: &[usize],
        surfaces: &[SurfaceMaterial],
        self_friction: Option<&[Vec<f64>]>,
    ) -> Result<(), Refusal> {
        let reference = MaterialField::build(
            &self.cloth,
            vertex_count,
            triangle_ids,
            surfaces,
            self_friction,
        )?;
        let field_matches = if surfaces.len() == 1 {
            self.surface_materials.is_none()
        } else {
            self.surface_materials.as_ref() == Some(&reference)
        };
        if !field_matches
            || self.params.thickness != reference.max_half()
            || self.params.mu != surfaces.iter().map(|s| s.friction_body).fold(0., f64::max)
            || match (self.self_params, self_friction) {
                (None, None) => false,
                (Some(p), Some(matrix)) => {
                    p.thickness != 2. * reference.max_half()
                        || p.mu != matrix.iter().flatten().copied().fold(0., f64::max)
                }
                _ => true,
            }
        {
            return Err(invalid(
                "liquid contact differs from its qualified source materials/interfaces",
            ));
        }
        self.params.validate()?;
        if let Some(p) = self.self_params {
            p.validate()?;
        }
        Ok(())
    }

    /// Reconstructing a collider must not discard the cloth's source-bound physical assignment.
    /// Install it before the new body's offset audit, so thin panels are not checked against a
    /// different panel's broad-phase envelope. Friction history is intentionally not transferred.
    pub(crate) fn retain_surface_materials(
        mut self,
        previous: Option<&Self>,
        x: &[Vec3],
    ) -> Result<Self, Refusal> {
        if let Some(field) = previous.and_then(|c| c.surface_materials.as_ref()) {
            field.validate_topology(&self.cloth, x.len())?;
            self.params.thickness = field.max_half();
            self.params.mu = field
                .surfaces
                .iter()
                .map(|s| s.friction_body)
                .fold(0.0, f64::max);
            self.surface_materials = Some(field.clone());
            self.params.validate()?;
        }
        Ok(self)
    }

    /// Bind sheet contact to the actual triangle topology and audit the full physical offsets.
    /// The returned contact owns its assignment and retains it when cloned for motion continuation.
    /// `Solver::with_contact` retains assignments when replacing a collider; constructing a fresh
    /// `Contact::new` requires an explicit bind. Changed cloth topology always requires a rebind.
    /// Global parameters become broad-phase envelopes, NOT the coefficients of every pair.
    /// At multi-material seam vertices/edges the maximum adjacent envelope is used explicitly.
    /// No friction matrix is needed for body-only contact; enabling self-contact requires one.
    ///
    /// # Errors
    /// Invalid coefficients/topology, missing interface pairs, or a physically overlapping start.
    pub fn with_surface_materials(
        mut self,
        x: &[Vec3],
        triangle_ids: &[usize],
        surfaces: &[SurfaceMaterial],
        self_friction: Option<&[Vec<f64>]>,
    ) -> Result<Self, Refusal> {
        self.check_cloth(x)?;
        let field =
            MaterialField::build(&self.cloth, x.len(), triangle_ids, surfaces, self_friction)?;
        self.params.thickness = field.max_half();
        self.params.mu = surfaces.iter().map(|s| s.friction_body).fold(0.0, f64::max);
        self.surface_materials = Some(field);
        self.params.validate()?;
        self.lag.clear();
        self.lag_x.clear();
        self.creases.clear();
        if let Some(p) = self.self_params {
            let p = self.material_self_params(p)?;
            self.self_params = Some(p);
            self.creases = self.find_start_creases(x, &p);
        }
        if self.first_intersection(x).is_some()
            || self
                .min_gap(x, self.params.dhat)
                .is_some_and(|g| !g.is_finite() || g <= 0.0)
        {
            return Err(Refusal::new(
                RefusalCode::IntersectingStart,
                "material-dependent cloth/body start overlaps",
            ));
        }
        self.audit_self_contact(x)?;
        Ok(self)
    }

    pub(super) fn material_self_params(
        &self,
        mut p: ContactParams,
    ) -> Result<ContactParams, Refusal> {
        if let Some(f) = &self.surface_materials {
            let mu = f.friction.as_ref().ok_or_else(|| invalid("unlike-material self-contact needs explicit interface friction; no mixing rule is assumed"))?;
            p.thickness = 2.0 * f.max_half();
            p.mu = mu.iter().flatten().copied().fold(0.0, f64::max);
        }
        p.validate()?;
        Ok(p)
    }

    /// Whether contact uses immutable per-triangle surface assignments.
    #[must_use]
    pub fn has_surface_materials(&self) -> bool {
        self.surface_materials.is_some()
    }
}

#[cfg(test)]
mod tests;

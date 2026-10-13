//! Swept prescribed-body queries for plan §9.5/§11.4. Reuses the pinned frankensim fs-query
//! hierarchy in `bvh.rs` and the existing C-IPC additive CCD; no endpoint-only collision test.

use super::*;

/// A conservative fraction of the simultaneous linear cloth/body trajectories.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MotionBound {
    /// Fraction in [0, 1]. A value below one is NOT permission to take the full motion.
    pub fraction: f64,
    /// Actual overlapping feature pairs examined, including enabled self-contact pairs.
    pub pairs_tested: usize,
}

fn bounds(ids: &[usize], start: &[Vec3], end: &[Vec3]) -> (Vec3, Vec3) {
    aabb(
        &ids.iter()
            .flat_map(|&i| [start[i], end[i]])
            .collect::<Vec<_>>(),
    )
}

// Round outward after padding: a boundary touch must survive the broad phase.
fn grow((lo, hi): (Vec3, Vec3), pad: f64) -> Result<(Vec3, Vec3), Refusal> {
    let lo = Vec3::new(
        (lo.x - pad).next_down(),
        (lo.y - pad).next_down(),
        (lo.z - pad).next_down(),
    );
    let hi = Vec3::new(
        (hi.x + pad).next_up(),
        (hi.y + pad).next_up(),
        (hi.z + pad).next_up(),
    );
    if !lo.is_finite() || !hi.is_finite() {
        return Err(Refusal::new(
            RefusalCode::NonFinite,
            "motion bounds overflow",
        ));
    }
    Ok((lo, hi))
}

/// Visit candidates incrementally; storage is linear in the meshes, not their Cartesian product.
fn visit(
    left: &[(Vec3, Vec3)],
    right: &[(Vec3, Vec3)],
    pad: f64,
    mut pair: impl FnMut(usize, usize) -> Result<(), Refusal>,
) -> Result<(), Refusal> {
    let tree = QueryBvh::build(right);
    let mut found = Vec::new();
    for (i, &b) in left.iter().enumerate() {
        let b = grow(b, pad)?;
        tree.query(b, &mut found);
        for &j in &found {
            if overlaps(right[j], b.0, b.1) {
                pair(i, j)?;
            }
        }
    }
    Ok(())
}

/// The body's contribution to each of a contact pair's four trajectories.
pub(super) fn body_delta(pair: Pair, start: &[Vec3], end: &[Vec3], body: &Body) -> [Vec3; 4] {
    let d = |i: usize| end[i] - start[i];
    match pair {
        Pair::VertexTri { bt, .. } => {
            let t = body.triangles[bt];
            [Vec3::ZERO, d(t[0]), d(t[1]), d(t[2])]
        }
        Pair::TriVertex { bv, .. } => [d(bv), Vec3::ZERO, Vec3::ZERO, Vec3::ZERO],
        Pair::EdgeEdge { be, .. } => {
            let e = body.edges[be];
            [Vec3::ZERO, Vec3::ZERO, d(e[0]), d(e[1])]
        }
        _ => [Vec3::ZERO; 4],
    }
}

impl Contact {
    /// CCD for simultaneous *linear* motion from `cloth_start` and the current body to the
    /// supplied endpoints, with unchanged topology. Covers all three cloth/body feature families
    /// and enabled cloth self-contact (including the existing crease offsets).
    ///
    /// The fraction is conservative, not a time of impact. It protects separation, not body
    /// self-intersection or element inversion. Callers must separately admit their prescribed
    /// body path; a sequence of rotations is not the straight chord between its endpoint poses.
    /// No state or friction history is changed. Uses the existing frankensim-derived BVH.
    ///
    /// # Errors
    /// Invalid geometry/topology or non-finite arithmetic; an initially non-positive pair gap;
    /// `BudgetExceeded` rather than dropping candidates when `max_pairs` is exceeded.
    pub fn motion_bound(
        &self,
        cloth_start: &[Vec3],
        cloth_end: &[Vec3],
        body_end: &[Vec3],
        max_pairs: usize,
    ) -> Result<MotionBound, Refusal> {
        self.params.validate()?;
        if let Some(p) = self.self_params {
            p.validate()?;
        }
        if max_pairs == 0
            || cloth_start.len() != cloth_end.len()
            || self.body.vertices.len() != body_end.len()
            || self.body.edges != edges_of(&self.body.triangles)
            || self.cloth.edges != edges_of(&self.cloth.triangles)
            || self.cloth.edge_rest.len() != self.cloth.edges.len()
        {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                "motion needs matching endpoint counts, unchanged topology and a positive pair budget",
            ));
        }
        validate_mesh(cloth_start, &self.cloth.triangles)?;
        validate_mesh(cloth_end, &self.cloth.triangles)?;
        validate_mesh(&self.body.vertices, &self.body.triangles)?;
        validate_mesh(body_end, &self.body.triangles)?;
        let dp: Vec<_> = cloth_end
            .iter()
            .zip(cloth_start)
            .map(|(a, b)| *a - *b)
            .collect();
        if dp.iter().any(|d| !d.norm().is_finite())
            || body_end
                .iter()
                .zip(&self.body.vertices)
                .any(|(a, b)| !(*a - *b).norm().is_finite())
        {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "motion displacement overflow",
            ));
        }
        let vertices =
            |a: &[Vec3], b: &[Vec3]| (0..a.len()).map(|i| bounds(&[i], a, b)).collect::<Vec<_>>();
        let cv = vertices(cloth_start, cloth_end);
        let bv = vertices(&self.body.vertices, body_end);
        let ct: Vec<_> = self
            .cloth
            .triangles
            .iter()
            .map(|t| bounds(t, cloth_start, cloth_end))
            .collect();
        let ce: Vec<_> = self
            .cloth
            .edges
            .iter()
            .map(|e| bounds(e, cloth_start, cloth_end))
            .collect();
        let bt: Vec<_> = self
            .body
            .triangles
            .iter()
            .map(|t| bounds(t, &self.body.vertices, body_end))
            .collect();
        let be: Vec<_> = self
            .body
            .edges
            .iter()
            .map(|e| bounds(e, &self.body.vertices, body_end))
            .collect();
        let mut result = MotionBound {
            fraction: 1.0,
            pairs_tested: 0,
        };
        let mut test = |pair: Pair| -> Result<(), Refusal> {
            if result.pairs_tested == max_pairs {
                return Err(Refusal::new(
                    RefusalCode::BudgetExceeded,
                    "swept contact pair budget exceeded",
                ));
            }
            result.pairs_tested += 1;
            let st = stencil(pair, cloth_start, &self.body, &self.cloth);
            let body_dp = body_delta(pair, &self.body.vertices, body_end, &self.body);
            let dp = std::array::from_fn(|i| st.cloth[i].map_or(body_dp[i], |v| dp[v]));
            let p = self.pair_params(pair);
            let (kind, _) = classify(pair, &st.pts);
            let d2 = squared_distance_f64(st.pts, kind);
            if !d2.is_finite() {
                return Err(Refusal::new(
                    RefusalCode::NonFinite,
                    "swept contact distance is non-finite",
                ));
            }
            if d2.sqrt() <= p.thickness {
                return Err(Refusal::new(
                    RefusalCode::IntersectingStart,
                    "swept contact starts at a non-positive gap",
                ));
            }
            let fraction = accd(pair, st.pts, dp, p.thickness, result.fraction);
            if !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) {
                return Err(Refusal::new(
                    RefusalCode::NonFinite,
                    "additive CCD produced an invalid motion bound",
                ));
            }
            result.fraction = result.fraction.min(fraction);
            Ok(())
        };
        let pad = self.params.thickness;
        visit(&cv, &bt, pad, |cv, bt| test(Pair::VertexTri { cv, bt }))?;
        visit(&ct, &bv, pad, |ct, bv| test(Pair::TriVertex { ct, bv }))?;
        visit(&ce, &be, pad, |ce, be| test(Pair::EdgeEdge { ce, be }))?;
        if let Some(p) = self.self_params {
            visit(&cv, &ct, p.thickness, |cv, ct| {
                if !self.cloth.triangles[ct].contains(&cv) {
                    test(Pair::SelfVertexTri { cv, ct })
                } else {
                    Ok(())
                }
            })?;
            visit(&ce, &ce, p.thickness, |a, b| {
                if a < b
                    && !self.cloth.edges[a]
                        .iter()
                        .any(|i| self.cloth.edges[b].contains(i))
                {
                    test(Pair::SelfEdgeEdge { a, b })
                } else {
                    Ok(())
                }
            })?;
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streamed_swept_bvh_matches_exhaustive_feature_pairs() {
        // Deterministic noncoplanar trajectories; all three body/cloth feature families occur.
        let points = |n: usize, z: f64| {
            (0..n)
                .map(|i| {
                    Vec3::new(
                        (i / 3 % 7) as f64 * 0.07 + f64::from(i % 3 == 1) * 0.09,
                        (i / 3 % 11) as f64 * 0.05 + f64::from(i % 3 == 2) * 0.08,
                        z + (i / 3 % 3) as f64 * 0.03,
                    )
                })
                .collect::<Vec<_>>()
        };
        let body = Body::new(
            points(36, 1.0),
            (0..12).map(|i| [3 * i, 3 * i + 1, 3 * i + 2]).collect(),
            0.1,
        )
        .unwrap();
        let x = points(45, 0.0);
        let c = Contact::new(
            body,
            ContactParams {
                dhat: 0.001,
                thickness: 0.0005,
                kappa: 10.0,
                mu: 0.0,
                eps_v: 0.001,
            },
            (0..15).map(|i| [3 * i, 3 * i + 1, 3 * i + 2]).collect(),
            |_, _| 0.1,
        );
        for direction in [-1.0, 0.0, 1.0] {
            let y: Vec<_> = x
                .iter()
                .map(|p| *p + Vec3::new(0.08, 0.0, direction))
                .collect();
            let b: Vec<_> = c
                .body
                .vertices
                .iter()
                .map(|p| *p - Vec3::new(0.08, 0.0, direction))
                .collect();
            let actual = c.motion_bound(&x, &y, &b, 100_000).unwrap();
            let mut count = 0;
            let mut fraction: f64 = 1.0;
            let mut check = |pair: Pair, a: &[usize], bi: &[usize]| {
                let ba = grow(bounds(a, &x, &y), c.params.thickness).unwrap();
                let bb = bounds(bi, &c.body.vertices, &b);
                if !overlaps(bb, ba.0, ba.1) {
                    return;
                }
                count += 1;
                let st = stencil(pair, &x, &c.body, &c.cloth);
                let db = body_delta(pair, &c.body.vertices, &b, &c.body);
                let dp = std::array::from_fn(|i| st.cloth[i].map_or(db[i], |j| y[j] - x[j]));
                fraction = fraction.min(accd(pair, st.pts, dp, c.params.thickness, fraction));
            };
            for cv in 0..x.len() {
                for (bt, t) in c.body.triangles.iter().enumerate() {
                    check(Pair::VertexTri { cv, bt }, &[cv], t);
                }
            }
            for (ct, t) in c.cloth.triangles.iter().enumerate() {
                for bv in 0..b.len() {
                    check(Pair::TriVertex { ct, bv }, t, &[bv]);
                }
            }
            for (ce, e) in c.cloth.edges.iter().enumerate() {
                for (be, b) in c.body.edges.iter().enumerate() {
                    check(Pair::EdgeEdge { ce, be }, e, b);
                }
            }
            assert_eq!(
                actual,
                MotionBound {
                    fraction,
                    pairs_tested: count
                }
            );
        }
    }
}

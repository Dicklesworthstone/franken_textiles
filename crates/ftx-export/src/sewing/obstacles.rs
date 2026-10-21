//! Source cutouts, zero-width slits and explicitly sized drill disks in the expanded cutter frame.
//! These are declared geometry, not inferred kerf, needle radius or presser-foot accessibility.

use ftx_core::{Refusal, RefusalCode, Vec2, polyline::point_in_polygon};
use ftx_pattern::{Instance, Pattern};

use super::super::{bad, path};
use crate::{geom::feature_maps, outline::segments_touch};

const MAX_FEATURES: usize = 256;
const MAX_POINTS: usize = 8192;
const EPS: f64 = 1e-8;

#[derive(Default)]
pub(in crate::sewing) struct Obstacles {
    pub cuts: Vec<(Vec<Vec2>, bool)>,
    /// Centre and full diameter in mm, exactly as in the source (no export default diameter).
    pub drills: Vec<(Vec2, f64)>,
}

fn bounded(p: Vec2) -> bool {
    p.is_finite() && p.x.abs().max(p.y.abs()) <= 1_000_000.0
}

fn edges(points: &[Vec2], closed: bool) -> impl Iterator<Item = (Vec2, Vec2)> + '_ {
    points
        .iter()
        .copied()
        .zip(points.iter().copied().cycle().skip(1))
        .take(if closed {
            points.len()
        } else {
            points.len().saturating_sub(1)
        })
}

fn point_distance(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let d = b - a;
    if d.norm2() == 0.0 {
        return p.dist(a);
    }
    p.dist(a + d * ((p - a).dot(d) / d.norm2()).clamp(0.0, 1.0))
}

fn distance(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> f64 {
    if segments_touch(a, b, c, d) {
        0.0
    } else {
        point_distance(a, c, d)
            .min(point_distance(b, c, d))
            .min(point_distance(c, a, b))
            .min(point_distance(d, a, b))
    }
}

fn budget() -> Refusal {
    Refusal::new(
        RefusalCode::BudgetExceeded,
        "sewing obstacle geometry budget exceeded",
    )
}

impl Obstacles {
    /// Removed source material, not a nominal ply. Open slits have no inferred kerf area.
    pub(in crate::sewing) fn removed(&self, p: Vec2, work: &mut usize) -> Result<bool, Refusal> {
        for (cut, closed) in &self.cuts {
            path::charge(work, cut.len())?;
            if *closed && point_in_polygon(p, cut) {
                return Ok(true);
            }
        }
        path::charge(work, self.drills.len())?;
        Ok(self.drills.iter().any(|(c, d)| p.dist(*c) <= d * 0.5))
    }

    /// Prove the whole source chord is in one removed region, including its rim clearance.
    /// Endpoint tests alone are insufficient for concave holes. Overlapping holes are not unioned:
    /// when no individual void contains the whole chord this deliberately returns no proof.
    pub(in crate::sewing) fn void_clearance(
        &self,
        a: Vec2,
        b: Vec2,
        work: &mut usize,
    ) -> Result<Option<f64>, Refusal> {
        let mut best: Option<f64> = None;
        for (cut, closed) in &self.cuts {
            path::charge(work, cut.len().saturating_mul(3))?;
            if !closed || !point_in_polygon(a, cut) || !point_in_polygon(b, cut) {
                continue;
            }
            let d = edges(cut, true)
                .map(|(c, d)| distance(a, b, c, d))
                .fold(f64::INFINITY, f64::min);
            if d.is_finite() && d > EPS {
                best = Some(best.map_or(d, |old| old.max(d)));
            }
        }
        path::charge(work, self.drills.len())?;
        for &(c, diameter) in &self.drills {
            // A disk is convex, and distance to its centre is convex along the chord.
            let d = diameter * 0.5 - a.dist(c).max(b.dist(c));
            if d.is_finite() && d > EPS {
                best = Some(best.map_or(d, |old| old.max(d)));
            }
        }
        Ok(best)
    }

    /// Consume strict source geometry, while sharing the cutter's instance transforms. A slit that
    /// reaches the exterior changes the outline/topology and must be represented there instead.
    pub(in crate::sewing) fn load(
        pattern: &Pattern,
        instance: &Instance,
        exterior: &[Vec2],
        work: &mut usize,
    ) -> Result<Self, Refusal> {
        let piece = &pattern
            .doc
            .as_ref()
            .ok_or_else(|| bad("invalid pattern"))?
            .pieces[instance.piece_index];
        let internal = piece.internal.as_deref().unwrap_or_default();
        let drills = piece.drills.as_deref().unwrap_or_default();
        if internal.len() > MAX_FEATURES || drills.len() > MAX_FEATURES {
            return Err(budget());
        }
        let raw = &pattern.raw()["pieces"][instance.piece_index];
        let maps = feature_maps(raw, Some(instance));
        let mut count = drills.len().saturating_mul(2);
        for m in internal.iter().filter(|m| m.kind.as_deref() == Some("cut")) {
            count = count.saturating_add(m.pts.len());
        }
        if count.saturating_mul(maps.len()) > MAX_POINTS {
            return Err(budget());
        }
        let mut out = Self::default();
        for m in internal.iter().filter(|m| m.kind.as_deref() == Some("cut")) {
            let closed = m
                .extra
                .get("closed")
                .and_then(|v| v.as_bool())
                .ok_or_else(|| {
                    bad("sewing internal cuts require an explicit boolean closed flag")
                })?;
            if m.pts.iter().any(|&p| !bounded(p)) {
                return Err(bad(
                    "sewing internal cuts require bounded finite coordinates",
                ));
            }
            for map in &maps {
                let mut points: Vec<_> = m.pts.iter().map(|&p| map.apply(p)).collect();
                if closed && points.len() > 1 && points.first() == points.last() {
                    points.pop();
                }
                if points.len() < if closed { 3 } else { 2 }
                    || points.iter().any(|&p| !bounded(p))
                    || (!closed && points.first() == points.last())
                    || edges(&points, closed).any(|(a, b)| a.dist(b) <= EPS)
                {
                    return Err(bad("degenerate or inconsistent internal cut geometry"));
                }
                let mut line = points.clone();
                if closed {
                    line.push(points[0]);
                }
                path::simple(&line, work)?;
                // The same full-segment test used for stitches checks the obstacle's containment.
                path::clearance(&line, exterior, EPS, work)?;
                out.cuts.push((points, closed));
            }
        }
        for d in drills {
            let diameter = d
                .extra
                .get("d")
                .and_then(|v| v.as_f64())
                .filter(|d| d.is_finite() && *d > 0.0 && *d <= 1_000_000.0)
                .ok_or_else(|| {
                    bad("sewing drills require an explicit finite positive diameter d in mm")
                })?;
            if !bounded(d.p) {
                return Err(bad("invalid sewing drill centre"));
            }
            for map in &maps {
                let p = map.apply(d.p);
                path::charge(work, exterior.len())?;
                if !bounded(p)
                    || !point_in_polygon(p, exterior)
                    || edges(exterior, true)
                        .any(|(a, b)| point_distance(p, a, b) <= diameter * 0.5 + EPS)
                {
                    return Err(bad(
                        "sewing drill disk must be strictly inside the cut outline",
                    ));
                }
                // Exact duplicates on a fold axis have one physical disk. Never discard another
                // centre by rounding or silently choose between conflicting explicit diameters.
                path::charge(work, out.drills.len())?;
                if let Some((_, old)) = out.drills.iter().find(|(q, _)| *q == p) {
                    if *old != diameter {
                        return Err(bad("conflicting diameters at a sewing drill centre"));
                    }
                } else {
                    out.drills.push((p, diameter));
                }
            }
        }
        Ok(out)
    }

    /// Distance to every declared cut/rim over complete path segments. Closed cutouts are removed
    /// regions: a line wholly inside a hole is not accepted merely because its rim is far away.
    /// Empty obstacle sets return infinity, leaving the original exterior result unchanged.
    pub(in crate::sewing) fn clearance(
        &self,
        points: &[Vec2],
        required: f64,
        work: &mut usize,
    ) -> Result<f64, Refusal> {
        if points.len() < 2 || points.iter().any(|&p| !bounded(p)) {
            return Err(bad("invalid stitch path for internal obstacle clearance"));
        }
        let mut minimum = f64::INFINITY;
        for (cut, closed) in &self.cuts {
            path::charge(work, points.len().saturating_mul(cut.len()))?;
            if *closed && point_in_polygon(points[0], cut) {
                return Err(bad("stitch path is inside an internal cutout"));
            }
            for w in points.windows(2) {
                for (a, b) in edges(cut, *closed) {
                    let d = distance(w[0], w[1], a, b);
                    if !d.is_finite() || d <= EPS || d + EPS < required {
                        return Err(bad("stitch path lacks internal cut/slit clearance"));
                    }
                    minimum = minimum.min(d);
                }
            }
        }
        path::charge(work, points.len().saturating_mul(self.drills.len()))?;
        for &(centre, diameter) in &self.drills {
            for w in points.windows(2) {
                let d = point_distance(centre, w[0], w[1]) - diameter * 0.5;
                if !d.is_finite() || d <= EPS || d + EPS < required {
                    return Err(bad("stitch path lacks drill-rim clearance"));
                }
                minimum = minimum.min(d);
            }
        }
        Ok(minimum)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Ctx, ExportOptions, Source};
    use serde_json::{Value, json};

    #[test]
    fn obstacles_share_the_exporters_mirror_and_on_fold_maps() {
        let mut raw: Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/materials/synthetic-sewing-pattern.json"
        ))
        .unwrap();
        raw["seams"] = json!([]);
        raw["pieces"][0]["cut"] = json!({"qty":2,"on_fold":"left"});
        raw["pieces"][0]["internal"] = json!([
            {"kind":"cut","closed":true,"pts":[[3,20],[6,20],[6,30],[3,30]]}
        ]);
        raw["pieces"][0]["drills"] = json!([{"p":[10,50],"d":2},{"p":[0,75],"d":2}]);
        let pattern = Pattern::new(raw);
        let ctx = Ctx::new(&pattern, ExportOptions::default()).unwrap();
        let mut checked = 0;
        for instance in pattern.instances.iter().filter(|i| i.piece_index == 0) {
            let geom = ctx.geom(Source::Instance(instance), 6.).unwrap();
            let obstacles = Obstacles::load(&pattern, instance, &geom.cut, &mut 0).unwrap();
            assert_eq!(obstacles.cuts, geom.internal_cut);
            assert_eq!(obstacles.drills, geom.drills);
            assert_eq!(obstacles.cuts.len(), 2);
            assert_eq!(obstacles.drills.len(), 3); // fold-axis duplicate occurs only once
            checked += 1;
        }
        assert_eq!(checked, 2);
    }

    #[test]
    fn curved_route_can_clear_a_hole_while_its_stitch_chord_does_not() {
        let obstacle = Obstacles {
            cuts: Vec::new(),
            drills: vec![(Vec2::new(0., 0.), 1.)],
        };
        let a = Vec2::new(-2., 0.);
        let b = Vec2::new(0., 2.);
        let c = Vec2::new(2., 0.);
        assert!(obstacle.clearance(&[a, b, c], 0.5, &mut 0).unwrap() > 0.9);
        assert!(obstacle.clearance(&[a, c], 0.5, &mut 0).is_err());
        assert!(obstacle.clearance(&[a, a], 0.5, &mut 0).is_ok());
        assert_eq!(
            obstacle
                .clearance(&[a, b, c], 0.5, &mut 50_000_000)
                .unwrap_err()
                .code,
            RefusalCode::BudgetExceeded
        );
    }
}

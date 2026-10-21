//! Convex planar partitions in dimensionless barycentric coordinates. No world-unit epsilon.
use ftx_core::{Refusal, Vec2};

use super::invalid;

pub(in crate::thermal::surface) const AREA_EPS: f64 = 1e-12;

#[derive(Default)]
pub(in crate::thermal::surface) struct Work(pub usize);
impl Work {
    pub(in crate::thermal::surface) fn add(&mut self, n: usize) -> Result<(), Refusal> {
        self.0 = self
            .0
            .checked_add(n)
            .filter(|n| *n <= 5_000_000)
            .ok_or_else(|| {
                invalid("thermal projection exceeds five million geometry operations")
            })?;
        Ok(())
    }
}

pub(in crate::thermal::surface) fn area(p: &[Vec2]) -> f64 {
    if p.len() < 3 {
        return 0.0;
    }
    p[1..]
        .windows(2)
        .map(|w| (w[0] - p[0]).cross(w[1] - p[0]))
        .sum::<f64>()
        * 0.5
}

pub(in crate::thermal::surface) fn ccw(mut p: Vec<Vec2>) -> Vec<Vec2> {
    if area(&p) < 0.0 {
        p.reverse();
    }
    p
}

// Both half-polygons retain their parent's winding. Boundary-only intersections have zero area.
pub(in crate::thermal::surface) fn split(
    p: &[Vec2],
    a: Vec2,
    b: Vec2,
    work: &mut Work,
) -> Result<(Vec<Vec2>, Vec<Vec2>), Refusal> {
    work.add(p.len())?;
    let mut inside = Vec::new();
    let mut outside = Vec::new();
    for i in 0..p.len() {
        let u = p[i];
        let v = p[(i + 1) % p.len()];
        let du = (b - a).cross(u - a);
        let dv = (b - a).cross(v - a);
        if !du.is_finite() || !dv.is_finite() {
            return Err(invalid("projection clipping overflow"));
        }
        if du >= 0.0 {
            inside.push(u);
        }
        if du <= 0.0 {
            outside.push(u);
        }
        if (du < 0.0 && dv > 0.0) || (du > 0.0 && dv < 0.0) {
            let t = du / (du - dv);
            let q = u * (1.0 - t) + v * t;
            if !q.is_finite() {
                return Err(invalid("projection intersection overflow"));
            }
            inside.push(q);
            outside.push(q);
        }
    }
    for result in [&mut inside, &mut outside] {
        result.dedup();
        if result.first() == result.last() {
            result.pop();
        }
        if result.len() > 256 {
            return Err(invalid("projection polygon exceeds 256 vertices"));
        }
        if !area(result).is_finite() {
            return Err(invalid("projection area overflow"));
        }
    }
    Ok((inside, outside))
}

pub(in crate::thermal::surface) fn intersection(
    p: &[Vec2],
    clip: &[Vec2],
    work: &mut Work,
) -> Result<Vec<Vec2>, Refusal> {
    let mut p = p.to_vec();
    for i in 0..clip.len() {
        p = split(&p, clip[i], clip[(i + 1) % clip.len()], work)?.0;
        if p.len() < 3 {
            break;
        }
    }
    Ok(p)
}

pub(in crate::thermal::surface) fn barycentric(p: Vec2, tri: [Vec2; 3]) -> Result<Vec2, Refusal> {
    let a = tri[1] - tri[0];
    let b = tri[2] - tri[0];
    let d = a.cross(b);
    let q = p - tri[0];
    let result = Vec2::new(q.cross(b) / d, a.cross(q) / d);
    if !result.is_finite() {
        return Err(invalid("singular projected triangle"));
    }
    Ok(result)
}

pub(in crate::thermal::surface) fn bounds_overlap(a: &[Vec2], b: &[Vec2]) -> bool {
    let bounds = |p: &[Vec2]| {
        p.iter().fold(
            [
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ],
            |r, v| [r[0].min(v.x), r[1].min(v.y), r[2].max(v.x), r[3].max(v.y)],
        )
    };
    let a = bounds(a);
    let b = bounds(b);
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

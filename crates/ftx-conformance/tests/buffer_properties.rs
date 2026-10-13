//! Properties of `ftx_trace::buffer::buffer` (the GEOS 3.14.1 port) that any correct buffer has, checked on
//! generated polygons with `ftx_conformance::propcheck` (shrinking, replayable seeds): every result ring is
//! closed and simple, shells run clockwise and holes counter-clockwise inside their shell; translating the
//! input translates the result; the area grows with the distance; the distance-0 clean-up keeps a simple
//! polygon's area; arbitrary point soup never panics. The oracles (`raster_buffer`, `buffer_topology`)
//! pin GEOS's exact output; these catch what no fixture happens to contain.

use ftx_conformance::propcheck::{Stream, check};
use ftx_core::Vec2;
use ftx_trace::buffer::{Join, Polygon, buffer};

/// GEOS `Area::ofRingSigned`'s sign convention: positive for clockwise.
fn signed_area(r: &[Vec2]) -> f64 {
    r.windows(2)
        .map(|w| (w[1].x - w[0].x) * (w[1].y + w[0].y))
        .sum::<f64>()
        / 2.0
}

/// The exact orientation sign of `c` against `a → b`.
fn orient(a: Vec2, b: Vec2, c: Vec2) -> i32 {
    match ftx_core::exact::orient2d([a.x, a.y], [b.x, b.y], [c.x, c.y]) {
        ftx_core::exact::Sign::Positive => 1,
        ftx_core::exact::Sign::Negative => -1,
        ftx_core::exact::Sign::Zero => 0,
    }
}

/// Do closed segments `ab` and `cd` cross or touch (exact)?
fn segments_meet(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> bool {
    let (o1, o2, o3, o4) = (
        orient(a, b, c),
        orient(a, b, d),
        orient(c, d, a),
        orient(c, d, b),
    );
    let within = |p: Vec2, q: Vec2, r: Vec2| {
        r.x >= p.x.min(q.x) && r.x <= p.x.max(q.x) && r.y >= p.y.min(q.y) && r.y <= p.y.max(q.y)
    };
    if o1 * o2 < 0 && o3 * o4 < 0 {
        return true;
    }
    (o1 == 0 && within(a, b, c))
        || (o2 == 0 && within(a, b, d))
        || (o3 == 0 && within(c, d, a))
        || (o4 == 0 && within(c, d, b))
}

/// A closed ring with at least three distinct vertices whose non-adjacent edges never meet.
fn simple_ring(r: &[Vec2]) -> Result<(), String> {
    if r.len() < 4 || r.first() != r.last() {
        return Err(format!(
            "not a closed ring of 3+ vertices ({} points)",
            r.len()
        ));
    }
    let n = r.len() - 1;
    for i in 0..n {
        for j in i + 1..n {
            if j == i + 1 || (i == 0 && j == n - 1) {
                continue;
            }
            if segments_meet(r[i], r[i + 1], r[j], r[j + 1]) {
                return Err(format!("edges {i} and {j} meet"));
            }
        }
    }
    Ok(())
}

fn inside(p: Vec2, ring: &[Vec2]) -> bool {
    let mut c = false;
    for w in ring.windows(2) {
        let (a, b) = (w[0], w[1]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            c = !c;
        }
    }
    c
}

/// GEOS's output conventions, checked on one result.
fn valid(polys: &[Polygon]) -> Result<(), String> {
    for (k, p) in polys.iter().enumerate() {
        simple_ring(&p.shell).map_err(|e| format!("polygon {k} shell: {e}"))?;
        if signed_area(&p.shell) <= 0.0 {
            return Err(format!("polygon {k}: shell not clockwise"));
        }
        for (h, hole) in p.holes.iter().enumerate() {
            simple_ring(hole).map_err(|e| format!("polygon {k} hole {h}: {e}"))?;
            if signed_area(hole) >= 0.0 {
                return Err(format!("polygon {k} hole {h}: not counter-clockwise"));
            }
            let mid = Vec2::new((hole[0].x + hole[1].x) / 2.0, (hole[0].y + hole[1].y) / 2.0);
            if !inside(mid, &p.shell) {
                return Err(format!("polygon {k} hole {h}: outside its shell"));
            }
        }
    }
    Ok(())
}

/// A star-shaped (hence simple) polygon from positive radii at even angles, closed.
fn star(radii: &[f64]) -> Vec<Vec2> {
    let n = radii.len();
    let mut ring: Vec<Vec2> = radii
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            Vec2::new(
                50.0 + r.abs().max(0.5) * a.cos(),
                50.0 + r.abs().max(0.5) * a.sin(),
            )
        })
        .collect();
    ring.push(ring[0]);
    ring
}

fn join(code: u64) -> Join {
    if code % 2 == 0 {
        Join::Mitre(4.0)
    } else {
        Join::Round(16)
    }
}

/// Radii (3 to 40 vertices, 1 to 40 units), a distance and a join.
fn gen_star(s: &mut Stream) -> (Vec<f64>, f64, u64) {
    let n = s.int_in(3, 40) as usize;
    let radii = (0..n).map(|_| s.f64_in(1.0, 40.0)).collect();
    (radii, s.f64_in(-15.0, 15.0), s.next_u64() % 2)
}

#[test]
fn results_are_valid_polygons() {
    check(
        "buffer: valid output",
        0xB0F1,
        2000,
        gen_star,
        |(radii, d, j)| {
            if radii.len() < 3 {
                return true;
            }
            match buffer(&[Polygon::new(star(radii))], *d, join(*j)) {
                Ok(polys) => match valid(&polys) {
                    Ok(()) => true,
                    Err(e) => {
                        eprintln!("{e}");
                        false
                    }
                },
                Err(e) => {
                    eprintln!("refused: {e}");
                    false // a star never needs GEOS's reduced-precision retry
                }
            }
        },
    );
}

#[test]
fn translation_moves_the_result() {
    check(
        "buffer: translation covariance",
        0xB0F2,
        1000,
        gen_star,
        |(radii, d, j)| {
            if radii.len() < 3 {
                return true;
            }
            let ring = star(radii);
            let shifted: Vec<Vec2> = ring
                .iter()
                .map(|p| Vec2::new(p.x + 1000.0, p.y - 250.0))
                .collect();
            let (Ok(a), Ok(b)) = (
                buffer(&[Polygon::new(ring)], *d, join(*j)),
                buffer(&[Polygon::new(shifted)], *d, join(*j)),
            ) else {
                return false;
            };
            let area = |ps: &[Polygon]| ps.iter().map(Polygon::area).sum::<f64>();
            a.len() == b.len() && (area(&a) - area(&b)).abs() <= 1e-9 * area(&a).max(1.0)
        },
    );
}

#[test]
fn area_grows_with_the_distance() {
    check(
        "buffer: monotone area",
        0xB0F3,
        1000,
        gen_star,
        |(radii, d, j)| {
            if radii.len() < 3 {
                return true;
            }
            let ring = star(radii);
            let area = |dist: f64| {
                buffer(&[Polygon::new(ring.clone())], dist, join(*j))
                    .map(|ps| ps.iter().map(Polygon::area).sum::<f64>())
            };
            let (Ok(a), Ok(b)) = (area(*d), area(*d + 0.75)) else {
                return false;
            };
            a <= b * (1.0 + 1e-12)
        },
    );
}

#[test]
fn clean_up_keeps_a_simple_polygon() {
    check(
        "buffer: distance 0 of a simple polygon",
        0xB0F4,
        2000,
        gen_star,
        |(radii, _, j)| {
            if radii.len() < 3 {
                return true;
            }
            let p = Polygon::new(star(radii));
            let Ok(out) = buffer(std::slice::from_ref(&p), 0.0, join(*j)) else {
                return false;
            };
            out.len() == 1
                && out[0].holes.is_empty()
                && (out[0].area() - p.area()).abs() <= 1e-9 * p.area()
        },
    );
}

#[test]
fn point_soup_never_panics_and_cleans_to_valid_polygons() {
    let soup = |s: &mut Stream| -> (Vec<(f64, f64)>, f64, u64) {
        let n = s.int_in(3, 30) as usize;
        let pts = (0..n)
            .map(|_| (s.f64_in(0.0, 20.0).round(), s.f64_in(0.0, 20.0).round()))
            .collect();
        (pts, s.f64_in(-3.0, 3.0), s.next_u64() % 2)
    };
    check(
        "buffer: arbitrary rings",
        0xB0F5,
        3000,
        soup,
        |(pts, d, j)| {
            let ring: Vec<Vec2> = pts.iter().map(|&(x, y)| Vec2::new(x, y)).collect();
            match buffer(&[Polygon::new(ring)], *d, join(*j)) {
                Ok(polys) => match valid(&polys) {
                    Ok(()) => true,
                    Err(e) => {
                        eprintln!("{e}: {polys:?}");
                        false
                    }
                },
                Err(e) => e.starts_with("buffer:"), // GEOS's own refusals (its retry is not ported)
            }
        },
    );
}

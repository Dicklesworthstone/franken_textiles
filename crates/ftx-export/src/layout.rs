//! Shelf layouts (`docs/spec/03` §6).

use ftx_core::{Vec2, det};

use crate::geom::{Affine, Geom, grain_vertical_rotation};

/// Geometries placed on a sheet.
#[derive(Clone, Debug)]
pub struct Layout {
    /// The geometries in the sheet frame, in input order.
    pub items: Vec<Geom>,
    /// Sheet width (mm).
    pub width: f64,
    /// Sheet height (mm).
    pub height: f64,
    /// The geometries before placing (rotated if asked), in input order.
    pub model: Vec<Geom>,
}

/// Counter-clockwise rotation by `deg` degrees.
#[must_use]
pub fn rotation(deg: f64) -> Affine {
    let (s, c) = det::sin_cos(deg.to_radians());
    Affine {
        a: [[c, -s], [s, c]],
        b: Vec2::ZERO,
    }
}

/// Places geometries on shelves: tallest first (ties by instance id), left to right, a new shelf when the
/// next would pass `max_width` (default: the widest piece or 1.25 × the square root of the pieces' total box
/// area, whichever is larger); `gap` between pieces and shelves, `margin` around, the first shelf `top` below
/// the margin. `rotate_grain` turns each piece's grainline vertical first; `y_down` flips to a sheet frame.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn shelf_layout(
    geoms: &[Geom],
    gap: f64,
    margin: f64,
    max_width: Option<f64>,
    rotate_grain: bool,
    y_down: bool,
    top: f64,
) -> Layout {
    let rotated: Vec<(Geom, f64, f64)> = geoms
        .iter()
        .map(|g| {
            let m = if rotate_grain {
                rotation(grain_vertical_rotation(g))
            } else {
                Affine::IDENTITY
            };
            let gr = g.transform(&m);
            let (x0, y0, x1, y1) = gr.bbox();
            (gr, x1 - x0, y1 - y0)
        })
        .collect();
    let max_width = max_width.unwrap_or_else(|| {
        let area: f64 = rotated.iter().map(|(_, w, h)| w * h).sum();
        let widest = rotated
            .iter()
            .map(|r| r.1)
            .fold(f64::NEG_INFINITY, f64::max);
        widest.max(area.sqrt() * 1.25)
    });
    let mut order: Vec<usize> = (0..rotated.len()).collect();
    order.sort_by(|&a, &b| {
        (-rotated[a].2)
            .total_cmp(&-rotated[b].2)
            .then_with(|| rotated[a].0.iid.cmp(&rotated[b].0.iid))
    });
    let (mut x, mut y, mut shelf_h, mut used_w) = (0.0, 0.0, 0.0f64, 0.0f64);
    let mut placed: Vec<Option<Geom>> = vec![None; rotated.len()];
    for i in order {
        let (gr, w, h) = &rotated[i];
        if x > 0.0 && x + w > max_width {
            y += shelf_h + gap;
            x = 0.0;
            shelf_h = 0.0;
        }
        let (x0, y0, _, y1) = gr.bbox();
        let (sx, sy) = (margin + x, top + margin + y);
        let m = if y_down {
            Affine {
                a: [[1.0, 0.0], [0.0, -1.0]],
                b: Vec2::new(sx - x0, sy + y1),
            }
        } else {
            Affine {
                a: [[1.0, 0.0], [0.0, 1.0]],
                b: Vec2::new(sx - x0, sy - y0),
            }
        };
        placed[i] = Some(gr.transform(&m));
        x += w + gap;
        used_w = used_w.max(x - gap);
        shelf_h = shelf_h.max(*h);
    }
    Layout {
        items: placed.into_iter().flatten().collect(),
        width: used_w + 2.0 * margin,
        height: top + y + shelf_h + 2.0 * margin,
        model: rotated.into_iter().map(|r| r.0).collect(),
    }
}

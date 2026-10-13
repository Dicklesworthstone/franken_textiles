//! The garment mesh (spec 04 §2): every simulated piece instance meshed flat in one vertex numbering,
//! with matched seam samples so that sewing pairs are vertex pairs.
//!
//! Port of the reference's `build_garment`. Selection, seam sampling, boundary samples, the hex interior
//! and the seam records are bit-exact with the reference. Triangulation is `spade`'s constrained
//! Delaunay on the same points and constraint loop where the reference calls Blender's
//! `delaunay_2d_cdt`: both are Delaunay, so they agree except where points are cocircular (the hex
//! interior has many such quadruples) and either diagonal is correct. Exactly coincident points are
//! merged (the reference merges within 1e-6 mm).

use std::collections::BTreeMap;

use ftx_core::curve::EDGE_DENSE;
use ftx_core::np::{arange, linspace};
use ftx_core::polyline::{at_fractions, point_in_polygon, polyline_length};
use ftx_core::{Refusal, RefusalCode, Vec2};
use ftx_pattern::{Instance, Pattern, PyError, truthy};
use serde_json::Value;
use spade::{ConstrainedDelaunayTriangulation, Point2, Triangulation};

/// One seam's sewing record (spec 04 §2.6).
#[derive(Clone, Debug, PartialEq)]
pub struct SeamRecord {
    /// Seam id.
    pub id: String,
    /// Sewing pairs `(side a, side b)`, without self-pairs.
    pub pairs: Vec<(usize, usize)>,
    /// Side a's global vertex path.
    pub side_a: Vec<usize>,
    /// Side b's path in its own loop order.
    pub side_b: Vec<usize>,
    /// `seam.ratio` (default 1).
    pub ratio: f64,
}

/// The garment mesh: 2D rest positions (mm, after `sim_scale`), CCW triangles and the sewing records.
#[derive(Clone, Debug)]
pub struct GarmentMesh {
    /// Simulated instance ids in construction order.
    pub instances: Vec<String>,
    /// 2D vertices (mm).
    pub verts2d: Vec<Vec2>,
    /// Triangles, counter-clockwise in 2D.
    pub faces: Vec<[usize; 3]>,
    /// Owning instance (index into `instances`) of each vertex.
    pub owner: Vec<usize>,
    /// Half-open global vertex range of each instance.
    pub inst_range: Vec<(usize, usize)>,
    /// All sewing pairs, seam by seam.
    pub pairs: Vec<(usize, usize)>,
    /// Seam records in pattern order.
    pub seams: Vec<SeamRecord>,
    /// `(instance, vertex)` → global index of the first sample of the edge starting there.
    pub corner: BTreeMap<(String, String), usize>,
    /// `(instance, edge)` → global indices along the edge, both ends included.
    pub index: BTreeMap<(String, String), Vec<usize>>,
}

fn doc_error(e: &PyError) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, e.message().to_string())
}

/// `np.linalg.norm` of a 2-vector, as numpy evaluates it (√(x² + y²), not `hypot`).
fn norm2(d: Vec2) -> f64 {
    (d.x * d.x + d.y * d.y).sqrt()
}

/// Splits a seam path's samples (loop order) into per-edge lists at the samples nearest the internal
/// junction vertices (the reference's `_assign_path`).
fn assign_path(
    inst: &Instance,
    eids: &[String],
    pts: &[Vec2],
    edge_pts: &mut BTreeMap<(String, String), Vec<Vec2>>,
) -> Result<(), Refusal> {
    let mut cuts = vec![0];
    for e in &eids[..eids.len().saturating_sub(1)] {
        let to = &inst.edge(e).map_err(|e| doc_error(&e))?.to;
        let v = inst
            .verts
            .get(to)
            .ok_or_else(|| Refusal::new(RefusalCode::InvalidDocument, format!("no vertex {to}")))?;
        let (mut best, mut best_d) = (0, f64::INFINITY);
        for (k, p) in pts.iter().enumerate() {
            let d = norm2(*p - v);
            if d < best_d {
                (best, best_d) = (k, d);
            }
        }
        cuts.push(best);
    }
    cuts.push(pts.len() - 1);
    for (k, e) in eids.iter().enumerate() {
        let (a, b) = (cuts[k], cuts[k + 1]);
        let slice = if a <= b {
            pts[a..=b].to_vec()
        } else {
            Vec::new()
        };
        edge_pts.insert((inst.id.clone(), e.clone()), slice);
    }
    Ok(())
}

fn path_indices(
    index: &BTreeMap<(String, String), Vec<usize>>,
    iid: &str,
    eids: &[String],
) -> Vec<usize> {
    let mut out = Vec::new();
    for (k, e) in eids.iter().enumerate() {
        let idx = &index[&(iid.to_string(), e.clone())];
        out.extend_from_slice(if k == 0 { idx } else { &idx[1..] });
    }
    out
}

/// The reference's hex interior: rows `0.866·s` apart from `lo + s/2`, odd rows shifted `s/2`, kept
/// inside the loop (even-odd) and more than `0.55·s` from every boundary sample.
fn interior(boundary: &[Vec2], s: f64) -> Vec<Vec2> {
    let (mut lo, mut hi) = (boundary[0], boundary[0]);
    for p in boundary {
        lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
        hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
    }
    let gx = arange(lo.x + s / 2.0, hi.x, s);
    let gy = arange(lo.y + s / 2.0, hi.y, s * 0.866);
    let mut out = Vec::new();
    for (j, &y) in gy.iter().enumerate() {
        for &x in &gx {
            let p = Vec2::new(if j % 2 == 1 { x + s / 2.0 } else { x }, y);
            if !point_in_polygon(p, boundary) {
                continue;
            }
            let d = boundary
                .iter()
                .map(|b| norm2(p - *b))
                .fold(f64::INFINITY, f64::min);
            if d > 0.55 * s {
                out.push(p);
            }
        }
    }
    out
}

/// Constrained Delaunay of `points` with the first `nb` as the closed boundary loop; the triangles
/// inside the loop, counter-clockwise, as indices into `points`.
fn triangulate(points: &[Vec2], nb: usize, inst: &str) -> Result<Vec<[usize; 3]>, Refusal> {
    let bad = |m: String| Refusal::new(RefusalCode::DegenerateGeometry, m);
    let mut cdt: ConstrainedDelaunayTriangulation<Point2<f64>> =
        ConstrainedDelaunayTriangulation::new();
    let mut handles = Vec::with_capacity(points.len());
    // First input index of each spade vertex (coincident points share one).
    let mut first: BTreeMap<usize, usize> = BTreeMap::new();
    for (i, p) in points.iter().enumerate() {
        let h = cdt
            .insert(Point2::new(p.x, p.y))
            .map_err(|e| bad(format!("{inst}: cannot triangulate point {i}: {e:?}")))?;
        first.entry(h.index()).or_insert(i);
        handles.push(h);
    }
    for k in 0..nb {
        let (a, b) = (handles[k], handles[(k + 1) % nb]);
        if a == b {
            continue;
        }
        if !cdt.can_add_constraint(a, b) {
            return Err(bad(format!(
                "{inst}: the outline crosses itself near boundary sample {k}"
            )));
        }
        cdt.add_constraint(a, b);
    }
    // Inside = an odd number of constraint edges crossed on the way in from the outer face (a flood
    // fill over face adjacency, as the reference keeps the faces of the constraint loop's region; a
    // centroid test would keep slivers along straight edges whose centroid rounds onto the line).
    let mut adjacent: Vec<Vec<(usize, usize)>> = vec![Vec::new(); cdt.num_all_faces()];
    for e in cdt.undirected_edges() {
        let d = e.as_directed();
        let (f1, f2) = (d.face().fix().index(), d.rev().face().fix().index());
        let crossing = usize::from(cdt.is_constraint_edge(e.fix()));
        adjacent[f1].push((f2, crossing));
        adjacent[f2].push((f1, crossing));
    }
    let mut depth: Vec<Option<usize>> = vec![None; adjacent.len()];
    let outer = cdt.outer_face().fix().index();
    depth[outer] = Some(0);
    let mut queue = std::collections::VecDeque::from([outer]);
    while let Some(fi) = queue.pop_front() {
        let d = depth[fi].unwrap_or(0);
        for &(n, crossing) in &adjacent[fi] {
            if depth[n].is_none() {
                depth[n] = Some(d + crossing);
                queue.push_back(n);
            }
        }
    }
    let mut faces = Vec::new();
    for f in cdt.inner_faces() {
        if depth[f.fix().index()].unwrap_or(0).is_multiple_of(2) {
            continue;
        }
        let v = f.vertices().map(|h| first[&h.fix().index()]);
        let (a, b, c) = (points[v[0]], points[v[1]], points[v[2]]);
        let cross = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
        faces.push(if cross < 0.0 { [v[2], v[1], v[0]] } else { v });
    }
    Ok(faces)
}

fn number(v: &Value, what: &str) -> Result<f64, Refusal> {
    v.as_f64().ok_or_else(|| {
        Refusal::new(
            RefusalCode::InvalidDocument,
            format!("{what} must be a number, not {v}"),
        )
    })
}

/// Builds the garment mesh of `pattern` at `spacing` mm (spec 04 §2; the reference uses 12 mm).
///
/// # Errors
/// [`RefusalCode::InvalidDocument`] for a broken reference (unknown edge, malformed seam, a non-numeric
/// ratio or `sim_scale`); [`RefusalCode::DegenerateGeometry`] for implausible seam sides, an outline
/// that crosses itself, or seam sides with different sample counts; [`RefusalCode::OutOfRange`] for a
/// non-positive spacing.
#[allow(clippy::too_many_lines)] // one pass, in the reference's order
pub fn build_garment(pattern: &Pattern, spacing: f64) -> Result<GarmentMesh, Refusal> {
    if !(spacing.is_finite() && spacing > 0.0) {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            format!("mesh spacing {spacing} mm must be positive"),
        ));
    }
    let raw = pattern.raw();
    let piece_def = |inst: &Instance| &raw["pieces"][inst.piece_index];
    let insts: Vec<&Instance> = pattern
        .instances
        .iter()
        .filter(|i| {
            let p = piece_def(i);
            p.get("sim") != Some(&Value::Bool(false)) && p.get("placement").is_some_and(truthy)
        })
        .collect();
    let ids: Vec<&str> = insts.iter().map(|i| i.id.as_str()).collect();
    let on_sim =
        |r: Option<&str>| r.is_some_and(|r| ids.contains(&r.split('/').next().unwrap_or(r)));
    let mut edge_pts: BTreeMap<(String, String), Vec<Vec2>> = BTreeMap::new();
    let mut seam_info = Vec::new();
    for s in pattern.seams() {
        if !(on_sim(s.side_ref("a")) && on_sim(s.side_ref("b"))) {
            continue;
        }
        let pr = pattern
            .seam_pairs(s, spacing)
            .map_err(|e| Refusal::new(RefusalCode::DegenerateGeometry, e.message().to_string()))?;
        let (ra, rb) = (s.side_ref("a").unwrap_or(""), s.side_ref("b").unwrap_or(""));
        let (ia, ea) = pattern.side(ra).map_err(|e| doc_error(&e))?;
        let (ib, eb) = pattern.side(rb).map_err(|e| doc_error(&e))?;
        let rev = s.reverse();
        let b_loop: Vec<Vec2> = if rev {
            pr.b.iter().rev().copied().collect()
        } else {
            pr.b.clone()
        };
        assign_path(ia, &ea, &pr.a, &mut edge_pts)?;
        assign_path(ib, &eb, &b_loop, &mut edge_pts)?;
        let ratio = match s.0.get("ratio") {
            None => 1.0,
            Some(v) => number(v, &format!("seam {} ratio", s.id_text()))?,
        };
        seam_info.push((
            s.id_text(),
            ia.id.clone(),
            ea,
            ib.id.clone(),
            eb,
            rev,
            ratio,
        ));
    }

    let mut mesh = GarmentMesh {
        instances: ids.iter().map(|s| (*s).to_string()).collect(),
        verts2d: Vec::new(),
        faces: Vec::new(),
        owner: Vec::new(),
        inst_range: Vec::new(),
        pairs: Vec::new(),
        seams: Vec::new(),
        corner: BTreeMap::new(),
        index: BTreeMap::new(),
    };
    for (k, inst) in insts.iter().enumerate() {
        let base = mesh.verts2d.len();
        let mut loop_pts: Vec<Vec2> = Vec::new();
        let mut loop_edge_idx = Vec::new();
        for e in &inst.edges {
            let key = (inst.id.clone(), e.id.clone());
            let pts = if let Some(p) = edge_pts.get(&key) {
                p.clone()
            } else {
                let dense = inst
                    .edge_dense(&e.id, EDGE_DENSE)
                    .map_err(|e| doc_error(&e))?;
                let n = ((polyline_length(&dense) / spacing).ceil() as usize).max(1);
                at_fractions(&dense, &linspace(0.0, 1.0, n + 1))
            };
            let start = loop_pts.len();
            loop_pts.extend_from_slice(&pts[..pts.len().saturating_sub(1)]);
            loop_edge_idx.push((e.id.clone(), e.from.clone(), start, pts.len()));
        }
        let nb = loop_pts.len();
        if nb < 3 {
            return Err(Refusal::new(
                RefusalCode::DegenerateGeometry,
                format!("{}: the outline has {nb} boundary samples", inst.id),
            ));
        }
        for (eid, from, start, n) in &loop_edge_idx {
            mesh.index.insert(
                (inst.id.clone(), eid.clone()),
                (0..*n).map(|j| base + (start + j) % nb).collect(),
            );
            mesh.corner
                .insert((inst.id.clone(), from.clone()), base + start);
        }
        let mut allp = loop_pts.clone();
        allp.extend(interior(&loop_pts, spacing));
        for f in triangulate(&allp, nb, &inst.id)? {
            mesh.faces.push(f.map(|j| base + j));
        }
        mesh.owner.extend(std::iter::repeat_n(k, allp.len()));
        mesh.verts2d.extend(allp);
        mesh.inst_range.push((base, mesh.verts2d.len()));
    }

    let pos = |id: &str| ids.iter().position(|x| *x == id).unwrap_or(0);
    for (sid, ia, ea, ib, eb, rev, ratio) in seam_info {
        let pa = path_indices(&mesh.index, &ia, &ea);
        let mut pb = path_indices(&mesh.index, &ib, &eb);
        if rev {
            pb.reverse();
        }
        if pa.len() != pb.len() {
            return Err(Refusal::new(
                RefusalCode::DegenerateGeometry,
                format!("seam {sid}: {} vs {} samples", pa.len(), pb.len()),
            ));
        }
        let sp: Vec<(usize, usize)> = pa
            .iter()
            .zip(&pb)
            .filter(|(a, b)| a != b)
            .map(|(a, b)| (*a, *b))
            .collect();
        mesh.pairs.extend_from_slice(&sp);
        if rev {
            pb.reverse();
        }
        mesh.seams.push(SeamRecord {
            id: sid,
            pairs: sp,
            side_a: pa,
            side_b: pb,
            ratio,
        });
    }
    for inst in &insts {
        if let Some(sc) = piece_def(inst).get("sim_scale").filter(|v| truthy(v)) {
            let (sx, sy) = (
                number(&sc[0], "sim_scale[0]")?,
                number(&sc[1], "sim_scale[1]")?,
            );
            let (lo, hi) = mesh.inst_range[pos(&inst.id)];
            for p in &mut mesh.verts2d[lo..hi] {
                *p = Vec2::new(p.x * sx, p.y * sy);
            }
        }
    }
    Ok(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rect(id: &str, w: f64, h: f64) -> Value {
        json!({
            "id": id, "name": id,
            "vertices": {"a": [0, 0], "b": [w, 0], "c": [w, h], "d": [0, h]},
            "edges": [
                {"id": "bottom", "from": "a", "to": "b"},
                {"id": "right", "from": "b", "to": "c", "curve": {"type": "quad", "c": [[0.5, 0.12]]}},
                {"id": "top", "from": "c", "to": "d"},
                {"id": "left", "from": "d", "to": "a"}
            ],
            "notches": [], "cut": {"qty": 1}, "grain": [[10, 10], [10, 40]],
            "placement": {"part": "torso"}
        })
    }

    /// Two rectangles sewn right edge (curved) to left edge: matched samples, CCW triangles covering
    /// both pieces, and a pinned digest so native and wasm agree.
    #[test]
    fn two_sewn_rectangles() {
        let doc = json!({
            "schema": "fabric-pattern/1", "units": "mm",
            "pieces": [rect("A", 100.0, 50.0), rect("B", 80.0, 50.0)],
            "seams": [{"id": "s1", "a": "A/right", "b": "B/left"}]
        });
        let g = build_garment(&Pattern::new(doc), 12.0).unwrap();
        assert_eq!(g.instances, ["A", "B"]);
        assert_eq!(g.seams.len(), 1);
        let s = &g.seams[0];
        assert_eq!(s.side_a.len(), s.side_b.len());
        assert_eq!(s.pairs.len(), s.side_a.len());
        for &(a, b) in &s.pairs {
            assert_eq!(g.owner[a], 0);
            assert_eq!(g.owner[b], 1);
        }
        let area = |f: &[usize; 3]| {
            let (a, b, c) = (g.verts2d[f[0]], g.verts2d[f[1]], g.verts2d[f[2]]);
            0.5 * ((b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x))
        };
        assert!(g.faces.iter().all(|f| area(f) > 0.0));
        let bytes: Vec<u8> = g
            .verts2d
            .iter()
            .flat_map(|p| [p.x, p.y])
            .flat_map(f64::to_le_bytes)
            .chain(
                g.faces
                    .iter()
                    .flatten()
                    .flat_map(|&i| (i as u64).to_le_bytes()),
            )
            .collect();
        let digest = ftx_core::receipt::sha256_hex(&bytes);
        println!(
            "{} vertices, {} triangles, digest {digest}",
            g.verts2d.len(),
            g.faces.len()
        );
        assert_eq!(
            digest,
            "180a58857b4049576200c36884c5420c96e3b3830fb7461f511fda5b368e78cf"
        );
    }
}

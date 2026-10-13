//! The kinematic start state against `tests/fixtures/oracle/drape/start_state.json` (captured in Blender
//! by `tools/oracle/capture_start_state.py`; spec 04 §3–4) for the 92 cm bolero and trousers.
//!
//! - The wrap is checked with the frames, anchor heights and radii the reference used (so the dress form
//!   itself is not needed) and must agree to rounding; gather weights and sewing groups exactly.
//! - Pre-sew targets must agree to float32 precision (Blender's `mathutils` is float32) for every sewing
//!   group whose nearest body point is unique. Where the nearest point is on a crease shared by several
//!   triangles, Blender keeps whichever its BVH meets first and ftx-drape the best-aligned normal; such
//!   groups are allowed to differ and are counted.
//! - The harmonic spread and the relaxation carry those few differences into their pieces, so the
//!   positions after pre-sew and after relaxation (each run from the reference's own input) are compared
//!   in distribution, and the relaxation must take the same number of blocks.

use ftx_conformance::{Compare, fixture};
use ftx_core::np::percentile;
use ftx_core::{Vec2, Vec3};
use ftx_drape::distance::point_triangle_closest;
use ftx_drape::start::{
    PRESEW_CLEARANCE, PRESEW_ITERS, RELAX_CLEARANCE, RELAX_ITERS, edge_strain, face_edges,
    gather_weights, presew, relax, sewing_groups,
};
use ftx_drape::{NearestTree, PartFrame, WrapPlacement, build_garment, start_state, wrap_points};
use ftx_pattern::Pattern;
use serde_json::Value;

fn v3(v: &Value) -> Vec3 {
    Vec3::new(
        v[0].as_f64().unwrap(),
        v[1].as_f64().unwrap(),
        v[2].as_f64().unwrap(),
    )
}

fn points(v: &Value) -> Vec<Vec3> {
    v.as_array().unwrap().iter().map(v3).collect()
}

/// Per-vertex distances: (median, p95, max).
fn stats(a: &[Vec3], b: &[Vec3]) -> (f64, f64, f64) {
    let d: Vec<f64> = a.iter().zip(b).map(|(p, q)| (*p - *q).norm()).collect();
    (
        percentile(&d, 50.0),
        percentile(&d, 95.0),
        d.iter().copied().fold(0.0, f64::max),
    )
}

/// Whether the nearest point of the soup to `p` is shared by triangles with different normals.
fn on_a_crease(p: Vec3, v: &[Vec3], t: &[[usize; 3]]) -> bool {
    let near: Vec<(f64, Vec3)> = t
        .iter()
        .map(|f| {
            let (a, b, c) = (v[f[0]], v[f[1]], v[f[2]]);
            let (w, _) = point_triangle_closest(p, a, b, c);
            (
                (p - (a * w[0] + b * w[1] + c * w[2])).norm(),
                (b - a).cross(c - a).normalized(),
            )
        })
        .collect();
    let dmin = near.iter().map(|x| x.0).fold(f64::INFINITY, f64::min);
    let tied: Vec<Vec3> = near
        .iter()
        .filter(|x| x.0 <= dmin * (1.0 + 1e-9))
        .map(|x| x.1)
        .collect();
    tied.iter().any(|n| (*n - tied[0]).norm() > 1e-6)
}

#[test]
#[allow(clippy::too_many_lines)] // one fixture, four stages
fn start_states_match_the_reference() {
    let fx = fixture("drape", "start_state");
    let mut cmp = Compare::new(1e-12);
    for case in fx["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let want = &case["output"];
        let mesh = build_garment(&Pattern::new(case["input"]["pattern"].clone()), 12.0).unwrap();
        // Wrap with the reference's frames, anchor heights and radii.
        let mut wrapped = vec![Vec3::ZERO; mesh.verts2d.len()];
        for (k, inst) in want["instances"].as_array().unwrap().iter().enumerate() {
            assert_eq!(
                mesh.instances[k],
                inst["id"].as_str().unwrap(),
                "{id}: order"
            );
            let f = &inst["frame"];
            let pl = &inst["placement"];
            let wp = WrapPlacement {
                frame: PartFrame {
                    origin: v3(&f["origin"]),
                    up: v3(&f["up"]),
                    front: v3(&f["front"]),
                    lat: v3(&f["lat"]),
                },
                h0: inst["h0"].as_f64().unwrap(),
                radius: inst["radius_mm"].as_f64().unwrap(),
                theta_deg: pl.get("theta_deg").and_then(Value::as_f64).unwrap_or(0.0),
                x0: pl.get("x0").and_then(Value::as_f64).unwrap_or(0.0),
                mirrored: inst["mirrored"].as_bool().unwrap(),
                own_side: matches!(inst["side"].as_str(), Some("R" | "L"))
                    && pl["part"].as_str() == Some("leg"),
            };
            let (lo, hi) = mesh.inst_range[k];
            let pts: Vec<Vec2> = mesh.verts2d[lo..hi].to_vec();
            wrapped[lo..hi].copy_from_slice(&wrap_points(&pts, &wp));
        }
        let want_wrapped = points(&want["wrapped"]);
        for (i, (p, w)) in wrapped.iter().zip(&want_wrapped).enumerate() {
            cmp.num(&format!("{id} wrapped {i}.x"), p.x, w.x);
            cmp.num(&format!("{id} wrapped {i}.y"), p.y, w.y);
            cmp.num(&format!("{id} wrapped {i}.z"), p.z, w.z);
        }
        // Gather weights and sewing groups.
        let gw = gather_weights(&mesh);
        cmp.nums(
            &format!("{id} gather weights"),
            &gw,
            &want["gather_weights"],
        );
        let groups = sewing_groups(mesh.verts2d.len(), &mesh.pairs);
        let mut want_groups: Vec<Vec<usize>> = want["groups"]
            .as_array()
            .unwrap()
            .iter()
            .map(|g| {
                g.as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap() as usize)
                    .collect()
            })
            .collect();
        want_groups.sort_by_key(|g| g[0]);
        assert_eq!(groups, want_groups, "{id}: sewing groups");
        let fv = points(&want["form"]["vertices"]);
        let ft: Vec<[usize; 3]> = want["form"]["triangles"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| std::array::from_fn(|j| t[j].as_u64().unwrap() as usize))
            .collect();
        let form = NearestTree::new(fv.clone(), ft.clone()).unwrap();
        let plain: Vec<(usize, usize)> = face_edges(&mesh.faces)
            .into_iter()
            .filter(|&(a, b)| gw[a] >= 1.0 && gw[b] >= 1.0)
            .collect();
        let blocks_of = |run: &Value| -> Vec<f64> {
            run["relax_blocks_p95"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap())
                .collect()
        };
        // Relaxation from `q`, `n` blocks: positions and the plain-edge p95 after each block.
        let relax_blocks = |mut q: Vec<Vec3>, n: usize| -> (Vec<Vec3>, Vec<f64>) {
            let mut blocks = Vec::new();
            for _ in 0..n {
                q = relax(&mesh, &q, &groups, &form, RELAX_ITERS, RELAX_CLEARANCE, &gw);
                let s: Vec<f64> = edge_strain(&q, &mesh.verts2d, &plain)
                    .into_iter()
                    .map(f64::abs)
                    .collect();
                blocks.push(percentile(&s, 95.0));
            }
            (q, blocks)
        };

        // Exact geometry: the reference's own pre-sew and relaxation in float64 with an exact
        // nearest-point query. These must agree to rounding.
        let exact = &want["exact"];
        let (presewn, _) = presew(&mesh, &want_wrapped, &form, PRESEW_CLEARANCE, PRESEW_ITERS);
        let (_, _, e_presew) = stats(&presewn, &points(&exact["presewn"]));
        let want_blocks = blocks_of(exact);
        let (relaxed, blocks) = relax_blocks(points(&exact["presewn"]), want_blocks.len());
        let (_, _, e_relax) = stats(&relaxed, &points(&exact["relaxed"]));
        let full = start_state(&mesh, wrapped.clone(), &form);
        let (_, _, e_full) = stats(&full.relaxed, &points(&exact["relaxed"]));
        println!(
            "{id} exact: pre-sew max {e_presew:.1e} m, relaxed max {e_relax:.1e} m, whole start state \
             max {e_full:.1e} m; blocks {blocks:.6?} vs {want_blocks:.6?}"
        );
        assert!(e_presew < 1e-9, "{id}: pre-sew differs by {e_presew:e} m");
        assert!(e_relax < 1e-9, "{id}: relaxation differs by {e_relax:e} m");
        assert!(
            e_full < 1e-9,
            "{id}: the start state differs by {e_full:e} m"
        );
        assert_eq!(
            full.relax_iterations,
            exact["relaxed_iterations"].as_u64().unwrap() as usize,
            "{id}: relaxation length"
        );
        for (k, (b, w)) in blocks.iter().zip(&want_blocks).enumerate() {
            assert!((b - w).abs() < 1e-9, "{id}: block {k} p95 {b} vs {w}");
        }

        // Blender: float32 queries, its own crease ties. Group targets with a unique nearest point
        // agree to float32 precision; the rest is compared in distribution.
        let blender = &want["blender"];
        let b_presewn = points(&blender["presewn"]);
        let mut crease_groups = 0;
        for g in &groups {
            let d = (presewn[g[0]] - b_presewn[g[0]]).norm();
            if d > 1e-6 {
                let m = g.iter().fold(Vec3::ZERO, |a, &v| a + want_wrapped[v]) / g.len() as f64;
                assert!(
                    on_a_crease(m, &fv, &ft),
                    "{id}: sewing group {} moved {d:e} m off Blender's with a unique nearest point",
                    g[0]
                );
                crease_groups += 1;
            }
        }
        let (p50, p95, pmax) = stats(&presewn, &b_presewn);
        let (b_relaxed, b_blocks) = relax_blocks(b_presewn.clone(), blocks_of(blender).len());
        let (r50, r95, rmax) = stats(&b_relaxed, &points(&blender["relaxed"]));
        println!(
            "{id} blender: {} groups, {crease_groups} on creases; pre-sew median {p50:.1e} p95 {p95:.1e} \
             max {pmax:.1e} m; relaxed median {r50:.1e} p95 {r95:.1e} max {rmax:.1e} m; blocks \
             {b_blocks:.4?} vs {:.4?}",
            groups.len(),
            blocks_of(blender)
        );
        assert!(
            crease_groups * 20 <= groups.len(),
            "{id}: too many crease groups"
        );
        assert!(
            p50 < 1e-4 && p95 < 2e-3,
            "{id}: pre-sew drifts from Blender's"
        );
        assert!(
            r50 < 2e-4 && r95 < 3e-3,
            "{id}: relaxation drifts from Blender's"
        );
    }
    cmp.finish("start_state wrap and gather weights");
}

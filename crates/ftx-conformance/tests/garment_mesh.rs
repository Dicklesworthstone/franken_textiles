//! The garment mesh against `tests/fixtures/oracle/drape/garment_mesh.json` (captured in Blender by
//! `tools/oracle/capture_garment_mesh.py`; spec 04 §2): the six simulated drafted garments at 12 mm and
//! the 92 cm bolero at 8 mm.
//!
//! Vertices, owners, instance ranges, sewing pairs, seam records, corners and edge index lists must
//! match exactly. Triangles come from a different constrained Delaunay (spade, not Blender's), so they
//! must match in count, orientation, total area and boundary coverage; identical triangles are counted
//! and reported, not required (cocircular hex points admit either diagonal).

use std::collections::{BTreeMap, BTreeSet};

use ftx_conformance::{Compare, fixture};
use ftx_core::Vec2;
use ftx_drape::build_garment;
use ftx_pattern::Pattern;
use serde_json::Value;

fn usizes(v: &Value) -> Vec<usize> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_u64().unwrap() as usize)
        .collect()
}

fn pairs(v: &Value) -> Vec<(usize, usize)> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p[0].as_u64().unwrap() as usize,
                p[1].as_u64().unwrap() as usize,
            )
        })
        .collect()
}

fn area(v: &[Vec2], f: &[usize; 3]) -> f64 {
    let (a, b, c) = (v[f[0]], v[f[1]], v[f[2]]);
    0.5 * ((b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x))
}

#[test]
fn garment_meshes_match_the_reference() {
    let fx = fixture("drape", "garment_mesh");
    let mut cmp = Compare::new(1e-12);
    let mut failures = Vec::new();
    let (mut same_faces, mut all_faces) = (0usize, 0usize);
    for case in fx["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let inp = &case["input"];
        let want = &case["output"];
        let pattern = Pattern::new(inp["pattern"].clone());
        let g = build_garment(&pattern, inp["spacing"].as_f64().unwrap())
            .unwrap_or_else(|r| panic!("{id}: {r}"));
        let inst: Vec<String> = want["instances"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap().to_string())
            .collect();
        assert_eq!(g.instances, inst, "{id}: instances");
        // Vertices.
        let wv = want["verts2d"].as_array().unwrap();
        assert_eq!(g.verts2d.len(), wv.len(), "{id}: vertex count");
        for (k, (p, w)) in g.verts2d.iter().zip(wv).enumerate() {
            cmp.num(&format!("{id} v{k}.x"), p.x, w[0].as_f64().unwrap());
            cmp.num(&format!("{id} v{k}.y"), p.y, w[1].as_f64().unwrap());
        }
        let owner: Vec<&str> = g.owner.iter().map(|&k| g.instances[k].as_str()).collect();
        let wo: Vec<&str> = want["owner"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap())
            .collect();
        assert_eq!(owner, wo, "{id}: owners");
        for (k, name) in g.instances.iter().enumerate() {
            let r = usizes(&want["inst_range"][name]);
            assert_eq!(
                (g.inst_range[k].0, g.inst_range[k].1),
                (r[0], r[1]),
                "{id}: range of {name}"
            );
        }
        // Sewing.
        assert_eq!(g.pairs, pairs(&want["pairs"]), "{id}: sewing pairs");
        let ws = want["seams"].as_array().unwrap();
        assert_eq!(g.seams.len(), ws.len(), "{id}: seam count");
        for (s, w) in g.seams.iter().zip(ws) {
            assert_eq!(s.id, w["id"].as_str().unwrap(), "{id}: seam order");
            assert_eq!(s.pairs, pairs(&w["pairs"]), "{id} {}: pairs", s.id);
            assert_eq!(s.side_a, usizes(&w["side_a"]), "{id} {}: side a", s.id);
            assert_eq!(s.side_b, usizes(&w["side_b"]), "{id} {}: side b", s.id);
            assert_eq!(
                s.ratio,
                w["ratio"].as_f64().unwrap(),
                "{id} {}: ratio",
                s.id
            );
        }
        let corner: BTreeMap<(String, String), usize> = want["corner"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                (
                    (c[0].as_str().unwrap().into(), c[1].as_str().unwrap().into()),
                    c[2].as_u64().unwrap() as usize,
                )
            })
            .collect();
        assert_eq!(g.corner, corner, "{id}: corners");
        let index: BTreeMap<(String, String), Vec<usize>> = want["index"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                (
                    (c[0].as_str().unwrap().into(), c[1].as_str().unwrap().into()),
                    usizes(&c[2]),
                )
            })
            .collect();
        assert_eq!(g.index, index, "{id}: edge index lists");
        // Triangles: count, orientation, area per instance, boundary coverage; identical ones counted.
        let wf: Vec<[usize; 3]> = want["faces"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| std::array::from_fn(|j| f[j].as_u64().unwrap() as usize))
            .collect();
        if g.faces.len() != wf.len() {
            failures.push(format!("{id}: {} triangles vs {}", g.faces.len(), wf.len()));
        }
        if let Some(f) = g.faces.iter().find(|f| area(&g.verts2d, f) <= 0.0) {
            failures.push(format!("{id}: triangle {f:?} is not counter-clockwise"));
        }
        for (k, &(lo, hi)) in g.inst_range.iter().enumerate() {
            let sum = |faces: &[[usize; 3]]| -> f64 {
                faces
                    .iter()
                    .filter(|f| (lo..hi).contains(&f[0]))
                    .map(|f| area(&g.verts2d, f))
                    .sum()
            };
            let (a, b) = (sum(&g.faces), sum(&wf));
            if (a - b).abs() > 1e-9 * b.abs() {
                failures.push(format!("{id} {}: area {a} vs {b}", g.instances[k]));
            }
        }
        let edges: BTreeSet<(usize, usize)> = g
            .faces
            .iter()
            .flat_map(|f| (0..3).map(move |j| (f[j].min(f[(j + 1) % 3]), f[j].max(f[(j + 1) % 3]))))
            .collect();
        for ((inst, e), idx) in &g.index {
            for w in idx.windows(2) {
                if w[0] != w[1] && !edges.contains(&(w[0].min(w[1]), w[0].max(w[1]))) {
                    failures.push(format!(
                        "{id}: boundary edge {inst}/{e} {w:?} is not a mesh edge"
                    ));
                }
            }
        }
        let canon = |f: &[usize; 3]| {
            let mut s = *f;
            s.sort_unstable();
            s
        };
        let ref_set: BTreeSet<[usize; 3]> = wf.iter().map(canon).collect();
        same_faces += g
            .faces
            .iter()
            .filter(|f| ref_set.contains(&canon(f)))
            .count();
        all_faces += wf.len();
    }
    cmp.finish("garment_mesh");
    println!(
        "triangles identical to the reference's: {same_faces} of {all_faces} ({:.1} %)",
        100.0 * same_faces as f64 / all_faces as f64
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

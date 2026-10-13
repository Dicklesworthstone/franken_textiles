//! Seam-allowance cut outlines against `tests/fixtures/oracle/export/outlines.json` (captured by
//! `tools/oracle/capture_exports.py`, spec 03 §3–4, §10.4): every piece as drafted and every instance of
//! TESTBODICE (also at SA 15 and mitre limit 2), ROBUST, the single test pieces and the drafted fez, bolero
//! and trousers at three sizes. Sew rings, edge breaks and seam allowances are bit-exact; the method
//! (mitre or union) matches; direct outlines agree vertex for vertex within 0.002 mm and union outlines
//! (GEOS in the reference) within a Hausdorff distance of 0.05 mm.

use ftx_conformance::fixture;
use ftx_core::Vec2;
use ftx_export::{cut_outline, drafted_instance, edge_loops, sew_breaks, sew_ring};
use ftx_pattern::{Instance, Pattern, Piece};
use serde_json::Value;

fn ring(v: &Value) -> Vec<Vec2> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|p| Vec2::new(p[0].as_f64().unwrap(), p[1].as_f64().unwrap()))
        .collect()
}

fn seg_dist(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let d = b - a;
    let l2 = d.x * d.x + d.y * d.y;
    let t = if l2 > 0.0 {
        (((p.x - a.x) * d.x + (p.y - a.y) * d.y) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (p - (a + d * t)).norm()
}

/// Largest distance from a vertex of `a` to the closed polyline `b`.
fn directed(a: &[Vec2], b: &[Vec2]) -> f64 {
    let n = b.len();
    a.iter()
        .map(|&p| {
            (0..n)
                .map(|i| seg_dist(p, b[i], b[(i + 1) % n]))
                .fold(f64::INFINITY, f64::min)
        })
        .fold(0.0, f64::max)
}

#[derive(Default)]
struct Tally {
    outlines: usize,
    sew_exact: usize,
    mitre: usize,
    union: usize,
    worst_mitre: f64,
    worst_union: f64,
    failures: Vec<String>,
}

impl Tally {
    #[allow(clippy::too_many_arguments)]
    fn check(
        &mut self,
        what: &str,
        inst: &Instance,
        piece: &Piece,
        fold: Option<&str>,
        sa: f64,
        limit: f64,
        want: &Value,
    ) {
        self.outlines += 1;
        let loops = edge_loops(inst, piece, fold, sa).unwrap();
        let sew: Vec<[f64; 2]> = sew_ring(&loops).iter().map(|p| [p.x, p.y]).collect();
        let want_sew: Vec<[f64; 2]> = ring(&want["sew"]).iter().map(|p| [p.x, p.y]).collect();
        let sew_close = sew.len() == want_sew.len()
            && sew.iter().zip(&want_sew).all(|(g, w)| {
                (g[0] - w[0]).abs() <= 1e-12 * w[0].abs().max(1.0)
                    && (g[1] - w[1]).abs() <= 1e-12 * w[1].abs().max(1.0)
            });
        self.sew_exact += usize::from(sew == want_sew);
        if !sew_close {
            let worst = sew
                .iter()
                .zip(&want_sew)
                .map(|(g, w)| (g[0] - w[0]).abs().max((g[1] - w[1]).abs()))
                .fold(0.0, f64::max);
            self.failures.push(format!(
                "{what}: sew ring differs ({} vs {} points, worst {worst:e})",
                sew.len(),
                want_sew.len()
            ));
        }
        let breaks: Vec<u64> = sew_breaks(&loops).iter().map(|&b| b as u64).collect();
        let want_breaks: Vec<u64> = want["breaks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap())
            .collect();
        if breaks != want_breaks {
            self.failures
                .push(format!("{what}: breaks {breaks:?}, want {want_breaks:?}"));
        }
        let sas: Vec<f64> = loops.iter().map(|l| l.sa).collect();
        let want_sas: Vec<f64> = want["sa"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        if sas != want_sas {
            self.failures.push(format!(
                "{what}: seam allowances {sas:?}, want {want_sas:?}"
            ));
        }
        let (got, method) = match cut_outline(inst, piece, fold, sa, limit) {
            Ok(x) => x,
            Err(e) => {
                self.failures.push(format!("{what}: {e:?}"));
                return;
            }
        };
        let want_ring = ring(&want["ring"]);
        let want_method = want["method"].as_str().unwrap();
        if method.as_str() != want_method {
            self.failures.push(format!(
                "{what}: method {}, want {want_method}",
                method.as_str()
            ));
            return;
        }
        if want_method == "mitre" {
            self.mitre += 1;
            if got.len() != want_ring.len() {
                self.failures.push(format!(
                    "{what}: {} vertices, want {}",
                    got.len(),
                    want_ring.len()
                ));
                return;
            }
            let worst = got
                .iter()
                .zip(&want_ring)
                .map(|(g, w)| (*g - *w).norm())
                .fold(0.0, f64::max);
            self.worst_mitre = self.worst_mitre.max(worst);
            if worst > 0.002 {
                self.failures
                    .push(format!("{what}: mitre outline off by {worst:.2e} mm"));
            }
        } else {
            self.union += 1;
            let h = directed(&got, &want_ring).max(directed(&want_ring, &got));
            self.worst_union = self.worst_union.max(h);
            if h > 0.05 {
                self.failures.push(format!(
                    "{what}: union outline Hausdorff {h:.3} mm ({} vs {} vertices)",
                    got.len(),
                    want_ring.len()
                ));
            }
        }
    }
}

#[test]
fn cut_outlines_match_the_reference() {
    let fx = fixture("export", "outlines");
    let mut t = Tally::default();
    for case in fx["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let (sa, limit) = (
            case["input"]["sa"].as_f64().unwrap(),
            case["input"]["mitre_limit"].as_f64().unwrap(),
        );
        let pat = Pattern::new(case["input"]["pattern"].clone());
        let doc = pat.doc.as_ref().unwrap();
        for (i, piece) in doc.pieces.iter().enumerate() {
            let inst = drafted_instance(piece, i);
            let want = &case["output"]["pieces"][&piece.id];
            t.check(
                &format!("{id}/{}", piece.id),
                &inst,
                piece,
                piece.on_fold(),
                sa,
                limit,
                want,
            );
        }
        for inst in &pat.instances {
            let piece = &doc.pieces[inst.piece_index];
            let want = &case["output"]["instances"][&inst.id];
            t.check(
                &format!("{id}/@{}", inst.id),
                inst,
                piece,
                None,
                sa,
                limit,
                want,
            );
        }
    }
    println!(
        "cut outlines: {} ({} sew rings bit-exact, the rest within 1e-12; {} mitre, worst vertex {:.2e} mm; {} union, worst Hausdorff {:.2e} mm)",
        t.outlines, t.sew_exact, t.mitre, t.worst_mitre, t.union, t.worst_union
    );
    assert!(
        t.failures.is_empty(),
        "{} failures:\n{}",
        t.failures.len(),
        t.failures.join("\n")
    );
}

//! Piece geometry against `tests/fixtures/oracle/export/geoms.json` (captured by
//! `tools/oracle/capture_exports.py`, spec 03 §5, §10.4): every piece as drafted and every instance of
//! TESTBODICE, ROBUST, the single test pieces and the drafted garments at text sizes 6 and 10. Facts, notches,
//! grainlines (given and default), fold marks, internal lines, drills, decor, the fold and internal labels,
//! grain marks and rotation agree within 1e-9 mm. The main label has the same lines and its block fits the
//! region a label may use; it is placed by a search for the largest block (the reference takes GEOS's pole and
//! a size bound it does not check), so it is never more than 5 % smaller than the reference's unless the
//! reference's block overlaps its region (spec 03 Q4), and is often larger.

use ftx_conformance::fixture;
use ftx_core::Vec2;
use ftx_export::{
    Geom, Source, build_geom, grain_marks, grain_vertical_rotation, label_texts, main_label_fits,
};
use ftx_pattern::Pattern;
use serde_json::{Value, json};

const TOL: f64 = 1e-9;

/// Polylines with a style or closed flag.
type Lines = Vec<(Vec<Vec2>, Value)>;

#[derive(Default)]
struct Check {
    numbers: usize,
    worst: f64,
    labels: usize,
    worst_size: f64,
    worst_anchor: f64,
    larger: usize,
    reference_overlaps: usize,
    failures: Vec<String>,
}

impl Check {
    fn num(&mut self, what: &str, got: f64, want: &Value) {
        let w = want.as_f64().unwrap_or(f64::NAN);
        self.numbers += 1;
        let d = (got - w).abs();
        self.worst = self.worst.max(d);
        if d.is_nan() || d > TOL * w.abs().max(1.0) {
            self.failures.push(format!("{what}: {got} vs {w}"));
        }
    }

    fn pt(&mut self, what: &str, got: Vec2, want: &Value) {
        self.num(&format!("{what}.x"), got.x, &want[0]);
        self.num(&format!("{what}.y"), got.y, &want[1]);
    }

    fn pts(&mut self, what: &str, got: &[Vec2], want: &Value) {
        let w = want.as_array().unwrap();
        if got.len() != w.len() {
            self.failures
                .push(format!("{what}: {} points vs {}", got.len(), w.len()));
            return;
        }
        for (i, (g, w)) in got.iter().zip(w).enumerate() {
            self.pt(&format!("{what}[{i}]"), *g, w);
        }
    }

    fn eq(&mut self, what: &str, got: Value, want: &Value) {
        if &got != want {
            self.failures.push(format!("{what}: {got} vs {want}"));
        }
    }

    fn geom(&mut self, what: &str, g: &Geom, w: &Value) {
        self.eq(
            &format!("{what}.ids"),
            json!([g.pid, g.iid, g.name]),
            &json!([w["pid"], w["iid"], w["name"]]),
        );
        let m = &w["meta"];
        let meta = json!({
            "qty": g.meta.qty, "mirror": g.meta.mirror, "material": g.meta.material, "on_fold": g.meta.on_fold,
            "fold_mode": g.meta.fold_mode.as_str(), "side": g.meta.side, "sa_mm": g.meta.sa_mm,
            "sa_values": g.meta.sa_values, "sim": g.meta.sim,
        });
        for (k, v) in meta.as_object().unwrap() {
            if let (Some(a), Some(b)) = (v.as_f64(), m[k].as_f64()) {
                self.num(&format!("{what}.meta.{k}"), a, &json!(b));
            } else if k == "sa_values" {
                let got: Vec<f64> = g.meta.sa_values.clone();
                let want: Vec<f64> = m[k]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|x| x.as_f64().unwrap())
                    .collect();
                if got != want {
                    self.failures
                        .push(format!("{what}.meta.sa_values: {got:?} vs {want:?}"));
                }
            } else {
                self.eq(&format!("{what}.meta.{k}"), v.clone(), &m[k]);
            }
        }
        self.eq(
            &format!("{what}.sew_breaks"),
            json!(g.sew_breaks),
            &w["sew_breaks"],
        );
        let wn = w["notches"].as_array().unwrap();
        if g.notches.len() != wn.len() {
            self.failures.push(format!(
                "{what}: {} notches vs {}",
                g.notches.len(),
                wn.len()
            ));
        } else {
            for (i, (n, wn)) in g.notches.iter().zip(wn).enumerate() {
                let at = format!("{what}.notch[{i}]");
                self.eq(
                    &at,
                    json!([n.id, n.kind, n.depth, n.width]),
                    &json!([wn["id"], wn["type"], wn["depth"], wn["width"]]),
                );
                self.pt(&format!("{at}.base"), n.base, &wn["base"]);
                self.pt(&format!("{at}.inward"), n.inward, &wn["inward"]);
                self.pt(&format!("{at}.tangent"), n.tangent, &wn["tangent"]);
                self.pt(&format!("{at}.sew_point"), n.sew_point, &wn["sew_point"]);
                for (k, (m, wm)) in n
                    .marks
                    .iter()
                    .zip(wn["marks"].as_array().unwrap())
                    .enumerate()
                {
                    self.pts(&format!("{at}.marks[{k}]"), m, wm);
                }
            }
        }
        match (g.grain, &w["grain"]) {
            (None, Value::Null) => {}
            (Some((a, b)), wg) if !wg.is_null() => {
                self.pts(&format!("{what}.grain"), &[a, b], wg);
            }
            (got, want) => self
                .failures
                .push(format!("{what}.grain: {got:?} vs {want}")),
        }
        self.eq(
            &format!("{what}.grain_default"),
            json!(g.grain_default),
            &w["grain_default"],
        );
        self.num(
            &format!("{what}.grain_rotation"),
            grain_vertical_rotation(g),
            &w["grain_rotation"],
        );
        for (k, (m, wm)) in grain_marks(g, 6.0)
            .iter()
            .zip(w["grain_marks"].as_array().unwrap())
            .enumerate()
        {
            self.pts(&format!("{what}.grain_marks[{k}]"), m, wm);
        }
        let lists: [(&str, Lines); 3] = [
            (
                "fold_lines",
                g.fold_lines
                    .iter()
                    .map(|(p, s)| (p.clone(), json!(s)))
                    .collect(),
            ),
            (
                "internal_draw",
                g.internal_draw
                    .iter()
                    .map(|(p, c)| (p.clone(), json!(c)))
                    .collect(),
            ),
            (
                "internal_cut",
                g.internal_cut
                    .iter()
                    .map(|(p, c)| (p.clone(), json!(c)))
                    .collect(),
            ),
        ];
        for (key, items) in lists {
            let want = w[key].as_array().unwrap();
            if items.len() != want.len() {
                self.failures
                    .push(format!("{what}.{key}: {} vs {}", items.len(), want.len()));
                continue;
            }
            for (i, ((p, flag), wi)) in items.iter().zip(want).enumerate() {
                self.pts(&format!("{what}.{key}[{i}]"), p, &wi["pts"]);
                let fk = if key == "fold_lines" {
                    "style"
                } else {
                    "closed"
                };
                self.eq(&format!("{what}.{key}[{i}].{fk}"), flag.clone(), &wi[fk]);
            }
        }
        let wd = w["drills"].as_array().unwrap();
        self.eq(
            &format!("{what}.drills"),
            json!(g.drills.len()),
            &json!(wd.len()),
        );
        for (i, ((p, d), wi)) in g.drills.iter().zip(wd).enumerate() {
            self.pt(&format!("{what}.drill[{i}]"), *p, &wi["p"]);
            self.num(&format!("{what}.drill[{i}].d"), *d, &wi["d"]);
        }
        let wd = w["decor"].as_array().unwrap();
        self.eq(
            &format!("{what}.decor"),
            json!(g.decor.len()),
            &json!(wd.len()),
        );
        for (i, (d, wi)) in g.decor.iter().zip(wd).enumerate() {
            self.eq(
                &format!("{what}.decor[{i}]"),
                json!([d.kind, d.width, d.closed]),
                &json!([wi["kind"], wi["width"], wi["closed"]]),
            );
            self.pts(&format!("{what}.decor[{i}].pts"), &d.pts, &wi["pts"]);
        }
        let wl = w["labels"].as_array().unwrap();
        if g.labels.len() != wl.len() {
            self.failures
                .push(format!("{what}: {} labels vs {}", g.labels.len(), wl.len()));
            return;
        }
        for (i, (lb, wi)) in g.labels.iter().zip(wl).enumerate() {
            let at = format!("{what}.label[{i}]");
            let lines: Vec<Value> = lb.lines.iter().map(|(t, r, b)| json!([t, r, b])).collect();
            self.eq(&format!("{at}.lines"), json!(lines), &wi["lines"]);
            self.eq(&format!("{at}.kind"), json!(lb.kind), &wi["kind"]);
            if i == 0 {
                self.labels += 1;
                let ws = wi["size"].as_f64().unwrap();
                let rel = (lb.size - ws).abs() / ws;
                self.worst_size = self.worst_size.max(rel);
                let wp = Vec2::new(wi["p"][0].as_f64().unwrap(), wi["p"][1].as_f64().unwrap());
                self.worst_anchor = self.worst_anchor.max((lb.p - wp).norm());
                if !main_label_fits(g, lb.p, &lb.lines, lb.size) {
                    self.failures
                        .push(format!("{at}: the block does not fit at {:?}", lb.p));
                }
                if lb.size > ws * 1.05 {
                    self.larger += 1;
                } else if lb.size < ws * 0.95 {
                    if main_label_fits(g, wp, &lb.lines, ws) {
                        self.failures.push(format!(
                            "{at}: size {:.3} vs {ws:.3} ({:.1} %)",
                            lb.size,
                            100.0 * rel
                        ));
                    } else {
                        self.reference_overlaps += 1;
                    }
                }
            } else {
                self.pt(&format!("{at}.p"), lb.p, &wi["p"]);
                self.num(&format!("{at}.size"), lb.size, &wi["size"]);
                let texts = label_texts(lb, true, 0.0, 1.0);
                let want = &w["label_texts"][2 * i];
                for (k, (x, y, t, s, b)) in texts.iter().enumerate() {
                    let wk = &want[k];
                    self.num(&format!("{at}.text[{k}].x"), *x, &wk[0]);
                    self.num(&format!("{at}.text[{k}].y"), *y, &wk[1]);
                    self.eq(
                        &format!("{at}.text[{k}]"),
                        json!([t, s, b]),
                        &json!([wk[2], wk[3], wk[4]]),
                    );
                }
            }
        }
    }
}

#[test]
fn piece_geometry_matches_the_reference() {
    let fx = fixture("export", "geoms");
    let mut c = Check::default();
    for case in fx["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let (sa, limit) = (
            case["input"]["sa"].as_f64().unwrap(),
            case["input"]["mitre_limit"].as_f64().unwrap(),
        );
        let pat = Pattern::new(case["input"]["pattern"].clone());
        let doc = pat.doc.as_ref().unwrap();
        for ts in [6.0, 10.0] {
            let out = &case["output"];
            for (i, piece) in doc.pieces.iter().enumerate() {
                let g = build_geom(&pat, Source::Piece(i), sa, limit, ts).unwrap();
                c.geom(
                    &format!("{id}@{ts}/{}", piece.id),
                    &g,
                    &out[format!("pieces@{ts}")][&piece.id],
                );
            }
            for inst in &pat.instances {
                let g = build_geom(&pat, Source::Instance(inst), sa, limit, ts).unwrap();
                c.geom(
                    &format!("{id}@{ts}/@{}", inst.id),
                    &g,
                    &out[format!("instances@{ts}")][&inst.id],
                );
            }
        }
    }
    println!(
        "piece geometry: {} numbers within {TOL:e} (worst {:.2e}); {} main labels all fitting their region: {} more than 5 % larger than the reference's, {} more than 5 % smaller where the reference's block overlaps its region (spec 03 Q4), the rest within 5 %; anchors up to {:.1} mm apart",
        c.numbers, c.worst, c.labels, c.larger, c.reference_overlaps, c.worst_anchor
    );
    assert!(
        c.failures.is_empty(),
        "{} failures:\n{}",
        c.failures.len(),
        c.failures
            .iter()
            .take(60)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

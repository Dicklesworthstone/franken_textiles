//! Construction diagnostics (spec 04 §11.8), not manufacturing output.
//!
//! Unlike a master SVG, a preview unfolds every physical instance, omits seam allowances and
//! highlights both sides of failed seams. Unknown scale is allowed but prominently labelled.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use ftx_core::{Refusal, RefusalCode, Vec2, det, py};
use ftx_pattern::{Instance, Pattern, Report, doc::truthy};
use serde_json::Value;

use crate::geom::{format_crease_angle, signed_crease_deg};
use crate::text::{fnum, xml_escape};

const GAP: f64 = 25.0;
const SCALE: f64 = 1.6;
const HEADER: f64 = 76.0;
const MAX_POINTS: usize = 1_000_000;
// Spec 04 §9.4's seam families, sorted across every seam, including non-simulated pieces.
const PALETTE: [&str; 10] = [
    "rgb(95%,75%,10%)",
    "rgb(10%,80%,90%)",
    "rgb(95%,30%,80%)",
    "rgb(30%,95%,30%)",
    "rgb(100%,50%,10%)",
    "rgb(60%,50%,100%)",
    "rgb(0%,50%,45%)",
    "rgb(90%,20%,20%)",
    "rgb(50%,30%,10%)",
    "rgb(15%,30%,90%)",
];

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::DegenerateGeometry, message)
}

fn family(id: &str) -> &str {
    id.rsplit_once('_').map_or(id, |(stem, _)| stem)
}

fn nonzero_crease_deg(raw: Option<&Value>) -> Option<f64> {
    let deg = raw.and_then(Value::as_f64)?;
    (deg.is_finite() && deg != 0.0 && deg.abs() < 180.0).then_some(deg)
}

fn fold_axis_for_piece(piece: &Value) -> Option<(Vec2, Vec2)> {
    let fold_id = piece
        .get("cut")
        .and_then(|c| c.get("on_fold"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())?;
    let edge = piece
        .get("edges")?
        .as_array()?
        .iter()
        .find(|e| e.get("id").and_then(Value::as_str) == Some(fold_id))?;
    let (from, to) = (edge.get("from")?.as_str()?, edge.get("to")?.as_str()?);
    let verts = piece.get("vertices")?.as_object()?;
    let pt = |k: &str| -> Option<Vec2> {
        let a = verts.get(k)?.as_array()?;
        Some(Vec2::new(a.first()?.as_f64()?, a.get(1)?.as_f64()?))
    };
    let (a, b) = (pt(from)?, pt(to)?);
    let len = det::hypot(b.x - a.x, b.y - a.y);
    (a.is_finite() && b.is_finite() && len.is_finite() && len > 0.0).then_some((a, b))
}

fn reflect_across(p: Vec2, a: Vec2, b: Vec2) -> Vec2 {
    let ab = b - a;
    let len = det::hypot(ab.x, ab.y);
    let d = Vec2::new(ab.x / len, ab.y / len);
    let w = p - a;
    let k = 2.0 * (w.x * d.x + w.y * d.y);
    Vec2::new((a.x + k * d.x) - w.x, (a.y + k * d.y) - w.y)
}

struct InstanceCrease {
    points: Vec<Vec2>,
    deg: f64,
    label: Option<String>,
}

fn instance_creases(pattern: &Pattern, instance: &Instance) -> Vec<InstanceCrease> {
    let Some(piece) = pattern.raw()["pieces"].get(instance.piece_index) else {
        return Vec::new();
    };
    let Some(marks) = piece.get("internal").and_then(Value::as_array) else {
        return Vec::new();
    };
    let fold_axis = fold_axis_for_piece(piece);
    let mut out = Vec::new();
    for item in marks {
        let Some(deg) = nonzero_crease_deg(item.get("crease_angle_deg")) else {
            continue;
        };
        if item.get("kind").and_then(Value::as_str) == Some("cut") {
            continue;
        }
        let Some(raw_pts) = item.get("pts").and_then(Value::as_array) else {
            continue;
        };
        if raw_pts.len() < 2 {
            continue;
        }
        let Some(mut drafted) = raw_pts
            .iter()
            .map(|pt| {
                let xy = pt.as_array()?;
                if xy.len() != 2 {
                    return None;
                }
                let x = xy[0].as_f64().filter(|v| v.is_finite())?;
                let y = xy[1].as_f64().filter(|v| v.is_finite())?;
                Some(Vec2::new(x, y))
            })
            .collect::<Option<Vec<Vec2>>>()
        else {
            continue;
        };
        if item.get("closed").is_some_and(truthy) {
            let mut ring: Vec<Vec2> = Vec::with_capacity(drafted.len());
            for pt in drafted {
                if ring
                    .last()
                    .is_none_or(|&prev| det::hypot(pt.x - prev.x, pt.y - prev.y) > 1e-6)
                {
                    ring.push(pt);
                }
            }
            if ring.len() > 1
                && det::hypot(
                    ring[0].x - ring.last().unwrap().x,
                    ring[0].y - ring.last().unwrap().y,
                ) <= 1e-6
            {
                ring.pop();
            }
            if ring.len() < 3 {
                continue;
            }
            ring.push(ring[0]);
            drafted = ring;
        } else if !drafted
            .windows(2)
            .any(|w| det::hypot(w[1].x - w[0].x, w[1].y - w[0].y) > 1e-6)
        {
            continue;
        }
        let label = item
            .get("label")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(String::from);
        let mut copies = vec![drafted.clone()];
        if let Some((fa, fb)) = fold_axis {
            let off_fold = drafted.iter().any(|&p| {
                let r = reflect_across(p, fa, fb);
                det::hypot(r.x - p.x, r.y - p.y) > 1e-3
            });
            if off_fold {
                copies.push(drafted.iter().map(|&p| reflect_across(p, fa, fb)).collect());
            }
        }
        for copy in copies {
            let points: Vec<Vec2> = if instance.mirrored {
                copy.into_iter().map(|p| Vec2::new(-p.x, p.y)).collect()
            } else {
                copy
            };
            if points.iter().all(|p| p.is_finite()) {
                out.push(InstanceCrease {
                    points,
                    deg,
                    label: label.clone(),
                });
            }
        }
    }
    out
}

struct Panel<'a> {
    instance: &'a Instance,
    outline: Vec<Vec2>,
    min: Vec2,
    max: Vec2,
    origin: Vec2,
}

impl Panel<'_> {
    fn mapped(&self, p: Vec2) -> Vec2 {
        Vec2::new(
            SCALE * (self.origin.x + (p.x - self.min.x)),
            HEADER + SCALE * (self.origin.y + (self.max.y - p.y)),
        )
    }

    fn path(&self, points: &[Vec2]) -> String {
        points
            .iter()
            .enumerate()
            .map(|(i, &p)| {
                let q = self.mapped(p);
                format!(
                    "{}{} {}",
                    if i == 0 { "M" } else { "L" },
                    fnum(q.x, 3),
                    fnum(q.y, 3)
                )
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn checked_points(points: &[Vec2], total: &mut usize) -> Result<(), Refusal> {
    *total = total.checked_add(points.len()).ok_or_else(|| {
        Refusal::new(RefusalCode::BudgetExceeded, "preview point budget overflow")
    })?;
    if *total > MAX_POINTS {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "preview exceeds one million points",
        ));
    }
    if points.iter().any(|p| !p.x.is_finite() || !p.y.is_finite()) {
        return Err(Refusal::new(
            RefusalCode::NonFinite,
            "non-finite preview geometry",
        ));
    }
    Ok(())
}

/// A self-contained, script-free SVG showing the validation's seam failures on physical instances.
/// Curves use 80 samples, shelves have 25-unit gaps, and the y-up pattern is mapped to y-down SVG.
/// Failed seam walks and failed truing highlight **both** referenced sides last, above passing seams.
/// All identifiers and diagnostic text are XML-escaped. No geometry or report is modified.
///
/// `report` must be the validation of this pattern. BLOCKED reports get an explanatory panel rather
/// than unreliable geometry. The output deliberately has pixel dimensions, no cutting outlines and
/// a "not for cutting" notice; an unscaled trace is never represented as a true-size pattern.
///
/// # Errors
/// Missing/degenerate or non-finite geometry, non-finite layout arithmetic, or the point budget.
pub fn diagnostic_svg(pattern: &Pattern, report: &Report, dark: bool) -> Result<String, Refusal> {
    let foreground = if dark { "#eeeeee" } else { "#202020" };
    let background = if dark { "#181a22" } else { "#ffffff" };
    let unscaled = pattern.raw()["meta"]["scale_status"] == "UNSCALED";
    let heading = format!(
        "Construction preview — {}{}",
        report.status(),
        if unscaled { " — UNSCALED" } else { "" }
    );
    let notice = if unscaled {
        "Unscaled image coordinates: no physical dimensions. Diagnostic only — not for cutting."
    } else {
        "Diagnostic only — not for cutting. Red seams fail validation or truing; no physical-fit claim."
    };
    let blocked = match report {
        Report::Blocked {
            structure_errors, ..
        } => structure_errors.clone(),
        Report::Full { .. } => Vec::new(),
    };
    let (mut panels, mut total) = (Vec::new(), 0usize);
    if blocked.is_empty() {
        for instance in &pattern.instances {
            let outline = instance.outline(80).map_err(|e| invalid(e.to_string()))?;
            checked_points(&outline, &mut total)?;
            let min = Vec2::new(
                outline.iter().map(|p| p.x).fold(f64::INFINITY, f64::min),
                outline.iter().map(|p| p.y).fold(f64::INFINITY, f64::min),
            );
            let max = Vec2::new(
                outline
                    .iter()
                    .map(|p| p.x)
                    .fold(f64::NEG_INFINITY, f64::max),
                outline
                    .iter()
                    .map(|p| p.y)
                    .fold(f64::NEG_INFINITY, f64::max),
            );
            if outline.len() < 3 || max.x <= min.x || max.y <= min.y {
                return Err(invalid(format!("{} has no drawable outline", instance.id)));
            }
            panels.push(Panel {
                instance,
                outline,
                min,
                max,
                origin: Vec2::ZERO,
            });
        }
        if panels.is_empty() {
            return Err(invalid("pattern has no drawable instances"));
        }
    }
    let sum: f64 = panels.iter().map(|p| p.max.x - p.min.x + GAP).sum();
    let max_h = panels.iter().map(|p| p.max.y - p.min.y).fold(1.0, f64::max);
    let rows = py::round((sum / (1.6 * max_h)).sqrt(), 0).max(1.0);
    let row_width = panels
        .iter()
        .map(|p| p.max.x - p.min.x + GAP)
        .fold(sum / rows, f64::max);
    let (mut x, mut y, mut shelf, mut used) = (GAP, GAP, 0.0f64, 0.0f64);
    for p in &mut panels {
        let (w, h) = (p.max.x - p.min.x, p.max.y - p.min.y);
        if x > GAP && x - GAP + w + GAP > 1.05 * row_width {
            x = GAP;
            y += shelf + GAP;
            shelf = 0.0;
        }
        p.origin = Vec2::new(x, y);
        x += w + GAP;
        shelf = shelf.max(h);
        used = used.max(x);
    }
    let width = (used * SCALE).max(920.0);
    let height = (HEADER + (y + shelf + GAP) * SCALE).max(120.0 + 24.0 * blocked.len() as f64);
    if ![sum, max_h, row_width, width, height]
        .iter()
        .all(|v| v.is_finite())
    {
        return Err(Refusal::new(
            RefusalCode::NonFinite,
            "preview layout overflow",
        ));
    }
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\" role=\"img\" aria-labelledby=\"title desc\">\n<title id=\"title\">{}</title>\n<desc id=\"desc\">{}</desc>\n<rect width=\"100%\" height=\"100%\" fill=\"{background}\"/>\n<g font-family=\"sans-serif\" fill=\"{foreground}\">\n<text x=\"20\" y=\"28\" font-size=\"22\">{}</text>\n<text x=\"20\" y=\"53\" font-size=\"14\">{}</text>\n",
        fnum(width, 3),
        fnum(height, 3),
        fnum(width, 3),
        fnum(height, 3),
        xml_escape(&heading),
        xml_escape(notice),
        xml_escape(&heading),
        xml_escape(notice)
    );
    for (i, error) in blocked.iter().enumerate() {
        let _ = writeln!(
            svg,
            "<text x=\"20\" y=\"{}\" font-size=\"14\">{}</text>",
            96 + 24 * i,
            xml_escape(error)
        );
    }
    for p in &panels {
        let _ = writeln!(
            svg,
            "<path data-instance=\"{}\" d=\"{} Z\" fill=\"none\" stroke=\"{foreground}\" stroke-width=\"1.5\"/>",
            xml_escape(&p.instance.id),
            p.path(&p.outline)
        );
    }
    let valley_colour = if dark { "#38bdf8" } else { "#0e7490" };
    let mountain_colour = if dark { "#fbbf24" } else { "#b45309" };
    for p in &panels {
        for crease in instance_creases(pattern, p.instance) {
            checked_points(&crease.points, &mut total)?;
            let (fold, colour, dash) = if crease.deg > 0.0 {
                ("valley", valley_colour, "8 4")
            } else {
                ("mountain", mountain_colour, "8 3 2 3")
            };
            let title = match &crease.label {
                Some(label) => format!("{label}: {}", format_crease_angle("CREASE", crease.deg)),
                None => format_crease_angle("CREASE", crease.deg),
            };
            let _ = writeln!(
                svg,
                "<path data-crease-instance=\"{}\" data-crease-angle-deg=\"{}\" data-crease-fold=\"{fold}\" d=\"{}\" fill=\"none\" stroke=\"{colour}\" stroke-width=\"2.5\" stroke-dasharray=\"{dash}\"><title>{}</title></path>",
                xml_escape(&p.instance.id),
                signed_crease_deg(crease.deg),
                p.path(&crease.points),
                xml_escape(&title)
            );
        }
    }
    let mut failed = BTreeSet::new();
    if let Report::Full { seams, truing, .. } = report {
        for row in seams
            .iter()
            .filter(|s| matches!(s.status.as_str(), "FAIL" | "ERROR"))
        {
            if let Some(id) = row.seam.as_str() {
                failed.insert(id);
            }
        }
        for row in truing.iter().filter(|s| s.status == "FAIL") {
            if let Some(id) = row.seam.as_str() {
                failed.insert(id);
            }
        }
    }
    let families: BTreeSet<&str> = pattern
        .seams()
        .iter()
        .filter_map(|s| s.id().map(family))
        .collect();
    let colours: BTreeMap<&str, &str> = families
        .into_iter()
        .enumerate()
        .map(|(i, f)| (f, PALETTE[i % PALETTE.len()]))
        .collect();
    // Two passes prevent a later passing seam sharing an edge from hiding a failure.
    for failing in [false, true] {
        for seam in pattern.seams() {
            let Some(id) = seam.id() else { continue };
            if failed.contains(id) != failing {
                continue;
            }
            let colour = if failing {
                "#dc1e1e"
            } else {
                colours[family(id)]
            };
            for side in ["a", "b"] {
                let Some(reference) = seam.side_ref(side) else {
                    continue;
                };
                let Ok((instance, edges)) = pattern.side(reference) else {
                    continue;
                };
                let Some(panel) = panels.iter().find(|p| p.instance.id == instance.id) else {
                    continue;
                };
                for edge in edges {
                    let points = instance
                        .edge_dense(&edge, 80)
                        .map_err(|e| invalid(e.to_string()))?;
                    checked_points(&points, &mut total)?;
                    let crease_deg = nonzero_crease_deg(seam.0.get("crease_angle_deg"));
                    let crease_attr = crease_deg.map_or_else(String::new, |deg| {
                        let fold = if deg > 0.0 { "valley" } else { "mountain" };
                        format!(
                            " data-crease-angle-deg=\"{}\" data-crease-fold=\"{fold}\"",
                            signed_crease_deg(deg)
                        )
                    });
                    let title = match crease_deg {
                        Some(deg) => {
                            format!("{reference} ({})", format_crease_angle("crease", deg))
                        }
                        None => reference.to_string(),
                    };
                    let _ = writeln!(
                        svg,
                        "<path data-seam=\"{}\" data-side=\"{side}\" data-failing=\"{failing}\"{crease_attr} d=\"{}\" fill=\"none\" stroke=\"{colour}\" stroke-width=\"{}\"><title>{}</title></path>",
                        xml_escape(id),
                        panel.path(&points),
                        if failing { 6 } else { 4 },
                        xml_escape(&title)
                    );
                }
            }
        }
    }
    for p in &panels {
        let q = p.outline.iter().fold(Vec2::ZERO, |sum, &v| {
            sum + p.mapped(v) / p.outline.len() as f64
        });
        // Two layers work in SVG 1.1 renderers too: an unsupported paint-order property would
        // otherwise paint the halo over the glyph and make the instance label disappear.
        let (x, y, label) = (fnum(q.x, 3), fnum(q.y, 3), xml_escape(&p.instance.id));
        let _ = writeln!(
            svg,
            "<text x=\"{x}\" y=\"{y}\" text-anchor=\"middle\" font-size=\"16\" aria-hidden=\"true\" fill=\"{background}\" stroke=\"{background}\" stroke-width=\"4\">{label}</text>\n<text x=\"{x}\" y=\"{y}\" text-anchor=\"middle\" font-size=\"16\" fill=\"{foreground}\">{label}</text>"
        );
    }
    svg.push_str("</g>\n</svg>\n");
    Ok(svg)
}

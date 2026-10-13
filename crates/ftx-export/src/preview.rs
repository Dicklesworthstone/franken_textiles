//! Construction diagnostics (spec 04 §11.8), not manufacturing output.
//!
//! Unlike a master SVG, a preview unfolds every physical instance, omits seam allowances and
//! highlights both sides of failed seams. Unknown scale is allowed but prominently labelled.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use ftx_core::{Refusal, RefusalCode, Vec2, py};
use ftx_pattern::{Instance, Pattern, Report};

use crate::text::{fnum, xml_escape};

const GAP: f64 = 25.0;
const SCALE: f64 = 1.6;
const HEADER: f64 = 76.0;
const MAX_POINTS: usize = 1_000_000;
// Spec 04 §9.4's seam families, sorted across every seam, including non-simulated pieces.
const PALETTE: [&str; 10] = [
    "rgb(95% 75% 10%)",
    "rgb(10% 80% 90%)",
    "rgb(95% 30% 80%)",
    "rgb(30% 95% 30%)",
    "rgb(100% 50% 10%)",
    "rgb(60% 50% 100%)",
    "rgb(0% 50% 45%)",
    "rgb(90% 20% 20%)",
    "rgb(50% 30% 10%)",
    "rgb(15% 30% 90%)",
];

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::DegenerateGeometry, message)
}

fn family(id: &str) -> &str {
    id.rsplit_once('_').map_or(id, |(stem, _)| stem)
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
                    let _ = writeln!(
                        svg,
                        "<path data-seam=\"{}\" data-side=\"{side}\" data-failing=\"{failing}\" d=\"{}\" fill=\"none\" stroke=\"{colour}\" stroke-width=\"{}\"><title>{}</title></path>",
                        xml_escape(id),
                        panel.path(&points),
                        if failing { 6 } else { 4 },
                        xml_escape(reference)
                    );
                }
            }
        }
    }
    for p in &panels {
        let q = p.outline.iter().fold(Vec2::ZERO, |sum, &v| {
            sum + p.mapped(v) / p.outline.len() as f64
        });
        let _ = writeln!(
            svg,
            "<text x=\"{}\" y=\"{}\" text-anchor=\"middle\" font-size=\"16\" paint-order=\"stroke\" stroke=\"{background}\" stroke-width=\"4\">{}</text>",
            fnum(q.x, 3),
            fnum(q.y, 3),
            xml_escape(&p.instance.id)
        );
    }
    svg.push_str("</g>\n</svg>\n");
    Ok(svg)
}

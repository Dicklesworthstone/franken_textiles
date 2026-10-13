//! Acceptance checks (`docs/spec/03` §8): each output file read back on its own. So far the SVG checks.

use serde_json::{Value, json};

use crate::xml::parse;

/// One check of one file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckItem {
    /// Check name (`svg.no_transform`, ...).
    pub check: String,
    /// File name.
    pub file: String,
    /// Passed.
    pub ok: bool,
    /// What was measured.
    pub detail: String,
}

impl CheckItem {
    fn new(check: &str, file: &str, ok: bool, detail: impl Into<String>) -> Self {
        Self {
            check: check.into(),
            file: file.into(),
            ok,
            detail: detail.into(),
        }
    }

    /// `{check, file, ok, detail}`.
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({"check": self.check, "file": self.file, "ok": self.ok, "detail": self.detail})
    }
}

/// The checks of the master SVG (`cutter: false`) or of a cutter SVG: no `transform` anywhere, millimetre
/// width and height matching the viewBox, and every piece group with all its sub-groups (master) or a single
/// closed path (cutter).
#[must_use]
pub fn check_svg(text: &str, file: &str, cutter: bool) -> Vec<CheckItem> {
    let mut out = vec![CheckItem::new(
        "svg.no_transform",
        file,
        !text.contains("transform"),
        "",
    )];
    let root = parse(text).unwrap_or_default();
    let attr = |k: &str| root.get(k).unwrap_or("").to_string();
    let (w, h, vb) = (attr("width"), attr("height"), attr("viewBox"));
    let mut ok = w.ends_with("mm") && h.ends_with("mm");
    if ok {
        let v: Option<Vec<f64>> = vb.split_whitespace().map(|q| q.parse().ok()).collect();
        let num = |s: &str| s[..s.len() - 2].trim().parse::<f64>().ok();
        ok = match (v, num(&w), num(&h)) {
            (Some(v), Some(wv), Some(hv)) => {
                v.len() == 4
                    && v[0] == 0.0
                    && v[1] == 0.0
                    && (v[2] - wv).abs() < 1e-6
                    && (v[3] - hv).abs() < 1e-6
            }
            _ => false,
        };
    }
    out.push(CheckItem::new(
        "svg.mm_units_viewbox",
        file,
        ok,
        format!("{w} x {h}, viewBox {vb}"),
    ));
    if cutter {
        let paths: Vec<_> = root
            .descendants()
            .into_iter()
            .filter(|e| e.name == "path")
            .collect();
        out.push(CheckItem::new(
            "cutter.single_closed_path",
            file,
            paths.len() == 1 && paths[0].get("d").unwrap_or("").trim_end().ends_with('Z'),
            "",
        ));
    } else {
        const NEED: [&str; 10] = [
            "cut",
            "sew",
            "notches",
            "grain",
            "fold",
            "internal-draw",
            "internal-cut",
            "drills",
            "labels",
            "decor-soutache",
        ];
        let bad: Vec<String> = root
            .children
            .iter()
            .filter(|g| g.name == "g" && g.get("id").unwrap_or("").starts_with("piece-"))
            .filter(|g| {
                let have: Vec<&str> = g
                    .children
                    .iter()
                    .filter(|c| c.name == "g")
                    .filter_map(|c| c.get("class"))
                    .collect();
                !NEED.iter().all(|n| have.contains(n))
            })
            .map(|g| g.get("id").unwrap_or("").to_string())
            .collect();
        out.push(CheckItem::new(
            "svg.piece_subgroups",
            file,
            bad.is_empty(),
            bad.join(", "),
        ));
    }
    out
}

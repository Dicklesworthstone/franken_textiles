//! The export context (`docs/spec/03` §1.2): options, the names every file uses and the master layout.

use std::cell::OnceCell;

use ftx_core::polyline::signed_area;
use ftx_core::{Refusal, RefusalCode, py};
use ftx_pattern::{Instance, Pattern, Side};
use serde_json::Value;

use crate::geom::{Geom, Source, build_geom};
use crate::layout::{Layout, shelf_layout};
use crate::outline::{DEFAULT_SA, cut_outline};
use crate::pyjson::PyJson;
use crate::text::{ascii_text, safe_name};

/// Export options (the CLI's flags).
#[derive(Clone, Debug, PartialEq)]
pub struct ExportOptions {
    /// Show pieces cut on the fold unfolded in the master, tiles, copy shop and HPGL.
    pub unfold: bool,
    /// Mitre limit in multiples of the seam allowance.
    pub mitre_limit: f64,
    /// Projector sheet width (mm).
    pub projector_width: f64,
    /// Master layout width (mm); automatic when `None`.
    pub layout_width: Option<f64>,
    /// Keep blank tiles.
    pub keep_blank: bool,
    /// Seam allowance override (mm).
    pub sa: Option<f64>,
    /// HPGL turned 90°.
    pub hpgl_rotate90: bool,
    /// HPGL mirrored.
    pub hpgl_mirror: bool,
    /// Creation time (`YYYY-MM-DDTHH:MM`) for file dates and the manifest; the pattern's `meta.created` when
    /// `None`.
    pub created: Option<String>,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            unfold: false,
            mitre_limit: 4.0,
            projector_width: 1500.0,
            layout_width: None,
            keep_blank: false,
            sa: None,
            hpgl_rotate90: false,
            hpgl_mirror: false,
            created: None,
        }
    }
}

/// What every export of one pattern shares.
pub struct Ctx<'a> {
    /// The pattern.
    pub pattern: &'a Pattern,
    /// `safe_name(style)` (20 characters): every file stem.
    pub style: String,
    /// The size label as ASCII (`base` when empty).
    pub size: String,
    /// The default seam allowance.
    pub sa: f64,
    /// The options.
    pub opts: ExportOptions,
    /// Creation time (`YYYY-MM-DDTHH:MM`; empty when neither the options nor the pattern give one).
    pub created: String,
    master: OnceCell<Layout>,
}

/// The first instance of a piece with side C, else R, else the first.
#[must_use]
pub fn primary_instance<'a>(pattern: &'a Pattern, pid: &str) -> Option<&'a Instance> {
    let insts: Vec<&Instance> = pattern
        .instances
        .iter()
        .filter(|i| i.piece == pid)
        .collect();
    [Side::C, Side::R]
        .iter()
        .find_map(|s| insts.iter().find(|i| i.side == *s).copied())
        .or_else(|| insts.first().copied())
}

impl<'a> Ctx<'a> {
    /// The context of a pattern.
    ///
    /// # Errors
    /// `unscaled` for a pattern traced in pixels (`meta.scale_status` UNSCALED): every file claims true
    /// size.
    pub fn new(pattern: &'a Pattern, opts: ExportOptions) -> Result<Self, Refusal> {
        let raw = pattern.raw();
        let meta = &raw["meta"];
        if meta["scale_status"] == "UNSCALED" {
            return Err(Refusal::new(
                RefusalCode::Unscaled,
                "pattern is UNSCALED (1 unit = 1 px of the traced sheet): run `vectorize scale-fit` before \
                 exporting; its PDFs and machine files would be printed and cut at the wrong size",
            ));
        }
        let style = match raw.get("style") {
            Some(Value::String(s)) if !s.is_empty() => s.clone(),
            Some(v) if !v.is_null() && v != &Value::Bool(false) => v.to_string(),
            _ => "STYLE".to_string(),
        };
        let size_label = match meta.get("size_label") {
            Some(Value::String(s)) if !s.is_empty() => s.clone(),
            Some(Value::Number(n)) => n.to_string(),
            _ => "base".to_string(),
        };
        let size = ascii_text(&size_label);
        let sa = opts
            .sa
            .or_else(|| meta.get("sa_mm").and_then(Value::as_f64))
            .unwrap_or(DEFAULT_SA);
        let created = opts
            .created
            .clone()
            .or_else(|| meta["created"].as_str().map(str::to_string))
            .unwrap_or_default();
        Ok(Self {
            pattern,
            created,
            style: safe_name(&style, 20),
            size: if size.is_empty() { "base".into() } else { size },
            sa,
            opts,
            master: OnceCell::new(),
        })
    }

    /// The geometry of a piece or an instance at a text size.
    ///
    /// # Errors
    /// As [`build_geom`].
    pub fn geom(&self, src: Source, text_size: f64) -> Result<Geom, Refusal> {
        build_geom(self.pattern, src, self.sa, self.opts.mitre_limit, text_size)
    }

    /// Every instance's geometry (the cutter files, flat DXF, DST): the main label's cut line names the side
    /// (`Cut 1 (R)`, `Cut 1 (L, mirrored)`) of a mirror pair.
    ///
    /// # Errors
    /// As [`build_geom`].
    pub fn instance_geoms(&self, text_size: f64) -> Result<Vec<Geom>, Refusal> {
        self.pattern
            .instances
            .iter()
            .map(|inst| {
                let mut g = self.geom(Source::Instance(inst), text_size)?;
                if matches!(inst.side, Side::R | Side::L) {
                    g.labels[0].lines[2] = (
                        format!(
                            "Cut 1 ({}{})",
                            inst.side.as_str(),
                            if inst.mirrored { ", mirrored" } else { "" }
                        ),
                        0.65,
                        false,
                    );
                }
                Ok(g)
            })
            .collect()
    }

    /// The manifest's `pieces`: per piece, keyed by its DXF block name (`safe_name(id)`, on a collision
    /// `safe_name(id, 17)_k`), the primary instance and its sew and cut line perimeters and areas.
    ///
    /// # Errors
    /// As [`cut_outline`].
    pub fn piece_reference(&self) -> Result<PyJson, Refusal> {
        let doc = self.pattern.doc.as_ref().ok_or_else(|| {
            Refusal::new(
                RefusalCode::InvalidDocument,
                "the pattern has no typed document",
            )
        })?;
        let mut items: Vec<(String, PyJson)> = Vec::new();
        for piece in &doc.pieces {
            let inst = primary_instance(self.pattern, &piece.id).ok_or_else(|| {
                Refusal::new(
                    RefusalCode::InvalidDocument,
                    format!("{}: no instance", piece.id),
                )
            })?;
            let mut name = safe_name(&piece.id, 20);
            let mut k = 2;
            while items.iter().any(|(n, _)| *n == name) {
                name = format!("{}_{k}", safe_name(&piece.id, 17));
                k += 1;
            }
            let (cut, _) = cut_outline(
                inst,
                &doc.pieces[inst.piece_index],
                None,
                self.sa,
                self.opts.mitre_limit,
            )?;
            let py_err =
                |e: ftx_pattern::PyError| Refusal::new(RefusalCode::InvalidDocument, e.to_string());
            let lengths: Vec<f64> = cut
                .iter()
                .zip(cut.iter().cycle().skip(1))
                .map(|(a, b)| ((b.x - a.x) * (b.x - a.x) + (b.y - a.y) * (b.y - a.y)).sqrt())
                .collect();
            let perimeter = ftx_core::np::pairwise_sum(&lengths);
            items.push((
                name,
                PyJson::dict(vec![
                    ("piece", PyJson::str(&piece.id)),
                    ("instance", PyJson::str(&inst.id)),
                    (
                        "sew_perimeter",
                        PyJson::Float(py::round(inst.perimeter().map_err(py_err)?, 3)),
                    ),
                    (
                        "sew_area",
                        PyJson::Float(py::round(
                            signed_area(&inst.outline(200).map_err(py_err)?),
                            2,
                        )),
                    ),
                    ("cut_perimeter", PyJson::Float(py::round(perimeter, 3))),
                    ("cut_area", PyJson::Float(py::round(signed_area(&cut), 2))),
                ]),
            ));
        }
        Ok(PyJson::Dict(items))
    }

    /// The master layout (master SVG, tiles, copy shop, HPGL): per piece its drafted half when cut on the fold
    /// (unless `unfold`), else its primary instance; shelves 15 mm apart, 10 mm margin.
    ///
    /// # Errors
    /// As [`build_geom`].
    pub fn master(&self) -> Result<&Layout, Refusal> {
        if let Some(l) = self.master.get() {
            return Ok(l);
        }
        let doc = self.pattern.doc.as_ref().ok_or_else(|| {
            Refusal::new(
                RefusalCode::InvalidDocument,
                "the pattern has no typed document",
            )
        })?;
        let mut geoms = Vec::new();
        for (i, piece) in doc.pieces.iter().enumerate() {
            let fold = piece.on_fold().filter(|f| !f.is_empty());
            let src = if self.opts.unfold || fold.is_none() {
                let inst = primary_instance(self.pattern, &piece.id).ok_or_else(|| {
                    Refusal::new(
                        RefusalCode::InvalidDocument,
                        format!("{}: no instance", piece.id),
                    )
                })?;
                Source::Instance(inst)
            } else {
                Source::Piece(i)
            };
            geoms.push(self.geom(src, 6.0)?);
        }
        let lay = shelf_layout(&geoms, 15.0, 10.0, self.opts.layout_width, false, true, 0.0);
        Ok(self.master.get_or_init(|| lay))
    }
}

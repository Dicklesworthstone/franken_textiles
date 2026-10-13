//! Cutter SVGs and their report (`docs/spec/03` §7.10): one file per instance for a rotary-blade cutting
//! machine, grain vertical, notches as small outward triangles, with whether it fits a 12 × 12 or 12 × 24 in mat.

use ftx_core::{Refusal, Vec2, py};

use crate::ctx::Ctx;
use crate::geom::grain_vertical_rotation;
use crate::layout::rotation;
use crate::outline::clean_ring;
use crate::pyjson::PyJson;
use crate::text::{fnum, safe_name, xml_escape};
use crate::union::union_exterior;

const MAT_W: f64 = 11.5 * 25.4;
const MAT_LONG: f64 = 23.5 * 25.4;

/// A cutter file: its path below the output directory and its text.
#[derive(Clone, Debug)]
pub struct CutterFile {
    /// `<S>_cutter/<instance>_<size>.svg`.
    pub path: String,
    /// The SVG.
    pub text: String,
}

/// The cutter SVGs, the report's path and text (`cutter_report.json`) and the manifest's `cutter` entry.
///
/// # Errors
/// As [`Ctx::geom`]; an outline without area.
pub fn export_cutter(ctx: &Ctx) -> Result<(Vec<CutterFile>, String, String, PyJson), Refusal> {
    let dir = format!("{}_cutter", ctx.style);
    let mut files = Vec::new();
    let mut report = Vec::new();
    let mut too_large = Vec::new();
    for g in ctx.instance_geoms(6.0)? {
        let gr = g.transform(&rotation(grain_vertical_rotation(&g)));
        let mut polys = vec![gr.cut.clone()];
        for n in &gr.notches {
            let out = Vec2::new(-n.inward.x, -n.inward.y);
            polys.push(vec![
                n.base + n.tangent * 2.0,
                n.base + out * 3.0,
                n.base - n.tangent * 2.0,
            ]);
        }
        let ring = clean_ring(&union_exterior(&polys).ok_or_else(|| {
            Refusal::new(
                ftx_core::RefusalCode::DegenerateGeometry,
                format!("{}: no cutter outline", g.iid),
            )
        })?);
        let (mut lo, mut hi) = (
            Vec2::new(f64::INFINITY, f64::INFINITY),
            Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
        );
        for p in &ring {
            lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
            hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
        }
        let margin = 1.0;
        let (w, h) = (hi.x - lo.x + 2.0 * margin, hi.y - lo.y + 2.0 * margin);
        let d: Vec<String> = ring
            .iter()
            .map(|p| {
                format!(
                    "{},{}",
                    fnum(p.x - lo.x + margin, 3),
                    fnum(hi.y - p.y + margin, 3)
                )
            })
            .collect();
        let fname = format!(
            "{}_{}.svg",
            safe_name(&g.iid.replace('.', "_"), 40),
            safe_name(&ctx.size, 20)
        );
        let text = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}mm\" \
             height=\"{h}mm\" viewBox=\"0 0 {w} {h}\">\n<title>{} {} size {}: cut line incl. seam allowance, grain \
             vertical</title>\n<path d=\"M{} Z\" fill=\"none\" stroke=\"#000\" stroke-width=\"0.1\"/>\n</svg>\n",
            xml_escape(&g.name),
            xml_escape(&g.iid),
            xml_escape(&ctx.size),
            d.join(" L"),
            w = fnum(w, 3),
            h = fnum(h, 3)
        );
        let (bw, bh) = (w - 2.0 * margin, h - 2.0 * margin);
        let fits_long = bw <= MAT_W && bh <= MAT_LONG;
        let fits_sq = bw <= MAT_W && bh <= MAT_W;
        let fits_cross = bh <= MAT_W && bw <= MAT_LONG;
        let mut rec = vec![
            ("file", PyJson::str(format!("{dir}/{fname}"))),
            ("instance", PyJson::str(&g.iid)),
            ("width_mm", PyJson::Float(py::round(bw, 1))),
            ("height_mm", PyJson::Float(py::round(bh, 1))),
            (
                "mat",
                if fits_sq {
                    PyJson::str("12x12")
                } else if fits_long {
                    PyJson::str("12x24")
                } else {
                    PyJson::Null
                },
            ),
            ("too_large", PyJson::Bool(!fits_long)),
            (
                "fits_if_fabric_turned_crosswise",
                PyJson::Bool(!fits_long && fits_cross),
            ),
        ];
        if !fits_long {
            rec.push((
                "note",
                PyJson::str("too large for mat (11.5 x 23.5 in): print, project, or split with an added seam"),
            ));
            too_large.push(PyJson::str(&g.iid));
        }
        report.push(PyJson::dict(rec));
        files.push(CutterFile {
            path: format!("{dir}/{fname}"),
            text,
        });
    }
    let report_path = format!("{dir}/cutter_report.json");
    let report_text = PyJson::dict(vec![
        ("mat_limit_in", PyJson::floats(&[11.5, 23.5])),
        ("pieces", PyJson::List(report)),
    ])
    .dumps(Some(1))
        + "\n";
    let entry = PyJson::dict(vec![
        ("file", PyJson::str(&report_path)),
        ("count", PyJson::Int(files.len() as i64)),
        ("too_large", PyJson::List(too_large)),
    ]);
    Ok((files, report_path, report_text, entry))
}

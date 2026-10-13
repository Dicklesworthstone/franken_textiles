//! The master SVG (`docs/spec/03` §7.1): every piece at 1 unit = 1 mm, with its cut and sew lines, notches,
//! grainline, fold marks, internal lines, drills, labels and braid, as one group per piece.

use ftx_core::{Refusal, Vec2, py};
use serde_json::{Value, json};

use crate::ctx::Ctx;
use crate::geom::{Geom, grain_marks, label_texts};
use crate::text::{fnum, safe_name, xml_escape};

/// The fabric-pattern export namespace.
pub const SVG_NAMESPACE: &str = "urn:fabric-pattern:export:1";

fn path_d(pts: &[Vec2], closed: bool) -> String {
    let body: Vec<String> = pts
        .iter()
        .map(|p| format!("{},{}", fnum(p.x, 3), fnum(p.y, 3)))
        .collect();
    format!("M{}{}", body.join(" L"), if closed { " Z" } else { "" })
}

/// One piece's `<g>` in a sheet frame (y down), one element per line.
#[must_use]
pub fn svg_piece_group(g: &Geom, size: &str, line: f64) -> Vec<String> {
    let pid = safe_name(&g.iid.replace('.', "_"), 64);
    let m = &g.meta;
    let material = m.material.clone().unwrap_or_else(|| "None".into());
    let lw_owned = py::repr_f64(line);
    let lw = lw_owned.as_str();
    let mut out = vec![format!(
        "<g id=\"piece-{pid}\" data-piece=\"{}\" data-instance=\"{}\" data-name=\"{}\" data-qty=\"{}\" \
         data-mirror=\"{}\" data-material=\"{}\" data-on-fold=\"{}\" data-fold-mode=\"{}\" data-size=\"{}\" \
         data-sa-mm=\"{}\" data-grain-default=\"{}\">",
        xml_escape(&g.pid),
        xml_escape(&g.iid),
        xml_escape(&g.name),
        m.qty,
        m.mirror,
        xml_escape(&material),
        xml_escape(&m.on_fold),
        m.fold_mode.as_str(),
        xml_escape(size),
        fnum(m.sa_mm, 3),
        g.grain_default
    )];
    let mut grp = |name: &str, body: Vec<String>| {
        out.push(format!("<g id=\"piece-{pid}-{name}\" class=\"{name}\">"));
        out.extend(body);
        out.push("</g>".into());
    };
    let stroke = |d: String, extra: &str| {
        format!("<path d=\"{d}\" fill=\"none\" stroke=\"#000\" stroke-width=\"{lw}\"{extra}/>")
    };
    grp("cut", vec![stroke(path_d(&g.cut, true), "")]);
    grp(
        "sew",
        vec![format!(
            "<path d=\"{}\" fill=\"none\" stroke=\"#000\" stroke-width=\"{}\" stroke-dasharray=\"3 1.5\"/>",
            path_d(&g.sew, true),
            py::repr_f64(line * 0.7)
        )],
    );
    grp(
        "notches",
        g.notches
            .iter()
            .flat_map(|n| {
                n.marks.iter().map(move |mk| {
                    format!(
                        "<path data-notch=\"{}\" data-type=\"{}\" d=\"{}\" fill=\"none\" stroke=\"#000\" \
                         stroke-width=\"{lw}\"/>",
                        xml_escape(&n.id),
                        xml_escape(&n.kind),
                        path_d(mk, false)
                    )
                })
            })
            .collect(),
    );
    grp(
        "grain",
        grain_marks(g, 6.0)
            .iter()
            .map(|mk| stroke(path_d(mk, false), ""))
            .collect(),
    );
    grp(
        "fold",
        g.fold_lines
            .iter()
            .map(|(p, s)| {
                stroke(
                    path_d(p, false),
                    if s == "dashdot" {
                        " stroke-dasharray=\"6 1.5 1 1.5\""
                    } else {
                        ""
                    },
                )
            })
            .collect(),
    );
    grp(
        "internal-draw",
        g.internal_draw
            .iter()
            .map(|(p, c)| {
                format!(
                    "<path d=\"{}\" fill=\"none\" stroke=\"#000\" stroke-width=\"{}\" stroke-dasharray=\"2 1\"/>",
                    path_d(p, *c),
                    py::repr_f64(line * 0.8)
                )
            })
            .collect(),
    );
    grp(
        "internal-cut",
        g.internal_cut
            .iter()
            .map(|(p, c)| stroke(path_d(p, *c), ""))
            .collect(),
    );
    grp(
        "drills",
        g.drills
            .iter()
            .map(|&(p, d)| {
                format!(
                    "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"none\" stroke=\"#000\" stroke-width=\"{lw}\"/>\
                     <path d=\"M{},{} L{},{} M{},{} L{},{}\" stroke=\"#000\" stroke-width=\"{}\"/>",
                    fnum(p.x, 3),
                    fnum(p.y, 3),
                    fnum(d / 2.0, 3),
                    fnum(p.x - d, 3),
                    fnum(p.y, 3),
                    fnum(p.x + d, 3),
                    fnum(p.y, 3),
                    fnum(p.x, 3),
                    fnum(p.y - d, 3),
                    fnum(p.x, 3),
                    fnum(p.y + d, 3),
                    py::repr_f64(line * 0.6)
                )
            })
            .collect(),
    );
    let mut texts = Vec::new();
    for lb in &g.labels {
        for (x, y, t, s, bold) in label_texts(lb, true, 0.0, 1.0) {
            texts.push(format!(
                "<text x=\"{}\" y=\"{}\" font-size=\"{}\" font-family=\"Helvetica, Arial, sans-serif\" \
                 text-anchor=\"middle\"{} paint-order=\"stroke\" stroke=\"#fff\" stroke-width=\"{}\" \
                 fill=\"#000\" data-kind=\"{}\">{}</text>",
                fnum(x, 3),
                fnum(y, 3),
                fnum(s, 2),
                if bold { " font-weight=\"bold\"" } else { "" },
                fnum(s * 0.12, 2),
                lb.kind,
                xml_escape(&t)
            ));
        }
    }
    grp("labels", texts);
    for kind in ["soutache", "embroidery"] {
        let mut body = Vec::new();
        for d in g.decor.iter().filter(|d| d.kind == kind) {
            let w = fnum(d.width, 3);
            body.push(format!(
                "<path d=\"{}\" fill=\"none\" stroke=\"#000\" stroke-opacity=\"0.35\" stroke-width=\"{w}\" \
                 stroke-linecap=\"round\" stroke-linejoin=\"round\" data-width-mm=\"{w}\"/>",
                path_d(&d.pts, d.closed)
            ));
            body.push(format!(
                "<path d=\"{}\" fill=\"none\" stroke=\"#000\" stroke-width=\"{}\"/>",
                path_d(&d.pts, d.closed),
                py::repr_f64(line * 0.6)
            ));
        }
        if !body.is_empty() || kind == "soutache" {
            grp(&format!("decor-{kind}"), body);
        }
    }
    out.push("</g>".into());
    out
}

/// The master SVG `<style>_master_mm.svg` and its manifest entry (`master`: file, size, the cut lines'
/// bounding box, fold mode).
///
/// # Errors
/// As [`Ctx::master`].
pub fn master_svg(ctx: &Ctx) -> Result<(String, Value), Refusal> {
    let lay = ctx.master()?;
    let (w, h) = (fnum(lay.width, 3), fnum(lay.height, 3));
    let fold_mode = if ctx.opts.unfold { "unfolded" } else { "half" };
    let mut out = vec![
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>".to_string(),
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:fp=\"{SVG_NAMESPACE}\" width=\"{w}mm\" height=\"{h}mm\" \
             viewBox=\"0 0 {w} {h}\" fp:schema=\"fabric-pattern/1\" data-style=\"{}\" data-size=\"{}\" \
             data-units=\"mm\" data-fold-mode=\"{fold_mode}\">",
            xml_escape(&ctx.style),
            xml_escape(&ctx.size)
        ),
        format!(
            "<title>{} master pattern, size {} (1 unit = 1 mm)</title>",
            xml_escape(&ctx.style),
            xml_escape(&ctx.size)
        ),
        format!("<rect x=\"0\" y=\"0\" width=\"{w}\" height=\"{h}\" fill=\"#fff\"/>"),
    ];
    for g in &lay.items {
        out.extend(svg_piece_group(g, &ctx.size, 0.25));
    }
    out.push("</svg>".into());
    let mut bb = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for p in lay.items.iter().flat_map(|g| g.cut.iter()) {
        bb = [
            bb[0].min(p.x),
            bb[1].min(p.y),
            bb[2].max(p.x),
            bb[3].max(p.y),
        ];
    }
    let file = format!("{}_master_mm.svg", ctx.style);
    let entry = json!({
        "file": file,
        "width_mm": py::round(lay.width, 3),
        "height_mm": py::round(lay.height, 3),
        "cut_bbox": bb.iter().map(|&v| py::round(v, 3)).collect::<Vec<_>>(),
        "fold_mode": fold_mode,
    });
    Ok((out.join("\n") + "\n", entry))
}

//! Every export of a pattern and the acceptance checks over a set of files (`docs/spec/03` §1.3, §7.13, §8.1).

use std::collections::BTreeMap;
use std::path::Path;

use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::Pattern;
use serde_json::Value;

use crate::checks::{
    CheckItem, check_dst, check_dxf, check_flat_dxf, check_garmentcode, check_hpgl, check_pdf,
    check_svg, svg_cut_points,
};
use crate::ctx::{Ctx, primary_instance};
use crate::cutter::export_cutter;
use crate::dst::export_embroidery;
use crate::dxf::{export_dxf_astm, export_flat_dxf};
use crate::gc::export_garmentcode;
use crate::geom::{Source, build_geom};
use crate::hpgl::export_hpgl;
use crate::pdf::{export_copyshop, export_projector, export_tiled};
use crate::pyjson::PyJson;
use crate::svg::master_svg;

/// The files of an export (paths relative to the output directory, `/` separators) and its manifest.
#[derive(Clone, Debug)]
pub struct ExportSet {
    /// Files in the order they were made.
    pub files: Vec<(String, Vec<u8>)>,
    /// The manifest (also among `files` as `<S>_export_manifest.json`).
    pub manifest: PyJson,
}

/// A JSON value as the reference ordered it: `keys` first in that order, then the rest.
fn ordered(v: &Value, keys: &[&str]) -> PyJson {
    let mut items: Vec<(String, PyJson)> = Vec::new();
    if let Some(o) = v.as_object() {
        for k in keys {
            if let Some(x) = o.get(*k) {
                items.push(((*k).to_string(), PyJson::from_value(x)));
            }
        }
        for (k, x) in o {
            if !keys.contains(&k.as_str()) {
                items.push((k.clone(), PyJson::from_value(x)));
            }
        }
    }
    PyJson::Dict(items)
}

/// Every target in the reference's order (master SVG, tiled PDFs, copy shop, projector, ASTM and AAMA DXF, flat
/// DXF, HPGL, cutter files, GarmentCode, soutache DSTs) and the manifest, in memory.
///
/// # Errors
/// As the targets (a projector page over 200 in, an unbuildable pattern).
pub fn export_all(ctx: &Ctx) -> Result<ExportSet, Refusal> {
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut entries: Vec<(String, PyJson)> = Vec::new();
    let file_only = |f: &str| PyJson::dict(vec![("file", PyJson::str(f))]);
    let (svg, master) = master_svg(ctx)?;
    let master_name = master["file"].as_str().unwrap_or("").to_string();
    files.push((master_name.clone(), svg.into_bytes()));
    entries.push(("svg_master".into(), file_only(&master_name)));
    let pdf_keys = [
        "file", "kind", "paper_mm", "cols", "rows", "tiles", "printed", "blank", "pages",
    ];
    for f in export_tiled(ctx)?
        .into_iter()
        .chain(export_copyshop(ctx)?)
        .chain(export_projector(ctx)?)
    {
        entries.push((f.key.clone(), ordered(&f.entry, &pdf_keys)));
        files.push((f.name, f.bytes));
    }
    let pieces = ctx.piece_reference()?;
    for astm in [true, false] {
        let (name, text, entry) = export_dxf_astm(ctx, &pieces, astm)?;
        entries.push((if astm { "dxf_astm" } else { "dxf_aama" }.into(), entry));
        files.push((name, text.into_bytes()));
    }
    let (name, text, entry) = export_flat_dxf(ctx)?;
    entries.push(("dxf_flat".into(), entry));
    files.push((name, text.into_bytes()));
    let (name, text, hpgl) = export_hpgl(ctx, ctx.opts.hpgl_rotate90, ctx.opts.hpgl_mirror)?;
    entries.push(("hpgl".into(), file_only(&name)));
    files.push((name, text.into_bytes()));
    let (cutter, report_path, report, entry) = export_cutter(ctx)?;
    for c in cutter {
        files.push((c.path, c.text.into_bytes()));
    }
    files.push((report_path, report.into_bytes()));
    entries.push(("cutter".into(), entry));
    let (name, text, entry) = export_garmentcode(ctx)?;
    entries.push(("garmentcode".into(), entry));
    files.push((name, text.into_bytes()));
    let (dsts, readme) = export_embroidery(ctx)?;
    for d in dsts {
        entries.push((d.key, d.entry));
        files.push((d.name, d.bytes));
    }
    if let Some((name, text)) = readme {
        files.push((name, text.into_bytes()));
    }
    let manifest = PyJson::Dict(vec![
        ("schema".into(), PyJson::str("fabric-pattern-export/1")),
        ("style".into(), PyJson::str(&ctx.style)),
        ("size".into(), PyJson::str(&ctx.size)),
        ("created".into(), PyJson::str(&ctx.created)),
        (
            "source_schema".into(),
            PyJson::from_value(ctx.pattern.raw().get("schema").unwrap_or(&Value::Null)),
        ),
        ("files".into(), PyJson::Dict(entries)),
        ("pieces".into(), pieces),
        (
            "master".into(),
            ordered(
                &master,
                &["file", "width_mm", "height_mm", "cut_bbox", "fold_mode"],
            ),
        ),
        ("hpgl".into(), hpgl),
    ]);
    files.push((
        format!("{}_export_manifest.json", ctx.style),
        (manifest.dumps(Some(1)) + "\n").into_bytes(),
    ));
    Ok(ExportSet { files, manifest })
}

/// The acceptance checks over a set of files (relative path → bytes), in the reference's order (§8.1): the DXFs,
/// the master SVG (whose cut vertices the tiles must restitch onto), every PDF, every cutter SVG, HPGL,
/// GarmentCode and every DST. With the pattern the sew-line references are recomputed from it and the DSTs'
/// braid lines checked against it. `dir` names the set in the manifest and nothing-checked items. A file the
/// manifest lists but the set lacks fails a `file.missing` item.
#[must_use]
pub fn check_files(
    files: &BTreeMap<String, Vec<u8>>,
    dir: &str,
    pattern: Option<&Pattern>,
) -> Vec<CheckItem> {
    let mut out = Vec::new();
    let Some((_, man_bytes)) = files
        .iter()
        .find(|(k, _)| !k.contains('/') && k.ends_with("_export_manifest.json"))
    else {
        return vec![CheckItem {
            check: "manifest".into(),
            file: dir.into(),
            ok: false,
            detail: "no *_export_manifest.json".into(),
        }];
    };
    let man: Value = serde_json::from_slice(man_bytes).unwrap_or(Value::Null);
    // the manifest's own order of its files (serde_json sorts keys)
    let order: Vec<String> = match crate::pyjson::PyJson::parse(&String::from_utf8_lossy(man_bytes))
    {
        Ok(PyJson::Dict(items)) => items
            .into_iter()
            .find(|(k, _)| k == "files")
            .and_then(|(_, f)| match f {
                PyJson::Dict(fi) => Some(fi.into_iter().map(|(k, _)| k).collect()),
                _ => None,
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    let missing = |name: &str| CheckItem {
        check: "file.missing".into(),
        file: name.into(),
        ok: false,
        detail: "listed in the manifest but not found".into(),
    };
    let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    let mut sew_ref: Option<Value> = None;
    if let Some(p) = pattern {
        let mut m = serde_json::Map::new();
        for (block, r) in man["pieces"].as_object().into_iter().flatten() {
            if let Some(inst) = r["piece"].as_str().and_then(|pid| primary_instance(p, pid)) {
                let per = inst.perimeter().unwrap_or(f64::NAN);
                let area = inst
                    .outline(200)
                    .map(|o| ftx_core::polyline::signed_area(&o))
                    .unwrap_or(f64::NAN);
                m.insert(
                    block.clone(),
                    serde_json::json!({"sew_perimeter": per, "sew_area": area}),
                );
            }
        }
        sew_ref = Some(Value::Object(m));
    }
    let empty = serde_json::Map::new();
    let listed = man["files"].as_object().unwrap_or(&empty);
    let path_of = |info: &Value| info["file"].as_str().unwrap_or("").to_string();
    for key in ["dxf_astm", "dxf_aama"] {
        if let Some(info) = listed.get(key) {
            match files.get(&path_of(info)) {
                Some(b) => out.extend(check_dxf(
                    b,
                    &path_of(info),
                    &man["pieces"],
                    sew_ref.as_ref(),
                )),
                None => out.push(missing(&path_of(info))),
            }
        }
    }
    if let Some(info) = listed.get("dxf_flat") {
        match files.get(&path_of(info)) {
            Some(b) => out.extend(check_flat_dxf(&text(b), &path_of(info))),
            None => out.push(missing(&path_of(info))),
        }
    }
    let mut master_pts = None;
    if let Some(m) = man.get("master")
        && let Some(b) = files.get(&path_of(m))
    {
        out.extend(check_svg(&text(b), &path_of(m), false));
        master_pts = Some(svg_cut_points(&text(b)));
    }
    for key in &order {
        let Some(info) = listed.get(key) else {
            continue;
        };
        if key.starts_with("pdf_") {
            let paper: Vec<f64> = info["paper_mm"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_f64)
                .collect();
            match files.get(&path_of(info)) {
                Some(b) if paper.len() == 2 => out.extend(check_pdf(
                    b,
                    &path_of(info),
                    [paper[0], paper[1]],
                    info["kind"].as_str().unwrap_or(""),
                    master_pts.as_deref(),
                )),
                _ => out.push(missing(&path_of(info))),
            }
        }
    }
    if let Some(info) = listed.get("cutter") {
        let report = path_of(info);
        let dir_prefix = report
            .rsplit_once('/')
            .map_or(String::new(), |(d, _)| format!("{d}/"));
        for (name, b) in files.range(dir_prefix.clone()..) {
            if !name.starts_with(&dir_prefix) {
                break;
            }
            let base = &name[dir_prefix.len()..];
            if !base.contains('/') && base.ends_with(".svg") {
                out.extend(check_svg(&text(b), base, true));
            }
        }
    }
    if let Some(info) = listed.get("hpgl") {
        match files.get(&path_of(info)) {
            Some(b) => out.extend(check_hpgl(
                &text(b),
                &path_of(info),
                man.get("master"),
                man.get("hpgl").unwrap_or(&Value::Null),
            )),
            None => out.push(missing(&path_of(info))),
        }
    }
    if let Some(info) = listed.get("garmentcode") {
        match files.get(&path_of(info)) {
            Some(b) => out.extend(check_garmentcode(&text(b), &path_of(info))),
            None => out.push(missing(&path_of(info))),
        }
    }
    for key in &order {
        let Some(info) = listed.get(key) else {
            continue;
        };
        if !key.starts_with("dst_") {
            continue;
        }
        let braid: Option<Vec<Vec<(f64, f64)>>> = pattern.and_then(|p| {
            let iid = info["instance"].as_str()?;
            let inst = p.instances.iter().find(|i| i.id == iid)?;
            let sa = p.raw()["meta"]
                .get("sa_mm")
                .and_then(Value::as_f64)
                .unwrap_or(crate::outline::DEFAULT_SA);
            let g = build_geom(p, Source::Instance(inst), sa, 4.0, 6.0).ok()?;
            Some(
                g.decor
                    .iter()
                    .filter(|d| d.kind == "soutache")
                    .map(|d| d.pts.iter().map(|q| (q.x, q.y)).collect())
                    .collect(),
            )
        });
        match files.get(&path_of(info)) {
            Some(b) => out.extend(check_dst(b, &path_of(info), info, braid.as_deref())),
            None => out.push(missing(&path_of(info))),
        }
    }
    if out.is_empty() {
        out.push(CheckItem {
            check: "check.nothing_checked".into(),
            file: dir.into(),
            ok: false,
            detail: "manifest lists no checkable files".into(),
        });
    }
    out
}

/// Reads every file of an output directory (and of its sub-directories one level down) into a set.
///
/// # Errors
/// `io` when the directory cannot be read.
pub fn read_dir_files(dir: &Path) -> Result<BTreeMap<String, Vec<u8>>, Refusal> {
    let io = |e: std::io::Error| Refusal::new(RefusalCode::Io, format!("{}: {e}", dir.display()));
    let mut out = BTreeMap::new();
    for entry in std::fs::read_dir(dir).map_err(io)? {
        let path = entry.map_err(io)?.path();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if path.is_dir() {
            for sub in std::fs::read_dir(&path).map_err(io)? {
                let sp = sub.map_err(io)?.path();
                if sp.is_file() {
                    let sn = sp
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    out.insert(format!("{name}/{sn}"), std::fs::read(&sp).map_err(io)?);
                }
            }
        } else if path.is_file() {
            out.insert(name, std::fs::read(&path).map_err(io)?);
        }
    }
    Ok(out)
}

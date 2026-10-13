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

/// An export target (`ftx export <target>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// The master SVG.
    Svg,
    /// Tiled Letter and A4 PDFs.
    Pdf,
    /// Tiled Letter only.
    Letter,
    /// Tiled A4 only.
    A4,
    /// Copy-shop A0 and 36 × 48 in.
    Copyshop,
    /// Projector light and dark.
    Projector,
    /// ASTM and AAMA DXF.
    Dxf,
    /// ASTM DXF only.
    Astm,
    /// AAMA DXF only.
    Aama,
    /// Flat DXF.
    FlatDxf,
    /// HPGL.
    Hpgl,
    /// Cutter SVGs and report.
    Cutter,
    /// GarmentCode JSON.
    Garmentcode,
    /// Soutache DSTs and README.
    Embroidery,
}

impl Target {
    /// Every target, in the order `all` runs them.
    pub const ALL: [Target; 10] = [
        Self::Svg,
        Self::Pdf,
        Self::Copyshop,
        Self::Projector,
        Self::Dxf,
        Self::FlatDxf,
        Self::Hpgl,
        Self::Cutter,
        Self::Garmentcode,
        Self::Embroidery,
    ];

    /// The target of a CLI name (`svg`, `pdf`, `letter`, `a4`, `copyshop`, `projector`, `dxf`, `astm`, `aama`,
    /// `flat-dxf`, `hpgl`, `cutter`, `garmentcode`, `embroidery`).
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "svg" => Self::Svg,
            "pdf" => Self::Pdf,
            "letter" => Self::Letter,
            "a4" => Self::A4,
            "copyshop" => Self::Copyshop,
            "projector" => Self::Projector,
            "dxf" => Self::Dxf,
            "astm" => Self::Astm,
            "aama" => Self::Aama,
            "flat-dxf" => Self::FlatDxf,
            "hpgl" => Self::Hpgl,
            "cutter" => Self::Cutter,
            "garmentcode" => Self::Garmentcode,
            "embroidery" => Self::Embroidery,
            _ => return None,
        })
    }
}

/// The targets' files and the manifest, starting from an existing manifest when given (the reference updates
/// the one already in the output directory: keys keep their place, new ones are appended, its other files stay
/// listed).
///
/// # Errors
/// As the targets (a projector page over 200 in, an unbuildable pattern).
pub fn export_targets(
    ctx: &Ctx,
    targets: &[Target],
    existing: Option<PyJson>,
) -> Result<ExportSet, Refusal> {
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut man = match existing {
        Some(m @ PyJson::Dict(_)) => m,
        _ => PyJson::Dict(Vec::new()),
    };
    man.set("schema", PyJson::str("fabric-pattern-export/1"));
    man.set("style", PyJson::str(&ctx.style));
    man.set("size", PyJson::str(&ctx.size));
    man.set("created", PyJson::str(&ctx.created));
    man.set(
        "source_schema",
        PyJson::from_value(ctx.pattern.raw().get("schema").unwrap_or(&Value::Null)),
    );
    if man.get("files").is_none() {
        man.set("files", PyJson::Dict(Vec::new()));
    }
    let pieces = ctx.piece_reference()?;
    man.set("pieces", pieces.clone());
    let record = |man: &mut PyJson, key: &str, entry: PyJson| {
        if let PyJson::Dict(items) = man
            && let Some((_, f)) = items.iter_mut().find(|(k, _)| k == "files")
        {
            f.set(key, entry);
        }
    };
    let file_only = |f: &str| PyJson::dict(vec![("file", PyJson::str(f))]);
    let pdf_keys = [
        "file", "kind", "paper_mm", "cols", "rows", "tiles", "printed", "blank", "pages",
    ];
    for &t in targets {
        match t {
            Target::Svg => {
                let (svg, master) = master_svg(ctx)?;
                let name = master["file"].as_str().unwrap_or("").to_string();
                man.set(
                    "master",
                    ordered(
                        &master,
                        &["file", "width_mm", "height_mm", "cut_bbox", "fold_mode"],
                    ),
                );
                record(&mut man, "svg_master", file_only(&name));
                files.push((name, svg.into_bytes()));
            }
            Target::Pdf | Target::Letter | Target::A4 | Target::Copyshop | Target::Projector => {
                let made = match t {
                    Target::Copyshop => export_copyshop(ctx)?,
                    Target::Projector => export_projector(ctx)?,
                    _ => export_tiled(ctx)?
                        .into_iter()
                        .filter(|f| match t {
                            Target::Letter => f.key == "pdf_letter",
                            Target::A4 => f.key == "pdf_a4",
                            _ => true,
                        })
                        .collect(),
                };
                for f in made {
                    record(&mut man, &f.key, ordered(&f.entry, &pdf_keys));
                    files.push((f.name, f.bytes));
                }
            }
            Target::Dxf | Target::Astm | Target::Aama => {
                for astm in [true, false] {
                    if (t == Target::Astm && !astm) || (t == Target::Aama && astm) {
                        continue;
                    }
                    let (name, text, entry) = export_dxf_astm(ctx, &pieces, astm)?;
                    record(&mut man, if astm { "dxf_astm" } else { "dxf_aama" }, entry);
                    files.push((name, text.into_bytes()));
                }
            }
            Target::FlatDxf => {
                let (name, text, entry) = export_flat_dxf(ctx)?;
                record(&mut man, "dxf_flat", entry);
                files.push((name, text.into_bytes()));
            }
            Target::Hpgl => {
                let (name, text, hpgl) =
                    export_hpgl(ctx, ctx.opts.hpgl_rotate90, ctx.opts.hpgl_mirror)?;
                man.set("hpgl", hpgl);
                record(&mut man, "hpgl", file_only(&name));
                files.push((name, text.into_bytes()));
            }
            Target::Cutter => {
                let (cutter, report_path, report, entry) = export_cutter(ctx)?;
                for c in cutter {
                    files.push((c.path, c.text.into_bytes()));
                }
                files.push((report_path, report.into_bytes()));
                record(&mut man, "cutter", entry);
            }
            Target::Garmentcode => {
                let (name, text, entry) = export_garmentcode(ctx)?;
                record(&mut man, "garmentcode", entry);
                files.push((name, text.into_bytes()));
            }
            Target::Embroidery => {
                let (dsts, readme) = export_embroidery(ctx)?;
                for d in dsts {
                    record(&mut man, &d.key, d.entry);
                    files.push((d.name, d.bytes));
                }
                if let Some((name, text)) = readme {
                    files.push((name, text.into_bytes()));
                }
            }
        }
    }
    files.push((
        format!("{}_export_manifest.json", ctx.style),
        (man.dumps(Some(1)) + "\n").into_bytes(),
    ));
    Ok(ExportSet {
        files,
        manifest: man,
    })
}

/// Every target in the reference's order (master SVG, tiled PDFs, copy shop, projector, ASTM and AAMA DXF, flat
/// DXF, HPGL, cutter files, GarmentCode, soutache DSTs) and a fresh manifest, in memory.
///
/// # Errors
/// As the targets.
pub fn export_all(ctx: &Ctx) -> Result<ExportSet, Refusal> {
    export_targets(ctx, &Target::ALL, None)
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

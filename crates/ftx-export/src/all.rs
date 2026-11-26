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
        None => PyJson::Dict(Vec::new()),
        Some(m @ PyJson::Dict(_)) => m,
        Some(_) => {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                "existing export manifest must be a JSON object",
            ));
        }
    };
    man.set("schema", PyJson::str("fabric-pattern-export/1"));
    man.set("style", PyJson::str(&ctx.style));
    man.set("size", PyJson::str(&ctx.size));
    man.set("created", PyJson::str(&ctx.created));
    man.set(
        "source_schema",
        PyJson::from_value(ctx.pattern.raw().get("schema").unwrap_or(&Value::Null)),
    );
    match man.get("files") {
        None => man.set("files", PyJson::Dict(Vec::new())),
        Some(PyJson::Dict(_)) => {}
        Some(_) => {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                "existing export manifest 'files' field must be a JSON object",
            ));
        }
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
    let Some((man_name, man_bytes)) = files
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
    let Ok(man @ Value::Object(_)) = serde_json::from_slice::<Value>(man_bytes) else {
        return vec![CheckItem {
            check: "manifest".into(),
            file: man_name.clone(),
            ok: false,
            detail: "manifest is not a valid JSON object".into(),
        }];
    };
    let Some(listed) = man["files"].as_object() else {
        return vec![CheckItem {
            check: "manifest".into(),
            file: man_name.clone(),
            ok: false,
            detail: "manifest 'files' field is not a JSON object".into(),
        }];
    };
    // the manifest's own order of its files (serde_json sorts keys)
    let mut order: Vec<String> =
        match crate::pyjson::PyJson::parse(&String::from_utf8_lossy(man_bytes)) {
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
    if order.is_empty() && !listed.is_empty() {
        order = listed.keys().cloned().collect();
    }
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
    let master_file = man
        .get("master")
        .map(path_of)
        .filter(|p| !p.is_empty())
        .or_else(|| {
            listed
                .get("svg_master")
                .map(path_of)
                .filter(|p| !p.is_empty())
        });
    if let Some(mfile) = master_file {
        match files.get(&mfile) {
            Some(b) => {
                out.extend(check_svg(&text(b), &mfile, false));
                master_pts = Some(svg_cut_points(&text(b)));
            }
            None => out.push(missing(&mfile)),
        }
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
            let pdf_file = path_of(info);
            match files.get(&pdf_file) {
                Some(b) if paper.len() == 2 && paper.iter().all(|v| v.is_finite() && *v > 0.0) => {
                    out.extend(check_pdf(
                        b,
                        &pdf_file,
                        [paper[0], paper[1]],
                        info["kind"].as_str().unwrap_or(""),
                        master_pts.as_deref(),
                    ))
                }
                Some(_) => out.push(CheckItem {
                    check: "manifest".into(),
                    file: pdf_file,
                    ok: false,
                    detail: format!("manifest entry {key} has invalid paper_mm"),
                }),
                None => out.push(missing(&pdf_file)),
            }
        }
    }
    if let Some(info) = listed.get("cutter") {
        let report = path_of(info);
        let dir_prefix = report
            .rsplit_once('/')
            .map_or(String::new(), |(d, _)| format!("{d}/"));
        if report.is_empty() || dir_prefix.is_empty() {
            out.push(CheckItem {
                check: "manifest".into(),
                file: man_name.clone(),
                ok: false,
                detail: "manifest entry cutter has invalid report path".into(),
            });
        } else {
            let mut reported_missing = false;
            match files.get(&report) {
                Some(raw) => match serde_json::from_slice::<Value>(raw) {
                    Ok(doc) => {
                        if let Some(pieces) = doc["pieces"].as_array() {
                            if let Some(expected) = info["count"].as_u64()
                                && u64::try_from(pieces.len()).ok() != Some(expected)
                            {
                                out.push(CheckItem {
                                    check: "cutter.report".into(),
                                    file: report.clone(),
                                    ok: false,
                                    detail: format!(
                                        "cutter report lists {} piece(s), manifest expects {expected}",
                                        pieces.len()
                                    ),
                                });
                                reported_missing = true;
                            }
                            for piece in pieces {
                                let pfile = path_of(piece);
                                if pfile.is_empty() || !files.contains_key(&pfile) {
                                    out.push(missing(if pfile.is_empty() {
                                        &report
                                    } else {
                                        &pfile
                                    }));
                                    reported_missing = true;
                                }
                            }
                        } else {
                            out.push(CheckItem {
                                check: "cutter.report".into(),
                                file: report.clone(),
                                ok: false,
                                detail: "cutter report lacks a pieces array".into(),
                            });
                            reported_missing = true;
                        }
                    }
                    Err(_) => {
                        out.push(CheckItem {
                            check: "cutter.report".into(),
                            file: report.clone(),
                            ok: false,
                            detail: "cutter report is not valid JSON".into(),
                        });
                        reported_missing = true;
                    }
                },
                None => {
                    out.push(missing(&report));
                    reported_missing = true;
                }
            }
            let mut svg_count = 0usize;
            for (name, b) in files.range(dir_prefix.clone()..) {
                if !name.starts_with(&dir_prefix) {
                    break;
                }
                let base = &name[dir_prefix.len()..];
                if !base.contains('/') && base.ends_with(".svg") {
                    svg_count += 1;
                    out.extend(check_svg(&text(b), base, true));
                }
            }
            if svg_count == 0 && !reported_missing && info["count"].as_u64().unwrap_or(1) > 0 {
                out.push(missing(&format!("{dir_prefix}*.svg")));
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

/// Reads every regular file of an output directory (and of its regular sub-directories one level
/// down) into a set, refusing symlinks at the root directory or any visited entry.
///
/// # Errors
/// [`RefusalCode::Io`] when a path cannot be inspected or read; [`RefusalCode::InvalidDocument`]
/// when `dir` is not a regular directory or any entry in `dir` (or one level down) is a symlink.
pub fn read_dir_files(dir: &Path) -> Result<BTreeMap<String, Vec<u8>>, Refusal> {
    let io_at = |path: &Path, e: std::io::Error| {
        Refusal::new(RefusalCode::Io, format!("{}: {e}", path.display()))
    };
    let root_meta = std::fs::symlink_metadata(dir).map_err(|e| io_at(dir, e))?;
    if !root_meta.file_type().is_dir() {
        return Err(Refusal::new(
            RefusalCode::InvalidDocument,
            format!("{}: expected a regular directory", dir.display()),
        ));
    }
    let mut out = BTreeMap::new();
    for entry in std::fs::read_dir(dir).map_err(|e| io_at(dir, e))? {
        let entry = entry.map_err(|e| io_at(dir, e))?;
        let path = entry.path();
        let ft = entry.file_type().map_err(|e| io_at(&path, e))?;
        if ft.is_symlink() {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                format!(
                    "{}: expected a regular file or directory, not a symlink",
                    path.display()
                ),
            ));
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if ft.is_dir() {
            for sub in std::fs::read_dir(&path).map_err(|e| io_at(&path, e))? {
                let sub = sub.map_err(|e| io_at(&path, e))?;
                let sp = sub.path();
                let sft = sub.file_type().map_err(|e| io_at(&sp, e))?;
                if sft.is_symlink() {
                    return Err(Refusal::new(
                        RefusalCode::InvalidDocument,
                        format!("{}: expected a regular file, not a symlink", sp.display()),
                    ));
                }
                if sft.is_file() {
                    let sn = sp
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    out.insert(
                        format!("{name}/{sn}"),
                        std::fs::read(&sp).map_err(|e| io_at(&sp, e))?,
                    );
                }
            }
        } else if ft.is_file() {
            out.insert(name, std::fs::read(&path).map_err(|e| io_at(&path, e))?);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fresh_temp_dir(tag: &str) -> std::path::PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("ftx-export-{tag}-{}-{stamp}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn read_dir_files_reads_two_levels_and_refuses_non_directories() {
        let root = fresh_temp_dir("read-dir-ok");
        let sub = root.join("cutter");
        let deep = sub.join("ignored_deep");
        fs::create_dir_all(&deep).unwrap();
        fs::write(root.join("a.txt"), b"top").unwrap();
        fs::write(sub.join("b.svg"), b"nested").unwrap();
        fs::write(deep.join("c.svg"), b"too-deep").unwrap();

        let files = read_dir_files(&root).unwrap();
        assert_eq!(
            files.get("a.txt").map(Vec::as_slice),
            Some(b"top".as_slice())
        );
        assert_eq!(
            files.get("cutter/b.svg").map(Vec::as_slice),
            Some(b"nested".as_slice())
        );
        assert_eq!(files.len(), 2);

        let err_file = read_dir_files(&root.join("a.txt")).unwrap_err();
        assert_eq!(err_file.code, RefusalCode::InvalidDocument);
        assert!(err_file.message.contains("regular directory"), "{err_file}");

        let err_missing = read_dir_files(&root.join("missing")).unwrap_err();
        assert_eq!(err_missing.code, RefusalCode::Io);

        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn read_dir_files_refuses_symlinks_at_root_top_and_nested_levels() {
        use std::os::unix::fs::symlink;

        let root = fresh_temp_dir("read-dir-symlink");
        let outside = root.join("outside.txt");
        fs::write(&outside, b"secret").unwrap();
        let outside_dir = root.join("outside_dir");
        fs::create_dir_all(&outside_dir).unwrap();
        fs::write(outside_dir.join("leak.svg"), b"leak").unwrap();

        let export_dir = root.join("out");
        fs::create_dir_all(&export_dir).unwrap();
        fs::write(export_dir.join("ok.txt"), b"ok").unwrap();

        // 1. Symlinked root directory.
        let alias = root.join("out_alias");
        symlink(&export_dir, &alias).unwrap();
        let err = read_dir_files(&alias).unwrap_err();
        assert_eq!(err.code, RefusalCode::InvalidDocument);
        assert!(err.message.contains("regular directory"), "{err}");

        // 2. Symlinked top-level file.
        let link_file = export_dir.join("linked.txt");
        symlink(&outside, &link_file).unwrap();
        let err = read_dir_files(&export_dir).unwrap_err();
        assert_eq!(err.code, RefusalCode::InvalidDocument);
        assert!(err.message.contains("symlink"), "{err}");
        fs::remove_file(&link_file).unwrap();

        // 3. Symlinked subdirectory.
        let link_sub = export_dir.join("cutter");
        symlink(&outside_dir, &link_sub).unwrap();
        let err = read_dir_files(&export_dir).unwrap_err();
        assert_eq!(err.code, RefusalCode::InvalidDocument);
        assert!(err.message.contains("symlink"), "{err}");
        fs::remove_file(&link_sub).unwrap();

        // 4. Symlinked file inside a regular subdirectory.
        let cutter = export_dir.join("cutter");
        fs::create_dir_all(&cutter).unwrap();
        symlink(&outside, cutter.join("piece.svg")).unwrap();
        let err = read_dir_files(&export_dir).unwrap_err();
        assert_eq!(err.code, RefusalCode::InvalidDocument);
        assert!(err.message.contains("symlink"), "{err}");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn check_files_compares_cutter_count_without_32bit_truncation() {
        let mut files = BTreeMap::new();
        let huge_count = (1u64 << 32) + 1;
        files.insert(
            "TEST_export_manifest.json".into(),
            serde_json::json!({
                "schema": "fabric-pattern-export/1",
                "files": {
                    "cutter": {
                        "file": "cutter/TEST_cutter_report.json",
                        "count": huge_count
                    }
                }
            })
            .to_string()
            .into_bytes(),
        );
        files.insert(
            "cutter/TEST_cutter_report.json".into(),
            serde_json::json!({
                "pieces": [{"file": "cutter/p.svg"}]
            })
            .to_string()
            .into_bytes(),
        );
        files.insert(
            "cutter/p.svg".into(),
            b"<svg width=\"10mm\" height=\"10mm\" viewBox=\"0 0 10 10\"><path d=\"M 0,0 L 10,0 L 10,10 Z\"/></svg>".to_vec(),
        );
        let items = check_files(&files, "out", None);
        assert!(
            items.iter().any(|i| i.check == "cutter.report" && !i.ok),
            "u64 cutter count exceeding 2^32 must fail cutter.report: {items:?}"
        );
    }
}

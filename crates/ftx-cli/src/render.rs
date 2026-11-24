//! `ftx render DIR [--res WxH]`: the six stills of an `ftx drape` run (`ftx_drape::render`), written into DIR
//! as `still_<view>.png` from its `body.obj`, `garment.obj` and `garment_strain.obj` (the counterpart of the
//! reference's `blender_sew.py --stills-from DIR`, with no Blender).
//!
//! `--shading legible` writes a second set, `still_<view>_legible.png`, beside those and leaves them as
//! they are: the same geometry through the same cameras, drawn for reading folds and edges by eye. It is
//! a drawing aid whose constants describe no fabric (see `ftx_drape::render::Shading`).

use std::path::Path;

use ftx_core::{Refusal, RefusalCode};
use ftx_drape::render::{ColouredMesh, Shading, png, still, still_camera, stills_shaded};
use serde_json::{Value, json};

use super::{Args, bad_input, known_options, required};

pub(super) fn cmd_render(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(
        args,
        "render",
        &["res", "compare", "view", "out", "shading"],
    )?;
    if args.positional.len() != 2 {
        return Err(bad_input("ftx render needs exactly one drape directory"));
    }
    let dir = Path::new(&args.positional[1]);
    let (width, height) = resolution(args.options.get("res").map(String::as_str))?;
    let shading = shading(args.options.get("shading").map(String::as_str))?;
    if let Some(revised) = args.options.get("compare") {
        return compare(
            dir,
            Path::new(revised),
            required(args, "view")?,
            Path::new(required(args, "out")?),
            (width, height),
            shading,
        );
    }
    if args.options.contains_key("view") || args.options.contains_key("out") {
        return Err(bad_input("--view and --out need --compare"));
    }
    let mut written = serde_json::Map::new();
    for (name, png) in shaded(dir, width, height, shading)? {
        let path = dir.join(&name);
        std::fs::write(&path, png)
            .map_err(|e| Refusal::new(RefusalCode::Io, format!("{}: {e}", path.display())))?;
        let view = name
            .trim_start_matches("still_")
            .trim_end_matches(".png")
            .trim_end_matches(shading.suffix())
            .to_string();
        written.insert(view, json!(path.display().to_string()));
    }
    let text = json!({"stills": written}).to_string();
    Ok((
        json!({"stills": written, "width": width, "height": height, "shading": shading.name()}),
        0,
        text,
    ))
}

/// `--shading`: the standard still unless asked for the legible one.
pub(super) fn shading(value: Option<&str>) -> Result<Shading, Refusal> {
    match value {
        None => Ok(Shading::Standard),
        Some(name) => Shading::parse(name)
            .ok_or_else(|| bad_input(format!("--shading {name}: give standard or legible"))),
    }
}

fn compare(
    baseline: &Path,
    revised: &Path,
    view: &str,
    out: &Path,
    (width, height): (usize, usize),
    shading: Shading,
) -> Result<(Value, i32, String), Refusal> {
    if crate::round::regular(out, true)? {
        return Err(bad_input("the comparison output directory must be new"));
    }
    let strain = strain_view(view)?;
    let mesh = |dir: &Path, name: &str| {
        crate::round::regular(dir, true)?;
        crate::round::regular(&dir.join(name), false)?;
        read_mesh(dir, name)
    };
    let before_body = mesh(baseline, "body.obj")?;
    let after_body = mesh(revised, "body.obj")?;
    let baseline_cloth = mesh(baseline, "garment.obj")?;
    let revised_cloth = mesh(revised, "garment.obj")?;
    let camera = still_camera(&before_body, &baseline_cloth, view)
        .ok_or_else(|| bad_input("unknown native still view"))?;
    let (before_cloth, after_cloth) = if strain {
        (
            checked_strain(baseline, &baseline_cloth)?,
            checked_strain(revised, &revised_cloth)?,
        )
    } else {
        (baseline_cloth, revised_cloth)
    };
    let same_body = before_body.vertices == after_body.vertices
        && before_body.triangles == after_body.triangles;
    let draw = |body, cloth| {
        png(
            width,
            height,
            &still(body, cloth, camera, [width, height], shading, strain),
        )
    };
    let before = draw(&before_body, &before_cloth);
    let after = draw(&after_body, &after_cloth);
    std::fs::create_dir(out).map_err(|e| crate::workspace::io_error(out, e))?;
    let before_path = out.join("before.png");
    let after_path = out.join("after.png");
    crate::round::write_new(&before_path, &before)?;
    crate::round::write_new(&after_path, &after)?;
    let report = json!({"view":view, "width":width, "height":height, "camera":"baseline",
        "shading":shading.name(), "same_body_geometry":same_body, "before":before_path,
        "after":after_path});
    Ok((report.clone(), 0, report.to_string()))
}

pub(super) fn strain_view(view: &str) -> Result<bool, Refusal> {
    match view {
        "front" | "side" | "back" | "three_quarter" => Ok(false),
        "strain_front" | "strain_back" => Ok(true),
        _ => Err(bad_input("unknown native still view")),
    }
}

pub(super) fn checked_strain(dir: &Path, garment: &ColouredMesh) -> Result<ColouredMesh, Refusal> {
    let path = dir.join("garment_strain.obj");
    crate::round::regular(&path, false)?;
    let strain = crate::fold::mesh(&path)?;
    if strain.vertices != garment.vertices || strain.triangles != garment.triangles {
        return Err(bad_input(
            "garment_strain.obj does not match garment.obj geometry",
        ));
    }
    Ok(strain)
}

/// Shared preflight: bad dimensions fail before reading a body or reserving a round.
pub(super) fn resolution(value: Option<&str>) -> Result<(usize, usize), Refusal> {
    let size = match value {
        None => (960, 540),
        Some(r) => {
            let parsed = r
                .split_once('x')
                .and_then(|(w, h)| Some((w.parse::<usize>().ok()?, h.parse::<usize>().ok()?)));
            match parsed {
                Some((w, h)) if (16..=8192).contains(&w) && (16..=8192).contains(&h) => (w, h),
                _ => {
                    return Err(Refusal::new(
                        RefusalCode::OutOfRange,
                        format!("--res {r}: give WIDTHxHEIGHT, each 16 to 8192 pixels"),
                    ));
                }
            }
        }
    };
    if size.0 * size.1 > 16_000_000 {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "stills exceed the 16-million-pixel per-view rendering budget",
        ));
    }
    Ok(size)
}

/// Compute the complete still set before the caller writes any output.
#[cfg(test)]
fn images(dir: &Path, width: usize, height: usize) -> Result<Vec<(String, Vec<u8>)>, Refusal> {
    shaded(dir, width, height, Shading::Standard)
}

/// The complete still set in `shading`, named `still_<view>.png` or `still_<view>_legible.png`.
pub(super) fn shaded(
    dir: &Path,
    width: usize,
    height: usize,
    shading: Shading,
) -> Result<Vec<(String, Vec<u8>)>, Refusal> {
    let body = read_mesh(dir, "body.obj")?;
    let garment = read_mesh(dir, "garment.obj")?;
    let strain = checked_strain(dir, &garment)?;
    Ok(stills_shaded(
        &body,
        &garment,
        &strain,
        [width, height],
        shading,
    ))
}

fn read_mesh(dir: &Path, name: &str) -> Result<ColouredMesh, Refusal> {
    let path = dir.join(name);
    crate::round::regular(&path, false)?;
    crate::fold::mesh(&path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn compare_uses_baseline_view_without_touching_either_scene() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("ftx-render-pair-{}-{stamp}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let before = root.join("before");
        let after = root.join("after");
        let out = root.join("pair");
        for (dir, low) in [(&before, 0.2), (&after, 0.9)] {
            fs::create_dir(dir).unwrap();
            let body = "v -0.3 0 0\nv 0.3 0 0\nv -0.3 0 1.5\nf 1 2 3\n";
            let cloth = format!("v -0.3 -0.1 {low}\nv 0.3 -0.1 {low}\nv -0.3 -0.1 1.2\nf 1 2 3\n");
            fs::write(dir.join("body.obj"), body).unwrap();
            for name in ["garment.obj", "garment_strain.obj"] {
                fs::write(dir.join(name), &cloth).unwrap();
            }
            let front = images(dir, 40, 32).unwrap().remove(0).1;
            fs::write(dir.join("still_front.png"), front).unwrap();
        }
        let original_before = fs::read(before.join("still_front.png")).unwrap();
        let original_after = fs::read(after.join("still_front.png")).unwrap();
        let standard = Shading::Standard;
        let (report, code, _) =
            compare(&before, &after, "front", &out, (40, 32), standard).unwrap();
        assert_eq!(code, 0);
        assert_eq!(report["same_body_geometry"], true);
        assert_eq!(report["shading"], "standard");
        assert_eq!(fs::read(out.join("before.png")).unwrap(), original_before);
        let rendered_after = fs::read(out.join("after.png")).unwrap();
        assert_ne!(rendered_after, original_before);
        assert_ne!(rendered_after, original_after);
        assert_eq!(
            fs::read(before.join("still_front.png")).unwrap(),
            original_before
        );
        assert_eq!(
            fs::read(after.join("still_front.png")).unwrap(),
            original_after
        );
        assert!(compare(&before, &after, "front", &out, (16, 16), standard).is_err());
        assert_eq!(fs::read(out.join("after.png")).unwrap(), rendered_after);

        // the legible pair goes to a directory of its own and is a different drawing of the same scenes
        let legible_out = root.join("pair-legible");
        let (report, code, _) = compare(
            &before,
            &after,
            "front",
            &legible_out,
            (40, 32),
            Shading::Legible,
        )
        .unwrap();
        assert_eq!((code, &report["shading"]), (0, &json!("legible")));
        assert_ne!(
            fs::read(legible_out.join("before.png")).unwrap(),
            original_before
        );
        assert_eq!(fs::read(out.join("before.png")).unwrap(), original_before);
    }

    #[test]
    fn the_legible_set_is_six_files_of_its_own_and_an_unknown_shading_is_refused() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("ftx-render-legible-{}-{stamp}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let body = "v -0.3 0 0\nv 0.3 0 0\nv -0.3 0 1.5\nf 1 2 3\n";
        let cloth = "v -0.3 -0.1 0.2\nv 0.3 -0.1 0.2\nv -0.3 -0.1 1.2\nf 1 2 3\n";
        fs::write(dir.join("body.obj"), body).unwrap();
        for name in ["garment.obj", "garment_strain.obj"] {
            fs::write(dir.join(name), cloth).unwrap();
        }
        let standard = images(&dir, 40, 32).unwrap();
        assert_eq!(
            standard,
            shaded(&dir, 40, 32, Shading::Standard).unwrap(),
            "the standard set is the one ftx render has always written"
        );
        let legible = shaded(&dir, 40, 32, Shading::Legible).unwrap();
        assert_eq!(legible.len(), 6);
        for ((plain_name, plain), (name, drawn)) in standard.iter().zip(&legible) {
            let view = plain_name.trim_end_matches(".png");
            assert_eq!(name, &format!("{view}_legible.png"));
            assert_eq!(&drawn[..8], b"\x89PNG\r\n\x1a\n");
            assert_ne!(plain, drawn, "{name}");
        }

        assert_eq!(shading(None).unwrap(), Shading::Standard);
        assert_eq!(shading(Some("standard")).unwrap(), Shading::Standard);
        assert_eq!(shading(Some("legible")).unwrap(), Shading::Legible);
        let refused = shading(Some("glossy")).unwrap_err();
        assert_eq!(refused.code, RefusalCode::InvalidDocument);
        assert!(refused.message.contains("standard or legible"));
    }

    #[test]
    fn images_refuse_mismatched_strain_geometry_and_out_of_range_colours() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "ftx-render-strain-check-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir(&dir).unwrap();
        let body = "v -0.3 0 0\nv 0.3 0 0\nv -0.3 0 1.5\nf 1 2 3\n";
        let garment = "v -0.3 -0.1 0.2 0.5 0.5 0.5\nv 0.3 -0.1 0.2 0.5 0.5 0.5\nv -0.3 -0.1 1.2 0.5 0.5 0.5\nf 1 2 3\n";
        let mismatched = "v -0.3 -0.1 0.2 0.5 0.5 0.5\nv 0.3 -0.1 0.2 0.5 0.5 0.5\nv -0.3 -0.1 1.2 0.5 0.5 0.5\nf 1 3 2\n";
        fs::write(dir.join("body.obj"), body).unwrap();
        fs::write(dir.join("garment.obj"), garment).unwrap();
        fs::write(dir.join("garment_strain.obj"), mismatched).unwrap();
        let err = images(&dir, 40, 32).unwrap_err();
        assert_eq!(err.code, RefusalCode::InvalidDocument);
        assert!(err.message.contains("garment_strain.obj"));

        let bad_colour = "v -0.3 -0.1 0.2 1.5 0.5 0.5\nv 0.3 -0.1 0.2 0.5 0.5 0.5\nv -0.3 -0.1 1.2 0.5 0.5 0.5\nf 1 2 3\n";
        fs::write(dir.join("garment_strain.obj"), bad_colour).unwrap();
        let err = images(&dir, 40, 32).unwrap_err();
        assert_eq!(err.code, RefusalCode::InvalidDocument);
        assert!(err.message.contains("valid colours"));

        fs::write(dir.join("garment_strain.obj"), garment).unwrap();
        assert_eq!(images(&dir, 40, 32).unwrap().len(), 6);
    }
}

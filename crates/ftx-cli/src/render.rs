//! `ftx render DIR [--res WxH]`: the six stills of an `ftx drape` run (`ftx_drape::render`), written into DIR
//! as `still_<view>.png` from its `body.obj`, `garment.obj` and `garment_strain.obj` (the counterpart of the
//! reference's `blender_sew.py --stills-from DIR`, with no Blender).

use std::path::Path;

use ftx_core::{Refusal, RefusalCode};
use ftx_drape::render::{ColouredMesh, png, read_obj, render, still_camera, stills};
use serde_json::{Value, json};

use super::{Args, bad_input, known_options, required};

pub(super) fn cmd_render(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(args, "render", &["res", "compare", "view", "out"])?;
    if args.positional.len() != 2 {
        return Err(bad_input("ftx render needs exactly one drape directory"));
    }
    let dir = Path::new(&args.positional[1]);
    let (width, height) = resolution(args.options.get("res").map(String::as_str))?;
    if let Some(revised) = args.options.get("compare") {
        return compare(
            dir,
            Path::new(revised),
            required(args, "view")?,
            Path::new(required(args, "out")?),
            width,
            height,
        );
    }
    if args.options.contains_key("view") || args.options.contains_key("out") {
        return Err(bad_input("--view and --out need --compare"));
    }
    let mut written = serde_json::Map::new();
    for (name, png) in images(dir, width, height)? {
        let path = dir.join(&name);
        std::fs::write(&path, png)
            .map_err(|e| Refusal::new(RefusalCode::Io, format!("{}: {e}", path.display())))?;
        let view = name
            .trim_start_matches("still_")
            .trim_end_matches(".png")
            .to_string();
        written.insert(view, json!(path.display().to_string()));
    }
    let text = json!({"stills": written}).to_string();
    Ok((
        json!({"stills": written, "width": width, "height": height}),
        0,
        text,
    ))
}

fn compare(
    baseline: &Path,
    revised: &Path,
    view: &str,
    out: &Path,
    width: usize,
    height: usize,
) -> Result<(Value, i32, String), Refusal> {
    if crate::round::regular(out, true)? {
        return Err(bad_input("the comparison output directory must be new"));
    }
    let cloth_file = match view {
        "front" | "side" | "back" | "three_quarter" => "garment.obj",
        "strain_front" | "strain_back" => "garment_strain.obj",
        _ => return Err(bad_input("unknown native still view")),
    };
    let mesh = |dir: &Path, name: &str| {
        crate::round::regular(dir, true)?;
        crate::round::regular(&dir.join(name), false)?;
        read_mesh(dir, name)
    };
    let before_body = mesh(baseline, "body.obj")?;
    let after_body = mesh(revised, "body.obj")?;
    let baseline_cloth = mesh(baseline, "garment.obj")?;
    let camera = still_camera(&before_body, &baseline_cloth, view)
        .ok_or_else(|| bad_input("unknown native still view"))?;
    let before_cloth = if cloth_file == "garment.obj" {
        baseline_cloth
    } else {
        mesh(baseline, cloth_file)?
    };
    let after_cloth = mesh(revised, cloth_file)?;
    let same_body = before_body.vertices == after_body.vertices
        && before_body.triangles == after_body.triangles;
    let draw = |body, cloth| {
        png(
            width,
            height,
            &render(&[body, cloth], camera, width, height),
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
        "same_body_geometry":same_body, "before":before_path, "after":after_path});
    Ok((report.clone(), 0, report.to_string()))
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
pub(super) fn images(
    dir: &Path,
    width: usize,
    height: usize,
) -> Result<Vec<(String, Vec<u8>)>, Refusal> {
    let (body, garment, strain) = (
        read_mesh(dir, "body.obj")?,
        read_mesh(dir, "garment.obj")?,
        read_mesh(dir, "garment_strain.obj")?,
    );
    Ok(stills(&body, &garment, &strain, width, height))
}

fn read_mesh(dir: &Path, name: &str) -> Result<ColouredMesh, Refusal> {
    let path = dir.join(name);
    let text = std::fs::read_to_string(&path)
        .map_err(|e| Refusal::new(RefusalCode::Io, format!("{}: {e}", path.display())))?;
    let mesh = read_obj(&text).map_err(|e| bad_input(format!("{}: {e}", path.display())))?;
    if mesh.vertices.is_empty()
        || mesh.triangles.is_empty()
        || mesh.vertices.iter().any(|v| !v.is_finite())
        || mesh.colours.iter().flatten().any(|v| !v.is_finite())
    {
        return Err(bad_input(format!(
            "{}: rendering needs a nonempty finite triangle mesh",
            path.display()
        )));
    }
    Ok(mesh)
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
        let (report, code, _) = compare(&before, &after, "front", &out, 40, 32).unwrap();
        assert_eq!(code, 0);
        assert_eq!(report["same_body_geometry"], true);
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
        assert!(compare(&before, &after, "front", &out, 16, 16).is_err());
        assert_eq!(fs::read(out.join("after.png")).unwrap(), rendered_after);
    }
}

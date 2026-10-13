//! `ftx render DIR [--res WxH]`: the six stills of an `ftx drape` run (`ftx_drape::render`), written into DIR
//! as `still_<view>.png` from its `body.obj`, `garment.obj` and `garment_strain.obj` (the counterpart of the
//! reference's `blender_sew.py --stills-from DIR`, with no Blender).

use std::path::Path;

use ftx_core::{Refusal, RefusalCode};
use ftx_drape::render::{read_obj, stills};
use serde_json::{Value, json};

use super::{Args, bad_input, known_options};

pub(super) fn cmd_render(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(args, "render", &["res"])?;
    if args.positional.len() != 2 {
        return Err(bad_input("ftx render needs exactly one drape directory"));
    }
    let dir = Path::new(&args.positional[1]);
    let (width, height) = match args.options.get("res") {
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
    let read = |name: &str| {
        let path = dir.join(name);
        let text = std::fs::read_to_string(&path)
            .map_err(|e| Refusal::new(RefusalCode::Io, format!("{}: {e}", path.display())))?;
        read_obj(&text).map_err(|e| bad_input(format!("{}: {e}", path.display())))
    };
    let (body, garment, strain) = (
        read("body.obj")?,
        read("garment.obj")?,
        read("garment_strain.obj")?,
    );
    let mut written = serde_json::Map::new();
    for (name, png) in stills(&body, &garment, &strain, width, height) {
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

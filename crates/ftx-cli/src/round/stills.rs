//! Optional construction stills of this round's completed simulation, never of a cached old run.
use std::path::Path;

use ftx_core::{Refusal, RefusalCode};
use serde_json::{Value, json};

pub(super) const VIEWS: [(&str, &str); 6] = [
    ("front", "Front"),
    ("side", "Side"),
    ("back", "Back"),
    ("three_quarter", "Three-quarter"),
    ("strain_front", "Front strain"),
    ("strain_back", "Back strain"),
];

pub(super) fn publish(
    rd: &Path,
    number: u64,
    sim: Option<&Value>,
    size: Option<(usize, usize)>,
) -> Result<(Value, Vec<String>), Refusal> {
    let Some((width, height)) = size else {
        return Ok((json!({"status":"NOT_REQUESTED"}), Vec::new()));
    };
    if sim.is_none_or(|s| s.get("refusal").is_some()) {
        return Ok((
            json!({"status":"SKIPPED", "reason":"no completed simulation in this round"}),
            Vec::new(),
        ));
    }
    let dir = rd.join("sim");
    let result = (|| {
        if !super::regular(&dir, true)? {
            return Err(super::bad_input(
                "the current round has no simulation directory",
            ));
        }
        for name in ["body.obj", "garment.obj", "garment_strain.obj"] {
            if !super::regular(&dir.join(name), false)? {
                return Err(super::bad_input(format!(
                    "the current simulation is missing {name}"
                )));
            }
        }
        // Check all destinations before computing or writing; stills from any earlier attempt
        // are evidence, not an invitation to overwrite or mix frames across runs.
        for (view, _) in VIEWS {
            let path = dir.join(format!("still_{view}.png"));
            if super::regular(&path, false)? {
                return Err(Refusal::new(
                    RefusalCode::Io,
                    format!("{} already exists; start a new round", path.display()),
                ));
            }
        }
        let rendered = crate::render::images(&dir, width, height)?;
        let expected: Vec<_> = VIEWS
            .iter()
            .map(|(v, _)| format!("still_{v}.png"))
            .collect();
        if rendered.iter().map(|(name, _)| name).ne(expected.iter()) {
            return Err(super::bad_input(
                "renderer did not produce the complete expected still set",
            ));
        }
        let mut images = Vec::new();
        for (name, png) in rendered {
            super::write_new(&dir.join(&name), &png)?;
            images.push(format!("rounds/{number:03}/sim/{name}"));
        }
        Ok(images)
    })();
    match result {
        Ok(images) => Ok((
            json!({"status":"PASS", "width":width, "height":height, "views":VIEWS.map(|(v, _)| v),
                "no_claim":"Still images visualize a drape prediction; they do not establish fit. No video was rendered."}),
            images,
        )),
        Err(refusal) => {
            let failure = json!({"status":"FAILED", "refusal":refusal});
            super::write_json(&rd.join("render_failure.json"), &failure)?;
            Ok((failure, Vec::new()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn scene() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let ws = std::env::temp_dir().join(format!(
            "ftx-round-stills-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let rd = ws.join("rounds/007");
        std::fs::create_dir_all(rd.join("sim")).unwrap();
        let body = "v -0.2 0 0\nv 0.2 0 0\nv 0.2 0 1.5\nv -0.2 0 1.5\nf 1 2 3\nf 1 3 4\n";
        let garment = "v -0.3 -0.1 0.5 0.9 0.1 0.1\nv 0.3 -0.1 0.5 0.9 0.1 0.1\nv 0.3 -0.1 1.2 0.9 0.1 0.1\nv -0.3 -0.1 1.2 0.9 0.1 0.1\nf 1 2 3\nf 1 3 4\n";
        for (name, contents) in [
            ("body.obj", body),
            ("garment.obj", garment),
            ("garment_strain.obj", garment),
        ] {
            std::fs::write(rd.join("sim").join(name), contents).unwrap();
        }
        rd
    }

    #[test]
    fn completed_simulation_renders_six_real_pngs_and_never_overwrites_them() {
        let rd = scene();
        let sim = json!({"settled":true, "gating":[], "advisory":[]});
        let (report, images) = publish(&rd, 7, Some(&sim), Some((48, 32))).unwrap();
        assert_eq!(report["status"], "PASS");
        assert_eq!(images.len(), 6);
        assert_eq!(images[0], "rounds/007/sim/still_front.png");
        for (view, _) in VIEWS {
            let bytes = std::fs::read(rd.join(format!("sim/still_{view}.png"))).unwrap();
            let pixels = ftx_trace::raster::image::load_png(&bytes).unwrap();
            assert_eq!((pixels.w, pixels.h), (48, 32));
        }
        let path = rd.join("sim/still_front.png");
        let original = std::fs::read(&path).unwrap();
        let (failure, images) = publish(&rd, 7, Some(&sim), Some((48, 32))).unwrap();
        assert_eq!(failure["status"], "FAILED");
        assert!(images.is_empty());
        assert_eq!(std::fs::read(path).unwrap(), original);
        assert!(rd.join("render_failure.json").is_file());
    }

    #[test]
    fn skipped_and_failed_physics_never_open_apparent_success_artifacts() {
        let rd = scene();
        let failed = json!({"settled":false,"gating":["solver failed"],"refusal":{"code":"io"}});
        for sim in [None, Some(&failed)] {
            let (report, images) = publish(&rd, 1, sim, Some((48, 32))).unwrap();
            assert_eq!(report["status"], "SKIPPED");
            assert!(images.is_empty());
            assert!(!rd.join("sim/still_front.png").exists());
        }
        let (report, _) = publish(&rd, 1, None, None).unwrap();
        assert_eq!(report["status"], "NOT_REQUESTED");
    }

    #[test]
    fn the_review_shows_only_this_completed_rounds_whitelisted_stills() {
        let rd = scene();
        let ws = rd.parent().unwrap().parent().unwrap();
        let sim = json!({"settled":true, "gating":[], "advisory":[]});
        let (rendering, images) = publish(&rd, 7, Some(&sim), Some((48, 32))).unwrap();
        let mut entry = json!({"round":7,"dir":"rounds/007", "sim":sim,
            "rendering":rendering,"images":images});
        entry["images"]
            .as_array_mut()
            .unwrap()
            .push(json!("https://untrusted.invalid/tracker.png"));
        entry["images"]
            .as_array_mut()
            .unwrap()
            .push(json!("rounds/006/sim/still_front.png"));
        let html = super::super::report::render(ws, &[entry.clone()]).unwrap();
        assert_eq!(html.matches("loading=\"lazy\"").count(), 6);
        assert!(html.contains("src=\"rounds/007/sim/still_front.png\""));
        assert!(!html.contains("untrusted.invalid"));
        assert!(!html.contains("rounds/006"));
        entry["sim"]["refusal"] = json!({"code":"io","message":"solver failed"});
        let html = super::super::report::render(ws, &[entry.clone()]).unwrap();
        assert!(!html.contains("loading=\"lazy\""));
        entry["rendering"] = json!({"status":"FAILED","refusal":{"message":"<bad mesh>"}});
        let html = super::super::report::render(ws, &[entry]).unwrap();
        assert!(html.contains("Stills failed"));
        assert!(html.contains("&lt;bad mesh&gt;"));
    }

    #[test]
    fn empty_and_nonfinite_meshes_never_publish_blank_success_images() {
        for contents in ["", "v NaN 0 0\nv 1 0 0\nv 0 0 1\nf 1 2 3\n"] {
            let rd = scene();
            std::fs::write(rd.join("sim/body.obj"), contents).unwrap();
            let (failure, images) =
                publish(&rd, 7, Some(&json!({"settled":true})), Some((48, 32))).unwrap();
            assert_eq!(failure["status"], "FAILED");
            assert!(images.is_empty());
            assert!(!rd.join("sim/still_front.png").exists());
        }
    }

    #[cfg(unix)]
    #[test]
    fn an_input_symlink_is_refused_without_reading_or_replacing_its_target() {
        let root = scene();
        let rd = root.join("next");
        std::fs::create_dir_all(rd.join("sim")).unwrap();
        std::os::unix::fs::symlink(root.join("sim/body.obj"), rd.join("sim/body.obj")).unwrap();
        let (failure, images) =
            publish(&rd, 2, Some(&json!({"settled":true})), Some((48, 32))).unwrap();
        assert_eq!(failure["status"], "FAILED");
        assert!(images.is_empty());
        assert!(!root.join("sim/still_front.png").exists());
    }
}

//! Optional construction stills of this round's completed simulation, never of a cached old run.
use std::path::Path;

use ftx_core::{Refusal, RefusalCode};
use ftx_drape::render::Shading;
use serde_json::{Value, json};

pub(super) const VIEWS: [(&str, &str); 6] = [
    ("front", "Front"),
    ("side", "Side"),
    ("back", "Back"),
    ("three_quarter", "Three-quarter"),
    ("strain_front", "Front strain"),
    ("strain_back", "Back strain"),
];

/// Which still set(s) `ftx round --stills` publishes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RoundShading {
    Standard,
    Legible,
    Both,
}

impl RoundShading {
    pub(super) fn parse(value: Option<&str>) -> Result<Self, Refusal> {
        match value {
            None | Some("standard") => Ok(Self::Standard),
            Some("legible") => Ok(Self::Legible),
            Some("both") => Ok(Self::Both),
            Some(other) => Err(super::bad_input(format!(
                "--shading {other}: give standard, legible or both"
            ))),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Legible => "legible",
            Self::Both => "both",
        }
    }

    fn shadings(self) -> &'static [Shading] {
        match self {
            Self::Standard => &[Shading::Standard],
            Self::Legible => &[Shading::Legible],
            Self::Both => &[Shading::Standard, Shading::Legible],
        }
    }
}

pub(super) fn publish(
    rd: &Path,
    number: u64,
    sim: Option<&Value>,
    size: Option<(usize, usize)>,
    shading: RoundShading,
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
        for &mode in shading.shadings() {
            let suffix = mode.suffix();
            for (view, _) in VIEWS {
                let path = dir.join(format!("still_{view}{suffix}.png"));
                if super::regular(&path, false)? {
                    return Err(Refusal::new(
                        RefusalCode::Io,
                        format!("{} already exists; start a new round", path.display()),
                    ));
                }
            }
        }
        let mut all_rendered = Vec::new();
        for &mode in shading.shadings() {
            let suffix = mode.suffix();
            let rendered = crate::render::shaded(&dir, width, height, mode)?;
            let expected: Vec<_> = VIEWS
                .iter()
                .map(|(v, _)| format!("still_{v}{suffix}.png"))
                .collect();
            if rendered.iter().map(|(name, _)| name).ne(expected.iter()) {
                return Err(super::bad_input(
                    "renderer did not produce the complete expected still set",
                ));
            }
            all_rendered.extend(rendered);
        }
        let mut images = Vec::new();
        for (name, png) in all_rendered {
            super::write_new(&dir.join(&name), &png)?;
            images.push(format!("rounds/{number:03}/sim/{name}"));
        }
        Ok(images)
    })();
    match result {
        Ok(images) => {
            let mut report = json!({"status":"PASS", "width":width, "height":height, "views":VIEWS.map(|(v, _)| v),
                "no_claim":"Still images visualize a drape prediction; they do not establish fit. No video was rendered."});
            if shading != RoundShading::Standard {
                report["shading"] = json!(shading.name());
            }
            Ok((report, images))
        }
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
        let (report, images) =
            publish(&rd, 7, Some(&sim), Some((48, 32)), RoundShading::Standard).unwrap();
        assert_eq!(report["status"], "PASS");
        assert!(report.get("shading").is_none());
        assert_eq!(images.len(), 6);
        assert_eq!(images[0], "rounds/007/sim/still_front.png");
        for (view, _) in VIEWS {
            let bytes = std::fs::read(rd.join(format!("sim/still_{view}.png"))).unwrap();
            let pixels = ftx_trace::raster::image::load_png(&bytes).unwrap();
            assert_eq!((pixels.w, pixels.h), (48, 32));
        }
        let path = rd.join("sim/still_front.png");
        let original = std::fs::read(&path).unwrap();
        let (failure, images) =
            publish(&rd, 7, Some(&sim), Some((48, 32)), RoundShading::Standard).unwrap();
        assert_eq!(failure["status"], "FAILED");
        assert!(images.is_empty());
        assert_eq!(std::fs::read(path).unwrap(), original);
        assert!(rd.join("render_failure.json").is_file());
    }

    #[test]
    fn legible_and_both_shadings_publish_distinct_pngs_and_render_in_report() {
        assert_eq!(RoundShading::parse(None).unwrap(), RoundShading::Standard);
        assert_eq!(
            RoundShading::parse(Some("standard")).unwrap(),
            RoundShading::Standard
        );
        assert_eq!(
            RoundShading::parse(Some("legible")).unwrap(),
            RoundShading::Legible
        );
        assert_eq!(
            RoundShading::parse(Some("both")).unwrap(),
            RoundShading::Both
        );
        assert!(RoundShading::parse(Some("glossy")).is_err());

        let sim = json!({"settled":true, "gating":[], "advisory":[]});

        // Legible-only round writes still_<view>_legible.png and shows the legible section in report.html.
        let rd_leg = scene();
        let ws_leg = rd_leg.parent().unwrap().parent().unwrap();
        let (rep_leg, imgs_leg) = publish(
            &rd_leg,
            7,
            Some(&sim),
            Some((48, 32)),
            RoundShading::Legible,
        )
        .unwrap();
        assert_eq!(rep_leg["status"], "PASS");
        assert_eq!(rep_leg["shading"], "legible");
        assert_eq!(imgs_leg.len(), 6);
        assert_eq!(imgs_leg[0], "rounds/007/sim/still_front_legible.png");
        assert!(!rd_leg.join("sim/still_front.png").exists());
        assert!(rd_leg.join("sim/still_front_legible.png").is_file());
        let entry_leg =
            json!({"round":7,"dir":"rounds/007","sim":sim,"rendering":rep_leg,"images":imgs_leg});
        let html_leg = super::super::report::render(ws_leg, &[entry_leg]).unwrap();
        assert_eq!(html_leg.matches("loading=\"lazy\"").count(), 6);
        assert!(html_leg.contains("src=\"rounds/007/sim/still_front_legible.png\""));
        assert!(html_leg.contains("legible shading"));

        // Both-shading round writes all 12 PNGs and renders both sections in report.html.
        let rd_both = scene();
        let ws_both = rd_both.parent().unwrap().parent().unwrap();
        let (rep_both, imgs_both) =
            publish(&rd_both, 7, Some(&sim), Some((48, 32)), RoundShading::Both).unwrap();
        assert_eq!(rep_both["status"], "PASS");
        assert_eq!(rep_both["shading"], "both");
        assert_eq!(imgs_both.len(), 12);
        let std_bytes = std::fs::read(rd_both.join("sim/still_front.png")).unwrap();
        let leg_bytes = std::fs::read(rd_both.join("sim/still_front_legible.png")).unwrap();
        assert_ne!(
            std_bytes, leg_bytes,
            "legible still must differ from standard still"
        );
        let entry_both =
            json!({"round":7,"dir":"rounds/007","sim":sim,"rendering":rep_both,"images":imgs_both});
        let html_both = super::super::report::render(ws_both, &[entry_both]).unwrap();
        assert_eq!(html_both.matches("loading=\"lazy\"").count(), 12);
        assert!(html_both.contains("src=\"rounds/007/sim/still_front.png\""));
        assert!(html_both.contains("src=\"rounds/007/sim/still_front_legible.png\""));
    }

    #[test]
    fn inspect_pixel_identifies_the_visible_panel_and_its_authored_edge() {
        let rd = scene();
        let sim = json!({"settled":false,"gating":["moving"]});
        let (rendering, images) =
            publish(&rd, 7, Some(&sim), Some((96, 72)), RoundShading::Standard).unwrap();
        let entry = json!({"round":7,"sim":sim,"rendering":rendering,"images":images});
        super::super::write_json(&rd.join("round.json"), &entry).unwrap();
        super::super::write_json(
            &rd.join("pattern.json"),
            &json!({"schema":"fabric-pattern/1","units":"mm","pieces":[{
            "id":"panel","cut":{"qty":2},"vertices":{"A":[0,0],"B":[60,0],"C":[60,70],"D":[0,70]},
            "edges":[{"id":"hem","from":"A","to":"B"},{"id":"outer","from":"B","to":"C"},
            {"id":"upper","from":"C","to":"D"},{"id":"inner","from":"D","to":"A"}]}],"seams":[]}),
        )
        .unwrap();
        super::super::write_json(&rd.join("sim/mesh_map.json"), &json!({"vertices":4,"inst_range":{"panel.L":[0,4]},
            "edges":{"panel.L/hem":[0,1],"panel.L/outer":[1,2],"panel.L/upper":[2,3],"panel.L/inner":[3,0]}})).unwrap();
        let pick = |view: &str, pixel: &str| {
            let argv = [
                "inspect-pixel",
                rd.to_str().unwrap(),
                "--view",
                view,
                "--pixel",
                pixel,
            ]
            .map(String::from);
            crate::inspect_pixel::run(&crate::parse(&argv).unwrap()).map(|r| r.0)
        };
        let r = pick("front", "48,58").unwrap();
        assert_eq!(r["hit"]["instance"], "panel.L");
        assert_eq!(r["hit"]["piece"], "panel");
        assert_eq!(r["hit"]["nearest_edge"]["source_edge"], "hem");
        assert_eq!(
            r["hit"]["nearest_edge"]["from"],
            json!({"vertex":"A","xy_mm":[0,0]})
        );
        assert_eq!(r["gating"], json!(["moving"]));
        assert_eq!(pick("strain_front", "48,58").unwrap()["hit"], r["hit"]);
        assert_eq!(pick("back", "48,36").unwrap()["hit"]["surface"], "body");
        assert_eq!(
            pick("front", "0,0").unwrap()["hit"]["surface"],
            "background"
        );
        assert!(pick("front", "96,0").is_err());
        assert!(pick("front", "-1,4").is_err());
        assert!(pick("../preview", "0,0").is_err());

        // Apparent success artifacts can survive a failed writer. Only the refused pose owns
        // diagnostic pixels, even when both directories contain a complete set of views.
        let diagnosis = rd.join("sim/refused");
        std::fs::create_dir(&diagnosis).unwrap();
        let original = json!({"code":"not_converged","message":"failed to settle"});
        let failed = json!({"round":7,"sim":{"refusal":original,"gating":["failed to settle"]},
            "rendering":{"status":"SKIPPED"},"images":[]});
        std::fs::write(rd.join("round.json"), failed.to_string()).unwrap();
        assert!(pick("front", "48,58").is_err());
        for name in [
            "body.obj",
            "garment.obj",
            "still_front.png",
            "still_strain_front.png",
        ] {
            std::fs::copy(rd.join("sim").join(name), diagnosis.join(name)).unwrap();
        }
        assert!(
            pick("front", "48,58")
                .unwrap_err()
                .message
                .contains("no diagnostic source-edge map")
        );
        let archived = pick("front", "0,0").unwrap();
        assert_eq!(archived["hit"]["surface"], "background");
        assert_eq!(archived["refusal"], original);
        assert!(archived.get("refused_pose").is_none());
        // Distinct physical copy from the apparent successful endpoint.
        let pose = json!({"phase":"settle","accepted_steps":2,"width":96,"height":72});
        let mut map = json!({"vertices":4,"inst_range":{"panel.R":[0,4]},"refused_pose":pose,
            "edges":{"panel.R/hem":[0,1],"panel.R/outer":[1,2],"panel.R/upper":[2,3],"panel.R/inner":[3,0]}});
        super::super::write_json(&diagnosis.join("mesh_map.json"), &map).unwrap();
        let hit = pick("strain_front", "48,58").unwrap();
        assert_eq!(
            hit["image"],
            "rounds/007/sim/refused/still_strain_front.png"
        );
        assert_eq!(hit["refused_pose"], pose);
        assert_eq!(hit["refusal"], original);
        assert_eq!(hit["hit"]["instance"], "panel.R");
        assert_eq!(hit["hit"]["nearest_edge"]["source_edge"], "hem");
        assert_eq!(
            pick("front", "0,0").unwrap()["hit"]["surface"],
            "background"
        );
        assert!(pick("side", "48,36").is_err());
        map["refused_pose"]["width"] = json!(95);
        std::fs::write(diagnosis.join("mesh_map.json"), map.to_string()).unwrap();
        assert!(
            pick("front", "48,58")
                .unwrap_err()
                .message
                .contains("dimensions")
        );

        // A placement refusal has its own image, geometry and source map even if an
        // interrupted round still contains plausible results in the parent directory.
        let mut failed_start = failed;
        failed_start["sim"]["refusal"]["code"] = json!("intersecting_start");
        std::fs::write(rd.join("round.json"), failed_start.to_string()).unwrap();
        let start = diagnosis.join("start");
        std::fs::create_dir(&start).unwrap();
        for name in ["body.obj", "garment.obj", "still_front.png"] {
            std::fs::copy(diagnosis.join(name), start.join(name)).unwrap();
        }
        super::super::write_json(
            &diagnosis.join("placement.json"),
            &json!({
                "stills":{"start":["relocated/work/start/still_front.png"]}
            }),
        )
        .unwrap();
        assert_eq!(
            pick("front", "0,0").unwrap()["refused_pose"]["phase"],
            "placement"
        );
        assert!(
            pick("front", "48,58")
                .unwrap_err()
                .message
                .contains("source-edge map")
        );
        assert!(
            pick("strain_front", "48,58")
                .unwrap_err()
                .message
                .contains("did not publish")
        );
        super::super::write_json(&start.join("mesh_map.json"), &map).unwrap();
        assert!(pick("front", "48,58").is_err());
        map["refused_pose"] =
            json!({"phase":"placement","accepted_steps":0,"width":96,"height":72});
        std::fs::write(start.join("mesh_map.json"), map.to_string()).unwrap();
        let hit = pick("front", "48,58").unwrap();
        assert_eq!(hit["image"], "rounds/007/sim/refused/start/still_front.png");
        assert_eq!(hit["refusal"]["code"], "intersecting_start");
        assert_eq!(hit["hit"]["instance"], "panel.R");
        assert_eq!(hit["hit"]["nearest_edge"]["source_edge"], "hem");
    }

    #[test]
    fn skipped_and_failed_physics_never_open_apparent_success_artifacts() {
        let rd = scene();
        let failed = json!({"settled":false,"gating":["solver failed"],"refusal":{"code":"io"}});
        for sim in [None, Some(&failed)] {
            let (report, images) =
                publish(&rd, 1, sim, Some((48, 32)), RoundShading::Standard).unwrap();
            assert_eq!(report["status"], "SKIPPED");
            assert!(images.is_empty());
            assert!(!rd.join("sim/still_front.png").exists());
        }
        let (report, _) = publish(&rd, 1, None, None, RoundShading::Standard).unwrap();
        assert_eq!(report["status"], "NOT_REQUESTED");
    }

    #[test]
    fn the_review_shows_only_this_completed_rounds_whitelisted_stills() {
        let rd = scene();
        let ws = rd.parent().unwrap().parent().unwrap();
        let sim = json!({"settled":true, "gating":[], "advisory":[]});
        let (rendering, images) =
            publish(&rd, 7, Some(&sim), Some((48, 32)), RoundShading::Standard).unwrap();
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
        let diagnostic = rd.join("sim/refused");
        std::fs::create_dir(&diagnostic).unwrap();
        std::fs::copy(
            rd.join("sim/still_front.png"),
            diagnostic.join("still_front.png"),
        )
        .unwrap();
        let html = super::super::report::render(ws, &[entry.clone()]).unwrap();
        assert!(!html.contains("loading=\"lazy\""));
        assert!(html.contains("href=\"rounds/007/sim/refused/still_front.png\""));
        assert!(!html.contains("href=\"rounds/007/sim/refused/still_back.png\""));
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
            let (failure, images) = publish(
                &rd,
                7,
                Some(&json!({"settled":true})),
                Some((48, 32)),
                RoundShading::Standard,
            )
            .unwrap();
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
        let (failure, images) = publish(
            &rd,
            2,
            Some(&json!({"settled":true})),
            Some((48, 32)),
            RoundShading::Standard,
        )
        .unwrap();
        assert_eq!(failure["status"], "FAILED");
        assert!(images.is_empty());
        assert!(!root.join("sim/still_front.png").exists());
    }
}

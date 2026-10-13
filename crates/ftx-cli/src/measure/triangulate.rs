//! Calibrated multi-view coordinates; no decoder, image detector or guessed metric scale.

use ftx_core::Refusal;
use ftx_measure::triangulate::uncertainty::{ClickNoiseInput, triangulate_with_click_noise};
use ftx_measure::triangulate::{TriangulationInput, triangulate};
use serde_json::Value;

use super::{option, photo, single};
use crate::{Args, bad_input, known_options, read_json};

pub(super) fn run(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(args, "measure triangulate", &["out", "click-noise"])?;
    let path = single(args, "triangulate")?;
    let input: TriangulationInput = serde_json::from_value(read_json(path)?.0)
        .map_err(|e| bad_input(format!("invalid triangulation input: {e}")))?;
    let (report, uncertainty) = if let Some(path) = option(args, "click-noise") {
        let noise: ClickNoiseInput = serde_json::from_value(read_json(path)?.0)
            .map_err(|e| bad_input(format!("invalid click-noise input: {e}")))?;
        let result = triangulate_with_click_noise(&input, &noise)?;
        (
            result.triangulation,
            Some(result.conditional_click_uncertainty),
        )
    } else {
        (triangulate(&input)?, None)
    };
    let code = if report.point_m.is_some()
        && uncertainty.as_ref().is_none_or(|u| u.status == "AVAILABLE")
    {
        0
    } else {
        2
    };
    let mut text = format!(
        "TRIANGULATION: {} ({} fit, {} held-out views)",
        report.status, report.fit_views, report.held_out_views
    );
    if let Some(point) = report.point_m {
        text.push_str(&format!(
            "\nWorld point [m, z up]: [{:.9}, {:.9}, {:.9}]",
            point.x, point.y, point.z
        ));
    } else {
        text.push_str("\nNo usable point was published.");
    }
    for flag in report.flags.iter().chain(&report.advisories) {
        text.push_str(&format!("\n  {flag}"));
    }
    text.push_str(&format!("\n{}", report.no_claim));
    let mut value = serde_json::to_value(&report).map_err(|e| bad_input(e.to_string()))?;
    if let Some(uncertainty) = uncertainty {
        text.push_str(&format!(
            "\nCONDITIONAL CLICK UNCERTAINTY: {}",
            uncertainty.status
        ));
        if let Some(sigma) = uncertainty.sigma_m {
            text.push_str(&format!(
                "\nClick-only one-sigma [m]: [{:.9}, {:.9}, {:.9}]",
                sigma.x, sigma.y, sigma.z
            ));
        }
        for flag in &uncertainty.flags {
            text.push_str(&format!("\n  {flag}"));
        }
        text.push_str(&format!("\n{}", uncertainty.no_claim));
        value["conditional_click_uncertainty"] =
            serde_json::to_value(uncertainty).map_err(|e| bad_input(e.to_string()))?;
    }
    if let Some(path) = option(args, "out") {
        // Reuse exclusive evidence creation: --out may not overwrite an input or old report,
        // including through a symlink/hard link. Finish computation before creating any file.
        photo::save(path, &value)?;
        eprintln!("wrote {path}");
    }
    Ok((value, code, text))
}

#[cfg(test)]
mod click_noise_tests {
    use serde_json::{Value, json};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    fn fixture() -> (PathBuf, PathBuf, PathBuf) {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "ftx-click-noise-{}-{nanos}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&dir).unwrap();
        let frame = |stream: &str| json!({"stream":stream,"timestamp_us":0});
        let views: Vec<_> = [("left", -0.5, 200.0), ("right", 0.5, -200.0)]
            .into_iter()
            .map(|(stream, center, pixel)| {
                json!({
                    "frame":frame(stream), "pixel_px":[pixel,0.0], "partition":"fit",
                    "camera":{"intrinsics":{"focal_px":[800.0,800.0],"principal_px":[0.0,0.0]},
                        "center_m":[center,-2.0,1.0],
                        "world_to_camera":{"rows":[[1.0,0.0,0.0],[0.0,0.0,-1.0],[0.0,1.0,0.0]]}}
                })
            })
            .collect();
        let point = json!({"schema":"ftx-triangulation-input/1",
            "metric_scale_source":"synthetic one-metre baseline",
            "capture":{"kind":"synchronized","max_skew_us":0,"evidence":"synthetic aligned exposures"},
            "observations":views});
        let noise = json!({"schema":"ftx-click-noise/1","evidence":"synthetic one-pixel clicks",
            "observations":[{"frame":frame("left"),"sigma_px":1.0},
                {"frame":frame("right"),"sigma_px":1.0}]});
        let source = dir.join("point.json");
        let noise_path = dir.join("noise.json");
        std::fs::write(&source, serde_json::to_vec(&point).unwrap()).unwrap();
        std::fs::write(&noise_path, serde_json::to_vec(&noise).unwrap()).unwrap();
        (dir, source, noise_path)
    }

    fn args(source: &Path, noise: Option<&Path>, output: Option<&Path>) -> crate::Args {
        let mut argv = vec![
            "measure".into(),
            "triangulate".into(),
            source.to_string_lossy().into_owned(),
            "--json".into(),
        ];
        if let Some(path) = noise {
            argv.extend(["--click-noise".into(), path.to_string_lossy().into_owned()]);
        }
        if let Some(path) = output {
            argv.extend(["--out".into(), path.to_string_lossy().into_owned()]);
        }
        crate::parse(&argv).unwrap()
    }

    #[test]
    fn optional_noise_adds_precision_without_changing_the_geometry_report() {
        let (dir, source, noise) = fixture();
        let output = dir.join("result.json");
        let (base, base_code, _) = super::run(&args(&source, None, None)).unwrap();
        let (mut augmented, code, text) =
            super::run(&args(&source, Some(&noise), Some(&output))).unwrap();
        assert_eq!((base_code, code), (0, 0));
        let written: Value = serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap();
        assert_eq!(written, augmented);
        let estimate = augmented
            .as_object_mut()
            .unwrap()
            .remove("conditional_click_uncertainty")
            .unwrap();
        assert_eq!(augmented, base);
        assert_eq!(estimate["status"], "AVAILABLE");
        assert!(estimate["sigma_m"][1].as_f64().unwrap() > 0.007);
        assert!(text.contains("Click-only one-sigma"));
    }

    #[test]
    fn invalid_noise_publishes_no_output() {
        let (dir, source, noise) = fixture();
        std::fs::write(
            &noise,
            br#"{"schema":"ftx-click-noise/1","evidence":"test","observations":[]}"#,
        )
        .unwrap();
        let output = dir.join("not-created").join("result.json");
        assert!(super::run(&args(&source, Some(&noise), Some(&output))).is_err());
        assert!(!output.parent().unwrap().exists());
    }

    #[test]
    fn noise_evidence_cannot_be_overwritten_by_the_report() {
        let (_, source, noise) = fixture();
        let before = std::fs::read(&noise).unwrap();
        assert!(super::run(&args(&source, Some(&noise), Some(&noise))).is_err());
        assert_eq!(std::fs::read(&noise).unwrap(), before);
    }

    #[test]
    fn unavailable_precision_is_exit_two_without_hiding_valid_geometry() {
        let (_, source, noise) = fixture();
        let mut input: Value = serde_json::from_slice(&std::fs::read(&source).unwrap()).unwrap();
        for v in input["observations"].as_array_mut().unwrap() {
            v["camera"]["intrinsics"]["distortion"] = json!({"max_radius":0.250001});
        }
        std::fs::write(&source, serde_json::to_vec(&input).unwrap()).unwrap();
        let (value, code, _) = super::run(&args(&source, Some(&noise), None)).unwrap();
        assert_eq!(code, 2);
        assert_eq!(value["status"], "ACCEPTED");
        assert!(value["point_m"].is_array());
        assert_eq!(
            value["conditional_click_uncertainty"]["status"],
            "UNAVAILABLE"
        );
        assert!(value["conditional_click_uncertainty"]["covariance_m2"].is_null());
    }
}

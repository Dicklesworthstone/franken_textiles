//! Actual accepted-shape drafting across native publication and the portable round.
//! Synthetic polygon geometry establishes numerical integration, never anatomical or physical fit.

use super::{invoke, model_bytes_with_locals, read, receipt, save, setup};
use ftx_core::receipt::sha256_hex;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const TODAY: &str = "2026-11-12";
const CREATED: &str = "2026-11-12T00:00:00";
const HEAD_SHAPE: &str = "synthetic-head-radius";

fn measurements(head: Option<f64>) -> Value {
    // Omit age/stature here so automatic chart priors cannot supply missing test observations.
    let records: Vec<_> = head
        .map(|value| {
            ftx_measure::make_record(
                "head",
                value,
                1.0,
                "MEASURED_TAPE",
                "synthetic conflicting comparator",
                None,
                "",
            )
            .unwrap()
        })
        .into_iter()
        .collect();
    json!({"schema":"fabric-measurements/1", "taken_on":TODAY,
        "wearer":{"sex":"F"}, "records":records,
        "fused":{"head":{"value_mm":999.0,"sigma_mm":1.0}}})
}

fn source(hash: &str, radius_change: f64) -> Value {
    // ACCEPTED imports require every active shape value strictly inside its authored bounds.
    assert!(0.0 < radius_change && radius_change < 1.0);
    let mut value = receipt(hash);
    let layout = json!([{"name":HEAD_SHAPE,"lower":0.0,"upper":1.0}]);
    value["model"]["shape"] = layout.clone();
    value["anny_configuration"]["shape_parameters"] = layout;
    value["anny_configuration"]["fixed_shape"] = json!({"height":0.6});
    value["solution"]["shape"] = json!([radius_change]);
    value["solution"]["anny_parameters"]["local_changes"] = json!([radius_change]);
    value
}

fn source_text(value: &Value) -> String {
    // Whitespace is intentional: retained evidence must preserve the source bytes, not just JSON.
    format!("\n{}\n", serde_json::to_string_pretty(value).unwrap())
}

fn fixture(measurements: &Value) -> (PathBuf, Vec<u8>, String) {
    let dir = setup();
    let bytes = model_bytes_with_locals(true, &[HEAD_SHAPE]);
    let hash = sha256_hex(&bytes);
    std::fs::write(dir.join("model.ftxbody"), &bytes).unwrap();
    save(&dir.join("m.json"), measurements);
    (dir, bytes, hash)
}

fn draft(dir: &Path, mode: &str, destination: &str, exit: i32) -> Value {
    let mut args = vec![
        "draft",
        "fez",
        "--measurements",
        "m.json",
        "--body-fit",
        "fit.json",
        "--model",
        "model.ftxbody",
        mode,
        destination,
    ];
    if mode == "--ws" {
        args.extend(["--as-of", TODAY]);
    }
    serde_json::from_slice(&invoke(dir, &args, exit).stdout).unwrap()
}

fn document(round: &ftx_wasm::page::Round, name: &str) -> Value {
    serde_json::from_slice(file(round, name)).unwrap()
}

fn file<'a>(round: &'a ftx_wasm::page::Round, name: &str) -> &'a [u8] {
    &round
        .files
        .iter()
        .find(|(path, _)| path == name)
        .unwrap_or_else(|| panic!("missing {name}"))
        .1
}

#[test]
fn accepted_drafting_uses_actual_shape_in_native_and_portable_geometry() {
    let evidence = measurements(Some(420.0));
    let fused = ftx_measure::fuse(&evidence, true).unwrap();
    let (dir, bytes, hash) = fixture(&evidence);
    let original_measurements = std::fs::read(dir.join("m.json")).unwrap();
    let model = ftx_wasm::fold::load_body(&bytes).unwrap();
    let request = json!({"program":"fez","body":{"mode":"file","measurements":evidence}});
    let ordinary = ftx_wasm::page::run(&request.to_string(), TODAY, CREATED).unwrap();
    assert_eq!(
        document(&ordinary, "pattern.json")["meta"]["solved"]["base_girth_mm"],
        430.0
    );
    let mut earlier_pieces = None;
    for (index, change) in [0.25, 0.75].into_iter().enumerate() {
        let accepted = source(&hash, change);
        let raw = source_text(&accepted);
        std::fs::write(dir.join("fit.json"), &raw).unwrap();
        let output = format!("native-{index}");
        draft(&dir, "--out", &output, 0);
        let native = dir.join(&output);
        let pattern = read(&native.join("pattern.json"));
        let body = read(&native.join("draft_body.json"));
        let validation = read(&native.join("validation.json"));
        // The fixture's head is a 16-gon of radius 100 mm * (1 + 0.1 * shape).
        // The two source shapes share the exact same model and conflicting tape record.
        let head = 3200.0 * (1.0 + 0.1 * change) * (std::f64::consts::PI / 16.0).sin();
        assert!((body["model_readings_mm"]["head"].as_f64().unwrap() - head).abs() < 1e-8);
        assert_eq!(body["provenance"], "MODEL_FIT");
        assert_eq!(body["body_fit"]["source_fit"], accepted);
        assert_eq!(
            body["identity"]["source_report_sha256"],
            sha256_hex(raw.as_bytes())
        );
        assert_eq!(pattern["meta"]["draft_body"], body["identity"]);
        assert_eq!(
            pattern["meta"]["solved"]["base_girth_mm"],
            ftx_core::py::round(head + 10.0, 1)
        );
        let tip = &pattern["pieces"][1]["vertices"]["p0"];
        let radius = tip[0].as_f64().unwrap().hypot(tip[1].as_f64().unwrap());
        // PieceBuilder serializes coordinates to 0.001 mm; p0 is the axial tip vertex.
        let expected_radius = ftx_core::py::round(0.78 * (head + 10.0) / std::f64::consts::TAU, 3);
        assert!((radius - expected_radius).abs() < 1e-8);
        assert_eq!(validation["status"], "CONVERGED");
        if let Some(previous) = &earlier_pieces {
            assert_ne!(&pattern["pieces"], previous);
        }
        earlier_pieces = Some(pattern["pieces"].clone());

        let portable =
            ftx_wasm::page::run_on_body(&model, &hash, &request.to_string(), TODAY, CREATED, &raw)
                .unwrap();
        for name in [
            "pattern.json",
            "fit_spec.json",
            "draft_body.json",
            "validation.json",
        ] {
            assert_eq!(
                document(&portable, name),
                read(&native.join(name)),
                "{name}"
            );
        }
        assert_eq!(document(&portable, "measurements.json"), fused);
        assert_eq!(
            document(&portable, "readiness.json"),
            document(&ordinary, "readiness.json")
        );
        assert_eq!(file(&portable, "body_fit.json"), raw.as_bytes());
        assert_eq!(
            std::fs::read(native.join("body_fit.json")).unwrap(),
            raw.as_bytes()
        );
        assert_eq!(std::fs::read(dir.join("fit.json")).unwrap(), raw.as_bytes());
        assert_eq!(
            std::fs::read(dir.join("m.json")).unwrap(),
            original_measurements
        );
        assert!(!portable.zip.is_empty());
        if index == 1
            && let Some(capture) = std::env::var_os("FTX_BODY_DRAFT_CAPTURE")
        {
            // A fresh synthetic replay bundle for an actual compiled-Wasm/browser comparison.
            // Candidate output is never stored as a conformance oracle or physical evidence.
            let capture = PathBuf::from(capture);
            std::fs::create_dir(&capture).unwrap();
            std::fs::write(capture.join("model.ftxbody"), &bytes).unwrap();
            std::fs::write(capture.join("accepted.json"), raw.as_bytes()).unwrap();
            std::fs::write(capture.join("measurements.json"), &original_measurements).unwrap();
            save(&capture.join("request.json"), &request);
            save(&capture.join("expected.json"), &portable.page.to_value());
            for (name, data) in &portable.files {
                let path = capture.join("artifacts").join(name);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, data).unwrap();
            }
            std::fs::write(capture.join(&portable.zip_name), &portable.zip).unwrap();
        }
    }
}

#[test]
fn tape_fitted_drafting_and_later_checks_share_native_portable_body_evidence() {
    let mut evidence = measurements(Some(420.0));
    // 1300 mm lies outside the automatic chart-prior range: only original head and
    // stature evidence reaches the fitter, with every ordinary fitter control available.
    evidence["wearer"]["age_months"] = json!(30);
    evidence["wearer"]["stature_mm"] = json!(1300.0);
    let fused = ftx_measure::fuse(&evidence, true).unwrap();
    let dir = setup();
    let mut labels = ftx_body::FREE[ftx_body::N_PHENOTYPE..].to_vec();
    labels.push(HEAD_SHAPE);
    let bytes = model_bytes_with_locals(true, &labels);
    let hash = sha256_hex(&bytes);
    std::fs::write(dir.join("model.ftxbody"), &bytes).unwrap();
    save(&dir.join("m.json"), &evidence);
    let original_evidence = std::fs::read(dir.join("m.json")).unwrap();
    invoke(
        &dir,
        &[
            "draft",
            "fez",
            "--measurements",
            "m.json",
            "--model",
            "model.ftxbody",
            "--out",
            "tape-draft",
        ],
        0,
    );
    let native = dir.join("tape-draft");
    let model = ftx_wasm::fold::load_body(&bytes).unwrap();
    let request = json!({"program":"fez","body":{"mode":"file","measurements":evidence}});
    let ordinary = ftx_wasm::page::run(&request.to_string(), TODAY, CREATED).unwrap();
    let portable = ftx_wasm::page::run_on_selected_body(
        &model,
        &hash,
        &request.to_string(),
        TODAY,
        CREATED,
        None,
    )
    .unwrap();
    for name in [
        "pattern.json",
        "fit_spec.json",
        "draft_body.json",
        "validation_body.json",
        "validation.json",
    ] {
        assert_eq!(
            document(&portable, name),
            read(&native.join(name)),
            "{name}"
        );
    }
    let body = read(&native.join("draft_body.json"));
    assert_eq!(body["provenance"], "MODEL_FIT");
    assert_eq!(
        body["identity"]["reference"]["shape_source"]["kind"],
        "tape_fit"
    );
    assert!(body["identity"].get("source_report_sha256").is_none());
    assert!(!native.join("body_fit.json").exists());
    assert!(
        portable
            .files
            .iter()
            .all(|(name, _)| name != "body_fit.json")
    );
    let expected_head = 3200.0 * (std::f64::consts::PI / 16.0).sin();
    assert!((body["model_readings_mm"]["head"].as_f64().unwrap() - expected_head).abs() < 1e-8);
    assert_eq!(
        read(&native.join("pattern.json"))["meta"]["solved"]["base_girth_mm"],
        ftx_core::py::round(expected_head + 10.0, 1)
    );
    assert_ne!(
        body["model_readings_mm"]["head"],
        fused["fused"]["head"]["value_mm"]
    );
    assert_eq!(document(&portable, "measurements.json"), fused);
    assert_eq!(
        document(&portable, "readiness.json"),
        document(&ordinary, "readiness.json")
    );
    assert_eq!(
        std::fs::read(dir.join("m.json")).unwrap(),
        original_evidence
    );

    // Imported/altered patterns use the same explicit shape without being redrafted.
    let pattern = read(&native.join("pattern.json"));
    let fit = read(&native.join("fit_spec.json"));
    let request = json!({"pattern":pattern,"fit_spec":fit,
        "body":{"mode":"file","measurements":evidence}});
    assert!(ftx_wasm::page::run(&request.to_string(), TODAY, CREATED).is_err());
    let portable = ftx_wasm::page::run_on_selected_body(
        &model,
        &hash,
        &request.to_string(),
        TODAY,
        CREATED,
        None,
    )
    .unwrap();
    assert_eq!(document(&portable, "pattern.json"), pattern);
    assert_eq!(
        document(&portable, "validation.json"),
        read(&native.join("validation.json"))
    );
    assert_eq!(document(&portable, "validation_body.json"), body);
    assert!(
        portable
            .files
            .iter()
            .all(|(name, _)| name != "draft_body.json" && name != "body_fit.json")
    );
    let checked: Value = serde_json::from_slice(
        &invoke(
            &dir,
            &[
                "validate",
                "tape-draft/pattern.json",
                "--measurements",
                "m.json",
                "--fit",
                "tape-draft/fit_spec.json",
                "--model",
                "model.ftxbody",
            ],
            0,
        )
        .stdout,
    )
    .unwrap();
    assert_eq!(checked["result"]["validation_body"], body);
    assert_eq!(
        checked["result"]["points"],
        document(&portable, "validation.json")["points"]
    );
}

#[test]
fn accepted_drafting_preserves_workspace_rounds_without_promoting_missing_measurements() {
    let evidence = measurements(None);
    let fused = ftx_measure::fuse(&evidence, true).unwrap();
    assert!(fused["fused"]["head"].is_null());
    let (dir, bytes, hash) = fixture(&evidence);
    let model = ftx_wasm::fold::load_body(&bytes).unwrap();
    let original_measurements = std::fs::read(dir.join("m.json")).unwrap();
    let mut first_pattern = Vec::new();
    let mut first_source = Vec::new();
    for (index, change) in [0.25, 0.75].into_iter().enumerate() {
        let raw = source_text(&source(&hash, change));
        std::fs::write(dir.join("fit.json"), &raw).unwrap();
        let result = draft(&dir, "--ws", "ws", 0);
        assert_eq!(result["result"]["round"], index);
        assert_eq!(result["result"]["readiness"]["status"], "NOT_READY");
        let round = dir.join(format!("ws/rounds/{index:03}"));
        assert_eq!(read(&round.join("validation.json"))["status"], "CONVERGED");
        assert_eq!(
            std::fs::read(round.join("body_fit.json")).unwrap(),
            raw.as_bytes()
        );
        assert_eq!(
            std::fs::read(round.join("measurements.json")).unwrap(),
            original_measurements
        );
        assert_eq!(read(&round.join("fused_measurements.json")), fused);
        if index == 0 {
            first_pattern = std::fs::read(round.join("pattern.json")).unwrap();
            first_source = raw.as_bytes().to_vec();
        } else {
            assert_ne!(
                std::fs::read(round.join("pattern.json")).unwrap(),
                first_pattern
            );
            let request = json!({"program":"fez","body":{"mode":"file","measurements":evidence}});
            let portable = ftx_wasm::page::run_on_body(
                &model,
                &hash,
                &request.to_string(),
                TODAY,
                CREATED,
                &raw,
            )
            .unwrap();
            assert_eq!(
                document(&portable, "readiness.json"),
                read(&round.join("readiness.json"))
            );
            assert_eq!(document(&portable, "measurements.json"), fused);
            assert_eq!(
                portable.page.to_value()["basis"]["measurements"],
                "NOT_READY"
            );
        }
    }
    assert_eq!(
        std::fs::read(dir.join("ws/rounds/000/pattern.json")).unwrap(),
        first_pattern
    );
    assert_eq!(
        std::fs::read(dir.join("ws/rounds/000/body_fit.json")).unwrap(),
        first_source
    );
    assert_eq!(
        std::fs::read(dir.join("ws/measurements.json")).unwrap(),
        original_measurements
    );
    assert_eq!(
        std::fs::read(dir.join("m.json")).unwrap(),
        original_measurements
    );
    assert!(!dir.join("ws/rounds.jsonl").exists());
}

#[test]
fn later_validation_and_altered_rounds_keep_the_explicit_selected_body() {
    let evidence = measurements(Some(420.0));
    let fused = ftx_measure::fuse(&evidence, true).unwrap();
    let (dir, _, hash) = fixture(&evidence);
    let raw = source_text(&source(&hash, 0.25));
    std::fs::write(dir.join("fit.json"), &raw).unwrap();
    draft(&dir, "--ws", "ws", 0);
    let original = read(&dir.join("ws/rounds/000/pattern.json"));
    let initial = read(&dir.join("ws/rounds/000/validation.json"));
    let ready = read(&dir.join("ws/rounds/000/readiness.json"));
    let original_evidence = std::fs::read(dir.join("ws/measurements.json")).unwrap();
    let expected_head =
        ftx_core::py::round(3200.0 * 1.025 * (std::f64::consts::PI / 16.0).sin(), 1);
    assert_eq!(initial["points"][0]["body_mm"], expected_head);
    assert_ne!(initial["points"][0]["body_mm"], 420.0);

    // A saved body identity cannot silently become an analytic tape-based validation.
    invoke(
        &dir,
        &[
            "validate",
            "ws/rounds/000/pattern.json",
            "--measurements",
            "m.json",
            "--fit",
            "ws/fit_spec.json",
        ],
        3,
    );
    invoke(&dir, &["round", "ws", "--as-of", TODAY], 3);
    assert!(!dir.join("ws/rounds/001").exists());

    let checked: Value = serde_json::from_slice(
        &invoke(
            &dir,
            &[
                "validate",
                "ws/rounds/000/pattern.json",
                "--measurements",
                "m.json",
                "--fit",
                "ws/fit_spec.json",
                "--body-fit",
                "fit.json",
                "--model",
                "model.ftxbody",
            ],
            0,
        )
        .stdout,
    )
    .unwrap();
    assert_eq!(checked["result"]["points"], initial["points"]);
    assert_eq!(
        checked["result"]["validation_body"]["identity"],
        original["meta"]["draft_body"]
    );

    let result: Value = serde_json::from_slice(
        &invoke(
            &dir,
            &[
                "round",
                "ws",
                "--body-fit",
                "fit.json",
                "--model",
                "model.ftxbody",
                "--as-of",
                TODAY,
            ],
            if ready["status"] == "NOT_READY" { 2 } else { 0 },
        )
        .stdout,
    )
    .unwrap();
    let result = &result["result"];
    assert_eq!(result["basis"]["flat_body"], "MODEL_FIT");
    assert_eq!(result["validation"], initial);
    assert_eq!(result["readiness"], ready);
    assert_eq!(read(&dir.join("ws/rounds/001/measurements.json")), fused);
    assert_eq!(
        std::fs::read(dir.join("ws/rounds/001/body_fit.json")).unwrap(),
        raw.as_bytes()
    );
    assert_eq!(
        read(&dir.join("ws/rounds/001/validation_body.json"))["identity"],
        original["meta"]["draft_body"]
    );
    let html = std::fs::read_to_string(dir.join("ws/report.html")).unwrap();
    assert!(html.contains("Body used for flat validation — MODEL_FIT"));
    assert!(html.contains("rounds/001/validation_body.json"));

    invoke(
        &dir,
        &[
            "alter",
            "move",
            "ws/rounds/001/pattern.json",
            "--piece",
            "crown",
            "--vertex",
            "b0",
            "--dy",
            "-25",
            "--out",
            "altered.json",
        ],
        0,
    );
    let altered = read(&dir.join("altered.json"));
    assert_eq!(
        altered["meta"]["draft_body"],
        original["meta"]["draft_body"]
    );
    let result: Value = serde_json::from_slice(
        &invoke(
            &dir,
            &[
                "round",
                "ws",
                "--pattern",
                "altered.json",
                "--body-fit",
                "fit.json",
                "--model",
                "model.ftxbody",
                "--as-of",
                TODAY,
            ],
            2,
        )
        .stdout,
    )
    .unwrap();
    assert_eq!(result["result"]["basis"]["flat_body"], "MODEL_FIT");
    assert_eq!(
        result["result"]["validation"]["points"][0]["body_mm"],
        expected_head
    );
    assert_ne!(
        result["result"]["validation"]["points"][0]["finished_mm"],
        initial["points"][0]["finished_mm"]
    );
    assert_eq!(result["result"]["readiness"], ready);
    assert_eq!(
        std::fs::read(dir.join("ws/measurements.json")).unwrap(),
        original_evidence
    );
    assert_eq!(read(&dir.join("ws/rounds/000/pattern.json")), original);
}

#[test]
fn explicit_flat_body_can_accompany_simulation_without_changing_legacy_sim_only_selection() {
    let evidence = measurements(Some(420.0));
    let (dir, _, hash) = fixture(&evidence);
    let raw = source_text(&source(&hash, 0.25));
    std::fs::write(dir.join("fit.json"), &raw).unwrap();
    draft(&dir, "--out", "draft", 0);
    let mut ordinary = read(&dir.join("draft/pattern.json"));
    ordinary["meta"]
        .as_object_mut()
        .unwrap()
        .remove("draft_body");
    save(&dir.join("ordinary.json"), &ordinary);
    for (workspace, explicit, expected) in [("legacy", false, 2), ("selected", true, 0)] {
        let mut args = vec![
            "round",
            workspace,
            "--pattern",
            "ordinary.json",
            "--fit",
            "draft/fit_spec.json",
            "--measurements",
            "m.json",
            "--body-fit",
            "fit.json",
            "--model",
            "model.ftxbody",
            "--sim",
            "--as-of",
            TODAY,
        ];
        if explicit {
            args.push("--flat-body");
        }
        let result: Value = serde_json::from_slice(&invoke(&dir, &args, expected).stdout).unwrap();
        let result = &result["result"];
        assert!(result["sim"].is_null(), "the fez needs no cloth simulation");
        if explicit {
            assert_eq!(result["basis"]["flat_body"], "MODEL_FIT");
            assert_ne!(result["validation"]["points"][0]["body_mm"], 420.0);
        } else {
            assert!(result["basis"].get("flat_body").is_none());
            assert_eq!(result["validation"]["points"][0]["body_mm"], 420.0);
        }
    }
    let mut invalid = source(&hash, 0.25);
    invalid["status"] = json!("REJECTED");
    save(&dir.join("fit.json"), &invalid);
    invoke(
        &dir,
        &[
            "round",
            "selected",
            "--body-fit",
            "fit.json",
            "--model",
            "model.ftxbody",
            "--flat-body",
            "--sim",
            "--as-of",
            TODAY,
        ],
        3,
    );
    assert!(!dir.join("selected/rounds/001").exists());
    assert_eq!(
        std::fs::read(dir.join("selected/rounds/000/body_fit.json")).unwrap(),
        raw.as_bytes()
    );
}

#[test]
fn accepted_drafting_refuses_wrong_rejected_and_conflicting_sources_before_publication() {
    for case in ["wrong_model", "rejected", "demographic", "missing_anatomy"] {
        let mut evidence = measurements(Some(420.0));
        let (dir, mut bytes, mut hash) = fixture(&evidence);
        let mut accepted = source(&hash, 0.25);
        match case {
            "wrong_model" => {
                accepted["anny_configuration"]["model_sha256"] = json!("f".repeat(64));
            }
            "rejected" => accepted["status"] = json!("AMBIGUOUS"),
            "demographic" => evidence["wearer"]["age_months"] = json!(31),
            _ => {
                bytes = super::model_bytes();
                hash = sha256_hex(&bytes);
                accepted = receipt(&hash);
                std::fs::write(dir.join("model.ftxbody"), &bytes).unwrap();
            }
        }
        save(&dir.join("m.json"), &evidence);
        let raw = source_text(&accepted);
        std::fs::write(dir.join("fit.json"), &raw).unwrap();
        for (mode, destination) in [("--out", "blocked/output"), ("--ws", "blocked/workspace")] {
            draft(&dir, mode, destination, 3);
            assert!(!dir.join("blocked").exists(), "{case} {mode}");
        }
        let model = ftx_wasm::fold::load_body(&bytes).unwrap();
        let request = json!({"program":"fez","body":{"mode":"file","measurements":evidence}});
        assert!(
            ftx_wasm::page::run_on_body(&model, &hash, &request.to_string(), TODAY, CREATED, &raw)
                .is_err(),
            "{case} must not fall back to the available tape head girth"
        );
        assert_eq!(std::fs::read(dir.join("fit.json")).unwrap(), raw.as_bytes());
    }
}

#[test]
fn accepted_drafting_never_clobbers_source_reports_or_completed_output() {
    let (dir, _, hash) = fixture(&measurements(Some(420.0)));
    let raw = source_text(&source(&hash, 0.25));
    std::fs::write(dir.join("fit.json"), &raw).unwrap();
    draft(&dir, "--out", "fit.json", 3);
    assert_eq!(std::fs::read(dir.join("fit.json")).unwrap(), raw.as_bytes());
    draft(&dir, "--out", "completed", 0);
    let files = [
        "pattern.json",
        "fit_spec.json",
        "draft_body.json",
        "body_fit.json",
        "validation.json",
    ];
    let original: Vec<_> = files
        .iter()
        .map(|name| std::fs::read(dir.join("completed").join(name)).unwrap())
        .collect();
    draft(&dir, "--out", "completed", 3);
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(dir.join("completed"), dir.join("alias")).unwrap();
        draft(&dir, "--out", "alias", 3);
    }
    for (name, expected) in files.into_iter().zip(original) {
        assert_eq!(
            std::fs::read(dir.join("completed").join(name)).unwrap(),
            expected,
            "{name}"
        );
    }
}

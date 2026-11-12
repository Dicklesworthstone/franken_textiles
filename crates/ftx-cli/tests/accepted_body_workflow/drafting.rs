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

fn passage_study_case(program: &str) -> (Value, Value, String, f64) {
    let fixtures: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/oracle/draft/programs.json"
    ))
    .unwrap();
    let case = fixtures["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == format!("{program}-920"))
        .unwrap();
    let pattern = case["output"]["pattern"].clone();
    let mut fit = case["output"]["fit_spec"].clone();
    let point = fit["points"]
        .as_array()
        .unwrap()
        .iter()
        .find(|point| point["id"] == "cuff_pass")
        .unwrap()
        .clone();
    let name = point["body"].as_str().unwrap().to_owned();
    let value = case["input"]["measurements"]["fused"][&name]["value_mm"]
        .as_f64()
        .unwrap();
    // Keep the actual garment's passage selector, target band, seams and truing checks.
    // The small model cannot establish the remaining full-body points; they are not claimed here.
    fit["points"] = json!([point]);
    let mut request: Value =
        serde_json::from_str(include_str!("../../../../examples/design-study.json")).unwrap();
    request.as_object_mut().unwrap().remove("sewing");
    request["variants"].as_array_mut().unwrap().truncate(1);
    request["variants"][0]["changes"]["default"]["record"]["properties"]["width"]["value"] =
        json!(2.0);
    request["design_fit"] = json!({"fit_spec":fit});
    request["comparison"] = json!({"basis":"nominal","required_consumers":["cutting"],
        "objectives":[{"metric":"stock_mass_kg","direction":"minimize"}]});
    request["designs"] = if name == "hand_pass" {
        json!([{"key":"wider-opening","operations":[
            {"op":"move","piece":"sleeve","vertex":"hem_f","dx_mm":5.0,"dy_mm":0.0},
            {"op":"move","piece":"sleeve","vertex":"hem_b","dx_mm":-5.0,"dy_mm":0.0}]}])
    } else {
        json!([{"key":"wider-opening","operations":[
            {"op":"slash_spread","piece":"cuff","axis":"x","at_mm":100.0,"add_mm":10.0}]}])
    };
    (pattern, request, name, value)
}

fn supplied_passage(value: f64) -> Value {
    json!({"unit":"mm","value_mm":value,"sigma_mm":2.0,
        "primary_provenance":"MEASURED_TAPE","flags":[],
        "sources":[{"provenance":"MEASURED_TAPE","used":true,"value_mm":value,
            "method":"Synthetic independent passage observation","sigma_mm":2.0}]})
}

fn baseline(report: &Value) -> &Value {
    report["alternatives"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["design"]["key"] == "baseline")
        .unwrap()
}

#[test]
fn selected_body_studies_admit_actual_garment_passage_contracts_and_replay_manufacturing() {
    use ftx_drape::{
        drafting::PreparedPatternBody,
        study::fitted::{BodyRequest, FittedBody},
    };
    use ftx_pattern::BodyMeasure;
    use ftx_wasm::materials::scenario::{manufacturing_on_model, on_model};

    let bytes = super::model_bytes_with_rig(true);
    let hash = sha256_hex(&bytes);
    let model = ftx_wasm::fold::load_body(&bytes).unwrap();
    let dir = setup();
    std::fs::write(dir.join("model.ftxbody"), &bytes).unwrap();
    for program in ["zouave_bolero", "zouave_trousers"] {
        let (pattern, request, name, value) = passage_study_case(program);
        let evidence = supplied_passage(value);
        let body_input = json!({"schema":"ftx-scenario-body/1",
            "accepted_body_fit":receipt(&hash),"arm_abduction_deg":50.0,
            "measurements":{"wearer":{"stature_mm":1111.0},"fused":{&name:evidence}}});
        let original = body_input.clone();
        let source = BodyRequest::read(&body_input).unwrap();
        let fitted = FittedBody::prepare(&model, &hash, &source).unwrap();
        let reference = fitted.flat_fit_body(&model, &hash).unwrap();
        let prepared = PreparedPatternBody::from_fitted(
            &model,
            &hash,
            &fitted,
            &source.measurements,
            &[&name],
        )
        .unwrap();
        assert_eq!(prepared.value("stature").unwrap(), 1300.0);
        assert_eq!(prepared.value(&name).unwrap(), value);
        assert_eq!(prepared.sigma(&name), Some(2.0));
        let mut identity = reference.identity().clone();
        identity["supplemental_measurements"] = json!({&name:{"value_mm":value,"source":evidence}});
        assert_eq!(prepared.flat_fit_identity(), identity);
        assert!(
            PreparedPatternBody::from_fitted(
                &model,
                &hash,
                &fitted,
                &source.measurements,
                &["stature"]
            )
            .is_err()
        );
        let without_passage =
            PreparedPatternBody::from_fitted(&model, &hash, &fitted, &source.measurements, &[])
                .unwrap();
        assert_eq!(without_passage.flat_fit_identity(), *reference.identity());

        let portable: Value = serde_json::from_str(
            &manufacturing_on_model(
                &model,
                &hash,
                &pattern.to_string(),
                &request.to_string(),
                &body_input.to_string(),
            )
            .unwrap(),
        )
        .unwrap();
        let row = baseline(&portable);
        assert_eq!(row["design"]["fit"]["result"]["body_identity"], identity);
        assert_eq!(
            row["design"]["fit"]["result"]["validation"]["points"][0]["body_mm"],
            value
        );
        assert_eq!(
            row["design"]["fit"]["result"]["passed"], true,
            "{program}: {row}"
        );
        assert_eq!(row["binding"]["status"], "available", "{program}: {row}");
        assert_eq!(row["cutting"]["status"], "available", "{program}: {row}");
        save(&dir.join("pattern.json"), &pattern);
        save(&dir.join("study.json"), &request);
        save(&dir.join("body-input.json"), &body_input);
        for (binary, prefix) in [
            (
                env!("CARGO_BIN_EXE_ftx"),
                &["--json", "materials", "study-on-body"][..],
            ),
            (env!("CARGO_BIN_EXE_ftx-materials"), &["study-on-body"][..]),
        ] {
            let output = std::process::Command::new(binary)
                .current_dir(&dir)
                .args(prefix)
                .args([
                    "pattern.json",
                    "study.json",
                    "body-input.json",
                    "model.ftxbody",
                ])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{} / {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            let native: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(native.get("result").unwrap_or(&native), &portable);
        }

        // Manufacturing admission is independent of the deliberately tiny numerical mesh allowance.
        let scenario = json!({"schema":"ftx-drape-scenario/1",
            "options":{"spacing":100.0,"sew_steps":3,"drape_steps":3,"step":0.001},
            "fit_spec":{},"max_mesh_points":3,"max_vertex_frames":1000});
        let matrix = json!({"schema":"ftx-drape-scenario-matrix/1","max_evaluations":4,
            "cases":[{"key":"first","scenario":scenario},{"key":"second","scenario":scenario}]});
        for scenario in [&scenario, &matrix] {
            let actual: Value = serde_json::from_str(
                &on_model(
                    &model,
                    &hash,
                    &pattern.to_string(),
                    &request.to_string(),
                    &scenario.to_string(),
                    &body_input.to_string(),
                    |_| {},
                )
                .unwrap(),
            )
            .unwrap();
            assert_eq!(
                baseline(&actual["manufacturing"])["design"]["fit"],
                row["design"]["fit"]
            );
            assert_eq!(
                baseline(&actual["manufacturing"])["cutting"],
                row["cutting"]
            );
            if let Some(cases) = actual["cases"].as_array() {
                for case in cases {
                    assert_eq!(case["scenario"]["design_fit_body"], identity);
                }
            } else {
                assert_eq!(actual["scenario"]["design_fit_body"], identity);
            }
            save(&dir.join("scenario.json"), scenario);
            let output = invoke(
                &dir,
                &[
                    "materials",
                    "scenario-study",
                    "pattern.json",
                    "study.json",
                    "scenario.json",
                    "body-input.json",
                    "model.ftxbody",
                ],
                0,
            );
            let native: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(native["result"], actual);
        }
        assert_eq!(body_input, original);
        assert_eq!(read(&dir.join("body-input.json")), original);
    }
}

#[test]
fn selected_body_study_passage_refusals_never_substitute_priors_or_model_values() {
    use ftx_wasm::materials::scenario::{manufacturing_on_model, on_model};

    let bytes = super::model_bytes_with_rig(true);
    let hash = sha256_hex(&bytes);
    let model = ftx_wasm::fold::load_body(&bytes).unwrap();
    let dir = setup();
    std::fs::write(dir.join("model.ftxbody"), &bytes).unwrap();
    for program in ["zouave_bolero", "zouave_trousers"] {
        let (pattern, request, name, value) = passage_study_case(program);
        let body = json!({"schema":"ftx-scenario-body/1","accepted_body_fit":receipt(&hash),
            "measurements":{"fused":{&name:supplied_passage(value)}}});
        save(&dir.join("pattern.json"), &pattern);
        save(&dir.join("study.json"), &request);
        for reason in ["missing", "prior", "model", "mixed"] {
            let mut invalid = body.clone();
            match reason {
                "missing" => {
                    invalid["measurements"]["fused"]
                        .as_object_mut()
                        .unwrap()
                        .remove(&name);
                }
                "prior" => invalid["measurements"]["fused"][&name]["flags"] = json!(["PRIOR_ONLY"]),
                "model" => {
                    invalid["measurements"]["fused"][&name]["primary_provenance"] =
                        json!("MODEL_FIT")
                }
                _ => {
                    invalid["measurements"]["fused"][&name]["sources"][0]["provenance"] =
                        json!("MODEL_FIT")
                }
            }
            let error = manufacturing_on_model(
                &model,
                &hash,
                &pattern.to_string(),
                &request.to_string(),
                &invalid.to_string(),
            )
            .unwrap_err();
            assert!(error.message.contains(&name), "{reason}: {error}");
            save(&dir.join("body-input.json"), &invalid);
            let output = invoke(
                &dir,
                &[
                    "materials",
                    "study-on-body",
                    "pattern.json",
                    "study.json",
                    "body-input.json",
                    "model.ftxbody",
                ],
                3,
            );
            let native: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(native["refusal"], serde_json::to_value(error).unwrap());

            let scenario = json!({"schema":"ftx-drape-scenario/1",
                "options":{"spacing":100.0,"sew_steps":3,"drape_steps":3,"step":0.001},
                "fit_spec":{},"max_mesh_points":3,"max_vertex_frames":1000});
            let refused: Value = serde_json::from_str(
                &on_model(
                    &model,
                    &hash,
                    &pattern.to_string(),
                    &request.to_string(),
                    &scenario.to_string(),
                    &invalid.to_string(),
                    |_| {},
                )
                .unwrap(),
            )
            .unwrap();
            assert!(
                refused["scenario"]["design_fit_setup_refusal"]["message"]
                    .as_str()
                    .unwrap()
                    .contains(&name)
            );
            assert!(refused["scenario"]["design_fit_body"].is_null());
            for row in refused["manufacturing"]["alternatives"].as_array().unwrap() {
                assert_eq!(row["design"]["fit"]["status"], "refused");
                assert_eq!(row["binding"]["status"], "refused");
                assert_eq!(row["cutting"]["status"], "refused");
            }
        }
        let mut larger = body;
        larger["measurements"]["fused"][&name] = supplied_passage(value + 100.0);
        let report: Value = serde_json::from_str(
            &manufacturing_on_model(
                &model,
                &hash,
                &pattern.to_string(),
                &request.to_string(),
                &larger.to_string(),
            )
            .unwrap(),
        )
        .unwrap();
        let row = baseline(&report);
        assert_eq!(row["design"]["fit"]["result"]["passed"], false);
        assert_eq!(
            row["design"]["fit"]["result"]["validation"]["points"][0]["body_mm"],
            value + 100.0
        );
        assert_eq!(row["binding"]["status"], "refused");
        assert_eq!(row["cutting"]["status"], "refused");
    }
}

#[test]
fn study_records_replace_stale_passage_caches_and_keep_original_replay_inputs() {
    use ftx_wasm::materials::scenario::{manufacturing_on_model, on_model};

    let (pattern, request, name, value) = passage_study_case("zouave_bolero");
    let bytes = super::model_bytes_with_rig(true);
    let hash = sha256_hex(&bytes);
    let model = ftx_wasm::fold::load_body(&bytes).unwrap();
    let record = ftx_measure::make_record(
        &name,
        value,
        2.0,
        "MEASURED_TAPE",
        "Synthetic current passage record",
        None,
        "",
    )
    .unwrap();
    let mut raw = json!({"schema":"fabric-measurements/1","wearer":{},"records":[record],
        "fused":{&name:{"value_mm":1.0,"sigma_mm":2.0,"flags":["PRIOR_ONLY"]}}});
    let fresh = ftx_measure::fuse(&raw, true).unwrap();
    let source = |measurements: &Value| {
        json!({"schema":"ftx-scenario-body/1",
        "accepted_body_fit":receipt(&hash),"measurements":measurements})
    };
    let evaluate = |body: &Value| -> Result<Value, ftx_core::Refusal> {
        serde_json::from_str(&manufacturing_on_model(
            &model,
            &hash,
            &pattern.to_string(),
            &request.to_string(),
            &body.to_string(),
        )?)
        .map_err(|error| {
            ftx_core::Refusal::new(ftx_core::RefusalCode::InvalidDocument, error.to_string())
        })
    };
    let expected = evaluate(&source(&fresh)).unwrap();
    assert_eq!(
        baseline(&expected)["design"]["fit"]["result"]["passed"],
        true
    );
    let dir = setup();
    std::fs::write(dir.join("model.ftxbody"), &bytes).unwrap();
    save(&dir.join("pattern.json"), &pattern);
    save(&dir.join("study.json"), &request);
    for cache in [true, false] {
        if !cache {
            raw.as_object_mut().unwrap().remove("fused");
        }
        let input = source(&raw);
        assert_eq!(evaluate(&input).unwrap(), expected);
        save(&dir.join("body-input.json"), &input);
        let original = std::fs::read(dir.join("body-input.json")).unwrap();
        let output = invoke(
            &dir,
            &[
                "materials",
                "study-on-body",
                "pattern.json",
                "study.json",
                "body-input.json",
                "model.ftxbody",
            ],
            0,
        );
        let native: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(native["result"], expected);
        assert_eq!(
            std::fs::read(dir.join("body-input.json")).unwrap(),
            original
        );

        let scenario = json!({"schema":"ftx-drape-scenario/1",
            "options":{"spacing":100.0,"sew_steps":3,"drape_steps":3,"step":0.001},
            "fit_spec":{},"max_mesh_points":3,"max_vertex_frames":1000});
        let matrix = json!({"schema":"ftx-drape-scenario-matrix/1","max_evaluations":4,
            "cases":[{"key":"first","scenario":scenario},{"key":"second","scenario":scenario}]});
        for scenario in [&scenario, &matrix] {
            let portable: Value = serde_json::from_str(
                &on_model(
                    &model,
                    &hash,
                    &pattern.to_string(),
                    &request.to_string(),
                    &scenario.to_string(),
                    &input.to_string(),
                    |_| {},
                )
                .unwrap(),
            )
            .unwrap();
            assert_eq!(
                portable["manufacturing"]["alternatives"],
                expected["alternatives"]
            );
            let descriptors: Vec<_> = if let Some(cases) = portable["cases"].as_array() {
                cases.iter().map(|case| &case["scenario"]).collect()
            } else {
                vec![&portable["scenario"]]
            };
            for descriptor in descriptors {
                assert_eq!(descriptor["body_input"], input);
                assert_eq!(descriptor["measurements"], fresh);
                assert_eq!(
                    descriptor["body_fit"]["shape_source"]["kind"],
                    "accepted_multi_frame_fit"
                );
                assert_eq!(
                    descriptor["design_fit_body"]["supplemental_measurements"][&name]["source"],
                    fresh["fused"][&name]
                );
            }
            save(&dir.join("scenario.json"), scenario);
            let output = invoke(
                &dir,
                &[
                    "materials",
                    "scenario-study",
                    "pattern.json",
                    "study.json",
                    "scenario.json",
                    "body-input.json",
                    "model.ftxbody",
                ],
                0,
            );
            let native: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(native["result"], portable);
        }
    }

    // An edited record wins even when the old cache would admit a now-too-small opening.
    raw["fused"] = fresh["fused"].clone();
    raw["records"][0]["value_mm"] = json!(value + 100.0);
    let changed = evaluate(&source(&raw)).unwrap();
    let fit = &baseline(&changed)["design"]["fit"]["result"];
    assert_eq!(fit["validation"]["points"][0]["body_mm"], value + 100.0);
    assert_eq!(fit["passed"], false);
    assert_eq!(baseline(&changed)["cutting"]["status"], "refused");
    raw["records"][0]["value_mm"] = json!(-1.0);
    let error = evaluate(&source(&raw)).unwrap_err();
    assert!(error.message.contains("invalid measurement file"));
    save(&dir.join("body-input.json"), &source(&raw));
    let output = invoke(
        &dir,
        &[
            "materials",
            "study-on-body",
            "pattern.json",
            "study.json",
            "body-input.json",
            "model.ftxbody",
        ],
        3,
    );
    let refused: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(refused["refusal"], serde_json::to_value(error).unwrap());

    // An explicitly fused-only document still owns its supplied value and original evidence.
    raw.as_object_mut().unwrap().remove("records");
    raw["fused"][&name] = supplied_passage(value - 5.0);
    let explicit = evaluate(&source(&raw)).unwrap();
    assert_eq!(
        baseline(&explicit)["design"]["fit"]["result"]["validation"]["points"][0]["body_mm"],
        value - 5.0
    );
}

#[test]
fn study_tape_fitting_tracks_current_stature_records_in_native_and_portable_routes() {
    use ftx_wasm::materials::scenario::{manufacturing_on_model, on_model};

    let (pattern, request, name, value) = passage_study_case("zouave_trousers");
    let bytes = model_bytes_with_locals(true, &ftx_body::FREE[ftx_body::N_PHENOTYPE..]);
    let hash = sha256_hex(&bytes);
    let model = ftx_wasm::fold::load_body(&bytes).unwrap();
    let stature = ftx_measure::make_record(
        "stature",
        1300.0,
        2.0,
        "MEASURED_TAPE",
        "Synthetic stature",
        None,
        "",
    )
    .unwrap();
    let passage = ftx_measure::make_record(
        &name,
        value,
        2.0,
        "MEASURED_TAPE",
        "Synthetic independent passage",
        None,
        "",
    )
    .unwrap();
    let mut raw = json!({"schema":"fabric-measurements/1","wearer":{"age_months":30.0,"sex":"F"},
        "records":[stature,passage],"fused":{"stature":{"value_mm":1150.0,"sigma_mm":2.0}}});
    let scenario = json!({"schema":"ftx-drape-scenario/1",
        "options":{"spacing":100.0,"sew_steps":3,"drape_steps":3,"step":0.001},
        "fit_spec":{},"max_mesh_points":3,"max_vertex_frames":1000});
    let dir = setup();
    std::fs::write(dir.join("model.ftxbody"), &bytes).unwrap();
    save(&dir.join("pattern.json"), &pattern);
    save(&dir.join("study.json"), &request);
    save(&dir.join("scenario.json"), &scenario);
    let mut previous_shape = None;
    for (height, nested) in [(1300.0, false), (1400.0, true)] {
        raw["records"][0]["value_mm"] = json!(height);
        let fresh = ftx_measure::fuse(&raw, true).unwrap();
        let input = if nested {
            json!({"schema":"ftx-scenario-body/1","measurements":raw,"arm_abduction_deg":50.0})
        } else {
            raw.clone()
        };
        let portable: Value = serde_json::from_str(
            &on_model(
                &model,
                &hash,
                &pattern.to_string(),
                &request.to_string(),
                &scenario.to_string(),
                &input.to_string(),
                |_| {},
            )
            .unwrap(),
        )
        .unwrap();
        let descriptor = &portable["scenario"];
        assert_eq!(descriptor["body_input"], input);
        assert_eq!(descriptor["measurements"], fresh);
        assert_eq!(descriptor["body_fit"]["shape_source"]["kind"], "tape_fit");
        assert!(
            (descriptor["body_fit"]["readings_mm"]["stature"]
                .as_f64()
                .unwrap()
                - height)
                .abs()
                < 2.0
        );
        assert_eq!(
            baseline(&portable["manufacturing"])["design"]["fit"]["result"]["passed"],
            true
        );
        assert_eq!(
            baseline(&portable["manufacturing"])["cutting"]["status"],
            "available"
        );
        if let Some(previous) = previous_shape {
            assert_ne!(descriptor["body_fit"]["shape_sha256"], previous);
        }
        previous_shape = Some(descriptor["body_fit"]["shape_sha256"].clone());
        let direct: Value = serde_json::from_str(
            &manufacturing_on_model(
                &model,
                &hash,
                &pattern.to_string(),
                &request.to_string(),
                &input.to_string(),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            direct["alternatives"],
            portable["manufacturing"]["alternatives"]
        );
        save(&dir.join("body-input.json"), &input);
        let original = std::fs::read(dir.join("body-input.json")).unwrap();
        let output = invoke(
            &dir,
            &[
                "materials",
                "scenario-study",
                "pattern.json",
                "study.json",
                "scenario.json",
                "body-input.json",
                "model.ftxbody",
            ],
            0,
        );
        let native: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(native["result"], portable);
        assert_eq!(
            std::fs::read(dir.join("body-input.json")).unwrap(),
            original
        );
    }
}

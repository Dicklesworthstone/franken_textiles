//! Browser workflow on synthetic sheets, using the same core as the native CLI.
use ftx_pattern::doc::PatternDoc;
use ftx_trace::{
    AuditOptions, TraceOptions,
    raster::image::{Gray, encode_png_gray, load_png},
};
use ftx_wasm::tracing::{Review, Sheet};
use serde_json::{Value, json};

fn png() -> Vec<u8> {
    let mut image = Gray::filled(180, 220, 255);
    for y in 25..195 {
        for x in 25..155 {
            if !(30..150).contains(&x) || !(30..190).contains(&y) {
                image.data[y * image.w + x] = 0;
            }
        }
    }
    encode_png_gray(&image)
}
fn file<'a>(review: &'a Review, name: &str) -> &'a [u8] {
    &review.files.iter().find(|(n, _)| n == name).unwrap().1
}
fn doc(review: &Review) -> PatternDoc {
    serde_json::from_slice(file(review, "pattern.json")).unwrap()
}

// Optional synthetic products for inspecting the browser UI; never oracle fixtures or wearer data.
fn sample(name: &str, review: &Review) {
    if let Some(root) = std::env::var_os("FTX_TRACE_SAMPLE_DIR") {
        let dir = std::path::PathBuf::from(root).join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("input.png"), png()).unwrap();
        std::fs::write(dir.join("report.json"), review.report.to_string()).unwrap();
        std::fs::write(dir.join("trace-review.zip"), review.zip()).unwrap();
        for (name, bytes) in &review.files {
            let path = dir.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
    }
}

#[test]
fn initial_review_matches_native_geometry_mask_and_images() {
    let bytes = png();
    let sheet = Sheet::new(&bytes, "{}").unwrap();
    let result = sheet.review("{}", "[]", "[]").unwrap();
    sample("initial", &result);
    let image = load_png(&bytes).unwrap();
    let native =
        ftx_trace::trace_image(&image, Some("browser-sheet.png"), &TraceOptions::default())
            .unwrap();
    assert_eq!(
        file(&result, "draft.pattern.json"),
        (native.pattern.dumps(Some(1)) + "\n").as_bytes()
    );
    assert_eq!(file(&result, "mask.png"), encode_png_gray(&native.mask));
    let doc = doc(&result);
    assert_eq!(result.report["scale_status"], "UNSCALED");
    assert_eq!(result.report["status"], "REVIEW_ONLY");
    assert_eq!(
        file(&result, "overlay.png"),
        ftx_trace::review::overlay(&doc, Some(&image)).unwrap()
    );
    assert_eq!(
        result.report["audit"],
        ftx_trace::audit(&doc, Some(&[]), &AuditOptions::default())
    );
    assert_eq!(
        file(&result, "labels/p1.png"),
        ftx_trace::review::label_crops(&native, &image).unwrap()[0].png
    );
    assert!(
        result
            .files
            .iter()
            .all(|(n, _)| !n.ends_with(".pdf") && !n.ends_with(".dxf"))
    );
    assert_eq!(result.zip(), sheet.review("{}", "[]", "[]").unwrap().zip());
}

#[test]
fn edits_are_repeatable_and_failed_attempts_cannot_change_the_original() {
    let sheet = Sheet::new(&png(), "{}").unwrap();
    let names = r#"{"pieces":{"p1":{"id":"front","drawn_side":"L"}}}"#;
    let targets = r#"[{"reference":"front/e1","target_mm":200}]"#;
    let first = sheet.review(names, targets, "[]").unwrap();
    sample("edited", &first);
    assert_eq!(first.report["scale_status"], "FITTED");
    assert_eq!(first.report["pieces"][0]["id"], "front");
    assert_eq!(
        first.zip(),
        sheet.review(names, targets, "[]").unwrap().zip()
    );
    let pattern = ftx_pattern::Pattern::from_doc(doc(&first));
    assert!((ftx_trace::measure_ref(&pattern, "front/e1").unwrap() - 200.0).abs() < 0.001);
    assert!(
        sheet
            .review(
                names,
                r#"[{"reference":"missing/edge","target_mm":100}]"#,
                "[]"
            )
            .is_err()
    );
    let original = sheet.review("{}", "[]", "[]").unwrap();
    assert_eq!(original.report["scale_status"], "UNSCALED");
    assert_eq!(original.report["pieces"][0]["id"], "p1");
    assert_eq!(
        file(&first, "draft.pattern.json"),
        file(&original, "draft.pattern.json")
    );
}

#[test]
fn scale_disagreement_and_audit_errors_are_not_hidden() {
    let sheet = Sheet::new(&png(), "{}").unwrap();
    let result = sheet
        .review(
            "{}",
            r#"[{"reference":"p1/e1","target_mm":100},{"reference":"p1/e2","target_mm":1000}]"#,
            "[]",
        )
        .unwrap();
    assert!(result.report["scale_fit"]["warning"].is_string());
    assert_eq!(
        result.report["scale_fit"]["targets"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(result.report["scale_fit"]["rms_pct"].as_f64().unwrap() > 2.0);
    assert_eq!(result.report["status"], "REVIEW_ONLY");
    assert!(result.report["audit"]["findings"].is_array());
}

#[test]
fn input_budget_and_domains_refuse_without_substitution() {
    let image = png();
    for options in [
        r#"{"threshod":"otsu"}"#,
        r#"{"threshold":"unknown"}"#,
        r#"{"crop":[170,0,20,20]}"#,
        r#"{"crop":[0,0,0,10]}"#,
        r#"{"crop":[-1,0,20,20]}"#,
        r#"{"stroke":0}"#,
        r#"{"min_area_frac":-1}"#,
    ] {
        assert!(Sheet::new(&image, options).is_err(), "{options}");
    }
    assert!(Sheet::new(b"\xff\xd8\xff", "{}").is_err());
    let large = encode_png_gray(&Gray::filled(2001, 2000, 255));
    assert!(Sheet::new(&large, "{}").is_err());
    let sheet = Sheet::new(&image, "{}").unwrap();
    for targets in [
        r#"[{"reference":"p1/e1","target_mm":0}]"#,
        r#"[{"reference":"p1/e1","target_mm":200,"typo":3}]"#,
        r#"[{"reference":"p1/e1","target_mm":100},{"reference":"p1/e1","target_mm":100}]"#,
    ] {
        assert!(sheet.review("{}", targets, "[]").is_err());
    }
    assert!(sheet.review("[]", "[]", "[]").is_err());
    assert!(sheet.review(r#"{"drop":["p1"]}"#, "[]", "[]").is_err());
    assert!(
        sheet
            .review(&" ".repeat(256 * 1024 + 1), "[]", "[]")
            .is_err()
    );
    assert!(sheet.review("{}", "[]", "{}").is_err());
}

#[test]
fn crop_coordinates_are_preserved_and_no_image_paths_are_followed() {
    let bytes = png();
    let sheet = Sheet::new(&bytes, r#"{"crop":[10,10,160,200],"channel":"dark"}"#).unwrap();
    let result = sheet.review("{}", "[]", "[]").unwrap();
    let draft: Value = serde_json::from_slice(file(&result, "draft.pattern.json")).unwrap();
    assert_eq!(draft["meta"]["source"]["crop"], json!([10, 10, 160, 200]));
    assert_eq!(draft["meta"]["source"]["image"], "browser-sheet.png");
    assert_eq!(
        result.report["source_sha256"],
        ftx_core::receipt::sha256_hex(&bytes)
    );
    assert_eq!(result.report["source_image_size"], json!([180, 220]));
}

/// A JPEG and a WebP sheet from `tests/fixtures/oracle/trace/trace_{jpeg,webp}.json` trace in the browser
/// exactly as natively, into the pieces the reference found.
#[test]
fn jpeg_and_webp_sheets_trace_as_natively() {
    sheet_traces_as_natively("trace_jpeg", "jpeg");
    sheet_traces_as_natively("trace_webp", "webp");
}

fn sheet_traces_as_natively(fixture: &str, key: &str) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../tests/fixtures/oracle/trace/{fixture}.json"));
    let fx: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let case = &fx["cases"][0];
    let b64: Vec<u8> = case["input"][key]
        .as_str()
        .unwrap()
        .bytes()
        .filter(|&c| c != b'=')
        .collect();
    let val = |c: u8| match c {
        b'A'..=b'Z' => u32::from(c - b'A'),
        b'a'..=b'z' => u32::from(c - b'a') + 26,
        b'0'..=b'9' => u32::from(c - b'0') + 52,
        b'+' => 62,
        _ => 63,
    };
    let mut jpeg = Vec::new();
    for chunk in b64.chunks(4) {
        let n = chunk.iter().fold(0u32, |acc, &c| (acc << 6) | val(c)) << (6 * (4 - chunk.len()));
        jpeg.extend(&n.to_be_bytes()[1..=chunk.len() * 6 / 8]);
    }
    let result = Sheet::new(&jpeg, "{}")
        .unwrap()
        .review("{}", "[]", "[]")
        .unwrap();
    let image = ftx_trace::raster::load_image(&jpeg).unwrap();
    let native =
        ftx_trace::trace_image(&image, Some("browser-sheet.png"), &TraceOptions::default())
            .unwrap();
    assert_eq!(
        file(&result, "draft.pattern.json"),
        (native.pattern.dumps(Some(1)) + "\n").as_bytes()
    );
    let ids = |pieces: &Value| -> Vec<String> {
        pieces
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["id"].as_str().unwrap().to_string())
            .collect()
    };
    let draft: Value = serde_json::from_slice(file(&result, "draft.pattern.json")).unwrap();
    assert_eq!(
        ids(&draft["pieces"]),
        ids(&case["output"]["pattern"]["pieces"])
    );
}

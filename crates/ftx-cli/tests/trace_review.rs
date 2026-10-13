//! Native trace review products on synthetic sheets; no external image stack or wearer data.

use ftx_trace::raster::{Gray, image::encode_png_gray, load_png};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

fn scratch() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let p = std::env::temp_dir().join(format!(
        "ftx-trace-review-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&p).unwrap();
    let mut image = Gray::filled(180, 120, 255);
    for (x0, x1, y0, y1) in [(15, 75, 20, 100), (100, 165, 30, 105)] {
        for y in y0..y1 {
            for x in x0..x1 {
                image.data[y * 180 + x] = 20;
            }
        }
    }
    std::fs::write(p.join("sheet.png"), encode_png_gray(&image)).unwrap();
    p
}
fn run(dir: &Path, args: &[&str], exit: i32) -> Value {
    let p = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(dir)
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(
        p.status.code(),
        Some(exit),
        "{args:?}\n{}\n{}",
        String::from_utf8_lossy(&p.stdout),
        String::from_utf8_lossy(&p.stderr)
    );
    serde_json::from_slice(&p.stdout).unwrap()
}
fn trace(dir: &Path) -> Value {
    run(
        dir,
        &["trace", "sheet.png", "--channel", "dark", "--out", "trace"],
        0,
    )
}
fn read(p: impl AsRef<Path>) -> Value {
    serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
}
fn put(p: impl AsRef<Path>, v: &Value) {
    std::fs::write(p, serde_json::to_vec_pretty(v).unwrap()).unwrap();
}

#[test]
fn complete_trace_products_and_unscaled_export_gate() {
    let dir = scratch();
    let result = trace(&dir);
    assert_eq!(result["result"]["pieces"].as_array().unwrap().len(), 2);
    for key in ["draft", "mask", "polygons", "overlay", "labels"] {
        assert!(!result["result"]["files"][key].is_null());
    }
    let d = read(dir.join("trace/draft.pattern.json"));
    assert_eq!(d["meta"]["scale_status"], "UNSCALED");
    let img = load_png(&std::fs::read(dir.join("trace/overlay.png")).unwrap()).unwrap();
    assert_eq!(img.w, 1600);
    for name in ["p1", "p2"] {
        let crop = load_png(&std::fs::read(dir.join(format!("trace/labels/{name}.png"))).unwrap())
            .unwrap();
        assert!(crop.w.max(crop.h) >= 600);
    }
    run(
        &dir,
        &[
            "export",
            "svg",
            "trace/draft.pattern.json",
            "--out",
            "cutting",
        ],
        3,
    );
    let before = std::fs::read(dir.join("trace/overlay.png")).unwrap();
    run(
        &dir,
        &[
            "trace",
            "sheet.png",
            "--channel",
            "dark",
            "--out",
            "trace-two",
        ],
        0,
    );
    assert_eq!(
        before,
        std::fs::read(dir.join("trace-two/overlay.png")).unwrap()
    );
}

#[test]
fn annotate_overlay_preserves_the_exact_pattern_serialization() {
    let dir = scratch();
    trace(&dir);
    put(
        dir.join("names.json"),
        &json!({"pieces":{"p1":{"id":"front","drawn_side":"L"},"p2":{"id":"back"}}}),
    );
    let input = std::fs::read(dir.join("trace/draft.pattern.json")).unwrap();
    run(
        &dir,
        &[
            "annotate",
            "trace/draft.pattern.json",
            "--names",
            "names.json",
            "--out",
            "without.json",
        ],
        0,
    );
    run(
        &dir,
        &[
            "annotate",
            "trace/draft.pattern.json",
            "--names",
            "names.json",
            "--out",
            "with.json",
            "--overlay",
            "named.png",
            "--image",
            "sheet.png",
        ],
        0,
    );
    assert_eq!(
        std::fs::read(dir.join("without.json")).unwrap(),
        std::fs::read(dir.join("with.json")).unwrap()
    );
    assert_eq!(
        input,
        std::fs::read(dir.join("trace/draft.pattern.json")).unwrap()
    );
    assert!(load_png(&std::fs::read(dir.join("named.png")).unwrap()).is_ok());
}

#[test]
fn audit_image_keeps_error_verdict_and_machine_readable_findings() {
    let dir = scratch();
    trace(&dir);
    let mut doc = read(dir.join("trace/draft.pattern.json"));
    doc["pieces"][0]["trace"]["touches_border"] = json!(true);
    put(dir.join("border.json"), &doc);
    let r = run(
        &dir,
        &[
            "audit",
            "border.json",
            "--out",
            "audit",
            "--image",
            "sheet.png",
        ],
        2,
    );
    assert_eq!(r["result"]["summary"]["ok"], false);
    let expected = ftx_trace::audit(
        &serde_json::from_value(doc).unwrap(),
        None,
        &ftx_trace::AuditOptions::default(),
    );
    assert_eq!(read(dir.join("audit/round0_audit.json")), expected);
    let png = load_png(&std::fs::read(dir.join("audit/round0_audit.png")).unwrap()).unwrap();
    assert!(png.data.as_chunks::<3>().0.contains(&[35, 35, 215]));
    assert_eq!(r["result"]["files"].as_array().unwrap().len(), 2);
}

#[test]
fn preexisting_artifacts_and_wrong_source_do_not_publish_new_documents() {
    let dir = scratch();
    std::fs::create_dir(dir.join("existing")).unwrap();
    std::fs::write(dir.join("existing/overlay.png"), b"previous review").unwrap();
    run(
        &dir,
        &[
            "trace",
            "sheet.png",
            "--channel",
            "dark",
            "--out",
            "existing",
        ],
        3,
    );
    assert_eq!(
        std::fs::read(dir.join("existing/overlay.png")).unwrap(),
        b"previous review"
    );
    assert!(!dir.join("existing/draft.pattern.json").exists());
    assert!(!dir.join("existing/mask.png").exists());
    trace(&dir);
    std::fs::write(
        dir.join("wrong.png"),
        encode_png_gray(&Gray::filled(40, 40, 255)),
    )
    .unwrap();
    run(
        &dir,
        &[
            "audit",
            "trace/draft.pattern.json",
            "--out",
            "wrong-review",
            "--image",
            "wrong.png",
        ],
        3,
    );
    assert!(!dir.join("wrong-review").exists());
}

#[cfg(unix)]
#[test]
fn output_links_cannot_replace_the_source_or_reuse_stale_labels() {
    use std::os::unix::fs::symlink;
    let dir = scratch();
    trace(&dir);
    let before = std::fs::read(dir.join("sheet.png")).unwrap();
    std::fs::create_dir(dir.join("linked")).unwrap();
    symlink(dir.join("sheet.png"), dir.join("linked/round0_audit.png")).unwrap();
    run(
        &dir,
        &["audit", "trace/draft.pattern.json", "--out", "linked"],
        3,
    );
    assert_eq!(before, std::fs::read(dir.join("sheet.png")).unwrap());
    assert!(!dir.join("linked/round0_audit.json").exists());
    symlink(dir.join("trace/labels"), dir.join("linked/labels")).unwrap();
    run(
        &dir,
        &["trace", "sheet.png", "--channel", "dark", "--out", "linked"],
        3,
    );
    assert!(!dir.join("linked/mask.png").exists());
}

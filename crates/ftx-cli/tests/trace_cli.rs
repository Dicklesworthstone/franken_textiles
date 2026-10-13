//! `ftx trace` on the real binary: sheets from `tests/fixtures/oracle/trace/raster_trace.json` written to
//! disk and traced with the reference's flags; `draft.pattern.json` byte-identical to what the reference
//! wrote (its temp path aside), the summary as the reference prints it, refusals for what is not ported.

use std::path::{Path, PathBuf};
use std::process::Command;

use ftx_export::pyjson::PyJson;
use serde_json::Value;

fn base64(s: &str) -> Vec<u8> {
    let val = |c: u8| -> u32 {
        match c {
            b'A'..=b'Z' => u32::from(c - b'A'),
            b'a'..=b'z' => u32::from(c - b'a') + 26,
            b'0'..=b'9' => u32::from(c - b'0') + 52,
            b'+' => 62,
            _ => 63,
        }
    };
    let bytes: Vec<u8> = s.bytes().filter(|&c| c != b'=').collect();
    let mut out = Vec::new();
    for chunk in bytes.chunks(4) {
        let n = chunk.iter().fold(0u32, |acc, &c| (acc << 6) | val(c)) << (6 * (4 - chunk.len()));
        let take = chunk.len() * 6 / 8;
        out.extend(&n.to_be_bytes()[1..=take]);
    }
    out
}

fn scratch() -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("ftx-trace-cli-{}-{stamp}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    dir
}

fn run(dir: &Path, args: &[&str], exit: i32) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(dir)
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(exit),
        "{args:?}\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

fn case(doc: &PyJson, id: &str) -> PyJson {
    let PyJson::List(cases) = doc.get("cases").unwrap() else {
        panic!("cases")
    };
    cases
        .iter()
        .find(|c| c.get("id") == Some(&PyJson::str(id)))
        .unwrap()
        .clone()
}

fn png(c: &PyJson) -> Vec<u8> {
    let Some(PyJson::Str(b)) = c.get("input").and_then(|i| i.get("png")) else {
        panic!("png")
    };
    base64(b)
}

#[test]
fn trace_reproduces_the_reference() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/oracle/trace/raster_trace.json");
    let doc = PyJson::parse(&std::fs::read_to_string(path).unwrap()).unwrap();
    let dir = scratch();
    let flags: [(&str, &[&str]); 4] = [
        ("outlines", &[]),
        (
            "crop-erase",
            &["--crop", "10,10,340,240", "--erase", "140,15,40,30"],
        ),
        ("panel-retry", &[]),
        ("dashed-fold-off", &["--no-dashes"]),
    ];
    for (id, extra) in flags {
        let c = case(&doc, id);
        std::fs::write(dir.join("sheet.png"), png(&c)).unwrap();
        let out = format!("out-{id}");
        let mut args = vec!["trace", "sheet.png", "--out", out.as_str()];
        args.extend_from_slice(extra);
        let res = run(&dir, &args, 0);
        let mut got = PyJson::parse(
            &std::fs::read_to_string(dir.join(&out).join("draft.pattern.json")).unwrap(),
        )
        .unwrap();
        // the reference records the image's resolved path; the capture recorded the file name
        let Some(PyJson::Dict(meta)) = got.get("meta").cloned() else {
            panic!("meta")
        };
        let mut meta = PyJson::Dict(meta);
        let mut source = meta.get("source").unwrap().clone();
        assert!(
            matches!(source.get("image"), Some(PyJson::Str(s)) if s.ends_with("/sheet.png")),
            "{id}"
        );
        source.set("image", PyJson::str("sheet.png"));
        meta.set("source", source);
        got.set("meta", meta);
        let want = c.get("output").unwrap().get("pattern").unwrap();
        assert_eq!(got.dumps(Some(1)), want.dumps(Some(1)), "{id}");
        let PyJson::List(pieces) = want.get("pieces").unwrap() else {
            panic!("pieces")
        };
        let summary = &res["result"]["pieces"];
        assert_eq!(summary.as_array().unwrap().len(), pieces.len(), "{id}");
        for (s, p) in summary.as_array().unwrap().iter().zip(pieces) {
            assert_eq!(s["id"], p.get("id").unwrap().to_value());
            assert!(s["area_px"].as_f64().unwrap() > 1000.0, "{id}: {s}");
        }
        assert!(dir.join(&out).join("mask.png").is_file());
    }
    let retries = &run(&dir, &["trace", "sheet.png", "--out", "again"], 0)["result"]["retries"];
    assert_eq!(
        retries.as_array().unwrap().len(),
        0,
        "dashed-fold-off has no panel"
    );

    // refusals
    let r = run(
        &dir,
        &["trace", "sheet.png", "--out", "x", "--channel", "nope"],
        3,
    );
    assert!(
        r["refusal"]["message"]
            .as_str()
            .unwrap()
            .starts_with("channel must be one of ('auto', 'bgdist'")
    );
    let r = run(
        &dir,
        &[
            "trace",
            "sheet.png",
            "--out",
            "x",
            "--threshold",
            "adaptive",
        ],
        3,
    );
    assert!(
        r["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("adaptive")
    );
    let r = run(
        &dir,
        &[
            "trace",
            "sheet.png",
            "--out",
            "x",
            "--invert",
            "--no-invert",
        ],
        3,
    );
    assert!(
        r["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("not allowed with argument --invert")
    );
    std::fs::write(dir.join("photo.jpg"), [0xff, 0xd8, 0xff, 0xe0, 0, 16]).unwrap();
    let r = run(&dir, &["trace", "photo.jpg", "--out", "x"], 3);
    assert!(
        r["refusal"]["message"].as_str().unwrap().contains("PNG"),
        "{r}"
    );
}

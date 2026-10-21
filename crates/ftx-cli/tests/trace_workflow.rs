//! `ftx annotate`, `ftx scale-fit` and `ftx audit` on the real binary, chained as the skill runs them on a
//! traced sheet: every file equal to what the reference wrote for the same inputs
//! (`tests/fixtures/oracle/trace/post.json`), refusals with the reference's messages.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use ftx_pattern::doc::PatternDoc;
use ftx_trace::OValue;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
struct Fixture {
    drafts: BTreeMap<String, PatternDoc>,
    annotated: BTreeMap<String, PatternDoc>,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    input: OValue,
    output: Value,
}

fn fixture() -> Fixture {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/oracle/trace/post.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn case<'a>(fx: &'a Fixture, id: &str) -> &'a Case {
    fx.cases.iter().find(|c| c.id == id).unwrap()
}

fn scratch() -> PathBuf {
    // macOS clocks tick in microseconds: parallel tests need the counter too
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "ftx-trace-{}-{stamp}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
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

fn doc_file(path: &Path) -> Value {
    let doc: PatternDoc = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    serde_json::to_value(doc).unwrap()
}

#[test]
fn annotate_scale_fit_and_audit_reproduce_the_reference() {
    let fx = fixture();
    let dir = scratch();
    std::fs::write(
        dir.join("draft.json"),
        serde_json::to_string(&fx.drafts["plain"]).unwrap(),
    )
    .unwrap();
    let names = case(&fx, "ann-plain").input.get("names").unwrap();
    std::fs::write(
        dir.join("names.json"),
        serde_json::to_string(names).unwrap(),
    )
    .unwrap();

    let ann = run(
        &dir,
        &[
            "annotate",
            "draft.json",
            "--names",
            "names.json",
            "--out",
            "pattern.json",
        ],
        0,
    );
    assert_eq!(
        ann["result"]["pieces"],
        serde_json::json!(["front", "back", "sleeve"])
    );
    assert_eq!(
        doc_file(&dir.join("pattern.json")),
        serde_json::to_value(&fx.annotated["ann-plain"]).unwrap()
    );

    let fit = case(&fx, "fit-annotated");
    let mut argv = vec![
        "scale-fit".to_string(),
        "pattern.json".into(),
        "--out".into(),
        "scaled.json".into(),
    ];
    for t in fit.input.get("targets").unwrap().items() {
        argv.push("--target".into());
        argv.push(format!(
            "{}={}",
            t.items()[0].as_str().unwrap(),
            t.items()[1].as_f64().unwrap()
        ));
    }
    let res = run(
        &dir,
        &argv.iter().map(String::as_str).collect::<Vec<_>>(),
        0,
    );
    let want = &fit.output["ok"]["report"];
    assert_eq!(res["result"]["k"], want["k"]);
    assert_eq!(res["result"]["targets"], want["targets"]);
    assert_eq!(
        doc_file(&dir.join("scaled.json")),
        serde_json::to_value(&fx.annotated["fit-annotated"]).unwrap()
    );

    // the side seam does not walk (the back is 1.1 x the front side): an error finding, exit 2
    let rep = run(&dir, &["audit", "scaled.json", "--out", "audit"], 2);
    let file: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("audit/round0_audit.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(file, case(&fx, "audit-fitted").output["ok"]);
    assert_eq!(rep["result"]["scale_basis"]["kind"], "FITTED");

    let labels = case(&fx, "audit-labels").input.get("labels").unwrap();
    std::fs::write(
        dir.join("labels.json"),
        serde_json::to_string(labels).unwrap(),
    )
    .unwrap();
    run(
        &dir,
        &[
            "audit",
            "draft.json",
            "--labels",
            "labels.json",
            "--out",
            "labelled",
        ],
        2,
    );
    let file: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("labelled/round0_audit.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(file, case(&fx, "audit-labels").output["ok"]);

    // refusals carry the reference's text
    let bad = case(&fx, "ann-bad-piece");
    std::fs::write(
        dir.join("bad.json"),
        serde_json::to_string(bad.input.get("names").unwrap()).unwrap(),
    )
    .unwrap();
    let r = run(
        &dir,
        &[
            "annotate",
            "draft.json",
            "--names",
            "bad.json",
            "--out",
            "x.json",
        ],
        3,
    );
    assert_eq!(r["refusal"]["message"], bad.output["message"]);
    assert!(!dir.join("x.json").exists());
    let r = run(
        &dir,
        &[
            "scale-fit",
            "pattern.json",
            "--target",
            "back/e1",
            "--out",
            "y.json",
        ],
        3,
    );
    assert_eq!(r["refusal"]["message"], "target 'back/e1' must be REF=MM");
    let r = run(
        &dir,
        &[
            "scale-fit",
            "pattern.json",
            "--target",
            "back/e1=about",
            "--out",
            "y.json",
        ],
        3,
    );
    assert_eq!(
        r["refusal"]["message"],
        "target 'back/e1=about': cannot parse length 'about'"
    );
    std::fs::remove_dir_all(&dir).ok();
}

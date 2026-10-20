//! Actual binaries over the same finite hide/remnant planner, with create-only output semantics.
use ftx_export::cutting::{preview::stock_svg, stock::Request};
use ftx_pattern::Pattern;
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};
const PATTERN: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-stock-pattern.json");
const STOCK: &str = include_str!("../../../tests/fixtures/materials/synthetic-stock-request.json");
fn workspace() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "ftx-finite-stock-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("pattern.json"), PATTERN).unwrap();
    fs::write(dir.join("stock.json"), STOCK).unwrap();
    dir
}
fn invoke(main: bool, dir: &PathBuf, action: &str, extra: &[&str]) -> Output {
    let mut command = Command::new(if main {
        env!("CARGO_BIN_EXE_ftx")
    } else {
        env!("CARGO_BIN_EXE_ftx-materials")
    });
    command.current_dir(dir);
    if main {
        command.args(["materials", "--json"]);
    }
    command
        .args([action, "pattern.json", "stock.json"])
        .args(extra)
        .output()
        .unwrap()
}
fn success(o: &Output) -> Value {
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    if v["schema"] == "ftx-cli/1" {
        assert_eq!(v["ok"], true);
        v["result"].clone()
    } else {
        v
    }
}
#[test]
fn both_binaries_share_finite_stock_and_preview_with_frozen_replay() {
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let request: Request = serde_json::from_str(STOCK).unwrap();
    let native = request.build(&pattern).unwrap();
    for main in [false, true] {
        let dir = workspace();
        let a = invoke(main, &dir, "stock-plan", &[]);
        let result = success(&a);
        assert_eq!(result, serde_json::to_value(&native).unwrap());
        assert_eq!(a.stdout, invoke(main, &dir, "stock-plan", &[]).stdout);
        assert_eq!(
            success(&invoke(main, &dir, "stock-residual", &[])),
            serde_json::to_value(native.residual_request().unwrap()).unwrap()
        );
        assert_eq!(
            fs::read_to_string(dir.join("pattern.json")).unwrap(),
            PATTERN
        );
        assert_eq!(fs::read_to_string(dir.join("stock.json")).unwrap(), STOCK);
        let report = success(&invoke(main, &dir, "stock-svg", &["0", "marker.svg"]));
        let svg = fs::read_to_string(dir.join("marker.svg")).unwrap();
        assert_eq!(svg, stock_svg(&native, 0).unwrap());
        assert_eq!(
            report["svg_sha256"],
            ftx_core::receipt::sha256_hex(svg.as_bytes())
        );
        assert_eq!(report["hardware_execution_authorized"], false);
        assert!(
            !invoke(main, &dir, "stock-svg", &["0", "marker.svg"])
                .status
                .success()
        );
        assert_eq!(fs::read_to_string(dir.join("marker.svg")).unwrap(), svg);
        fs::write(
            dir.join("stock.json"),
            serde_json::to_string(native.request()).unwrap(),
        )
        .unwrap();
        assert_eq!(success(&invoke(main, &dir, "stock-plan", &[])), result);
    }
}
#[test]
fn unsupported_inputs_and_incomplete_inventory_never_publish_a_preview() {
    for main in [false, true] {
        let dir = workspace();
        for index in ["-1", "bad", "3"] {
            assert!(
                !invoke(main, &dir, "stock-svg", &[index, "missing.svg"])
                    .status
                    .success()
            );
            assert!(!dir.join("missing.svg").exists());
        }
        assert!(
            !invoke(main, &dir, "stock-plan", &["--allow-estimates"])
                .status
                .success()
        );
        assert!(
            !invoke(main, &dir, "stock-plan", &["extra"])
                .status
                .success()
        );
        for bad in [
            json!({"schema":"unsupported"}),
            {
                let mut r: Value = serde_json::from_str(STOCK).unwrap();
                r["options"]["copies"] = json!(4000);
                r
            },
            {
                let mut r: Value = serde_json::from_str(STOCK).unwrap();
                r["stocks"][0]["material"]["record"]["name"] = json!("Different lot");
                r
            },
            {
                let mut r: Value = serde_json::from_str(STOCK).unwrap();
                r["options"]["work_limit"] = json!(1);
                r
            },
        ] {
            fs::write(dir.join("stock.json"), bad.to_string()).unwrap();
            assert!(
                !invoke(main, &dir, "stock-svg", &["0", "missing.svg"])
                    .status
                    .success()
            );
            assert!(!dir.join("missing.svg").exists());
        }
        assert_eq!(
            fs::read_to_string(dir.join("pattern.json")).unwrap(),
            PATTERN
        );
        // Bound the sum, not only each input. Sparse files avoid a 64 MiB test source string.
        let file = fs::File::create(dir.join("stock.json")).unwrap();
        file.set_len(64 * 1024 * 1024 + 1).unwrap();
        let failed = invoke(main, &dir, "stock-plan", &[]);
        assert!(!failed.status.success());
        let diagnostics = format!(
            "{}{}",
            String::from_utf8_lossy(&failed.stdout),
            String::from_utf8_lossy(&failed.stderr)
        );
        assert!(diagnostics.contains("64 MiB"));
    }
}

#[test]
fn both_binaries_enforce_regions_and_export_reusable_source_preserving_inventory() {
    const ZONES: &str =
        include_str!("../../../tests/fixtures/materials/synthetic-stock-zones.json");
    let p = Pattern::from_json(PATTERN).unwrap();
    let request: Request = serde_json::from_str(ZONES).unwrap();
    let direct = request.build(&p).unwrap();
    for main in [false, true] {
        let dir = workspace();
        fs::write(dir.join("stock.json"), ZONES).unwrap();
        let plan = success(&invoke(main, &dir, "stock-plan", &[]));
        assert_eq!(plan, serde_json::to_value(&direct).unwrap());
        let residual = success(&invoke(main, &dir, "stock-residual", &[]));
        assert_eq!(
            residual,
            serde_json::to_value(direct.residual_request().unwrap()).unwrap()
        );
        let _ = success(&invoke(main, &dir, "stock-svg", &["0", "zone-review.svg"]));
        let preview = fs::read_to_string(dir.join("zone-review.svg")).unwrap();
        assert_eq!(preview, stock_svg(&direct, 0).unwrap());
        assert!(preview.contains("data-required-zone=\"prime\""));
        assert_eq!(fs::read_to_string(dir.join("stock.json")).unwrap(), ZONES);
        let mut invalid: Value = serde_json::from_str(ZONES).unwrap();
        invalid["options"]["piece_zones"]["front"] = json!("missing");
        fs::write(dir.join("stock.json"), invalid.to_string()).unwrap();
        for action in ["stock-plan", "stock-residual"] {
            assert!(!invoke(main, &dir, action, &[]).status.success());
        }
        assert!(
            !invoke(main, &dir, "stock-svg", &["0", "invalid.svg"])
                .status
                .success()
        );
        assert!(!dir.join("invalid.svg").exists());
        assert_eq!(
            fs::read_to_string(dir.join("zone-review.svg")).unwrap(),
            preview
        );
    }
}

#[test]
fn both_binaries_reconsider_stock_and_never_publish_budget_exhausted_previews() {
    let mut p: Value = serde_json::from_str(PATTERN).unwrap();
    p["pieces"][1]["vertices"] = json!({"a":[0,0],"b":[30,0],"c":[30,10],"d":[0,10]});
    let mut r: Value = serde_json::from_str(STOCK).unwrap();
    r["stocks"][0]["defects_mm"] = json!([]);
    r["stocks"][0]["outline_mm"] = json!([[0, 0], [35, 0], [35, 25], [0, 25]]);
    let mut second = r["stocks"][0].clone();
    second["id"] = json!("second");
    second["outline_mm"] = json!([[0, 0], [25, 0], [25, 25], [0, 25]]);
    r["stocks"].as_array_mut().unwrap().push(second);
    r["options"]["step_mm"] = json!(10);
    for main in [false, true] {
        let dir = workspace();
        fs::write(dir.join("pattern.json"), p.to_string()).unwrap();
        fs::write(dir.join("stock.json"), r.to_string()).unwrap();
        assert!(!invoke(main, &dir, "stock-plan", &[]).status.success());
        let mut search = r.clone();
        search["options"]["backtrack_limit"] = json!(20);
        fs::write(dir.join("stock.json"), search.to_string()).unwrap();
        let expected: Request = serde_json::from_value(search.clone()).unwrap();
        let expected = expected.build(&Pattern::new(p.clone())).unwrap();
        let report = success(&invoke(main, &dir, "stock-plan", &[]));
        assert_eq!(report, serde_json::to_value(&expected).unwrap());
        assert!(report["backtracks_used"].as_u64().unwrap() > 0);
        assert_eq!(
            success(&invoke(main, &dir, "stock-residual", &[])),
            serde_json::to_value(expected.residual_request().unwrap()).unwrap()
        );
        search["options"]["backtrack_limit"] = json!(1);
        fs::write(dir.join("stock.json"), search.to_string()).unwrap();
        assert!(
            !invoke(main, &dir, "stock-svg", &["0", "exhausted.svg"])
                .status
                .success()
        );
        assert!(!dir.join("exhausted.svg").exists());
        assert!(!invoke(main, &dir, "stock-residual", &[]).status.success());
    }
}

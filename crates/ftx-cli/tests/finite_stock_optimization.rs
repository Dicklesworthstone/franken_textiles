//! Both real binaries use the same selected complete layout, even when improvement is interrupted.
use ftx_export::cutting::{preview::stock_svg, stock::Request};
use ftx_pattern::Pattern;
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};
const PATTERN: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-stock-pattern.json");
const STOCK: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-stock-optimization.json");

fn run(main: bool, dir: &Path, action: &str, extra: &[&str]) -> std::process::Output {
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
fn result(output: std::process::Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    if v["schema"] == "ftx-cli/1" {
        assert_eq!(v["ok"], true);
        v["result"].clone()
    } else {
        v
    }
}

#[test]
fn optimized_and_interrupted_jobs_round_trip_through_both_binaries() {
    let root = std::env::temp_dir().join(format!(
        "ftx-stock-objectives-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    for main in [false, true] {
        for (objective, limit, count) in [
            ("minimize_gross_area", 10000, 2),
            ("minimize_stock_count", 10000, 1),
            ("minimize_gross_area", 1, 1),
        ] {
            let dir = root.join(format!("{main}-{objective}-{limit}"));
            fs::create_dir(&dir).unwrap();
            fs::write(dir.join("pattern.json"), PATTERN).unwrap();
            let mut input: Value = serde_json::from_str(STOCK).unwrap();
            input["options"]["optimize"] = json!(objective);
            input["options"]["backtrack_limit"] = json!(limit);
            let text = input.to_string();
            fs::write(dir.join("stock.json"), &text).unwrap();
            let request: Request = serde_json::from_str(&text).unwrap();
            let plan = request
                .build(&Pattern::from_json(PATTERN).unwrap())
                .unwrap();
            let actual = result(run(main, &dir, "stock-plan", &[]));
            assert_eq!(actual, serde_json::to_value(&plan).unwrap());
            assert_eq!(actual["used_stock_count"], count);
            assert_eq!(actual["optimization"]["budget_exhausted"], limit == 1);
            assert_eq!(
                result(run(main, &dir, "stock-residual", &[])),
                serde_json::to_value(plan.residual_request().unwrap()).unwrap()
            );
            let i = plan
                .markers()
                .iter()
                .position(|m| !m.placements.is_empty())
                .unwrap();
            result(run(
                main,
                &dir,
                "stock-svg",
                &[&i.to_string(), "selected.svg"],
            ));
            let svg = fs::read_to_string(dir.join("selected.svg")).unwrap();
            assert_eq!(svg, stock_svg(&plan, i).unwrap());
            assert!(
                !run(main, &dir, "stock-svg", &[&i.to_string(), "selected.svg"])
                    .status
                    .success()
            );
            assert_eq!(fs::read_to_string(dir.join("selected.svg")).unwrap(), svg);
            assert_eq!(fs::read_to_string(dir.join("stock.json")).unwrap(), text);
            assert_eq!(
                fs::read_to_string(dir.join("pattern.json")).unwrap(),
                PATTERN
            );
            fs::write(
                dir.join("stock.json"),
                serde_json::to_string(plan.request()).unwrap(),
            )
            .unwrap();
            assert_eq!(result(run(main, &dir, "stock-plan", &[])), actual);
            input["options"]["work_limit"] = json!(1);
            fs::write(dir.join("stock.json"), input.to_string()).unwrap();
            assert!(
                !run(main, &dir, "stock-svg", &["0", "partial.svg"])
                    .status
                    .success()
            );
            assert!(!dir.join("partial.svg").exists());
        }
    }
}

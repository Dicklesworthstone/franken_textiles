//! Execute both native command surfaces over exact synthetic physical-stock prices.
use ftx_export::cutting::{procurement::finite, stock};
use ftx_pattern::Pattern;
use serde_json::Value;
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};
const PATTERN: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-stock-pattern.json");
const STOCK: &str = include_str!("../../../tests/fixtures/materials/synthetic-stock-request.json");
const PRICE: &str = include_str!("../../../tests/fixtures/materials/synthetic-stock-pricing.json");
fn workspace() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let p = std::env::temp_dir().join(format!(
        "ftx-stock-price-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&p).unwrap();
    for (name, text) in [
        ("pattern.json", PATTERN),
        ("stock.json", STOCK),
        ("price.json", PRICE),
    ] {
        fs::write(p.join(name), text).unwrap();
    }
    p
}
fn run(main: bool, p: &PathBuf, action: &str, extra: &[&str]) -> Output {
    let mut command = Command::new(if main {
        env!("CARGO_BIN_EXE_ftx")
    } else {
        env!("CARGO_BIN_EXE_ftx-materials")
    });
    command.current_dir(p);
    if main {
        command.args(["materials", "--json"]);
    }
    command
        .args([action, "pattern.json", "stock.json"])
        .args(extra)
        .output()
        .unwrap()
}
fn success(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
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
fn both_native_commands_match_the_real_plan_and_quote_engine_without_mutation() {
    let stock: stock::Request = serde_json::from_str(STOCK).unwrap();
    let plan = stock.build(&Pattern::from_json(PATTERN).unwrap()).unwrap();
    let price: finite::Request = serde_json::from_str(PRICE).unwrap();
    let expected = serde_json::to_value(finite::evaluate(&plan, &price).unwrap()).unwrap();
    assert_eq!(expected["least_observed_independent_order_total"], 17.0);
    for main in [false, true] {
        let p = workspace();
        assert_eq!(
            success(run(main, &p, "stock-identities", &[])),
            serde_json::to_value(finite::identities(&plan).unwrap()).unwrap()
        );
        assert_eq!(
            success(run(main, &p, "stock-price", &["price.json"])),
            expected
        );
        assert_eq!(
            run(main, &p, "stock-price", &["price.json"]).stdout,
            run(main, &p, "stock-price", &["price.json"]).stdout
        );
        for (name, text) in [
            ("pattern.json", PATTERN),
            ("stock.json", STOCK),
            ("price.json", PRICE),
        ] {
            assert_eq!(fs::read_to_string(p.join(name)).unwrap(), text);
        }
        let mut unknown: Value = serde_json::from_str(PRICE).unwrap();
        unknown["bindings"] = serde_json::json!([]);
        fs::write(p.join("price.json"), unknown.to_string()).unwrap();
        let result = success(run(main, &p, "stock-price", &["price.json"]));
        assert_eq!(result["complete_offer_coverage"], false);
        assert!(result["least_observed_independent_order_total"].is_null());
    }
}
#[test]
fn malformed_input_arguments_and_combined_budget_refuse_in_both_binaries() {
    for main in [false, true] {
        let p = workspace();
        for args in [vec![], vec!["price.json", "extra"], vec!["absent.json"]] {
            assert!(!run(main, &p, "stock-price", &args).status.success());
        }
        fs::write(p.join("price.json"), "{malformed").unwrap();
        assert!(
            !run(main, &p, "stock-price", &["price.json"])
                .status
                .success()
        );
        let f = fs::File::create(p.join("price.json")).unwrap();
        f.set_len(64 * 1024 * 1024 + 1).unwrap();
        let failed = run(main, &p, "stock-price", &["price.json"]);
        assert!(!failed.status.success());
        assert!(
            format!(
                "{}{}",
                String::from_utf8_lossy(&failed.stdout),
                String::from_utf8_lossy(&failed.stderr)
            )
            .contains("64 MiB")
        );
        assert_eq!(fs::read_to_string(p.join("pattern.json")).unwrap(), PATTERN);
    }
}

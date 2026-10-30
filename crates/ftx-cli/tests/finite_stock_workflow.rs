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

#[test]
fn qualified_finite_and_mixed_knife_programs_replay_through_both_binaries() {
    use ftx_export::cutting::{job, procurement, program};
    use ftx_wasm::manufacturing;

    let mut pattern: Value = serde_json::from_str(PATTERN).unwrap();
    let properties = &mut pattern["fabric"]["material"]["record"]["properties"];
    properties["thickness"] = json!({"value":0.4,"unit":"mm","source":"test","evidence":"estimate",
        "method":"Synthetic loaded gauge, not unloaded clearance","conditions":{"pressure_pa":"500"}});
    properties["unloaded_thickness"] = json!({"value":1.0,"bounds":[0.8,1.2],"unit":"mm",
        "source":"test","evidence":"estimate","method":"Independent synthetic unloaded observation",
        "conditions":{"specimen_id":"example-hide","conditioning":"synthetic dry","thickness_state":"unloaded"}});
    pattern["pieces"][0]["internal"] = json!([
        {"kind":"cut","closed":false,"pts":[[5,5],[15,5]]}
    ]);
    let mut stock: Value = serde_json::from_str(STOCK).unwrap();
    stock["stocks"][0]["material"] = pattern["fabric"]["material"].clone();
    let ordinary_stock = stock.clone();
    let baseline: Value = serde_json::from_str(
        &manufacturing::stock_plan(&pattern.to_string(), &stock.to_string()).unwrap(),
    )
    .unwrap();
    let hide_hash = baseline["markers"][0]["material"]["sha256"]
        .as_str()
        .unwrap();
    let machine: program::Machine = serde_json::from_value(json!({
        "id":"synthetic-finite-table","blade":"synthetic-cold-knife","bed_mm":[200,200],
        "maximum_thickness_mm":2,"maximum_feed_mm_s":100,"travel_mm_s":100,
        "lift_lower_seconds":0.25,"start_mm":[0,0],"return_to_start":true
    }))
    .unwrap();
    let machine_hash = program::machine_sha256(&machine).unwrap();
    let policy = |route: &str, material: &str, feed: f64| {
        json!({
            "model":"qualified_cold_knife_paths/1","machine":machine,"ordering":"nearest_path","work_limit":10000,
            "qualifications":[{"id":format!("synthetic-{route}-knife"),"material_sha256":material,
                "machine_sha256":machine_hash,"thickness_mm":[0.8,1.2],"feed_mm_s":feed,
                "directions":"all_in_plane","evidence":"estimate","allow_estimates":true,
                "source":{"reference":"test:finite-knife","accessed_on":"2026-10-30","note":"Synthetic applicability and timing; no measured cutter claim"},
                "method":"Invented single-ply all-direction cutting, raised travel and complete lift/lower phases",
                "conditions":{"conditioning":"synthetic dry","workholding":"synthetic supported single ply"}}]
        })
    };
    stock["options"]["program"] = policy("finite", hide_hash, 20.0);
    let request: Request = serde_json::from_value(stock.clone()).unwrap();
    let native = request.build(&Pattern::new(pattern.clone())).unwrap();
    let finite: Value = serde_json::from_str(
        &manufacturing::stock_plan(&pattern.to_string(), &stock.to_string()).unwrap(),
    )
    .unwrap();
    assert_eq!(finite, serde_json::to_value(&native).unwrap());
    assert_eq!(finite["markers"], baseline["markers"]);
    assert_eq!(finite["program"]["schema"], "ftx-knife-cut-program/2");
    assert!(finite["program_refusal"].is_null());
    let physical = &finite["program"]["markers"][0]["finite_stock"];
    assert_eq!(physical["stock_id"], "hide-001");
    assert_eq!(
        physical["stock_sha256"],
        procurement::finite::identities(&native).unwrap()[0].stock_sha256
    );
    let runs = finite["program"]["markers"][0]["selected"]["runs"]
        .as_array()
        .unwrap();
    assert_eq!(runs.len(), 3);
    assert_eq!(runs[0]["path"]["internal_cut"], 0);
    assert_eq!(
        finite["program"]["markers"][0]["thickness_mm"],
        json!([0.8, 1.2])
    );
    assert_eq!(finite["program"]["hardware_execution_authorized"], false);

    let mut mixed = pattern.clone();
    let mut roll = pattern["fabric"]["material"].clone();
    roll["id"] = json!("synthetic-roll");
    roll["record"]["id"] = json!("synthetic-roll");
    roll["record"]["name"] = json!("Synthetic woven roll, not measured");
    roll["record"]["form"] = json!("woven");
    roll["record"]["properties"]["width"] = json!({"value":0.08,"unit":"m",
        "source":"test","evidence":"estimate","method":"Synthetic roll width"});
    roll["record"]["properties"]["unloaded_thickness"]["conditions"]["specimen_id"] =
        json!("synthetic-roll");
    mixed["pieces"][1]["fabric"] = json!({"material":roll});
    let mut job_request = json!({"schema":"ftx-cutting-job/1",
        "routes":[{"piece":"front","stock":"finite_stock"},{"piece":"pocket","stock":"roll"}],
        "finite_stock":ordinary_stock,"roll":{"copies":1,"gap_mm":2,"edge_margin_mm":2,"end_margin_mm":2}});
    // Resolve the actual scoped roll selection through the portable production job before sealing it.
    let job_baseline: Value = serde_json::from_str(
        &manufacturing::cut_job(&mixed.to_string(), &job_request.to_string()).unwrap(),
    )
    .unwrap();
    let roll_hash = job_baseline["roll"]["rolls"][0]["material"]["sha256"]
        .as_str()
        .unwrap();
    assert_ne!(roll_hash, hide_hash);
    job_request["roll"]["program"] = policy("roll", roll_hash, 30.0);
    job_request["finite_stock"]["options"]["program"] = stock["options"]["program"].clone();
    let mixed_request: job::Request = serde_json::from_value(job_request.clone()).unwrap();
    let direct = mixed_request.build(&Pattern::new(mixed.clone())).unwrap();
    let complete: Value = serde_json::from_str(
        &manufacturing::cut_job(&mixed.to_string(), &job_request.to_string()).unwrap(),
    )
    .unwrap();
    assert_eq!(complete, serde_json::to_value(&direct).unwrap());
    assert_eq!(complete["complete_inventory"], true);
    assert_eq!(complete["inventory"], job_baseline["inventory"]);
    assert_eq!(complete["inventory"].as_array().unwrap().len(), 2);
    assert_eq!(
        complete["finite_stock"]["source_piece_ids"],
        json!(["front"])
    );
    assert_eq!(complete["roll"]["source_piece_ids"], json!(["pocket"]));
    for (route, markers) in [("finite_stock", "markers"), ("roll", "rolls")] {
        assert_eq!(complete[route][markers], job_baseline[route][markers]);
        assert!(complete[route]["program"].is_object());
        assert!(complete[route]["program_refusal"].is_null());
        assert_eq!(
            complete[route][markers][0]["placements"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            complete[route]["program"]["hardware_execution_authorized"],
            false
        );
    }
    assert_eq!(
        complete["finite_stock"]["program"]["markers"][0]["finite_stock"],
        *physical
    );
    assert_eq!(
        complete["finite_stock"]["program"]["markers"][0]["selected"]["runs"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        complete["roll"]["program"]["markers"][0]["selected"]["runs"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let mut refused_job = job_request.clone();
    refused_job["finite_stock"]["options"]["program"]["qualifications"][0]["material_sha256"] =
        json!("f".repeat(64));
    let refused: Value = serde_json::from_str(
        &manufacturing::cut_job(&mixed.to_string(), &refused_job.to_string()).unwrap(),
    )
    .unwrap();
    assert_eq!(refused["complete_inventory"], true);
    assert_eq!(refused["inventory"], complete["inventory"]);
    assert_eq!(
        refused["finite_stock_identities"],
        complete["finite_stock_identities"]
    );
    assert_eq!(refused["roll"], complete["roll"]);
    assert_eq!(
        refused["finite_stock"]["markers"],
        complete["finite_stock"]["markers"]
    );
    assert!(refused["finite_stock"]["program"].is_null());
    assert_eq!(
        refused["finite_stock"]["program_refusal"]["code"],
        "invalid_document"
    );

    for main in [false, true] {
        let dir = workspace();
        for (p, r, action, expected) in [
            (&pattern, &stock, "stock-plan", &finite),
            (&mixed, &job_request, "cut-job", &complete),
            (&mixed, &refused_job, "cut-job", &refused),
        ] {
            fs::write(dir.join("pattern.json"), p.to_string()).unwrap();
            fs::write(dir.join("stock.json"), r.to_string()).unwrap();
            assert_eq!(success(&invoke(main, &dir, action, &[])), *expected);
        }
    }
    if let Some(dir) = std::env::var_os("FTX_FINITE_KNIFE_REPLAY") {
        let dir = PathBuf::from(dir);
        fs::create_dir_all(&dir).unwrap();
        for (name, value) in [
            ("finite-pattern.json", &pattern),
            ("stock.json", &stock),
            ("plan-report.json", &finite),
            ("mixed-pattern.json", &mixed),
            ("job.json", &job_request),
            ("job-report.json", &complete),
            ("refused-job.json", &refused_job),
            ("refused-job-report.json", &refused),
        ] {
            fs::write(dir.join(name), serde_json::to_vec_pretty(value).unwrap()).unwrap();
        }
    }
}

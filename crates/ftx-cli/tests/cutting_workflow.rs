//! Exercise the real material-workbench binary with synthetic shapes, stock and price observations.
//! Values here are software known answers, never live supplier or physical-fabric measurements.
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

use serde_json::{Value, json};

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        // macOS clocks tick in microseconds: parallel tests need the counter too
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "ftx-cut-workflow-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
    fn write(&self, name: &str, value: &Value) {
        fs::write(self.0.join(name), serde_json::to_vec(value).unwrap()).unwrap();
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ftx-materials"))
            .current_dir(&self.0)
            .args(args)
            .output()
            .unwrap()
    }
    fn ok(&self, args: &[&str]) -> Value {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    }
}
fn fabric(id: &str, width: f64) -> Value {
    json!({"material":{"id":id,"allow_estimates":true,"record":{
        "id":id,"name":"Synthetic stock & <not-a-brand>","form":"woven",
        "properties":{"width":{"value":width,"unit":"m","evidence":"estimate","source":"t","method":"Synthetic test width"}},
        "sources":{"t":{"reference":"test:stock","accessed_on":"2026-10-14","note":"Invented test observation"}}
    }}})
}
fn piece(id: &str, w: f64, h: f64) -> Value {
    json!({"id":id,"cut":{"qty":1},"grain":[[5,5],[5,15]],
        "vertices":{"a":[0,0],"b":[w,0],"c":[w,h],"d":[0,h]},
        "edges":[{"id":"bottom","from":"a","to":"b"},{"id":"right","from":"b","to":"c"},
            {"id":"top","from":"c","to":"d"},{"id":"left","from":"d","to":"a"}]})
}
fn pattern() -> Value {
    let mut cuff = piece("cuff", 40., 60.);
    cuff["fabric"] = fabric("knit", 0.12);
    json!({"schema":"fabric-pattern/1","units":"mm","fabric":fabric("shell",0.1),
        "meta":{"sa_mm":0},"pieces":[piece("front",50.,100.),cuff],"seams":[]})
}
fn options() -> Value {
    json!({"copies":1,"gap_mm":5,"edge_margin_mm":5,"end_margin_mm":10})
}
fn quote(id: &str, price: f64) -> Value {
    json!({"offer_id":format!("supplier/{id}"),"material_id":id,"supplier":"Synthetic supplier",
        "sku":id,"price":price,"quantity":0.5,"unit":"linear_meter","minimum_quantity":0.05,
        "order_increment":0.05,"currency":"USD","region":"US","quality":"test","terms":"ex-tax-shipping",
        "observed_on":"2026-10-14","available":true,"source":"test:offer","source_sha256":"0".repeat(64)})
}
fn setup() -> Workspace {
    let w = Workspace::new();
    w.write("pattern.json", &pattern());
    w.write("options.json", &options());
    w.write(
        "quotes.json",
        &json!({"schema":"ftx-textile-quotes/1","quotes":[quote("shell",10.),quote("knit",16.)]}),
    );
    w.write(
        "market.json",
        &json!({"currency":"USD","region":"US","quality":"test","terms":"ex-tax-shipping",
        "as_of":"2026-10-14","max_age_days":3}),
    );
    w
}
const PLAN: &[&str] = &["cut-plan", "pattern.json", "options.json"];
const PRICE: &[&str] = &[
    "cut-price",
    "pattern.json",
    "options.json",
    "quotes.json",
    "market.json",
];

#[test]
fn qualified_knife_paths_replay_through_both_commands_portable_api_and_material_studies() {
    use ftx_export::cutting::program::{Machine, machine_sha256};

    let w = Workspace::new();
    let mut p = pattern();
    for path in ["/fabric", "/pieces/1/fabric"] {
        p.pointer_mut(path).unwrap()["material"]["record"]["properties"]["thickness"] = json!({"value":0.4,"unit":"mm","source":"t","evidence":"estimate",
                "method":"Synthetic clearance for software replay, not measured knife applicability"});
    }
    p["pieces"][0]["internal"] = json!([
        {"kind":"cut","closed":false,"pts":[[15.,20.],[25.,20.]]}
    ]);
    let mut options = options();
    let baseline: Value = serde_json::from_str(
        &ftx_wasm::manufacturing::cut_plan(&p.to_string(), &options.to_string()).unwrap(),
    )
    .unwrap();
    let machine: Machine = serde_json::from_value(json!({"id":"synthetic-table",
        "blade":"synthetic-cold-knife","bed_mm":[500.,500.],"maximum_thickness_mm":2.,
        "maximum_feed_mm_s":100.,"travel_mm_s":200.,"lift_lower_seconds":0.25,
        "start_mm":[0.,0.],"return_to_start":true}))
    .unwrap();
    let machine_hash = machine_sha256(&machine).unwrap();
    let qualifications: Vec<_> = baseline["rolls"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(i, roll)| {
            json!({"id":format!("synthetic-knife-{i}"),"material_sha256":roll["material"]["sha256"],
            "machine_sha256":machine_hash,"thickness_mm":[0.2,0.8],"feed_mm_s":20.,
            "directions":"all_in_plane","evidence":"estimate","allow_estimates":true,
            "source":{"reference":"test:knife-workflow","accessed_on":"2026-10-30",
                "note":"Artificial process inputs for deterministic consumer equality"},
            "method":"Synthetic constant-rate cut, raised travel and lift/lower phases"})
        })
        .collect();
    options["program"] = json!({"model":"qualified_cold_knife_paths/1","machine":machine,
        "qualifications":qualifications,"ordering":"nearest_path","work_limit":1000});
    w.write("pattern.json", &p);
    w.write("options.json", &options);
    let portable: Value = serde_json::from_str(
        &ftx_wasm::manufacturing::cut_plan(&p.to_string(), &options.to_string()).unwrap(),
    )
    .unwrap();
    let workbench = w.ok(PLAN);
    assert_eq!(workbench, portable);
    let primary = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(&w.0)
        .arg("materials")
        .args(PLAN)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        primary.status.success(),
        "{}",
        String::from_utf8_lossy(&primary.stderr)
    );
    let primary: Value = serde_json::from_slice(&primary.stdout).unwrap();
    assert_eq!(primary["result"], portable);
    assert_eq!(portable["rolls"], baseline["rolls"]);
    let program = &portable["program"];
    assert_eq!(program["schema"], "ftx-knife-cut-program/1");
    assert_eq!(program["markers"].as_array().unwrap().len(), 2);
    assert_eq!(program["hardware_execution_authorized"], false);
    for marker in program["markers"].as_array().unwrap() {
        let run = &marker["selected"];
        assert!(run["modeled_path_seconds"].as_f64().unwrap() > 0.);
        assert!(
            run["travel_length_mm"].as_f64().unwrap()
                <= marker["baseline"]["travel_length_mm"].as_f64().unwrap()
        );
    }
    let mut changed = p["fabric"]["material"].clone();
    changed["record"]["structure"] =
        json!({"finish":"Synthetic different finish requiring a new process observation"});
    let study = json!({"schema":"ftx-material-study/1","cutting":options,
    "variants":[
        {"key":"qualified","changes":{"schema":"ftx-material-bindings/1","default":p["fabric"]["material"]}},
        {"key":"changed-finish","changes":{"schema":"ftx-material-bindings/1","default":changed}}
    ]});
    let report: Value = serde_json::from_str(
        &ftx_wasm::materials::study(&p.to_string(), &study.to_string()).unwrap(),
    )
    .unwrap();
    let rows = report["alternatives"].as_array().unwrap();
    let qualified = rows.iter().find(|row| row["key"] == "qualified").unwrap();
    let changed = rows
        .iter()
        .find(|row| row["key"] == "changed-finish")
        .unwrap();
    assert_eq!(qualified["cutting"]["status"], "available");
    for roll in qualified["cutting"]["result"]["rolls"].as_array().unwrap() {
        let original = baseline["rolls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|original| {
                original["material"]["authored"]["id"] == roll["material"]["authored"]["id"]
            })
            .unwrap();
        assert_eq!(roll["material"], original["material"]);
    }
    assert!(
        qualified["cutting"]["result"]
            .get("program_refusal")
            .is_none(),
        "{}",
        qualified["cutting"]["result"]["program_refusal"]
    );
    assert_eq!(qualified["cutting"]["result"]["program"], *program);
    assert_eq!(changed["cutting"]["status"], "available");
    assert!(changed["cutting"]["result"].get("program").is_none());
    assert!(
        changed["cutting"]["result"]["program_refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("qualification")
    );
    assert_eq!(
        changed["metrics"]["stock_area_m2"],
        qualified["metrics"]["stock_area_m2"]
    );
    if let Some(capture) = std::env::var_os("FTX_KNIFE_PROGRAM_REPLAY") {
        let capture = PathBuf::from(capture);
        fs::create_dir_all(&capture).unwrap();
        for (name, value) in [
            ("pattern.json", &p),
            ("options.json", &options),
            ("plan-report.json", &workbench),
            ("study.json", &study),
            ("study-report.json", &report),
        ] {
            fs::write(capture.join(name), serde_json::to_vec(value).unwrap()).unwrap();
        }
    }
    // The same optional source-qualified fixture policy crosses both real binaries and the
    // portable boundary; obstruction withdraws only the process, never the ordinary markers.
    for blocked in [false, true] {
        let fixture = if blocked {
            json!([[7.9, 11.9], [8.1, 11.9], [8.1, 12.1], [7.9, 12.1]])
        } else {
            json!([[400., 400.], [410., 400.], [410., 410.], [400., 410.]])
        };
        options["program"]["machine"]["fixture_clearance"] = json!({
            "model":"planar_disk_sweep/1","cutting_radius_mm":0.2,"raised_radius_mm":0.2,
            "fixtures":[{"id":"test-fixture","outline_mm":fixture}],
            "source":{"reference":"test:knife-fixture-workflow","accessed_on":"2026-10-30",
                "note":"Synthetic machine-frame fixture and circular envelopes"},
            "method":"Independent known geometry at both tool elevations",
            "conditions":{},"evidence":"estimate","allow_estimates":true
        });
        let machine: Machine =
            serde_json::from_value(options["program"]["machine"].clone()).unwrap();
        let lock = machine_sha256(&machine).unwrap();
        for q in options["program"]["qualifications"].as_array_mut().unwrap() {
            q["machine_sha256"] = json!(lock);
        }
        w.write("options.json", &options);
        let portable: Value = serde_json::from_str(
            &ftx_wasm::manufacturing::cut_plan(&p.to_string(), &options.to_string()).unwrap(),
        )
        .unwrap();
        assert_eq!(w.ok(PLAN), portable);
        let primary = Command::new(env!("CARGO_BIN_EXE_ftx"))
            .current_dir(&w.0)
            .arg("materials")
            .args(PLAN)
            .arg("--json")
            .output()
            .unwrap();
        assert!(
            primary.status.success(),
            "{}",
            String::from_utf8_lossy(&primary.stderr)
        );
        let primary: Value = serde_json::from_slice(&primary.stdout).unwrap();
        assert_eq!(primary["result"], portable);
        assert_eq!(portable["rolls"], baseline["rolls"]);
        if blocked {
            assert!(portable.get("program").is_none());
            assert!(
                portable["program_refusal"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("fixture")
            );
        } else {
            assert_eq!(
                portable["program"]["fixture_clearance"],
                options["program"]["machine"]["fixture_clearance"]
            );
        }
    }
}

#[test]
fn bounded_markers_and_combined_orders_reach_both_native_commands() {
    let w = setup();
    let mut policy = options();
    policy["copies"] = json!(3);
    policy["maximum_marker_length_m"] = json!(0.13);
    w.write("options.json", &policy);
    let plan = w.ok(PLAN);
    assert_eq!(plan["schema"], "ftx-roll-cut-plan/2");
    let rolls = plan["rolls"].as_array().unwrap();
    assert_eq!(rolls.len(), 5);
    assert!(
        rolls
            .iter()
            .all(|r| r["length_m"].as_f64().unwrap() <= 0.13)
    );
    assert_eq!(
        rolls
            .iter()
            .map(|r| r["placements"].as_array().unwrap().len())
            .sum::<usize>(),
        6
    );
    let mut quotes = vec![quote("shell", 10.0), quote("knit", 16.0)];
    for q in &mut quotes {
        q["maximum_continuous_quantity"] = json!(0.13);
        q["available_quantity"] = json!(if q["material_id"] == "shell" {
            0.4
        } else {
            0.2
        });
    }
    w.write(
        "quotes.json",
        &json!({"schema":"ftx-textile-quotes/1","quotes":quotes}),
    );
    let result = w.ok(PRICE);
    assert_eq!(result["schema"], "ftx-cut-procurement/2");
    assert_eq!(result["cut_plan"], plan);
    assert_eq!(result["material_orders"].as_array().unwrap().len(), 2);
    assert_eq!(result["complete_observed_quantity_coverage"], true);
    assert_eq!(result["complete_continuous_length_limit_coverage"], true);
    assert!(
        (result["least_observed_independent_order_total"]
            .as_f64()
            .unwrap()
            - 14.4)
            .abs()
            < 1e-10
    );
    assert!(result["roll_prices"].as_array().unwrap().iter().all(|row| {
        row.get("sample").is_none()
            && row.get("least_observed_order").is_none()
            && row["material_order_index"].is_u64()
    }));
    let primary = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(&w.0)
        .arg("materials")
        .args(PRICE)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        primary.status.success(),
        "{}",
        String::from_utf8_lossy(&primary.stderr)
    );
    let primary: Value = serde_json::from_slice(&primary.stdout).unwrap();
    assert_eq!(primary["result"], result);
    // The selected fifth marker is an actual independently rendered layout.
    let preview = w.ok(&["cut-svg", "pattern.json", "options.json", "4", "fifth.svg"]);
    assert_eq!(preview["roll_index"], 4);
    assert!(
        fs::read_to_string(w.0.join("fifth.svg"))
            .unwrap()
            .contains("<svg")
    );
    // A supplier's total stock cannot be reused for each short marker.
    quotes[0]["available_quantity"] = json!(0.3);
    w.write(
        "quotes.json",
        &json!({"schema":"ftx-textile-quotes/1","quotes":quotes}),
    );
    let unavailable = w.ok(PRICE);
    assert_eq!(unavailable["complete_offer_coverage"], false);
    assert!(unavailable["least_observed_independent_order_total"].is_null());
    assert!(
        unavailable["material_orders"]
            .as_array()
            .unwrap()
            .iter()
            .any(|order| {
                order["sample"]["excluded_offers"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|excluded| excluded["reason"] == "insufficient_quantity")
            })
    );
}

#[test]
fn split_supplier_orders_preserve_complete_assignments_through_both_native_commands() {
    let w = setup();
    let mut p = pattern();
    p["pieces"] = json!([piece("front", 50., 100.)]);
    w.write("pattern.json", &p);
    let mut policy = options();
    policy["copies"] = json!(2);
    policy["maximum_marker_length_m"] = json!(0.13);
    w.write("options.json", &policy);
    let plan = w.ok(PLAN);
    assert_eq!(plan["rolls"].as_array().unwrap().len(), 2);
    let mut first = quote("shell", 10.0);
    first["material_sha256"] = plan["rolls"][0]["material"]["sha256"].clone();
    first["available_quantity"] = json!(0.15);
    first["maximum_continuous_quantity"] = json!(0.13);
    let mut second = first.clone();
    second["offer_id"] = json!("second/shell");
    second["supplier"] = json!("Second synthetic supplier");
    second["price"] = json!(15.0);
    w.write(
        "quotes.json",
        &json!({"schema":"ftx-textile-quotes/1","quotes":[first,second]}),
    );
    let mut market = json!({"currency":"USD","region":"US","quality":"test",
        "terms":"ex-tax-shipping","as_of":"2026-10-14","max_age_days":3,
        "require_observed_quantity":true,"require_observed_continuous_length":true});
    w.write("market.json", &market);
    assert_eq!(w.ok(PRICE)["complete_offer_coverage"], false);
    market["split_orders"] = json!({"schema":"ftx-cut-order-allocation/1","max_search_nodes":1000});
    w.write("market.json", &market);
    let result = w.ok(PRICE);
    assert_eq!(result["schema"], "ftx-cut-procurement/3");
    assert_eq!(result["cut_plan"], plan);
    assert_eq!(result["market"], market);
    assert_eq!(result["complete_offer_coverage"], true);
    assert_eq!(result["complete_observed_quantity_coverage"], true);
    assert_eq!(result["order_allocation_optimality_proven"], true);
    assert!((result["selected_observed_order_total"].as_f64().unwrap() - 7.5).abs() < 1e-10);
    assert_eq!(
        result["least_observed_independent_order_total"],
        result["selected_observed_order_total"]
    );
    let orders = result["material_orders"][0]["allocation"]["selected_orders"]
        .as_array()
        .unwrap();
    assert_eq!(orders.len(), 2);
    let mut assigned: Vec<_> = orders
        .iter()
        .flat_map(|order| order["roll_indices"].as_array().unwrap())
        .map(|index| index.as_u64().unwrap())
        .collect();
    assigned.sort_unstable();
    assert_eq!(assigned, vec![0, 1]);
    let primary = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(&w.0)
        .arg("materials")
        .args(PRICE)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        primary.status.success(),
        "{}",
        String::from_utf8_lossy(&primary.stderr)
    );
    let primary: Value = serde_json::from_slice(&primary.stdout).unwrap();
    assert_eq!(primary["result"], result);
}

#[test]
fn cli_derives_mixed_material_stock_without_requiring_mechanical_data() {
    let w = setup();
    let p = w.ok(PLAN);
    assert_eq!(p["schema"], "ftx-roll-cut-plan/1");
    assert_eq!(p["rolls"].as_array().unwrap().len(), 2);
    let priced = w.ok(PRICE);
    assert_eq!(priced["cut_plan"], p);
    assert_eq!(priced["complete_offer_coverage"], true);
    let total = priced["least_observed_independent_order_total"]
        .as_f64()
        .unwrap();
    assert!((total - 6.2).abs() < 1e-10);
    for row in priced["roll_prices"].as_array().unwrap() {
        let index = row["roll_index"].as_u64().unwrap() as usize;
        let roll = &p["rolls"][index];
        assert_eq!(row["sample"]["material_sha256"], roll["material"]["sha256"]);
        assert_eq!(row["sample"]["scope"]["requirement"], roll["stock_area_m2"]);
        assert_eq!(row["sample"]["scope"]["basis"], "area");
        assert!(roll["stock_mass_kg"].is_null());
        assert!(
            roll["stock_area_m2"].as_f64().unwrap()
                > roll["external_cut_area_m2"].as_f64().unwrap()
        );
    }
}

#[test]
fn continuous_cut_limits_reach_both_native_commands_and_explicit_unknown_policy() {
    let w = setup();
    let plan = w.ok(PLAN);
    let mut quotes = vec![quote("shell", 10.0), quote("knit", 16.0)];
    for q in &mut quotes {
        q["available_quantity"] = json!(1000.0);
        let roll = plan["rolls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|roll| roll["material"]["authored"]["id"] == q["material_id"])
            .unwrap();
        q["maximum_continuous_quantity"] = roll["length_m"].clone();
    }
    w.write(
        "quotes.json",
        &json!({"schema":"ftx-textile-quotes/1","quotes":quotes}),
    );
    let admitted = w.ok(PRICE);
    assert_eq!(admitted["complete_continuous_length_limit_coverage"], true);
    let primary = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(&w.0)
        .arg("materials")
        .args(PRICE)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        primary.status.success(),
        "{}",
        String::from_utf8_lossy(&primary.stderr)
    );
    let primary: Value = serde_json::from_slice(&primary.stdout).unwrap();
    assert_eq!(primary["result"], admitted);

    quotes[0]["maximum_continuous_quantity"] = json!(0.1);
    w.write(
        "quotes.json",
        &json!({"schema":"ftx-textile-quotes/1","quotes":quotes}),
    );
    let blocked = w.ok(PRICE);
    assert_eq!(blocked["complete_offer_coverage"], false);
    assert!(blocked["least_observed_independent_order_total"].is_null());
    assert!(blocked["roll_prices"].as_array().unwrap().iter().any(
        |row| row["sample"]["excluded_offers"][0]["reason"] == "insufficient_continuous_length"
    ));

    quotes[0]
        .as_object_mut()
        .unwrap()
        .remove("maximum_continuous_quantity");
    w.write(
        "quotes.json",
        &json!({"schema":"ftx-textile-quotes/1","quotes":quotes}),
    );
    let unknown = w.ok(PRICE);
    assert_eq!(unknown["complete_offer_coverage"], true);
    assert_eq!(unknown["complete_observed_quantity_coverage"], true);
    assert_eq!(unknown["complete_continuous_length_limit_coverage"], false);
    let mut market: Value =
        serde_json::from_slice(&fs::read(w.0.join("market.json")).unwrap()).unwrap();
    market["require_observed_continuous_length"] = json!(true);
    w.write("market.json", &market);
    assert_eq!(w.ok(PRICE)["complete_offer_coverage"], false);
}

#[test]
fn incomplete_stale_unavailable_and_wrong_market_offers_never_become_free_stock() {
    let w = setup();
    for case in 0..4 {
        let mut knit = quote("knit", 16.);
        match case {
            0 => knit["material_id"] = json!("unrelated"),
            1 => knit["observed_on"] = json!("2026-10-01"),
            2 => knit["available"] = json!(false),
            _ => knit["currency"] = json!("EUR"),
        }
        w.write(
            "quotes.json",
            &json!({"schema":"ftx-textile-quotes/1","quotes":[quote("shell",10.),knit]}),
        );
        let result = w.ok(PRICE);
        assert_eq!(result["complete_offer_coverage"], false);
        assert!(result["least_observed_independent_order_total"].is_null());
        assert!(
            result["roll_prices"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["least_observed_order"].is_null())
        );
    }
}

#[test]
fn minimum_order_and_selling_increment_price_whole_markers_not_net_pattern_area() {
    let w = setup();
    let mut shell = quote("shell", 10.);
    shell["minimum_quantity"] = json!(1.0);
    shell["order_increment"] = json!(0.5);
    w.write(
        "quotes.json",
        &json!({"schema":"ftx-textile-quotes/1","quotes":[shell,quote("knit",16.)]}),
    );
    let result = w.ok(PRICE);
    assert!(
        (result["least_observed_independent_order_total"]
            .as_f64()
            .unwrap()
            - 23.2)
            .abs()
            < 1e-10
    );
    let shell = result["roll_prices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["least_observed_order"]["quote"]["material_id"] == "shell")
        .unwrap();
    assert_eq!(shell["least_observed_order"]["order_quantity"], 1.0);
    assert_eq!(shell["least_observed_order"]["order_cost"], 20.0);
}

#[test]
fn source_width_substitution_changes_layout_and_actual_procurement_cost() {
    let w = setup();
    let mut p = pattern();
    p["pieces"] = json!([piece("a", 40., 100.), piece("b", 40., 100.)]);
    w.write("pattern.json", &p);
    let a = w.ok(PRICE);
    p["fabric"] = fabric("shell", 0.075);
    w.write("pattern.json", &p);
    let b = w.ok(PRICE);
    assert!(
        b["least_observed_independent_order_total"]
            .as_f64()
            .unwrap()
            > a["least_observed_independent_order_total"]
                .as_f64()
                .unwrap()
    );
    assert_ne!(
        a["cut_plan"]["rolls"][0]["material"]["sha256"],
        b["cut_plan"]["rolls"][0]["material"]["sha256"]
    );
    assert_eq!(
        a["cut_plan"]["rolls"][0]["external_cut_area_m2"],
        b["cut_plan"]["rolls"][0]["external_cut_area_m2"]
    );
}

#[test]
fn locked_finite_supplier_stock_gates_real_marker_orders_and_material_edits() {
    let w = setup();
    let plan = w.ok(PLAN);
    let shell_hash = plan["rolls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|roll| roll["material"]["authored"]["id"] == "shell")
        .unwrap()["material"]["sha256"]
        .clone();
    let mut shell = quote("shell", 10.0);
    shell["material_sha256"] = shell_hash;
    // The marker needs 0.12 m but its source sells 0.05 m increments: 0.125 m is insufficient.
    shell["available_quantity"] = json!(0.125);
    let mut knit = quote("knit", 16.0);
    let publish = |shell: &Value, knit: &Value| {
        w.write(
            "quotes.json",
            &json!({"schema":"ftx-textile-quotes/1","quotes":[shell,knit]}),
        );
    };
    publish(&shell, &knit);
    let limited = w.ok(PRICE);
    assert_eq!(limited["complete_offer_coverage"], false);
    assert!(limited["least_observed_independent_order_total"].is_null());
    let excluded = limited["roll_prices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["sample"]["excluded_offers"][0]["quote"]["material_id"] == "shell")
        .unwrap();
    assert_eq!(
        excluded["sample"]["excluded_offers"][0]["reason"],
        "insufficient_quantity"
    );
    assert!(
        (excluded["sample"]["excluded_offers"][0]["required_order_quantity"]
            .as_f64()
            .unwrap()
            - 0.15)
            .abs()
            < 1e-14
    );

    shell["available_quantity"] = json!(0.15);
    publish(&shell, &knit);
    let observed_price = w.ok(PRICE);
    assert_eq!(observed_price["complete_offer_coverage"], true);
    assert_eq!(observed_price["complete_observed_quantity_coverage"], false);
    let mut market: Value =
        serde_json::from_slice(&fs::read(w.0.join("market.json")).unwrap()).unwrap();
    market["require_observed_quantity"] = json!(true);
    w.write("market.json", &market);
    assert_eq!(w.ok(PRICE)["complete_offer_coverage"], false);
    knit["available_quantity"] = json!(0.1);
    publish(&shell, &knit);
    let quantified = w.ok(PRICE);
    assert_eq!(quantified["complete_offer_coverage"], true);
    assert_eq!(quantified["complete_observed_quantity_coverage"], true);
    assert!(
        (quantified["least_observed_independent_order_total"]
            .as_f64()
            .unwrap()
            - 6.2)
            .abs()
            < 1e-10
    );

    let mut changed = pattern();
    changed["fabric"] = fabric("shell", 0.075);
    w.write("pattern.json", &changed);
    let swapped = w.ok(PRICE);
    assert_eq!(swapped["complete_offer_coverage"], false);
    assert!(swapped["least_observed_independent_order_total"].is_null());
    assert!(
        swapped["roll_prices"].as_array().unwrap().iter().any(
            |row| row["sample"]["excluded_offers"][0]["reason"] == "material_snapshot_mismatch"
        )
    );
}

#[test]
fn intraday_marker_orders_use_exact_instants_and_never_revive_older_stock() {
    let w = setup();
    let mut old = quote("shell", 10.0);
    old["observed_at"] = json!("2026-10-14T09:00:00Z");
    let mut later = quote("shell", 20.0);
    later["observed_at"] = json!("2026-10-14T12:00:00.123456789Z");
    later["source_sha256"] = json!("1".repeat(64));
    let mut knit = quote("knit", 16.0);
    knit["observed_at"] = old["observed_at"].clone();
    let publish = |later: &Value| {
        w.write(
            "quotes.json",
            &json!({"schema":"ftx-textile-quotes/2","quotes":[later,knit,old]}),
        );
    };
    let mut market = json!({"currency":"USD","region":"US","quality":"test",
        "terms":"ex-tax-shipping","as_of":"2026-10-14","max_age_days":3});
    publish(&later);
    for (instant, expected_cost, expected_observation) in [
        ("2026-10-14T12:00:00.123456788Z", 6.2, &old),
        ("2026-10-14T12:00:00.123456789Z", 9.2, &later),
    ] {
        market["as_of_at"] = json!(instant);
        w.write("market.json", &market);
        let result = w.ok(PRICE);
        assert_eq!(result["complete_offer_coverage"], true);
        assert!(
            (result["least_observed_independent_order_total"]
                .as_f64()
                .unwrap()
                - expected_cost)
                .abs()
                < 1e-10
        );
        for row in result["roll_prices"].as_array().unwrap() {
            assert_eq!(row["sample"]["scope"]["as_of_at"], instant);
            if row["least_observed_order"]["quote"]["material_id"] == "shell" {
                assert_eq!(row["least_observed_order"]["quote"], *expected_observation);
            }
        }
    }
    market["as_of_at"] = json!("2026-10-14T13:00:00Z");
    w.write("market.json", &market);
    for (field, value, reason) in [
        ("available", json!(false), "unavailable"),
        ("valid_through_at", json!("2026-10-14T12:30:00Z"), "expired"),
        ("available_quantity", json!(0.125), "insufficient_quantity"),
    ] {
        let mut unusable = later.clone();
        unusable[field] = value;
        publish(&unusable);
        let result = w.ok(PRICE);
        assert_eq!(result["complete_offer_coverage"], false);
        assert!(result["least_observed_independent_order_total"].is_null());
        let excluded = result["roll_prices"]
            .as_array()
            .unwrap()
            .iter()
            .find_map(|row| row["sample"]["excluded_offers"].as_array())
            .unwrap();
        assert_eq!(excluded.len(), 1);
        assert_eq!(excluded[0]["reason"], reason);
        assert_eq!(excluded[0]["quote"], unusable);
    }
}

#[test]
fn intraday_history_requires_explicit_scope_and_rejects_conflicting_instants() {
    let w = setup();
    let mut shell = quote("shell", 10.0);
    shell["observed_at"] = json!("2026-10-14T09:00:00Z");
    let mut knit = quote("knit", 16.0);
    knit["observed_at"] = shell["observed_at"].clone();
    let book = json!({"schema":"ftx-textile-quotes/2","quotes":[shell,knit]});
    w.write("quotes.json", &book);
    assert!(!w.run(PRICE).status.success()); // Calendar date alone cannot choose intraday stock.
    let market = json!({"currency":"USD","region":"US","quality":"test",
        "terms":"ex-tax-shipping","as_of":"2026-10-14","max_age_days":3,
        "as_of_at":"2026-10-14T10:00:00Z"});
    w.write("market.json", &market);
    assert_eq!(w.ok(PRICE)["complete_offer_coverage"], true);

    let mut conflicting = book.clone();
    let mut duplicate = conflicting["quotes"][0].clone();
    duplicate["price"] = json!(99.0);
    conflicting["quotes"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    w.write("quotes.json", &conflicting);
    assert!(!w.run(PRICE).status.success());

    let mut legacy = book.clone();
    legacy["schema"] = json!("ftx-textile-quotes/1");
    w.write("quotes.json", &legacy);
    assert!(!w.run(PRICE).status.success());
    w.write("quotes.json", &book);
    let mut mismatched = market;
    mismatched["as_of"] = json!("2026-10-15");
    w.write("market.json", &mismatched);
    assert!(!w.run(PRICE).status.success());
}

#[test]
fn preview_is_source_locked_script_free_and_cannot_overwrite_an_existing_file() {
    let w = setup();
    let plan = w.ok(PLAN);
    let args = ["cut-svg", "pattern.json", "options.json", "0", "marker.svg"];
    let result = w.ok(&args);
    let svg = fs::read_to_string(w.0.join("marker.svg")).unwrap();
    assert_eq!(result["pattern_sha256"], plan["pattern_sha256"]);
    assert_eq!(
        result["material_sha256"],
        plan["rolls"][0]["material"]["sha256"]
    );
    assert_eq!(
        result["svg_sha256"],
        ftx_core::receipt::sha256_hex(svg.as_bytes())
    );
    assert!(svg.contains("class=\"cut-exterior\"") && svg.contains("class=\"grain\""));
    assert!(svg.contains("Synthetic stock &amp; &lt;not-a-brand&gt;"));
    assert!(!svg.contains("<script") && !svg.contains("<image") && !svg.contains("<foreignObject"));
    assert!(!w.run(&args).status.success());
    assert_eq!(svg, fs::read_to_string(w.0.join("marker.svg")).unwrap());
    assert!(
        !w.run(&[
            "cut-svg",
            "pattern.json",
            "options.json",
            "99",
            "absent.svg"
        ])
        .status
        .success()
    );
    assert!(!w.0.join("absent.svg").exists());
    // Intentionally retain test outputs per the repository's no-deletion policy.
}

#[test]
fn quantity_is_derived_and_conflicting_user_requirements_are_not_silently_ignored() {
    let w = setup();
    let mut market: Value =
        serde_json::from_slice(&fs::read(w.0.join("market.json")).unwrap()).unwrap();
    market["requirement"] = json!(0.001);
    w.write("market.json", &market);
    assert!(!w.run(PRICE).status.success());
    let mut p = pattern();
    p["pieces"][0]["fabric"] = json!({"width":1});
    w.write("pattern.json", &p);
    assert!(!w.run(PLAN).status.success());
}

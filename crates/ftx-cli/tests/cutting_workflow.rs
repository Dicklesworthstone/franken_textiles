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

//! Actual bound material assignments drive the existing vapor network and study comparison.
//! All observations are synthetic; potential transfer is not a measured garment or sweat rate.

use ftx_export::material_study::Request;
use ftx_pattern::{Pattern, material_vapor, materials::Catalog};
use serde_json::{Value, json};

const PATTERN: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-sewing-pattern.json");

fn selection(id: &str, resistance: f64) -> Value {
    let source: Value = serde_json::from_str(PATTERN).unwrap();
    let mut selected = source["fabric"]["material"].clone();
    selected["id"] = json!(id);
    selected["record"]["id"] = json!(id);
    selected["record"]["properties"]["width"] = json!({
        "value":0.2,"unit":"m","evidence":"estimate","source":"test",
        "method":"Synthetic cutting width"});
    selected["record"]["properties"]["vapor_resistance"] = json!({
        "value":resistance,"unit":"m2*Pa/W","evidence":"estimate","source":"test",
        "method":"Synthetic constant-resistance acquisition",
        "conditions":{"air_temperature_c":"35","conditioning":"synthetic dry state"}});
    selected
}

fn pattern() -> Pattern {
    let mut raw: Value = serde_json::from_str(PATTERN).unwrap();
    raw["fabric"]["material"] = selection("original-shell", 4.0);
    raw["pieces"][1]["fabric"] = json!({"material":selection("fixed-lining",3.0)});
    Pattern::new(raw)
}

fn request() -> Value {
    json!({"schema":"ftx-material-study/1",
        "variants":[
            {"key":"a-fast","changes":{"schema":"ftx-material-bindings/1",
                "default":selection("fast-shell",2.0)}},
            {"key":"b-slow","changes":{"schema":"ftx-material-bindings/1",
                "default":selection("slow-shell",7.0)}},
            {"key":"c-piece","changes":{"schema":"ftx-material-bindings/1",
                "pieces":{"front":selection("piece-shell",5.0)}}}],
        "vapor":{
            "layers":[{"key":"shell","target":{"scope":"piece","id":"front"}},
                      {"key":"lining","target":{"scope":"piece","id":"back"}}],
            "area_m2":0.5,"inside_vapor_pressure_pa":2000.0,"outside_vapor_pressure_pa":1000.0,
            "inside_film_resistance_m2_pa_w":1.0,"outside_film_resistance_m2_pa_w":4.0,
            "latent_heat_j_kg":2000000.0},
        "comparison":{"basis":"nominal","required_consumers":["vapor"],
            "objectives":[{"metric":"vapor_potential_mass_flow_kg_s","direction":"maximize"}]}
    })
}

fn run(request: &Value, pattern: &Pattern) -> Value {
    let request: Request = serde_json::from_value(request.clone()).unwrap();
    serde_json::to_value(request.evaluate(pattern).unwrap()).unwrap()
}

fn row<'a>(report: &'a Value, key: &str) -> &'a Value {
    report["alternatives"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["key"] == key)
        .unwrap()
}

#[test]
fn final_piece_assignments_drive_known_networks_and_exact_offline_replay() {
    let pattern = pattern();
    let original = pattern.raw().clone();
    let report = run(&request(), &pattern);
    assert_eq!(report["comparison"]["frontier"], json!(["a-fast"]));
    let empty_catalog = Catalog {
        schema: ftx_pattern::materials::CATALOG_SCHEMA.into(),
        materials: Vec::new(),
    };
    for (key, shell_id, resistance) in [
        ("a-fast", "fast-shell", 2.0),
        ("b-slow", "slow-shell", 7.0),
        ("c-piece", "piece-shell", 5.0),
    ] {
        let alternative = row(&report, key);
        let result = &alternative["vapor"]["result"];
        assert_eq!(alternative["vapor"]["status"], "available");
        assert_eq!(result["layers"][0]["selection"]["id"], shell_id);
        assert_eq!(result["layers"][1]["selection"]["id"], "fixed-lining");
        let power = 0.5 * 1000.0 / (1.0 + resistance + 3.0 + 4.0);
        assert!((result["potential_heat_flow_w"].as_f64().unwrap() - power).abs() < 1e-12);
        assert!((result["potential_mass_flow_kg_s"].as_f64().unwrap() - power / 2e6).abs() < 1e-18);
        assert_eq!(result["physical_validation_claim"], false);
        assert_eq!(result["layers"][0]["observation"]["evidence"], "estimate");
        assert_eq!(result["layers"][0]["source"]["reference"], "test:sewing");
        let replay: material_vapor::Request =
            serde_json::from_str(result["replay_request_json"].as_str().unwrap()).unwrap();
        assert_eq!(
            serde_json::to_value(replay.evaluate(&empty_catalog).unwrap()).unwrap(),
            *result
        );
        let reading =
            &row(&report["comparison"], key)["readings"]["vapor_potential_mass_flow_kg_s"];
        assert_eq!(reading["value"], result["potential_mass_flow_kg_s"]);
        assert_eq!(reading["unit"], "kg/s");
    }
    assert_eq!(pattern.raw(), &original);
    assert_eq!(report["geometry_changed"], false);
}

#[test]
fn default_targets_and_repeated_occurrences_do_not_guess_piece_coverage() {
    let mut request = request();
    request["variants"].as_array_mut().unwrap().truncate(1);
    request["vapor"]["layers"] = json!([
        {"key":"first","target":{"scope":"default"}},
        {"key":"second","target":{"scope":"default"}}]);
    let report = run(&request, &pattern());
    let result = &row(&report, "a-fast")["vapor"]["result"];
    assert_eq!(result["layers"].as_array().unwrap().len(), 2);
    assert_eq!(result["layer_resistance_m2_pa_w"], 4.0);
    assert_eq!(result["request"]["area_m2"], 0.5);
    assert_eq!(
        result["layers"][0]["material_sha256"],
        result["layers"][1]["material_sha256"]
    );
    assert_ne!(result["layers"][0]["key"], result["layers"][1]["key"]);
}

#[test]
fn missing_vapor_and_failed_binding_remain_independent_of_other_consumers() {
    let mut request = request();
    request["cutting"] = json!({"copies":1,"gap_mm":2.0,"edge_margin_mm":0.0,"end_margin_mm":0.0});
    request["variants"][1]["changes"]["default"]["record"]["properties"]
        .as_object_mut()
        .unwrap()
        .remove("vapor_resistance");
    request["variants"][2]["changes"] = json!({"schema":"ftx-material-bindings/1",
        "default":{"id":"missing-catalog-selection"}});
    let report = run(&request, &pattern());
    let unavailable = row(&report, "b-slow");
    assert_eq!(unavailable["binding"]["status"], "available");
    assert_eq!(unavailable["cutting"]["status"], "available");
    assert_eq!(unavailable["vapor"]["status"], "refused");
    let compared = row(&report["comparison"], "b-slow");
    assert_eq!(compared["eligible"], false);
    assert!(
        compared["readings"]
            .get("vapor_potential_mass_flow_kg_s")
            .is_none()
    );
    assert_eq!(report["comparison"]["frontier"], json!(["a-fast"]));
    let failed = row(&report, "c-piece");
    assert_eq!(failed["binding"]["status"], "refused");
    assert_eq!(failed["vapor"]["refusal"], failed["binding"]["refusal"]);
    assert_eq!(failed["cutting"]["refusal"], failed["binding"]["refusal"]);
}

#[test]
fn canonical_evidence_protocol_and_layer_refusals_are_retained() {
    let mut base = request();
    base["variants"].as_array_mut().unwrap().truncate(1);
    for invalid in [
        json!({"layers":[{"key":"x","target":{"scope":"piece","id":"absent"}}]}),
        json!({"layers":[{"key":"x","target":{"scope":"default"}},
                          {"key":"x","target":{"scope":"default"}}]}),
        json!({"layers":[]}),
        json!({"required_conditions":{"air_temperature_c":"20"}}),
        json!({"required_method":"a different test"}),
        json!({"outside_vapor_pressure_pa":3000.0}),
    ] {
        let mut request = base.clone();
        request["vapor"]
            .as_object_mut()
            .unwrap()
            .extend(invalid.as_object().unwrap().clone());
        let report = run(&request, &pattern());
        assert_eq!(row(&report, "a-fast")["binding"]["status"], "available");
        assert_eq!(row(&report, "a-fast")["vapor"]["status"], "refused");
    }
    let mut request = base.clone();
    request["variants"][0]["changes"]["default"]["allow_estimates"] = json!(false);
    request["vapor"]["allow_mixed_protocols"] = json!(true);
    assert_eq!(
        row(&run(&request, &pattern()), "a-fast")["vapor"]["status"],
        "refused"
    );
    let mut request = base;
    request["vapor"]["layers"] = json!(
        (0..129)
            .map(|i| json!({"key":format!("p{i}"),
        "target":{"scope":"default"}}))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        row(&run(&request, &pattern()), "a-fast")["vapor"]["refusal"]["code"],
        "budget_exceeded"
    );
}

#[test]
fn only_consumed_protocols_control_comparison_and_bounds_are_not_invented() {
    let mut request = request();
    request["variants"].as_array_mut().unwrap().truncate(2);
    request["vapor"]["layers"]
        .as_array_mut()
        .unwrap()
        .truncate(1);
    let mut unused = selection("unused-lining", 50.0);
    unused["record"]["properties"]["vapor_resistance"]["method"] = json!("unrelated acquisition");
    request["variants"][1]["changes"]["pieces"] = json!({"back":unused});
    assert_eq!(
        run(&request, &pattern())["comparison"]["frontier"],
        json!(["a-fast"])
    );
    request["variants"][1]["changes"]["default"]["record"]["properties"]["vapor_resistance"]["method"] =
        json!("different consumed acquisition");
    let report = run(&request, &pattern());
    assert_eq!(row(&report, "b-slow")["vapor"]["status"], "available");
    assert_eq!(
        report["comparison"]["frontier"],
        json!(["a-fast", "b-slow"])
    );
    request["comparison"]["allow_mixed_protocols"] = json!(true);
    assert_eq!(
        run(&request, &pattern())["comparison"]["frontier"],
        json!(["a-fast"])
    );
    request["comparison"]["basis"] = json!("reported_bounds");
    let typed: Request = serde_json::from_value(request).unwrap();
    let unavailable = serde_json::to_value(typed.evaluate(&pattern()).unwrap()).unwrap();
    assert_eq!(unavailable["comparison"]["frontier"], json!([]));
    assert_eq!(row(&unavailable, "a-fast")["vapor"]["status"], "available");
    assert!(row(&unavailable["comparison"], "a-fast")["readings"]
        ["vapor_potential_mass_flow_kg_s"]["reported_bounds"].is_null());
}

#[test]
fn robust_stack_tradeoffs_use_all_bound_layer_endpoints_and_inclusive_limits() {
    let mut pattern = pattern().raw().clone();
    pattern["pieces"][1]["fabric"]["material"]["record"]["properties"]["vapor_resistance"]["bounds"] =
        json!([2., 4.]);
    let pattern = Pattern::new(pattern);
    let mut request = request();
    request["variants"].as_array_mut().unwrap().truncate(2);
    request["variants"][0]["changes"]["default"]["record"]["properties"]["vapor_resistance"]["bounds"] =
        json!([1., 3.]);
    request["variants"][1]["changes"]["default"]["record"]["properties"]["vapor_resistance"]["bounds"] =
        json!([6., 8.]);
    request["comparison"]["basis"] = json!("reported_bounds");
    request["comparison"]["objectives"] = json!([
        {"metric":"vapor_potential_heat_flow_w","direction":"maximize"},
        {"metric":"vapor_potential_mass_flow_kg_s","direction":"maximize"}]);
    let report = run(&request, &pattern);
    assert_eq!(report["comparison"]["frontier"], json!(["a-fast"]));
    let readings = &row(&report["comparison"], "a-fast")["readings"];
    let heat: [f64; 2] =
        serde_json::from_value(readings["vapor_potential_heat_flow_w"]["reported_bounds"].clone())
            .unwrap();
    let mass: [f64; 2] = serde_json::from_value(
        readings["vapor_potential_mass_flow_kg_s"]["reported_bounds"].clone(),
    )
    .unwrap();
    for (actual, expected) in heat.into_iter().zip([500. / 12., 500. / 8.]) {
        assert!((actual - expected).abs() < 1e-12);
    }
    for (actual, expected) in mass.into_iter().zip(heat.map(|x| x / 2e6)) {
        assert!((actual - expected).abs() < 1e-18);
    }
    request["comparison"]["constraints"] = json!([
        {"metric":"vapor_potential_heat_flow_w","minimum":heat[0],"maximum":heat[1]}]);
    assert_eq!(
        row(&run(&request, &pattern)["comparison"], "a-fast")["eligible"],
        true
    );
    request["comparison"]["constraints"][0]["maximum"] = json!(50.);
    assert_eq!(
        row(&run(&request, &pattern)["comparison"], "a-fast")["eligible"],
        false
    );
    request["comparison"]["constraints"] = json!([]);
    request["variants"][1]["changes"]["default"]["record"]["properties"]["vapor_resistance"]["bounds"] =
        json!([1.5, 9.]);
    assert_eq!(
        run(&request, &pattern)["comparison"]["frontier"],
        json!(["a-fast", "b-slow"])
    );
    let mut missing = pattern.raw().clone();
    missing["pieces"][1]["fabric"]["material"]["record"]["properties"]["vapor_resistance"]
        .as_object_mut()
        .unwrap()
        .remove("bounds");
    let unavailable = run(&request, &Pattern::new(missing));
    assert_eq!(unavailable["comparison"]["frontier"], json!([]));
    assert_eq!(row(&unavailable, "a-fast")["vapor"]["status"], "available");
}

#[test]
fn omitted_vapor_preserves_the_existing_serialized_request_and_result_shape() {
    let mut raw = request();
    raw.as_object_mut().unwrap().remove("vapor");
    raw.as_object_mut().unwrap().remove("comparison");
    raw["cutting"] = json!({"copies":1,"gap_mm":2.0,"edge_margin_mm":0.0,"end_margin_mm":0.0});
    let request: Request = serde_json::from_value(raw).unwrap();
    assert!(
        serde_json::to_value(&request)
            .unwrap()
            .get("vapor")
            .is_none()
    );
    let report = serde_json::to_value(request.evaluate(&pattern()).unwrap()).unwrap();
    assert!(
        report["alternatives"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row.get("vapor").is_none())
    );
    assert!(
        !report["notes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|note| note.as_str().unwrap().contains("vapor"))
    );
}

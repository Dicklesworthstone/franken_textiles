//! Native-callable browser boundaries over measured denim observations and synthetic panel geometry.
//! These checks establish software propagation, not measured garment fit or thermal comfort.

use ftx_core::{RefusalCode, receipt::sha256_hex};
use ftx_export::assembly_mass;
use ftx_pattern::{
    Pattern,
    material_treatment::{self, geometry},
    textile_import,
};
use ftx_wasm::{materials, thermal};
use serde_json::{Value, json};

const DENIM: &str =
    include_str!("../../../tests/fixtures/materials/kanat-ozdil-2026-denim-1-treatment.json");
const DENIM_FIVE: &str =
    include_str!("../../../tests/fixtures/materials/kanat-ozdil-2026-denim-5-treatment.json");

fn inputs(mode: &str) -> (String, String) {
    let treatment: Value = serde_json::from_str(DENIM).unwrap();
    let pattern = json!({"schema":"fabric-pattern/1","units":"mm",
        "fabric":{"material":treatment["before"]},"pieces":[{"id":"panel","sa":0.,
            "vertices":{"a":[0.,0.],"b":[100.,0.],"c":[100.,200.],"d":[0.,200.]},
            "edges":[{"id":"bottom","from":"a","to":"b"},
                {"id":"right","from":"b","to":"c"},
                {"id":"top","from":"c","to":"d"},
                {"id":"left","from":"d","to":"a"}],
            "grain":[[0.,0.],[0.,100.]]}],"seams":[]});
    let request = json!({"schema":geometry::SCHEMA,"mode":mode,
        "transitions":{"wash":treatment},"pieces":{"panel":"wash"}});
    (pattern.to_string(), request.to_string())
}

fn close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-11, "{actual} != {expected}");
}

#[test]
fn portable_transition_matches_exact_core_bytes_and_keeps_partial_readiness() {
    for fixture in [DENIM, DENIM_FIVE] {
        let input: material_treatment::Input = serde_json::from_str(fixture).unwrap();
        let expected = material_treatment::evaluate(&input).unwrap();
        let text = materials::treatment(fixture).unwrap();
        assert_eq!(text, serde_json::to_string(&expected).unwrap());
        assert_eq!(
            expected.input_sha256,
            sha256_hex(&serde_json::to_vec(&input).unwrap())
        );
        let inspection: Value =
            serde_json::from_str(&materials::inspect(&expected.after_selection_json).unwrap())
                .unwrap();
        assert_eq!(inspection["selection_json"], expected.after_selection_json);
        assert_eq!(
            inspection["capabilities"]["steady_conduction"]["ready"],
            true
        );
        for capability in [
            "mechanical_coefficients",
            "transient_thermal",
            "stock_width_m",
            "sewing_thickness_m",
        ] {
            assert_eq!(inspection["capabilities"][capability]["ready"], false);
        }
        assert_eq!(
            expected.properties_no_longer_available,
            ["thread_density_warp", "thread_density_weft"]
        );
    }
}

#[test]
fn ordinary_derived_patterns_change_retained_mass_and_steady_heat_automatically() {
    let (source, request) = inputs("treat");
    let input: geometry::Input = serde_json::from_str(&request).unwrap();
    let expected = input.apply(&Pattern::from_json(&source).unwrap()).unwrap();
    let encoded = materials::treatment_pattern(&source, &request).unwrap();
    assert_eq!(encoded, serde_json::to_string(&expected).unwrap());
    let output: Value = serde_json::from_str(&encoded).unwrap();
    let derived = output["pattern_json"].as_str().unwrap();
    assert_eq!(output["pattern_sha256"], sha256_hex(derived.as_bytes()));
    assert_eq!(
        output["request_sha256"],
        sha256_hex(output["frozen_request_json"].as_str().unwrap().as_bytes())
    );
    assert_eq!(
        materials::treatment_pattern(
            output["source_pattern_json"].as_str().unwrap(),
            output["frozen_request_json"].as_str().unwrap(),
        )
        .unwrap(),
        encoded
    );
    let mass: assembly_mass::Request = serde_json::from_value(json!({
        "schema":"ftx-assembly-mass/1","inventory_complete":true,"extras":[],
        "seam_allowance_mm":0.0
    }))
    .unwrap();
    let area_after = 0.02 * (1.0 - 0.0483) * (1.0 - 0.055);
    let mut powers = Vec::new();
    for (text, area, density, resistance, state) in [
        (source.as_str(), 0.02, 0.33993, 0.0152, "before"),
        (derived, area_after, 0.37010, 0.0245, "after"),
    ] {
        let pattern = Pattern::from_json(text).unwrap();
        let retained = mass.evaluate(&pattern).unwrap();
        close(retained.pieces[0].retained_area_m2, area);
        close(retained.assembly_mass.kg.unwrap(), area * density);
        let selected =
            textile_import::resolve_fabric(textile_import::piece_fabric(&pattern, 0).unwrap())
                .unwrap()
                .unwrap();
        assert_eq!(
            selected.sha256(),
            output["transitions"]["wash"][format!("{state}_sha256")]
                .as_str()
                .unwrap()
        );
        let heat = json!({"schema":"ftx-thermal-steady/1",
            "layers":[selected.frozen_selection()],"interface_resistance_m2_k_w":[],
            "area_m2":area,"inside_c":35.,"outside_c":20.,
            "inside_resistance_m2_k_w":0.04,"outside_resistance_m2_k_w":0.04});
        let result: Value =
            serde_json::from_str(&thermal::steady(&heat.to_string()).unwrap()).unwrap();
        let power = result["power_w"].as_f64().unwrap();
        close(power, area * 15.0 / (0.08 + resistance));
        powers.push(power);
        assert_eq!(result["storage_required"], false);
        assert!(selected.mechanics().is_err());
    }
    assert!(powers[1] < powers[0]);
    // Compensation stays in the BEFORE material; applying the observed treatment recovers the target.
    let (_, compensation) = inputs("compensate");
    let compensated: Value =
        serde_json::from_str(&materials::treatment_pattern(&source, &compensation).unwrap())
            .unwrap();
    let recovered: Value = serde_json::from_str(
        &materials::treatment_pattern(compensated["pattern_json"].as_str().unwrap(), &request)
            .unwrap(),
    )
    .unwrap();
    close(
        recovered["pattern"]["pieces"][0]["vertices"]["c"][0]
            .as_f64()
            .unwrap(),
        100.,
    );
    close(
        recovered["pattern"]["pieces"][0]["vertices"]["c"][1]
            .as_f64()
            .unwrap(),
        200.,
    );
}

#[test]
fn portable_boundaries_refuse_unknown_schemas_and_stale_observations() {
    let mut transition: Value = serde_json::from_str(DENIM).unwrap();
    transition["schema"] = json!("ftx-material-treatment/99");
    assert_eq!(
        materials::treatment(&transition.to_string())
            .unwrap_err()
            .code,
        RefusalCode::InvalidDocument
    );
    transition = serde_json::from_str(DENIM).unwrap();
    transition["after"]["record"]["name"] = json!("Changed source lot without a new lock");
    assert!(materials::treatment(&transition.to_string()).is_err());
    assert!(materials::treatment("{").is_err());
    let (pattern, request) = inputs("treat");
    let mut wrong: Value = serde_json::from_str(&request).unwrap();
    wrong["schema"] = json!("ftx-pattern-treatment/99");
    assert_eq!(
        materials::treatment_pattern(&pattern, &wrong.to_string())
            .unwrap_err()
            .code,
        RefusalCode::InvalidDocument
    );
    wrong = serde_json::from_str(&request).unwrap();
    wrong["unknown_property"] = json!(true);
    assert!(materials::treatment_pattern(&pattern, &wrong.to_string()).is_err());
    let mut stale: Value = serde_json::from_str(&pattern).unwrap();
    stale["fabric"]["material"]["record"]["name"] = json!("Different untreated lot");
    assert!(materials::treatment_pattern(&stale.to_string(), &request).is_err());
    assert!(materials::treatment_pattern("{", &request).is_err());
}

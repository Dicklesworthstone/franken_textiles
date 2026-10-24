//! Direct decision identities on synthetic receipts; actual solver replay is tested by consumers.
use super::*;

fn case() -> (Alternative, Value, Value, Value) {
    let (mut alternative, mut descriptor, mut motion) = super::super::heating_tests::case(false);
    let mut raw = serde_json::to_value(
        alternative
            .frozen_changes
            .as_ref()
            .unwrap()
            .default
            .as_ref()
            .unwrap(),
    )
    .unwrap();
    raw["expected_sha256"] = Value::Null;
    raw["record"]["properties"]["vapor_resistance"] = json!({"value":7.,"unit":"m2*Pa/W",
        "source":"test","evidence":"estimate","method":"Synthetic canonical Ret; no empirical claim"});
    let catalog = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: vec![],
    };
    let material = catalog
        .resolve(&serde_json::from_value(raw).unwrap())
        .unwrap();
    alternative.frozen_changes.as_mut().unwrap().default = Some(material.frozen_selection());
    let pattern = ftx_pattern::Pattern::new(
        serde_json::from_str(include_str!(
            "../../../../../../tests/fixtures/materials/synthetic-sewing-pattern.json"
        ))
        .unwrap(),
    );
    alternative.binding = super::super::super::Outcome::Available {
        result: alternative
            .frozen_changes
            .as_ref()
            .unwrap()
            .apply(&pattern)
            .unwrap(),
    };
    let positions = json!([
        [0., 0., 0.],
        [0.02, 0., 0.],
        [0., 0.1, 0.],
        [0., 0.2, 0.],
        [0.02, 0.2, 0.],
        [0., 0.3, 0.]
    ]);
    motion["samples"][0]["positions_m"] = positions.clone();
    let patches = json!([{"vertices":[0,1,2],"reference_area_m2":0.001,"column":0},
        {"vertices":[3,4,5],"reference_area_m2":0.001,"column":0}]);
    let cells = 2;
    let c = material
        .material()
        .require("areal_density", "kg/m2", true)
        .unwrap()
        * material
            .material()
            .require("specific_heat", "J/(kg*K)", true)
            .unwrap()
        / cells as f64;
    let r = material.thermal_resistance().unwrap() / cells as f64;
    let column = json!({"schema":"ftx-dry-thermal-model/1","layers":[material],
        "cells_per_layer":cells,"interface_resistance_m2_k_w":[],
        "cell_capacity_j_m2_k":vec![c;cells],"cell_resistance_m2_k_w":vec![r;cells]});
    let mut model = json!({"schema":"ftx-thermal-surface/1","vertex_count":6,
        "columns":[column],"patches":patches});
    model["sha256"] = json!(hash(&model).unwrap());
    let material_report =
        json!({"basis":"characterized_record","record_sha256":material.sha256(),"record":material});
    let mut frame = json!({"schema":"ftx-drape-thermal-surface/1","available":true,
        "model":model,"instances":["front","back"],"triangle_instance":[0,1],
        "positions_m":positions,"material_report_sha256":hash(&material_report).unwrap()});
    frame["sha256"] = json!(hash(&frame).unwrap());
    let simulation = json!({"positions_m":positions,"triangles":motion["triangles"],
        "drape":{"material":material_report,"thermal_surface":frame}});
    let policy = json!({"schema":"ftx-textile-surface-water/1",
        "model":"inner_cell_equilibrium_free_film_frozen_properties/1",
        "bindings":{"front":{"material_sha256":material.sha256(),"initial_liquid_kg_m2":1e-5},
            "back":{"material_sha256":material.sha256(),"initial_liquid_kg_m2":0.}},
        "maximum_liquid_loading_kg_m2":1e-5,"liquid_specific_heat_j_kg_k":4200.,
        "latent_heat_j_kg":2500000.,"enthalpy_reference_c":0.,
        "wet_vapor_pressure_pa":[[-20.,0.],[80.,10000.]],"outside_vapor_pressure_pa":[1000.],
        "inside_film_resistance_m2_pa_w":1.,"outside_film_resistance_m2_pa_w":1.,
        "replenishment_kg_m2_s":0.,"source":{"reference":"test:water-admission","accessed_on":"2026-10-23",
            "note":"Synthetic receipt identities, not measured material data"},
        "evidence":"estimate","allow_estimates":true,"method":"Synthetic film",
        "qualification":"Synthetic fixed dry thermal/Ret domain [-20,80] C and [0,1e-5] kg/m2"});
    let network = material_vapor::Request {
        schema: material_vapor::SCHEMA.into(),
        layers: vec![material_vapor::Layer {
            key: material.material().id.clone(),
            selection: material.frozen_selection(),
            area_m2: None,
        }],
        area_m2: 1.,
        inside_vapor_pressure_pa: 1.,
        outside_vapor_pressure_pa: 0.,
        inside_film_resistance_m2_pa_w: 1.,
        outside_film_resistance_m2_pa_w: 1.,
        latent_heat_j_kg: 2500000.,
        required_conditions: BTreeMap::new(),
        required_method: None,
        allow_mixed_protocols: false,
        interface_resistance_m2_pa_w: vec![],
    }
    .evaluate(&catalog)
    .unwrap();
    let initial = 1e-5 * 0.001;
    let water = json!({"schema":"ftx-textile-surface-water-result/1","initial_liquid_kg":initial,
        "remaining_liquid_kg":0.,"evaporated_liquid_kg":initial,"latent_energy_j":initial*2500000.,
        "exported_vapor_sensible_energy_j":initial*4200.*20.,"liquid_stored_change_j":-initial*4200.*20.,
        "mass_balance_error_kg":0.,"dryout_events":1,"balance_error_j":0.});
    let heat = &mut motion["transient_surface"]["result"];
    heat["request"]["surface_water"] = policy.clone();
    heat["request"]["cells_per_layer"] = json!(cells);
    heat["initial_temperature_c"] = json!(20.);
    heat["model"] = model;
    heat["final"] = json!({"temperature_c":[[21.,21.],[21.,21.]]});
    heat["thermal_steps"] = json!(2);
    heat["cell_steps"] = json!(8);
    let storage = 2. * 0.001 * c * cells as f64;
    let net = storage + initial * 2500000. - 3.; // Maxwell heat is independently admitted.
    for row in [
        "inside_energy_j",
        "outside_energy_j",
        "net_boundary_energy_j",
        "stored_change_j",
    ] {
        let value = match row {
            "inside_energy_j" => 0.,
            "outside_energy_j" => -net,
            "net_boundary_energy_j" => net,
            _ => storage,
        };
        heat[row] = json!(value);
        heat["trace"][0][row] = json!(value);
    }
    heat["trace"][0]["thermal_substeps"] = json!(2);
    heat["trace"][0]["positions_sha256"] = json!(hash(&positions).unwrap());
    heat["trace"][0]["temperature_range_c"] = json!([21., 21.]);
    heat["trace"][0]["surface_water"] = water.clone();
    heat["surface_water"] = water;
    heat["surface_water"]["request"] = policy;
    heat["surface_water"]["unit_pressure_networks"] = json!([network]);
    heat["surface_water"]["patches"] = json!([
        {"source_triangle":0,"instance":"front","initial_liquid_kg":initial,"remaining_liquid_kg":0.,"evaporated_liquid_kg":initial},
        {"source_triangle":1,"instance":"back","initial_liquid_kg":0.,"remaining_liquid_kg":0.,"evaporated_liquid_kg":0.}]);
    heat["relaxation_dissipation_heat"]["material_fractions"][0]["material_sha256"] =
        json!(material.sha256());
    let law = material
        .membrane_relaxation()
        .unwrap()
        .unwrap()
        .fingerprint()
        .to_owned();
    heat["relaxation_dissipation_heat"]["material_fractions"][0]["law_sha256"] = json!(law);
    descriptor["scenario"]["motion"]["transient_surface"] = heat["request"].clone();
    motion["initial_membrane_history"]["elements"][0]["law_sha256"] = json!(law);
    (alternative, descriptor, simulation, motion)
}

#[test]
fn textile_water_and_maxwell_heat_share_one_conserved_decision_ledger() {
    let (a, d, s, m) = case();
    let heat =
        admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).unwrap_or_else(|r| panic!("{r:?}"));
    let admitted = Numerical {
        drape: &s,
        vapor_surface: &Value::Null,
        vapor_coverage: None,
        motion: &m,
        motion_vapor_coverage: None,
        motion_wet_film: None,
        transient_surface: Some(heat),
        transient_coverage: None,
    };
    for (metric, expected) in [
        (Metric::TextileWaterEvaporatedMassKg, 1e-5 * 0.001),
        (Metric::TextileWaterRemainingMassKg, 0.),
        (Metric::TextileWaterLatentEnergyJ, 1e-5 * 0.001 * 2500000.),
    ] {
        assert_eq!(metric.consumer(), Consumer::TransientSurface);
        assert!(!metric.supports_bounds());
        assert_eq!(metric.value(&a, &admitted), Some(expected));
    }
    assert_eq!(
        Metric::TransientSurfaceMinSampleTemperatureC.value(&a, &admitted),
        Some(20.)
    );
    assert_eq!(
        Metric::TransientSurfaceFinalMaxTemperatureC.value(&a, &admitted),
        Some(21.)
    );
}

#[test]
fn incomplete_or_changed_water_cannot_become_a_feasible_objective() {
    for fault in 0..19 {
        let (mut a, mut d, mut s, mut m) = case();
        let h = &mut m["transient_surface"]["result"];
        match fault {
            0 => h["surface_water"]["patches"][0]["instance"] = json!("back"),
            1 => h["surface_water"]["patches"][0]["initial_liquid_kg"] = json!(0.),
            2 => {
                h["surface_water"]["unit_pressure_networks"][0]["potential_heat_flow_w"] = json!(0.)
            }
            3 => h["surface_water"]["request"]["source"]["reference"] = json!("changed"),
            4 => h["trace"][0]["surface_water"]["latent_energy_j"] = json!(0.),
            5 => h["trace"][0]["surface_water"]["exported_vapor_sensible_energy_j"] = json!(0.),
            6 => h["trace"][0]["surface_water"]["liquid_stored_change_j"] = json!(0.),
            7 => h["trace"][0]["surface_water"]["remaining_liquid_kg"] = json!(1e-9),
            8 => h["final"]["temperature_c"][0][0] = json!(22.),
            9 => h["surface_water"]["dryout_events"] = json!(0),
            10 => h["surface_water"]["schema"] = json!("ftx-textile-surface-water-result/2"),
            11 => h["stored_change_j"] = json!(0.),
            12 => h["model"]["patches"][0]["reference_area_m2"] = json!(0.002),
            13 => s["drape"]["thermal_surface"]["triangle_instance"] = json!([1, 0]),
            14 => {
                a.frozen_changes
                    .as_mut()
                    .unwrap()
                    .default
                    .as_mut()
                    .unwrap()
                    .expected_sha256 = Some("f".repeat(64))
            }
            15 => {
                d["scenario"]["motion"]["transient_surface"]["surface_water"]["allow_estimates"] =
                    json!(false);
                h["request"] = d["scenario"]["motion"]["transient_surface"].clone();
                h["surface_water"]["request"] = h["request"]["surface_water"].clone();
            }
            16 => h["surface_water"]["supplied_liquid_kg"] = json!(0.),
            17 => {
                h["request"]["surface_water"]["outside_vapor_pressure_pa"] = json!([1e9]);
                h["surface_water"]["request"] = h["request"]["surface_water"].clone();
                d["scenario"]["motion"]["transient_surface"] = h["request"].clone();
            }
            _ => {
                // A self-consistent residual is not sufficient: production conserves energy.
                let inside = h["inside_energy_j"].as_f64().unwrap() + 1.;
                let net = h["net_boundary_energy_j"].as_f64().unwrap() + 1.;
                h["inside_energy_j"] = json!(inside);
                h["net_boundary_energy_j"] = json!(net);
                h["balance_error_j"] = json!(-1.);
                h["surface_water"]["balance_error_j"] = json!(-1.);
                h["trace"][0]["inside_energy_j"] = json!(inside);
                h["trace"][0]["net_boundary_energy_j"] = json!(net);
                h["trace"][0]["balance_error_j"] = json!(-1.);
                h["trace"][0]["surface_water"]["balance_error_j"] = json!(-1.);
                h["absolute_step_balance_error_j"] = json!(1.);
            }
        }
        assert!(
            admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_err(),
            "fault {fault}"
        );
    }
}

#[test]
fn condensation_refusal_tracks_wet_inventory_and_actual_ambient() {
    for wet in [false, true] {
        let (a, mut d, s, mut m) = case();
        let h = &mut m["transient_surface"]["result"];
        let initial = if wet { 1e-5 * 0.001 } else { 0. };
        let remaining = initial / 2.;
        let evaporated = initial - remaining;
        let liquid = 4200. * (remaining * 19. - initial * 20.);
        let exported = evaporated * 4200. * 20.;
        let latent = evaporated * 2500000.;
        let storage = -h["stored_change_j"].as_f64().unwrap();
        let net = storage + liquid + exported + latent - 3.;
        h["request"]["surface_water"]["bindings"]["front"]["initial_liquid_kg_m2"] =
            json!(if wet { 1e-5 } else { 0. });
        h["surface_water"]["request"] = h["request"]["surface_water"].clone();
        h["final"]["temperature_c"] = json!([[19., 19.], [19., 19.]]);
        h["trace"][0]["temperature_range_c"] = json!([19., 19.]);
        h["surface_water"]["patches"][0]["initial_liquid_kg"] = json!(initial);
        h["surface_water"]["patches"][0]["remaining_liquid_kg"] = json!(remaining);
        h["surface_water"]["patches"][0]["evaporated_liquid_kg"] = json!(evaporated);
        for (field, value) in [
            ("initial_liquid_kg", initial),
            ("remaining_liquid_kg", remaining),
            ("evaporated_liquid_kg", evaporated),
            ("latent_energy_j", latent),
            ("liquid_stored_change_j", liquid),
            ("exported_vapor_sensible_energy_j", exported),
        ] {
            h["surface_water"][field] = json!(value);
            h["trace"][0]["surface_water"][field] = json!(value);
        }
        h["surface_water"]["dryout_events"] = json!(0);
        h["trace"][0]["surface_water"]["dryout_events"] = json!(0);
        for (field, value) in [
            ("stored_change_j", storage),
            ("inside_energy_j", 0.),
            ("outside_energy_j", -net),
            ("net_boundary_energy_j", net),
        ] {
            h[field] = json!(value);
            h["trace"][0][field] = json!(value);
        }
        d["scenario"]["motion"]["transient_surface"] = h["request"].clone();
        assert!(admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_ok());
        let h = &mut m["transient_surface"]["result"];
        h["request"]["surface_water"]["outside_vapor_pressure_pa"] =
            json!([if wet { 3950. } else { 1e9 }]);
        h["surface_water"]["request"] = h["request"]["surface_water"].clone();
        d["scenario"]["motion"]["transient_surface"] = h["request"].clone();
        assert_eq!(
            admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_ok(),
            !wet
        );
    }
}

#[test]
fn an_enthalpy_reference_change_does_not_change_the_water_objectives() {
    let (a, mut d, s, mut m) = case();
    let reference = 100.;
    let h = &mut m["transient_surface"]["result"];
    h["request"]["surface_water"]["enthalpy_reference_c"] = json!(reference);
    h["surface_water"]["request"] = h["request"]["surface_water"].clone();
    {
        let w = &mut h["surface_water"];
        w["exported_vapor_sensible_energy_j"] = json!(1e-5 * 0.001 * 4200. * (20. - reference));
        w["liquid_stored_change_j"] = json!(-1e-5 * 0.001 * 4200. * (20. - reference));
    }
    h["trace"][0]["surface_water"]["exported_vapor_sensible_energy_j"] =
        h["surface_water"]["exported_vapor_sensible_energy_j"].clone();
    h["trace"][0]["surface_water"]["liquid_stored_change_j"] =
        h["surface_water"]["liquid_stored_change_j"].clone();
    d["scenario"]["motion"]["transient_surface"] = h["request"].clone();
    assert!(admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_ok());
}

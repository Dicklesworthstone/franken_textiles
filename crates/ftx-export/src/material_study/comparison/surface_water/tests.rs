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
        material_strain: None,
        contact_loads: None,
        vapor_surface: &Value::Null,
        vapor_coverage: None,
        motion: &m,
        motion_material_strain: None,
        motion_contact_loads: None,
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

fn supplied_case() -> (Alternative, Value, Value, Value) {
    let (a, mut d, s, mut m) = case();
    m["keyframes"][0]["time_s"] = json!(1.);
    let h = &mut m["transient_surface"]["result"];
    let policy = &mut h["request"]["surface_water"];
    policy["schema"] = json!("ftx-textile-surface-water/2");
    policy
        .as_object_mut()
        .unwrap()
        .remove("replenishment_kg_m2_s");
    policy["maximum_liquid_loading_kg_m2"] = json!(5e-5);
    policy["supply"] = json!({"schema":"ftx-textile-liquid-supply/1",
        "bindings":{"front":{"rate_kg_m2_s":[2e-5],"temperature_c":[30.]},
            "back":{"rate_kg_m2_s":[0.],"temperature_c":[20.]}},
        "source":{"reference":"test:liquid-delivery","accessed_on":"2026-10-24",
            "note":"Synthetic delivery independent of film and material evidence"},
        "evidence":"estimate","allow_estimates":true,"method":"Synthetic specified liquid supply"});
    h["surface_water"]["request"] = h["request"]["surface_water"].clone();
    let initial = 1e-5 * 0.001;
    let supplied = 2e-5 * 0.001;
    let incoming_j = supplied * 4200. * 30.;
    let liquid_j = 4200. * (supplied * 21. - initial * 20.);
    let net = h["stored_change_j"].as_f64().unwrap()
        + liquid_j
        + initial * 4200. * 20.
        + initial * 2500000.
        - incoming_j
        - 3.;
    for field in ["remaining_liquid_kg", "supplied_liquid_kg"] {
        h["surface_water"][field] = json!(supplied);
        h["trace"][0]["surface_water"][field] = json!(supplied);
    }
    for (field, value) in [
        ("supplied_liquid_sensible_energy_j", incoming_j),
        ("liquid_stored_change_j", liquid_j),
    ] {
        h["surface_water"][field] = json!(value);
        h["trace"][0]["surface_water"][field] = json!(value);
    }
    h["surface_water"]["schema"] = json!("ftx-textile-surface-water-result/2");
    h["trace"][0]["surface_water"]["schema"] = json!("ftx-textile-surface-water-result/2");
    h["surface_water"]["dryout_events"] = json!(0);
    h["trace"][0]["surface_water"]["dryout_events"] = json!(0);
    h["surface_water"]["patches"][0]["remaining_liquid_kg"] = json!(supplied);
    h["surface_water"]["patches"][0]["supplied_liquid_kg"] = json!(supplied);
    h["surface_water"]["patches"][0]["supplied_liquid_sensible_energy_j"] = json!(incoming_j);
    h["surface_water"]["patches"][1]["supplied_liquid_kg"] = json!(0.);
    h["surface_water"]["patches"][1]["supplied_liquid_sensible_energy_j"] = json!(0.);
    h["outside_energy_j"] = json!(-net);
    h["net_boundary_energy_j"] = json!(net);
    h["trace"][0]["outside_energy_j"] = json!(-net);
    h["trace"][0]["net_boundary_energy_j"] = json!(net);
    d["scenario"]["motion"]["transient_surface"] = h["request"].clone();
    (a, d, s, m)
}

fn condensing_case() -> (Alternative, Value, Value, Value) {
    let (a, mut d, s, mut m) = supplied_case();
    let h = &mut m["transient_surface"]["result"];
    let policy = &mut h["request"]["surface_water"];
    policy["schema"] = json!("ftx-textile-surface-water/3");
    policy["outside_vapor_pressure_pa"] = json!([6000.]);
    policy["condensation"] = json!({"schema":"ftx-textile-condensation/1",
        "vapor_temperature_c":[40.],"source":{"reference":"test:condensation-ledger",
        "accessed_on":"2026-10-27","note":"Synthetic phase source; not empirical textile evidence"},
        "evidence":"estimate","allow_estimates":true,"method":"Explicit synthetic vapor reservoir",
        "qualification":"Exact synthetic reverse Ret and wet/dry phase closure with fixed latent and cp_liquid effective sensible enthalpy"});
    let condensed = 1e-5 * 0.001;
    let incoming = condensed * 4200. * 40.;
    let released = condensed * 2500000.;
    let storage = condensed * 4200. * 21.;
    let remaining = h["surface_water"]["remaining_liquid_kg"].as_f64().unwrap() + condensed;
    let stored = h["surface_water"]["liquid_stored_change_j"]
        .as_f64()
        .unwrap()
        + storage;
    let net = h["net_boundary_energy_j"].as_f64().unwrap() + storage - incoming - released;
    {
        let row = &mut h["surface_water"];
        row["schema"] = json!("ftx-textile-surface-water-result/3");
        row["remaining_liquid_kg"] = json!(remaining);
        row["condensed_liquid_kg"] = json!(condensed);
        row["incoming_vapor_sensible_energy_j"] = json!(incoming);
        row["condensation_latent_energy_j"] = json!(released);
        row["liquid_stored_change_j"] = json!(stored);
    }
    for (i, patch) in h["surface_water"]["patches"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        patch["condensed_liquid_kg"] = json!(if i == 0 { condensed } else { 0. });
        patch["incoming_vapor_sensible_energy_j"] = json!(if i == 0 { incoming } else { 0. });
        if i == 0 {
            patch["remaining_liquid_kg"] = json!(remaining);
        }
    }
    h["surface_water"]["request"] = h["request"]["surface_water"].clone();
    for field in [
        "schema",
        "remaining_liquid_kg",
        "condensed_liquid_kg",
        "incoming_vapor_sensible_energy_j",
        "condensation_latent_energy_j",
        "liquid_stored_change_j",
    ] {
        h["trace"][0]["surface_water"][field] = h["surface_water"][field].clone();
    }
    h["net_boundary_energy_j"] = json!(net);
    h["outside_energy_j"] = json!(-net);
    h["trace"][0]["net_boundary_energy_j"] = json!(net);
    h["trace"][0]["outside_energy_j"] = json!(-net);
    d["scenario"]["motion"]["transient_surface"] = h["request"].clone();
    (a, d, s, m)
}

#[test]
fn condensation_admission_reconciles_each_mass_source_and_incoming_vapor_enthalpy() {
    let (a, d, s, m) = condensing_case();
    assert!(admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_ok());
    for fault in 0..8 {
        let (a, mut d, s, mut m) = condensing_case();
        let h = &mut m["transient_surface"]["result"];
        match fault {
            0 => h["surface_water"]["patches"][0]["condensed_liquid_kg"] = json!(0.),
            1 => h["trace"][0]["surface_water"]["condensed_liquid_kg"] = json!(0.),
            2 => h["surface_water"]["condensation_latent_energy_j"] = json!(0.),
            3 => h["surface_water"]["patches"][0]["incoming_vapor_sensible_energy_j"] = json!(0.),
            4 => {
                h["request"]["surface_water"]["condensation"]["vapor_temperature_c"] = json!([50.])
            }
            5 => h["request"]["surface_water"]["condensation"]["allow_estimates"] = json!(false),
            6 => h["request"]["surface_water"]["schema"] = json!("ftx-textile-surface-water/2"),
            _ => h["request"]["surface_water"]["condensation"]["qualification"] = json!(""),
        }
        h["surface_water"]["request"] = h["request"]["surface_water"].clone();
        d["scenario"]["motion"]["transient_surface"] = h["request"].clone();
        assert!(
            admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_err(),
            "fault {fault}"
        );
    }
}

#[test]
fn supplied_inventory_and_incoming_enthalpy_join_the_same_conserved_history() {
    let (a, d, s, m) = supplied_case();
    let result =
        admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).unwrap_or_else(|r| panic!("{r:?}"));
    assert_eq!(result.result["surface_water"]["supplied_liquid_kg"], 2e-8);
    assert_eq!(result.result["surface_water"]["remaining_liquid_kg"], 2e-8);
    assert_eq!(result.result["surface_water"]["evaporated_liquid_kg"], 1e-8);
}

#[test]
fn incoming_enthalpy_reference_cancels_against_liquid_and_vapor_storage() {
    let (a, mut d, s, mut m) = supplied_case();
    let h = &mut m["transient_surface"]["result"];
    let reference = 100.;
    h["request"]["surface_water"]["enthalpy_reference_c"] = json!(reference);
    h["surface_water"]["request"] = h["request"]["surface_water"].clone();
    let incoming = 2e-5 * 0.001 * 4200. * (30. - reference);
    h["surface_water"]["patches"][0]["supplied_liquid_sensible_energy_j"] = json!(incoming);
    for (field, value) in [
        ("supplied_liquid_sensible_energy_j", incoming),
        (
            "exported_vapor_sensible_energy_j",
            1e-5 * 0.001 * 4200. * (20. - reference),
        ),
        (
            "liquid_stored_change_j",
            4200. * (2e-5 * 0.001 * (21. - reference) - 1e-5 * 0.001 * (20. - reference)),
        ),
    ] {
        h["surface_water"][field] = json!(value);
        h["trace"][0]["surface_water"][field] = json!(value);
    }
    d["scenario"]["motion"]["transient_surface"] = h["request"].clone();
    assert!(admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_ok());
}

#[test]
fn refined_histories_preserve_valid_accumulated_latent_and_liquid_enthalpy() {
    let (a, mut d, s, mut m) = supplied_case();
    let steps = 50_000;
    let initial = 0.003;
    let mut remaining = initial;
    let mut evaporated = 0.;
    let mut supplied = 0.;
    let mut latent = 0.;
    let mut liquid = 0.;
    let mut exported = 0.;
    let mut incoming = 0.;
    for step in 0..steps {
        let old = remaining;
        remaining += 1.13e-8 - 3.7e-9;
        evaporated += 3.7e-9;
        supplied += 1.13e-8;
        latent += 3.7e-9 * 2500000.;
        let temperature = if step + 1 == steps { 21. } else { 20. };
        liquid += 4200. * (remaining * temperature - old * 20.);
        exported += 3.7e-9 * 4200. * 20.;
        incoming += 1.13e-8 * 4200. * 30.;
    }
    // These independently accumulated positive quantities satisfy the producer tolerances, but
    // a fixed epsilon check incorrectly rejects their ordinary summation drift after refinement.
    let mass_error: f64 = remaining + evaporated - initial - supplied;
    assert!(mass_error.abs() < 1024. * f64::EPSILON * (initial + supplied));
    assert!(!near(latent, evaporated * 2500000., latent));
    assert!((latent - evaporated * 2500000.).abs() < 1e-8 * (1. + latent));
    let h = &mut m["transient_surface"]["result"];
    h["request"]["max_cell_steps"] = json!(steps * 4);
    h["request"]["surface_water"]["bindings"]["front"]["initial_liquid_kg_m2"] =
        json!(initial / 0.001);
    h["request"]["surface_water"]["maximum_liquid_loading_kg_m2"] = json!(10.);
    h["request"]["surface_water"]["qualification"] =
        json!("Synthetic fixed dry properties over [-20,80] C and [0,10] kg/m2");
    h["request"]["surface_water"]["supply"]["bindings"]["front"]["rate_kg_m2_s"] =
        json!([1.13e-8 * steps as f64 / 0.001]);
    h["surface_water"]["request"] = h["request"]["surface_water"].clone();
    h["thermal_steps"] = json!(steps);
    h["cell_steps"] = json!(steps * 4);
    h["trace"][0]["thermal_substeps"] = json!(steps);
    for (field, value) in [
        ("initial_liquid_kg", initial),
        ("remaining_liquid_kg", remaining),
        ("evaporated_liquid_kg", evaporated),
        ("supplied_liquid_kg", supplied),
        ("supplied_liquid_sensible_energy_j", incoming),
    ] {
        h["surface_water"]["patches"][0][field] = json!(value);
        h["surface_water"][field] = json!(value);
        h["trace"][0]["surface_water"][field] = json!(value);
    }
    for (field, value) in [
        ("latent_energy_j", latent),
        ("liquid_stored_change_j", liquid),
        ("exported_vapor_sensible_energy_j", exported),
        ("mass_balance_error_kg", mass_error),
    ] {
        h["surface_water"][field] = json!(value);
        h["trace"][0]["surface_water"][field] = json!(value);
    }
    let net = h["stored_change_j"].as_f64().unwrap() + liquid + exported + latent - incoming - 3.;
    for (field, value) in [("outside_energy_j", -net), ("net_boundary_energy_j", net)] {
        h[field] = json!(value);
        h["trace"][0][field] = json!(value);
    }
    d["scenario"]["motion"]["transient_surface"] = h["request"].clone();
    admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).unwrap_or_else(|r| panic!("{r:?}"));
}

#[test]
fn wrong_incomplete_or_unqualified_delivery_never_enters_a_comparison() {
    for fault in 0..14 {
        let (a, mut d, s, mut m) = supplied_case();
        let h = &mut m["transient_surface"]["result"];
        let supply = &mut h["request"]["surface_water"]["supply"];
        match fault {
            0 => supply["allow_estimates"] = json!(false),
            1 => supply["source"]["reference"] = json!(""),
            2 => {
                supply["bindings"]
                    .as_object_mut()
                    .unwrap()
                    .remove("back")
                    .unwrap();
            }
            3 => supply["bindings"]["front"]["rate_kg_m2_s"] = json!([]),
            4 => supply["bindings"]["front"]["temperature_c"] = json!([100.]),
            5 => supply["bindings"]["front"]["rate_kg_m2_s"] = json!([3e-5]),
            6 => supply["bindings"]["front"]["temperature_c"] = json!([35.]),
            7 => h["surface_water"]["patches"][0]["supplied_liquid_kg"] = json!(0.),
            8 => h["surface_water"]["patches"][0]["supplied_liquid_sensible_energy_j"] = json!(0.),
            9 => h["trace"][0]["surface_water"]["supplied_liquid_kg"] = json!(0.),
            10 => h["trace"][0]["surface_water"]["supplied_liquid_sensible_energy_j"] = json!(0.),
            11 => h["request"]["surface_water"]["replenishment_kg_m2_s"] = json!(0.),
            12 => h["request"]["surface_water"]["maximum_liquid_loading_kg_m2"] = json!(1e-5),
            _ => h["surface_water"]["dryout_events"] = json!(100),
        }
        h["surface_water"]["request"] = h["request"]["surface_water"].clone();
        d["scenario"]["motion"]["transient_surface"] = h["request"].clone();
        assert!(
            admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_err(),
            "fault {fault}"
        );
    }
}

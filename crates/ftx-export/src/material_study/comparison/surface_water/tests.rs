//! Direct decision identities on synthetic receipts; actual solver replay is tested by consumers.
use super::*;

fn case() -> (Alternative, Value, Value, Value) {
    case_with_variable_heat(false)
}

fn case_with_variable_heat(variable: bool) -> (Alternative, Value, Value, Value) {
    let (mut alternative, mut descriptor, _, mut motion) = super::super::heating_tests::case(false);
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
    if variable {
        raw["record"]["properties"]
            .as_object_mut()
            .unwrap()
            .remove("specific_heat");
        for key in ["areal_density", "thermal_resistance"] {
            raw["record"]["properties"][key]["conditions"]["specimen"] =
                json!("synthetic-maxwell-heating-A");
            raw["record"]["properties"][key]["conditions"]["conditioning"] =
                json!("synthetic constant dry state");
        }
        raw["record"]["thermal_response"] = json!({
            "specimen":"synthetic-maxwell-heating-A","conditioning":"synthetic constant dry state",
            "specific_heat":{"unit":"J/(kg*K)","evidence":"estimate","source":"test",
                "method":"Synthetic Cp(T)=1000+20*T; not measured textile data",
                "points":[{"temperature_c":0.,"mean":1000.},{"temperature_c":60.,"mean":2200.}]}});
    }
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
    let law = ThermalLaw::from_material(material.material(), true).unwrap();
    let c = law.capacity_at(20.).unwrap() / cells as f64;
    let r = material.thermal_resistance().unwrap() / cells as f64;
    let column = if variable {
        json!({"schema":"ftx-dry-thermal-model/2","layers":[material],
            "cells_per_layer":cells,"interface_resistance_m2_k_w":[],
            "temperature_domain_c":[0.,60.]})
    } else {
        json!({"schema":"ftx-dry-thermal-model/1","layers":[material],
        "cells_per_layer":cells,"interface_resistance_m2_k_w":[],
        "cell_capacity_j_m2_k":vec![c;cells],"cell_resistance_m2_k_w":vec![r;cells]})
    };
    let mut model = json!({"schema":"ftx-thermal-surface/1","vertex_count":6,
        "columns":[column],"patches":patches});
    model["sha256"] = json!(hash(&model).unwrap());
    let material_report =
        json!({"basis":"characterized_record","record_sha256":material.sha256(),"record":material});
    let mut frame = json!({"schema":"ftx-drape-thermal-surface/1","available":true,
        "model":model,"instances":["front","back"],"triangle_instance":[0,1],
        "positions_m":positions,"material_report_sha256":hash(&material_report).unwrap()});
    frame["sha256"] = json!(hash(&frame).unwrap());
    let simulation = json!({"positions_m":positions,"positions_sha256":hash(&positions).unwrap(),
        "triangles":motion["triangles"],
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
    heat["cell_steps"] = json!(if variable { 1032 } else { 8 });
    if variable {
        heat["request"]["max_cell_steps"] = json!(2000);
    }
    let storage = if variable {
        // Independent integral: two .001 m² patches, .2 kg/m², Cp=1000+20*T, from 20 to 21 C.
        2. * 0.001 * 0.2 * (1000. + 10. * (21_f64.powi(2) - 20_f64.powi(2)))
    } else {
        2. * 0.001 * c * cells as f64
    };
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

fn with_alternative_qualifications(descriptor: &mut Value, motion: &mut Value) {
    let heat = &mut motion["transient_surface"]["result"];
    let selected = heat["request"]
        .as_object_mut()
        .unwrap()
        .remove("surface_water")
        .unwrap();
    let mut other = selected.clone();
    for binding in other["bindings"].as_object_mut().unwrap().values_mut() {
        binding["material_sha256"] = json!("a".repeat(64));
    }
    other["source"]["reference"] = json!("test:separate-material-qualification");
    other["qualification"] = json!("Synthetic independent applicability to replacement textiles");
    other["maximum_liquid_loading_kg_m2"] =
        json!(2. * selected["maximum_liquid_loading_kg_m2"].as_f64().unwrap());
    for phase in ["supply", "condensation", "drainage", "vapor_reservoir"] {
        if !other[phase].is_null() {
            other[phase]["source"]["reference"] = json!(format!("test:other-material-{phase}"));
        }
    }
    heat["surface_water_selection"] = json!({"index":1,"request_sha256":hash(&selected).unwrap()});
    heat["request"]["surface_water_alternatives"] = json!([other, selected]);
    descriptor["scenario"]["motion"]["transient_surface"] = heat["request"].clone();
}

fn instance_initial_case(variable: bool, starts: [f64; 2]) -> (Alternative, Value, Value, Value) {
    let (a, mut d, s, mut m) = case_with_variable_heat(variable);
    let h = &mut m["transient_surface"]["result"];
    // Both physical owners share one material column. Their unequal water masses make the
    // initial liquid enthalpy different from either the scalar fallback or an unweighted mean.
    let masses = [1e-5 * 0.001, 5e-6 * 0.001];
    let initial = masses.iter().sum::<f64>();
    let sensible = masses[0] * 4200. * starts[0] + masses[1] * 4200. * starts[1];
    let storage = if variable {
        // Independent integral of synthetic Cp(T)=1000+20*T over each retained reference mass.
        starts
            .iter()
            .map(|&t| 0.001 * 0.2 * (1000. * (21. - t) + 10. * (21_f64.powi(2) - t.powi(2))))
            .sum::<f64>()
    } else {
        let capacity = h["model"]["columns"][0]["cell_capacity_j_m2_k"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c.as_f64().unwrap())
            .sum::<f64>();
        starts.iter().map(|t| 0.001 * capacity * (21. - t)).sum()
    };
    h["request"]["initial_temperature_c"] = json!(100.);
    h["initial_temperature_c"] = json!(100.);
    h["request"]["initial_instance_temperatures_c"] = json!({"front":starts[0],"back":starts[1]});
    h["initial_condition"] = json!({"schema":"ftx-motion-surface-initial-condition/1",
        "model_sha256":h["model"]["sha256"],"instances":s["drape"]["thermal_surface"]["instances"],
        "triangle_instance":s["drape"]["thermal_surface"]["triangle_instance"],
        "instance_temperature_c":starts,
        "temperature_range_c":[starts[0].min(starts[1]),starts[0].max(starts[1])]});
    h["request"]["surface_water"]["bindings"]["back"]["initial_liquid_kg_m2"] = json!(5e-6);
    h["surface_water"]["request"] = h["request"]["surface_water"].clone();
    for (index, mass) in masses.into_iter().enumerate() {
        h["surface_water"]["patches"][index]["initial_liquid_kg"] = json!(mass);
        h["surface_water"]["patches"][index]["evaporated_liquid_kg"] = json!(mass);
    }
    for (field, value) in [
        ("initial_liquid_kg", initial),
        ("evaporated_liquid_kg", initial),
        ("latent_energy_j", initial * 2500000.),
        ("exported_vapor_sensible_energy_j", sensible),
        ("liquid_stored_change_j", -sensible),
    ] {
        h["surface_water"][field] = json!(value);
        h["trace"][0]["surface_water"][field] = json!(value);
    }
    h["surface_water"]["dryout_events"] = json!(2);
    h["trace"][0]["surface_water"]["dryout_events"] = json!(2);
    let net = storage + initial * 2500000. - 3.;
    for (field, value) in [
        ("stored_change_j", storage),
        ("outside_energy_j", -net),
        ("net_boundary_energy_j", net),
    ] {
        h[field] = json!(value);
        h["trace"][0][field] = json!(value);
    }
    d["scenario"]["motion"]["transient_surface"] = h["request"].clone();
    (a, d, s, m)
}

#[test]
fn independent_wet_starts_preserve_physical_owners_and_integrated_enthalpy() {
    for variable in [false, true] {
        for alternative in [false, true] {
            let (a, mut d, s, mut m) = instance_initial_case(variable, [10., 30.]);
            if alternative {
                with_alternative_qualifications(&mut d, &mut m);
            }
            let h = admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).unwrap_or_else(|r| {
                panic!("variable={variable}, alternative={alternative}: {r:?}")
            });
            assert_eq!(h.range_c, [10., 30.]);
            assert_eq!(h.final_range_c, [21., 21.]);
            if variable {
                assert!((h.result["stored_change_j"].as_f64().unwrap() - 0.164).abs() < 1e-14);
            }
            assert!(
                (h.result["surface_water"]["liquid_stored_change_j"]
                    .as_f64()
                    .unwrap()
                    + 0.00105)
                    .abs()
                    < 1e-16
            );
        }
    }
}

#[test]
fn wet_starts_follow_retained_names_and_apply_partial_fallbacks() {
    let (a, mut d, mut s, mut m) = instance_initial_case(true, [10., 30.]);
    let h = &mut m["transient_surface"]["result"];
    // Reordering the retained names and their owner indices preserves the same physical starts.
    let frame = &mut s["drape"]["thermal_surface"];
    frame["instances"] = json!(["back", "front"]);
    frame["triangle_instance"] = json!([1, 0]);
    frame.as_object_mut().unwrap().remove("sha256");
    frame["sha256"] = json!(hash(frame).unwrap());
    h["initial_condition"]["instances"] = frame["instances"].clone();
    h["initial_condition"]["triangle_instance"] = frame["triangle_instance"].clone();
    h["initial_condition"]["instance_temperature_c"] = json!([30., 10.]);
    h["request"]["initial_temperature_c"] = json!(30.);
    h["initial_temperature_c"] = json!(30.);
    h["request"]["initial_instance_temperatures_c"] = json!({"front":10.});
    d["scenario"]["motion"]["transient_surface"] = h["request"].clone();
    let admitted =
        admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).unwrap_or_else(|r| panic!("{r:?}"));
    assert_eq!(admitted.range_c, [10., 30.]);
}

#[test]
fn wet_initial_receipts_refuse_changed_maps_owners_and_applied_domains() {
    for fault in 0..12 {
        // Fault 6 keeps the full liquid/dry ledger and initial receipt consistent at 61 C;
        // the characterized dry law itself must refuse that applied state.
        let starts = if fault == 6 { [10., 61.] } else { [10., 30.] };
        let (a, mut d, mut s, mut m) = instance_initial_case(true, starts);
        let h = &mut m["transient_surface"]["result"];
        match fault {
            0 => h["initial_condition"] = Value::Null,
            1 => h["initial_condition"]["instance_temperature_c"] = json!([30., 10.]),
            2 => h["initial_condition"]["triangle_instance"] = json!([1, 0]),
            3 => h["initial_condition"]["model_sha256"] = json!("f".repeat(64)),
            4 => h["request"]["initial_instance_temperatures_c"]["unknown"] = json!(20.),
            5 => {
                h["request"]["initial_instance_temperatures_c"]
                    .as_object_mut()
                    .unwrap()
                    .remove("back");
            }
            6 => {}
            7 => {
                // Water pressure must qualify every applied start.
                h["request"]["surface_water"]["wet_vapor_pressure_pa"] =
                    json!([[15., 0.], [80., 10000.]]);
                h["surface_water"]["request"] = h["request"]["surface_water"].clone();
            }
            8 => {
                // Only the warm owner exceeds ambient; a global maximum cannot qualify both.
                h["request"]["surface_water"]["outside_vapor_pressure_pa"] = json!([4000.]);
                h["surface_water"]["request"] = h["request"]["surface_water"].clone();
            }
            9 => {
                let frame = &mut s["drape"]["thermal_surface"];
                frame["triangle_instance"] = json!([1, 0]);
                frame.as_object_mut().unwrap().remove("sha256");
                frame["sha256"] = json!(hash(frame).unwrap());
            }
            10 => h["request"]["mechanical_coupling"] = json!({}),
            _ => h["request"]["coupled_water_alternatives"] = json!([]),
        }
        d["scenario"]["motion"]["transient_surface"] = h["request"].clone();
        assert!(
            admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_err(),
            "fault {fault}"
        );
    }
}

#[test]
fn wet_initial_enthalpy_cannot_be_replaced_by_a_scalar_even_with_balanced_receipts() {
    for trace_only in [false, true] {
        for dry in [false, true] {
            let (a, d, s, mut m) = instance_initial_case(true, [10., 30.]);
            let h = &mut m["transient_surface"]["result"];
            let row = if trace_only { &mut h["trace"][0] } else { h };
            if dry {
                // Uniform 20 C has the same unweighted mean as the authored 10/30 C starts,
                // but a different integral for temperature-dependent heat capacity.
                let wrong = 0.564;
                let increment = wrong - row["stored_change_j"].as_f64().unwrap();
                row["stored_change_j"] = json!(wrong);
                row["net_boundary_energy_j"] =
                    json!(row["net_boundary_energy_j"].as_f64().unwrap() + increment);
                row["outside_energy_j"] =
                    json!(row["outside_energy_j"].as_f64().unwrap() - increment);
            } else {
                // Equal and opposite forged liquid/vapor energies leave the total balance
                // intact. Their initial inventory still requires the mass-weighted starts.
                let wrong =
                    row["surface_water"]["initial_liquid_kg"].as_f64().unwrap() * 4200. * 20.;
                row["surface_water"]["liquid_stored_change_j"] = json!(-wrong);
                row["surface_water"]["exported_vapor_sensible_energy_j"] = json!(wrong);
            }
            assert!(
                admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_err(),
                "trace_only={trace_only}, dry={dry}"
            );
        }
    }
}

#[test]
fn alternative_water_sources_admit_the_exact_selected_policy_through_every_independent_phase() {
    let (drain_a, mut drain_d, drain_s, mut drain_m) = draining_case(true);
    drain_m.as_object_mut().unwrap().remove("liquid_mechanics");
    drain_m["initial_sample"]
        .as_object_mut()
        .unwrap()
        .remove("liquid_mechanics");
    let drain_heat = &mut drain_m["transient_surface"]["result"];
    drain_heat["request"]
        .as_object_mut()
        .unwrap()
        .remove("mechanical_coupling");
    drain_heat
        .as_object_mut()
        .unwrap()
        .remove("mechanical_coupling");
    for sample in drain_heat["trace"].as_array_mut().unwrap() {
        sample
            .as_object_mut()
            .unwrap()
            .remove("mechanical_coupling");
    }
    drain_d["scenario"]["motion"]["transient_surface"] = drain_heat["request"].clone();
    for (a, mut d, s, mut m) in [
        case(),
        supplied_case(),
        condensing_case(),
        (drain_a, drain_d, drain_s, drain_m),
        finite_reservoir_case(false, false),
    ] {
        let original_water = m["transient_surface"]["result"]["surface_water"].clone();
        with_alternative_qualifications(&mut d, &mut m);
        assert!(admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_ok());
        assert_eq!(
            m["transient_surface"]["result"]["surface_water"],
            original_water
        );
        let heat = &m["transient_surface"]["result"];
        assert_eq!(
            selected_policy(&heat["request"], heat, &s).unwrap().0,
            &original_water["request"]
        );
    }
}

#[test]
fn alternative_water_source_selection_rejects_changed_sources_and_invented_receipts() {
    let (a, mut d, s, mut m) = case();
    with_alternative_qualifications(&mut d, &mut m);
    for change in [
        "index",
        "digest",
        "missing",
        "duplicate",
        "unqualified",
        "case",
        "source",
        "partial",
        "single",
        "empty",
        "budget",
    ] {
        let mut changed = m.clone();
        let mut descriptor = d.clone();
        let heat = &mut changed["transient_surface"]["result"];
        match change {
            "index" => heat["surface_water_selection"]["index"] = json!(0),
            "digest" => heat["surface_water_selection"]["request_sha256"] = json!("b".repeat(64)),
            "missing" => {
                heat.as_object_mut()
                    .unwrap()
                    .remove("surface_water_selection");
            }
            "duplicate" => {
                heat["request"]["surface_water_alternatives"][0]["bindings"] =
                    heat["request"]["surface_water_alternatives"][1]["bindings"].clone()
            }
            "unqualified" => {
                heat["request"]["surface_water_alternatives"][0]["allow_estimates"] = json!(false)
            }
            "case" => {
                heat["request"]["surface_water_alternatives"][0]["outside_vapor_pressure_pa"] =
                    json!([999.])
            }
            "source" => {
                // Updating the selection digest cannot silently replace the qualified source
                // in the water receipt: both must still name the exact original policy.
                heat["request"]["surface_water_alternatives"][1]["source"]["reference"] =
                    json!("test:changed-source");
                heat["surface_water_selection"]["request_sha256"] =
                    json!(hash(&heat["request"]["surface_water_alternatives"][1]).unwrap());
            }
            "partial" => {
                heat["request"]["surface_water_alternatives"][1]["bindings"]["back"]["material_sha256"] =
                    json!("a".repeat(64))
            }
            "single" => heat["request"]["surface_water"] = heat["surface_water"]["request"].clone(),
            "empty" => heat["request"]["surface_water_alternatives"] = json!([]),
            "budget" => {
                heat["request"]["surface_water_alternatives"] =
                    json!(vec![heat["surface_water"]["request"].clone(); 65])
            }
            _ => unreachable!(),
        }
        descriptor["scenario"]["motion"]["transient_surface"] = heat["request"].clone();
        assert!(
            admit_heat(&a, &descriptor, &s, &changed, Consumer::TransientSurface).is_err(),
            "{change}"
        );
    }
    let (a, d, s, mut legacy) = case();
    legacy["transient_surface"]["result"]["surface_water_selection"] =
        json!({"index":0,"request_sha256":"a".repeat(64)});
    assert!(admit_heat(&a, &d, &s, &legacy, Consumer::TransientSurface).is_err());
}

#[test]
fn alternative_water_sources_cannot_change_phase_boundaries_or_borrow_mechanical_qualification() {
    let (a, mut d, s, mut m) = supplied_case();
    with_alternative_qualifications(&mut d, &mut m);
    for field in ["rate_kg_m2_s", "temperature_c"] {
        let mut changed = m.clone();
        let mut descriptor = d.clone();
        let heat = &mut changed["transient_surface"]["result"];
        heat["request"]["surface_water_alternatives"][0]["supply"]["bindings"]["front"][field] =
            json!([1.]);
        descriptor["scenario"]["motion"]["transient_surface"] = heat["request"].clone();
        assert!(admit_heat(&a, &descriptor, &s, &changed, Consumer::TransientSurface).is_err());
    }
    let (a, mut d, s, mut m) = coupled_case();
    with_alternative_qualifications(&mut d, &mut m);
    assert!(admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_err());
}

#[test]
fn unselected_water_qualifications_cannot_hide_invalid_metadata_during_case_comparison() {
    let (a, mut d, s, mut m) = supplied_case();
    with_alternative_qualifications(&mut d, &mut m);
    for path in ["", "/supply"] {
        for value in [Value::Null, json!("true"), json!(1)] {
            let mut changed = m.clone();
            let mut descriptor = d.clone();
            let heat = &mut changed["transient_surface"]["result"];
            let policy = &mut heat["request"]["surface_water_alternatives"][0];
            let metadata = policy.pointer_mut(path).unwrap();
            metadata["evidence"] = json!("published");
            metadata["allow_estimates"] = value;
            descriptor["scenario"]["motion"]["transient_surface"] = heat["request"].clone();
            assert!(admit_heat(&a, &descriptor, &s, &changed, Consumer::TransientSurface).is_err());
        }
        let mut changed = m.clone();
        let mut descriptor = d.clone();
        let heat = &mut changed["transient_surface"]["result"];
        heat["request"]["surface_water_alternatives"][0]
            .pointer_mut(path)
            .unwrap()["source"]["unrecognized"] = json!("not a Source field");
        descriptor["scenario"]["motion"]["transient_surface"] = heat["request"].clone();
        assert!(admit_heat(&a, &descriptor, &s, &changed, Consumer::TransientSurface).is_err());
    }
    for extra in [
        Value::Null,
        json!("supply has no qualification field in its schema"),
    ] {
        let mut changed = m.clone();
        let mut descriptor = d.clone();
        let heat = &mut changed["transient_surface"]["result"];
        heat["request"]["surface_water_alternatives"][0]["supply"]["qualification"] = extra;
        descriptor["scenario"]["motion"]["transient_surface"] = heat["request"].clone();
        assert!(admit_heat(&a, &descriptor, &s, &changed, Consumer::TransientSurface).is_err());
    }
}

#[test]
fn variable_water_histories_admit_integrated_dry_storage_not_endpoint_capacity() {
    let (a, d, s, m) = case_with_variable_heat(true);
    assert!(admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_ok());
    assert!(
        (m["transient_surface"]["result"]["stored_change_j"]
            .as_f64()
            .unwrap()
            - 0.564)
            .abs()
            < 1e-14
    );

    for trace_only in [false, true] {
        let mut changed = m.clone();
        let heat = &mut changed["transient_surface"]["result"];
        // Reconcile the forged boundary ledger too: a generic conservation check would accept
        // endpoint Cp(21)*delta instead of the source's integrated dry enthalpy.
        let wrong = 2. * 0.001 * 0.2 * (1000. + 20. * 21.);
        let row = if trace_only {
            &mut heat["trace"][0]
        } else {
            heat
        };
        let increment = wrong - row["stored_change_j"].as_f64().unwrap();
        row["stored_change_j"] = json!(wrong);
        row["net_boundary_energy_j"] =
            json!(row["net_boundary_energy_j"].as_f64().unwrap() + increment);
        row["outside_energy_j"] = json!(row["outside_energy_j"].as_f64().unwrap() - increment);
        assert!(admit_heat(&a, &d, &s, &changed, Consumer::TransientSurface).is_err());
    }
}

#[test]
fn variable_water_temperature_receipts_cannot_extrapolate_a_material_law() {
    let (a, d, s, mut m) = case_with_variable_heat(true);
    m["transient_surface"]["result"]["final"]["temperature_c"] = json!([[61., 61.], [61., 61.]]);
    m["transient_surface"]["result"]["trace"][0]["temperature_range_c"] = json!([61., 61.]);
    assert!(admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_err());
}

#[test]
fn textile_water_and_maxwell_heat_share_one_conserved_decision_ledger() {
    let (a, d, s, m) = case();
    let heat =
        admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).unwrap_or_else(|r| panic!("{r:?}"));
    let admitted = Numerical {
        cutting_program: None,
        finite_stock_program: None,
        cutting_job_program: None,
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

fn coupled_case() -> (Alternative, Value, Value, Value) {
    with_coupled_mechanics(case())
}

fn with_coupled_mechanics(
    (a, mut d, mut s, mut m): (Alternative, Value, Value, Value),
) -> (Alternative, Value, Value, Value) {
    let h = &mut m["transient_surface"]["result"];
    let mut bindings = serde_json::Map::new();
    for (id, binding) in h["request"]["surface_water"]["bindings"]
        .as_object()
        .unwrap()
    {
        bindings.insert(
            id.clone(),
            json!({"material_sha256":binding["material_sha256"],
            "maximum_liquid_loading_kg_m2":1e-5,"temperature_range_c":[-20.,80.]}),
        );
    }
    let q = json!({"schema":"ftx-evaporating-liquid-mechanics/1",
        "model":"co_moving_evaporation_frozen_mechanics/1",
        "material_report_sha256":hash(&s["drape"]["material"]).unwrap(),"bindings":bindings,
        "source":{"reference":"test:coupled-mass-admission","accessed_on":"2026-10-28",
            "note":"Synthetic accounting only; no physical textile qualification"},
        "evidence":"estimate","allow_estimates":true,"method":"Synthetic co-moving outflow",
        "qualification":"Explicit synthetic frozen mechanics over the full declared liquid domain"});
    let initial = h["surface_water"]["initial_liquid_kg"].as_f64().unwrap();
    let substrate = 0.0004;
    s["drape"]["mass_kg"] = json!(substrate);
    let mut final_report = json!({"schema":"ftx-evaporating-liquid-mechanics-result/1",
        "qualification":q,"scope":"Synthetic accounting fixture",
        "substrate_and_attached_mass_kg":substrate,"initial_liquid_mass_kg":initial,
        "remaining_liquid_mass_kg":initial,"evaporated_liquid_mass_kg":0.,
        "total_inertial_mass_kg":substrate+initial,"mass_balance_residual_kg":0.,
        "exported_momentum_kg_m_s":[0.,0.,0.],"exported_kinetic_energy_j":0.,
        "exported_gravitational_energy_j":0.,"transfer_momentum_residual_kg_m_s":[0.,0.,0.],
        "transfer_kinetic_residual_j":0.,"transfer_gravitational_residual_j":0.});
    let initial_report = final_report.clone();
    final_report["remaining_liquid_mass_kg"] = json!(0.);
    final_report["evaporated_liquid_mass_kg"] = json!(initial);
    final_report["total_inertial_mass_kg"] = json!(substrate);
    final_report["exported_momentum_kg_m_s"] = json!([initial, 0., 0.]);
    final_report["exported_kinetic_energy_j"] = json!(0.5 * initial);
    final_report["exported_gravitational_energy_j"] = json!(3. * initial);
    h["request"]["mechanical_coupling"] = q;
    h["mechanical_coupling"] = final_report.clone();
    h["trace"][0]["mechanical_coupling"] = json!({
        "substrate_and_attached_mass_kg":substrate,"initial_liquid_kg":initial,
        "remaining_liquid_kg":0.,"evaporated_liquid_kg":initial,
        "total_inertial_mass_kg":substrate,"mass_balance_residual_kg":0.,
        "exported_momentum_kg_m_s":[initial,0.,0.],"exported_kinetic_energy_j":0.5*initial,
        "exported_gravitational_energy_j":3.*initial,"transfer_momentum_residual_kg_m_s":[0.,0.,0.],
        "transfer_kinetic_residual_j":0.,"transfer_gravitational_residual_j":0.});
    d["scenario"]["motion"]["transient_surface"] = h["request"].clone();
    m["initial_sample"]["liquid_mechanics"] = initial_report;
    m["liquid_mechanics"] = final_report;
    (a, d, s, m)
}

#[test]
fn coupled_wet_initial_states_require_exact_mapped_sources_for_direct_and_paired_decisions() {
    for variable in [false, true] {
        for paired in [false, true] {
            let (a, mut d, s, mut m) =
                with_coupled_mechanics(instance_initial_case(variable, [10., 30.]));
            if paired {
                with_paired_qualifications(&mut d, &mut m);
            }
            let admitted = admit_heat(&a, &d, &s, &m, Consumer::TransientSurface)
                .unwrap_or_else(|r| panic!("variable={variable}, paired={paired}: {r:?}"));
            assert_eq!(admitted.range_c, [10., 30.]);
            assert_eq!(admitted.final_range_c, [21., 21.]);
            assert_eq!(admitted.result["initial_temperature_c"], 100.);
            assert_eq!(
                admitted.result["mechanical_coupling"],
                m["liquid_mechanics"]
            );
            for fault in 0..6 {
                let (a, mut d, s, mut m) =
                    with_coupled_mechanics(instance_initial_case(variable, [10., 30.]));
                if fault < 4 {
                    let h = &mut m["transient_surface"]["result"];
                    match fault {
                        0 => h["initial_condition"] = Value::Null,
                        1 => h["initial_condition"]["triangle_instance"] = json!([1, 0]),
                        2 => h["initial_condition"]["instance_temperature_c"] = json!([30., 10.]),
                        _ => h["request"]["initial_instance_temperatures_c"]["front"] = json!(12.),
                    }
                    d["scenario"]["motion"]["transient_surface"] = h["request"].clone();
                } else {
                    let mut qualification = m["liquid_mechanics"]["qualification"].clone();
                    if fault == 4 {
                        qualification["material_report_sha256"] = json!("f".repeat(64));
                    } else {
                        // Covering the initial values alone cannot qualify the full water domain.
                        qualification["bindings"]["front"]["temperature_range_c"] =
                            json!([10., 30.]);
                    }
                    replace_mechanical_qualification(&mut d, &mut m, qualification);
                }
                if paired {
                    with_paired_qualifications(&mut d, &mut m);
                }
                assert!(
                    admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_err(),
                    "variable={variable}, paired={paired}, fault={fault}"
                );
            }
        }
    }
}

fn with_paired_qualifications(descriptor: &mut Value, motion: &mut Value) {
    let heat = &mut motion["transient_surface"]["result"];
    let request = heat["request"].as_object_mut().unwrap();
    let water = request.remove("surface_water").unwrap();
    let mechanics = request.remove("mechanical_coupling").unwrap();
    let selected = json!({"surface_water":water,"mechanical_coupling":mechanics});
    let mut other = selected.clone();
    other["surface_water"]["maximum_liquid_loading_kg_m2"] = json!(
        2. * selected["surface_water"]["maximum_liquid_loading_kg_m2"]
            .as_f64()
            .unwrap()
    );
    other["surface_water"]["source"]["reference"] = json!("test:other-paired-water");
    for binding in other["surface_water"]["bindings"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        binding["material_sha256"] = json!("a".repeat(64));
    }
    other["mechanical_coupling"]["material_report_sha256"] = json!("b".repeat(64));
    other["mechanical_coupling"]["source"]["reference"] = json!("test:other-paired-mechanics");
    for binding in other["mechanical_coupling"]["bindings"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        binding["material_sha256"] = json!("a".repeat(64));
        binding["maximum_liquid_loading_kg_m2"] = json!(
            3. * selected["surface_water"]["maximum_liquid_loading_kg_m2"]
                .as_f64()
                .unwrap()
        );
        binding["temperature_range_c"] = json!([-30., 90.]);
    }
    heat["coupled_water_selection"] = json!({"index":1,"pair_sha256":hash(&selected).unwrap()});
    heat["request"]["coupled_water_alternatives"] = json!([other, selected]);
    descriptor["scenario"]["motion"]["transient_surface"] = heat["request"].clone();
}

#[test]
fn paired_water_sources_preserve_every_existing_phase_and_mechanical_ledger() {
    let mut cases = vec![
        coupled_case(),
        exchanging_case(false),
        exchanging_case(true),
        draining_case(false),
        draining_case(true),
        finite_reservoir_case(false, true),
        finite_reservoir_case(true, true),
    ];
    for model in 2..=6 {
        let (a, mut d, s, mut m) = draining_case(true);
        match model {
            2 => prescribe_incoming_velocity(&mut d, &mut m),
            3 => schedule_incoming_velocity(&mut d, &mut m),
            4 => qualify_incoming_velocities(&mut d, &mut m, true),
            5 => schedule_phase_velocities(&mut d, &mut m, false),
            _ => schedule_phase_velocities(&mut d, &mut m, true),
        }
        cases.push((a, d, s, m));
    }
    for (a, mut d, s, mut m) in cases {
        assert!(admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_ok());
        let original_water = m["transient_surface"]["result"]["surface_water"].clone();
        let original_mechanics = m["liquid_mechanics"].clone();
        let original_trace = m["transient_surface"]["result"]["trace"].clone();
        with_paired_qualifications(&mut d, &mut m);
        assert!(
            admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_ok(),
            "water {}, mechanics {}",
            original_water["request"]["schema"],
            original_mechanics["qualification"]["schema"]
        );
        let heat = &m["transient_surface"]["result"];
        assert_eq!(heat["surface_water"], original_water);
        assert_eq!(heat["mechanical_coupling"], original_mechanics);
        assert_eq!(heat["trace"], original_trace);
        assert_eq!(
            selected_policy(&heat["request"], heat, &s).unwrap(),
            (
                &original_water["request"],
                &original_mechanics["qualification"]
            )
        );
    }
}

#[test]
fn paired_water_sources_refuse_mixed_domains_operating_inputs_and_forged_receipts() {
    let (a, mut d, s, mut m) = draining_case(true);
    schedule_phase_velocities(&mut d, &mut m, true);
    with_paired_qualifications(&mut d, &mut m);
    for fault in [
        "index",
        "digest",
        "missing-receipt",
        "wrong-receipt-kind",
        "duplicate",
        "empty",
        "too-many",
        "extra-pair-field",
        "partial-map",
        "mixed-materials",
        "loading-domain",
        "temperature-domain",
        "water-loading",
        "incoming-velocity",
        "outgoing-velocity",
        "mechanical-model",
        "selected-report",
        "unselected-report",
        "estimate-consent",
        "consent-type",
        "extra-source-field",
        "water-source",
        "mechanical-source",
        "single-water",
        "single-mechanics",
        "uncoupled-alternatives",
    ] {
        let mut changed = m.clone();
        let heat = &mut changed["transient_surface"]["result"];
        match fault {
            "index" => heat["coupled_water_selection"]["index"] = json!(0),
            "digest" => heat["coupled_water_selection"]["pair_sha256"] = json!("c".repeat(64)),
            "missing-receipt" => {
                heat.as_object_mut()
                    .unwrap()
                    .remove("coupled_water_selection");
            }
            "wrong-receipt-kind" => heat["surface_water_selection"] = Value::Null,
            "empty" => heat["request"]["coupled_water_alternatives"] = json!([]),
            "too-many" => {
                heat["request"]["coupled_water_alternatives"] = json!(vec![
                        heat["request"]["coupled_water_alternatives"][1].clone();
                        65
                    ]);
            }
            "single-water" => {
                heat["request"]["surface_water"] = heat["surface_water"]["request"].clone();
            }
            "single-mechanics" => {
                heat["request"]["mechanical_coupling"] =
                    heat["mechanical_coupling"]["qualification"].clone();
            }
            "uncoupled-alternatives" => {
                heat["request"]["surface_water_alternatives"] =
                    json!([heat["surface_water"]["request"]]);
            }
            _ => {
                let pairs = &mut heat["request"]["coupled_water_alternatives"];
                match fault {
                    "duplicate" => pairs[0] = pairs[1].clone(),
                    "extra-pair-field" => pairs[0]["unknown"] = json!(true),
                    "partial-map" => {
                        pairs[0]["mechanical_coupling"]["bindings"]
                            .as_object_mut()
                            .unwrap()
                            .remove("back");
                    }
                    "mixed-materials" => {
                        pairs[0]["mechanical_coupling"]["bindings"]["front"]["material_sha256"] =
                            json!("e".repeat(64));
                    }
                    "loading-domain" => {
                        let binding = &mut pairs[0]["mechanical_coupling"]["bindings"]["front"];
                        binding["maximum_liquid_loading_kg_m2"] = json!(1e-99);
                    }
                    "temperature-domain" => {
                        let binding = &mut pairs[0]["mechanical_coupling"]["bindings"]["front"];
                        binding["temperature_range_c"] = json!([0., 80.]);
                    }
                    "water-loading" => {
                        let binding = &mut pairs[0]["surface_water"]["bindings"]["front"];
                        binding["initial_liquid_kg_m2"] = json!(0.);
                    }
                    "incoming-velocity" => {
                        let binding = &mut pairs[0]["mechanical_coupling"]["bindings"]["front"];
                        binding["incoming_stream_velocities_world_m_s_by_interval"][0]["supply"] =
                            json!([0., 1., 0.]);
                    }
                    "outgoing-velocity" => {
                        let binding = &mut pairs[0]["mechanical_coupling"]["bindings"]["front"];
                        let schedule = &mut binding["outgoing_stream_velocities_relative_to_cloth_m_s_by_interval"];
                        schedule[0]["drainage"] = json!([0., 0., 1.]);
                    }
                    "mechanical-model" => {
                        pairs[0]["mechanical_coupling"]["model"] = json!("unsupported");
                    }
                    "selected-report" => {
                        pairs[1]["mechanical_coupling"]["material_report_sha256"] =
                            json!("e".repeat(64));
                    }
                    "unselected-report" => {
                        pairs[0]["mechanical_coupling"]["material_report_sha256"] =
                            json!("invalid");
                    }
                    "estimate-consent" => {
                        pairs[0]["mechanical_coupling"]["allow_estimates"] = json!(false);
                    }
                    "consent-type" => {
                        pairs[0]["mechanical_coupling"]["evidence"] = json!("published");
                        pairs[0]["mechanical_coupling"]["allow_estimates"] = json!("invalid");
                    }
                    "extra-source-field" => {
                        pairs[0]["mechanical_coupling"]["source"]["unknown"] = json!(true);
                    }
                    "water-source" => {
                        pairs[1]["surface_water"]["source"]["reference"] =
                            json!("test:changed-water");
                    }
                    "mechanical-source" => {
                        pairs[1]["mechanical_coupling"]["source"]["reference"] =
                            json!("test:changed-mechanics");
                    }
                    _ => unreachable!(),
                }
                heat["coupled_water_selection"]["pair_sha256"] = json!(hash(&pairs[1]).unwrap());
            }
        }
        let mut descriptor = d.clone();
        descriptor["scenario"]["motion"]["transient_surface"] = heat["request"].clone();
        assert!(
            admit_heat(&a, &descriptor, &s, &changed, Consumer::TransientSurface).is_err(),
            "fault {fault}"
        );
    }
}

#[test]
fn coupled_motion_protocol_includes_thermal_and_vapor_methods_without_changing_static_protocols() {
    for coupling in ["none", "direct", "paired"] {
        let (mut a, mut d, s, mut m) = if coupling == "none" {
            case()
        } else {
            coupled_case()
        };
        if coupling == "paired" {
            with_paired_qualifications(&mut d, &mut m);
        }
        let admitted = Numerical {
            cutting_program: None,
            finite_stock_program: None,
            cutting_job_program: None,
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
            transient_surface: None,
            transient_coverage: None,
        };
        let selection = a.frozen_changes.as_ref().unwrap().default.clone().unwrap();
        for (metric, moving) in [
            ("motion_max_absolute_edge_strain_pct", true),
            ("max_absolute_edge_strain_pct", false),
            ("assembly_mass_kg", false),
        ] {
            let policy: Policy = serde_json::from_value(json!({"basis":"nominal",
                "objectives":[{"metric":metric,"direction":"minimize"}]}))
            .unwrap();
            let original = policy.protocol(&a, &d, &admitted).unwrap();
            for property in [
                "vapor_resistance",
                "specific_heat",
                "thermal_resistance",
                "membrane_relaxation_heat_fraction",
            ] {
                // Project qualifiers only; altered selections are not admitted as valid receipts.
                let mut raw = serde_json::to_value(&selection).unwrap();
                raw["record"]["properties"][property]["method"] = json!("Different test method");
                a.frozen_changes.as_mut().unwrap().default =
                    Some(serde_json::from_value(raw).unwrap());
                let changed = policy.protocol(&a, &d, &admitted).unwrap();
                assert_eq!(
                    original != changed,
                    moving && coupling != "none",
                    "{coupling}: {metric}: {property}"
                );
                a.frozen_changes.as_mut().unwrap().default = Some(selection.clone());
            }
            let mut changed = d.clone();
            let heat = &mut changed["scenario"]["motion"]["transient_surface"];
            let water = if coupling == "paired" {
                &mut heat["coupled_water_alternatives"][1]["surface_water"]
            } else {
                &mut heat["surface_water"]
            };
            water["source"]["reference"] = json!("test:changed-water-qualification");
            assert_eq!(
                original != policy.protocol(&a, &changed, &admitted).unwrap(),
                moving && coupling != "none",
                "{coupling}: {metric}: water source"
            );
        }
    }
}

#[test]
fn direct_and_paired_motion_require_complete_heat_and_liquid_receipts() {
    let moving: Policy = serde_json::from_value(json!({"basis":"nominal",
        "objectives":[{"metric":"motion_max_absolute_edge_strain_pct","direction":"minimize"}]}))
    .unwrap();
    let stationary: Policy = serde_json::from_value(json!({"basis":"nominal",
        "objectives":[{"metric":"max_absolute_edge_strain_pct","direction":"minimize"}]}))
    .unwrap();
    for (paired, mapped) in [(false, false), (true, false), (false, true), (true, true)] {
        let (a, mut d, mut s, mut m) = if mapped {
            with_coupled_mechanics(instance_initial_case(false, [10., 30.]))
        } else {
            coupled_case()
        };
        if paired {
            with_paired_qualifications(&mut d, &mut m);
        }
        // Complete only the enclosing trajectory metadata around the existing conservative
        // synthetic heat/liquid receipt. No numerical solver or physical fit claim is made here.
        d["scenario_sha256"] = json!("c".repeat(64));
        d["body_geometry_sha256"] = json!("d".repeat(64));
        d["motion_path"] = json!({"sha256":m["path_sha256"]});
        d["scenario"]["motion"]["schema"] = json!("ftx-drape-motion/1");
        d["scenario"]["motion"]["max_trials"] = json!(1);
        d["scenario"]["motion"]["keyframes"] = json!([{"time_s":1.,"arm_abduction_deg":35.}]);
        m["schema"] = json!("ftx-drape-motion-result/1");
        m["continuous_contact_checked"] = json!(true);
        m["initial_drape_settled"] = json!(true);
        m["initial_drape_gating"] = json!([]);
        m["trials"] = json!(1);
        m["body_triangles"] = json!([[0, 1, 2]]);
        m["samples"][0]["body_positions_m"] = json!([[0., 0., -1.], [1., 0., -1.], [0., 1., -1.]]);
        m["samples"][0]["solve"] = json!({"converged":true});
        m["metrics"] = json!({"max_absolute_edge_strain_pct":3.});
        m["keyframes"] = json!([{"sample_index":0,"time_s":1.,"arm_abduction_deg":35.,
            "positions_sha256":hash(&m["samples"][0]["positions_m"]).unwrap(),
            "body_collision_sha256":hash(&(&m["samples"][0]["body_positions_m"],
                &m["body_triangles"])).unwrap()}]);
        s["schema"] = json!("ftx-drape-scenario-result/1");
        s["pattern_sha256"] = a.binding.value().unwrap()["pattern_sha256"].clone();
        s["scenario_sha256"] = d["scenario_sha256"].clone();
        s["body_geometry_sha256"] = d["body_geometry_sha256"].clone();
        s["positions_sha256"] = m["initial_positions_sha256"].clone();
        s["drape"]["settled"] = json!(true);
        s["drape"]["gating"] = json!([]);
        s["drape"]["global_strain_pct"] = json!({"min":-2.,"max":3.});
        s["metrics"] = json!({"simulation_gating_clear":true});
        s["motion"] = json!({"status":"available","result":m});
        let admitted = moving.candidate(&a, None, &d, &s);
        assert!(
            admitted.eligible,
            "paired={paired}, mapped={mapped}: {:?}",
            admitted.exclusions
        );
        assert_eq!(
            admitted.readings[&Metric::MotionMaxAbsoluteEdgeStrainPct].value,
            3.
        );
        let static_before = stationary.candidate(&a, None, &d, &s);
        assert!(static_before.eligible, "{:?}", static_before.exclusions);
        for fault in [
            "unavailable_heat",
            "missing_heat",
            "missing_water",
            "missing_initial_mass",
            "missing_final_mass",
            "missing_sample_mass",
            "changed_water_source",
            "changed_mass",
            "changed_initial_receipt",
            "changed_heat_request",
        ] {
            let mut changed = s.clone();
            let motion = &mut changed["motion"]["result"];
            match fault {
                "unavailable_heat" => motion["transient_surface"]["status"] = json!("refused"),
                "missing_heat" => motion["transient_surface"] = Value::Null,
                "missing_water" => {
                    motion["transient_surface"]["result"]["surface_water"] = Value::Null;
                }
                "missing_initial_mass" => {
                    motion["initial_sample"]["liquid_mechanics"] = Value::Null;
                }
                "missing_final_mass" => motion["liquid_mechanics"] = Value::Null,
                "missing_sample_mass" => {
                    motion["transient_surface"]["result"]["trace"][0]["mechanical_coupling"] =
                        Value::Null;
                }
                "changed_water_source" => {
                    motion["transient_surface"]["result"]["surface_water"]["request"]["bindings"]
                        ["front"]["material_sha256"] = json!("f".repeat(64));
                }
                "changed_mass" => {
                    motion["liquid_mechanics"]["remaining_liquid_mass_kg"] = json!(1e-9);
                }
                "changed_initial_receipt" => {
                    motion["transient_surface"]["result"]["initial_condition"]["instance_temperature_c"] =
                        json!([30., 10.]);
                }
                _ => {
                    motion["transient_surface"]["result"]["request"]["initial_temperature_c"] =
                        json!(19.);
                }
            }
            let rejected = moving.candidate(&a, None, &d, &changed);
            assert!(!rejected.eligible, "paired={paired}: {fault}");
            assert!(
                !rejected
                    .readings
                    .contains_key(&Metric::MotionMaxAbsoluteEdgeStrainPct),
                "paired={paired}: {fault}"
            );
            assert_eq!(
                serde_json::to_value(stationary.candidate(&a, None, &d, &changed)).unwrap(),
                serde_json::to_value(&static_before).unwrap(),
                "paired={paired}: {fault}"
            );
        }
        let mut dry_descriptor = d.clone();
        dry_descriptor["scenario"]["motion"]
            .as_object_mut()
            .unwrap()
            .remove("transient_surface");
        let motion = &mut s["motion"]["result"];
        motion.as_object_mut().unwrap().remove("transient_surface");
        motion.as_object_mut().unwrap().remove("liquid_mechanics");
        motion["initial_sample"]
            .as_object_mut()
            .unwrap()
            .remove("liquid_mechanics");
        let dry = moving.candidate(&a, None, &dry_descriptor, &s);
        assert!(dry.eligible, "{:?}", dry.exclusions);
    }
}

fn optical_sources(alternative: &mut Alternative) -> Value {
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
    for (property, face) in [
        ("emissivity_public", "public"),
        ("emissivity_reverse", "reverse"),
        ("emissivity", "unused"),
    ] {
        raw["record"]["properties"][property] = json!({"value":0.8,"unit":"1",
            "source":"test","evidence":"estimate","method":"Synthetic optical protocol",
            "conditions":{"face":face,"specimen":"synthetic-optical-sheet",
                "conditioning":"synthetic dry state","spectral_basis":"total_hemispherical",
                "temperature_min_c":"-20","temperature_max_c":"80"}});
    }
    raw["record"]["optical_faces"] = json!({
        "positive":{"face":"public","emissivity_property":"emissivity_public"},
        "negative":{"face":"reverse","emissivity_property":"emissivity_reverse"},
        "source":"test","evidence":"estimate","method":"Synthetic source-normal mapping",
        "specimen":"synthetic-optical-sheet","conditioning":"synthetic dry state"});
    alternative.frozen_changes.as_mut().unwrap().default =
        Some(serde_json::from_value(raw.clone()).unwrap());
    raw
}

fn optical_protocol(
    alternative: &Alternative,
    descriptor: &Value,
    simulation: &Value,
    motion: &Value,
    metric: &str,
) -> String {
    // Exercise the real comparison projection only. These source mutations are not admitted
    // numerical receipts; native exterior solvers independently test optical admission/replay.
    let admitted = Numerical {
        cutting_program: None,
        finite_stock_program: None,
        cutting_job_program: None,
        drape: simulation,
        material_strain: None,
        contact_loads: None,
        vapor_surface: &Value::Null,
        vapor_coverage: None,
        motion,
        motion_material_strain: None,
        motion_contact_loads: None,
        motion_vapor_coverage: None,
        motion_wet_film: None,
        transient_surface: None,
        transient_coverage: None,
    };
    let policy: Policy = serde_json::from_value(json!({"basis":"nominal",
        "objectives":[{"metric":metric,"direction":"minimize"}]}))
    .unwrap();
    policy.protocol(alternative, descriptor, &admitted).unwrap()
}

#[test]
fn optical_protocols_follow_used_observations_and_positive_independent_faces() {
    let (mut a, mut d, mut s, mut m) = coupled_case();
    with_paired_qualifications(&mut d, &mut m);
    let raw = optical_sources(&mut a);
    s["drape"]["steady_thermal_surface"] = s["drape"]["thermal_surface"].clone();
    for automatic in [false, true] {
        let radiation = if automatic {
            json!({"ambient_c":10.,"automatic_material_face":true,
                "allow_opaque_gray_approximation":true})
        } else {
            json!({"ambient_c":10.,"emissivity_property":"emissivity_public",
                "exposed_face":"public","allow_opaque_gray_approximation":true})
        };
        d["scenario"]["surface"] = json!({"exterior_exchange":[{"radiation":radiation}]});
        d["scenario"]["motion"]["transient_surface"]["exterior_exchange"] =
            json!([{"radiation":radiation}]);
        for (metric, uses_optics) in [
            ("independent_surface_power_w", true),
            ("transient_surface_inside_energy_j", true),
            ("motion_max_absolute_edge_strain_pct", true),
            ("max_absolute_edge_strain_pct", false),
            ("assembly_mass_kg", false),
        ] {
            let original = optical_protocol(&a, &d, &s, &m, metric);
            for (fault, changes_protocol) in [
                ("used-method", true),
                ("used-evidence", true),
                ("used-conditions", true),
                ("used-value", false),
                ("unused-generic", false),
                ("unused-reverse", false),
                ("mapping-method", automatic),
                ("mapping-evidence", automatic),
            ] {
                let mut changed = raw.clone();
                match fault {
                    "used-method" => {
                        changed["record"]["properties"]["emissivity_public"]["method"] =
                            json!("Different optical method")
                    }
                    "used-evidence" => {
                        changed["record"]["properties"]["emissivity_public"]["evidence"] =
                            json!("published")
                    }
                    "used-conditions" => {
                        changed["record"]["properties"]["emissivity_public"]["conditions"]["temperature_max_c"] =
                            json!("70")
                    }
                    "used-value" => {
                        changed["record"]["properties"]["emissivity_public"]["value"] = json!(0.7)
                    }
                    "unused-generic" => {
                        changed["record"]["properties"]["emissivity"]["method"] =
                            json!("Unconsumed generic method")
                    }
                    "unused-reverse" => {
                        changed["record"]["properties"]["emissivity_reverse"]["method"] =
                            json!("Unconsumed reverse-side method")
                    }
                    "mapping-method" => {
                        changed["record"]["optical_faces"]["method"] =
                            json!("Different orientation method")
                    }
                    "mapping-evidence" => {
                        changed["record"]["optical_faces"]["evidence"] = json!("published")
                    }
                    _ => unreachable!(),
                }
                a.frozen_changes.as_mut().unwrap().default =
                    Some(serde_json::from_value(changed).unwrap());
                assert_eq!(
                    original != optical_protocol(&a, &d, &s, &m, metric),
                    uses_optics && changes_protocol,
                    "{metric}, automatic={automatic}, {fault}"
                );
                a.frozen_changes.as_mut().unwrap().default =
                    Some(serde_json::from_value(raw.clone()).unwrap());
            }
        }
    }
    // Prescribed convection consumes no emissivity, even when paired liquid mechanics needs heat.
    d["scenario"]["surface"]["exterior_exchange"] = json!([{}]);
    d["scenario"]["motion"]["transient_surface"]["exterior_exchange"] = json!([{}]);
    for metric in [
        "independent_surface_power_w",
        "motion_max_absolute_edge_strain_pct",
    ] {
        let original = optical_protocol(&a, &d, &s, &m, metric);
        let mut changed = raw.clone();
        changed["record"]["properties"]["emissivity"]["method"] = json!("Unused generic method");
        changed["record"]["properties"]["emissivity_public"]["method"] =
            json!("Unused face method");
        changed["record"]["optical_faces"]["method"] = json!("Unused mapping method");
        a.frozen_changes.as_mut().unwrap().default = Some(serde_json::from_value(changed).unwrap());
        assert_eq!(original, optical_protocol(&a, &d, &s, &m, metric));
        a.frozen_changes.as_mut().unwrap().default =
            Some(serde_json::from_value(raw.clone()).unwrap());
    }
}

#[test]
fn optical_schedule_protocol_keeps_observations_bound_to_their_selectors() {
    let (mut a, mut d, s, m) = case();
    let mut raw = optical_sources(&mut a);
    raw["record"]["properties"]["emissivity_public"]["method"] = json!("Optical method A");
    raw["record"]["properties"]["emissivity_reverse"]["method"] = json!("Optical method B");
    // Match the face qualification here so a swap tests selector ownership, not a distinct face.
    raw["record"]["properties"]["emissivity_reverse"]["conditions"] =
        raw["record"]["properties"]["emissivity_public"]["conditions"].clone();
    raw["record"]["optical_faces"] = Value::Null;
    a.frozen_changes.as_mut().unwrap().default = Some(serde_json::from_value(raw.clone()).unwrap());
    d["scenario"]["motion"]["transient_surface"]["exterior_exchange"] = json!([
        {"radiation":{"emissivity_property":"emissivity_public","exposed_face":"public"}},
        {"radiation":{"emissivity_property":"emissivity_reverse","exposed_face":"public"}}]);
    let metric = "transient_surface_inside_energy_j";
    let original = optical_protocol(&a, &d, &s, &m, metric);
    raw["record"]["properties"]["emissivity_public"]["method"] = json!("Optical method B");
    raw["record"]["properties"]["emissivity_reverse"]["method"] = json!("Optical method A");
    a.frozen_changes.as_mut().unwrap().default = Some(serde_json::from_value(raw).unwrap());
    assert_ne!(original, optical_protocol(&a, &d, &s, &m, metric));
}

#[test]
fn steady_optical_protocols_follow_each_pieces_own_column_environment() {
    let (mut a, mut d, mut s, m) = case();
    let raw = optical_sources(&mut a);
    a.frozen_changes
        .as_mut()
        .unwrap()
        .pieces
        .insert("back".into(), serde_json::from_value(raw.clone()).unwrap());
    s["drape"]["steady_thermal_surface"] = s["drape"]["thermal_surface"].clone();
    s["drape"]["steady_thermal_surface"]["model"]["patches"][1]["column"] = json!(1);
    d["scenario"]["surface"] = json!({"exterior_exchange":[
        {"radiation":{"emissivity_property":"emissivity_public"}},
        {"radiation":{"emissivity_property":"emissivity_reverse"}}]});
    let metric = "independent_surface_power_w";
    let original = optical_protocol(&a, &d, &s, &m, metric);
    for (piece, used) in [
        ("front", "emissivity_public"),
        ("back", "emissivity_reverse"),
    ] {
        for property in ["emissivity_public", "emissivity_reverse"] {
            let mut changed = raw.clone();
            changed["record"]["properties"][property]["method"] = json!("Different optical method");
            a.frozen_changes
                .as_mut()
                .unwrap()
                .pieces
                .insert(piece.into(), serde_json::from_value(changed).unwrap());
            assert_eq!(
                original != optical_protocol(&a, &d, &s, &m, metric),
                property == used,
                "{piece}: {property}"
            );
            a.frozen_changes
                .as_mut()
                .unwrap()
                .pieces
                .insert(piece.into(), serde_json::from_value(raw.clone()).unwrap());
        }
    }
}

#[test]
fn radiative_coverage_conservatively_retains_both_mapped_side_protocols() {
    let (mut a, mut d, mut s, m) = case();
    let raw = optical_sources(&mut a);
    s["drape"]["steady_thermal_surface"] = s["drape"]["thermal_surface"].clone();
    for (metric, field, nested) in [
        ("body_coverage_power_w", "exterior_exchange", true),
        (
            "transient_coverage_inside_energy_j",
            "exterior_exchange",
            false,
        ),
        (
            "transient_coverage_inside_energy_j",
            "exposed_exterior_exchange",
            false,
        ),
    ] {
        d["scenario"]["coverage"] = json!({"conditions":{}});
        d["scenario"]["motion"]["transient_coverage"] = json!({});
        let policy = if nested {
            &mut d["scenario"]["coverage"]["conditions"]
        } else {
            &mut d["scenario"]["motion"]["transient_coverage"]
        };
        policy[field] = json!([{"radiation":{"automatic_material_face":true}}]);
        let original = optical_protocol(&a, &d, &s, &m, metric);
        for property in ["emissivity_public", "emissivity_reverse"] {
            let mut changed = raw.clone();
            changed["record"]["properties"][property]["method"] =
                json!("Different exposed-side method");
            a.frozen_changes.as_mut().unwrap().default =
                Some(serde_json::from_value(changed).unwrap());
            assert_ne!(
                original,
                optical_protocol(&a, &d, &s, &m, metric),
                "{metric}: {field}: {property}"
            );
            a.frozen_changes.as_mut().unwrap().default =
                Some(serde_json::from_value(raw.clone()).unwrap());
        }
    }
}

#[test]
fn coupled_mass_admission_preserves_the_independent_thermal_energy_objective() {
    let (a, d, s, m) = coupled_case();
    assert!(admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_ok());
    let h = &m["transient_surface"]["result"];
    let correction = admit(&a, &s, &m, h, &h["request"], false, &[3., 3.])
        .unwrap()
        .unwrap();
    // Exported bulk kinetic/gravitational energy belongs to the mechanical transfer ledger.
    assert!(near(correction[0], 1e-8 * 2500000., 0.025));
    assert_eq!(correction[0], correction[1]);
}

#[test]
fn refused_liquid_mechanics_leaves_source_assembly_mass_comparable() {
    let (mut a, d, mut s, mut m) = coupled_case();
    let inventory: crate::assembly_mass::Request = serde_json::from_value(json!({
        "schema":"ftx-assembly-mass/1","inventory_complete":true,"extras":[]
    }))
    .unwrap();
    let pattern = ftx_pattern::Pattern::new(a.binding.value().unwrap()["pattern"].clone());
    a.assembly_mass = super::super::super::Outcome::Available {
        result: inventory.evaluate(&pattern).unwrap(),
    };
    let policy: Policy = serde_json::from_value(json!({"basis":"nominal",
        "objectives":[{"metric":"assembly_mass_kg","direction":"minimize"}]}))
    .unwrap();
    s["motion"] = json!({"status":"available","result":m});
    let before = policy.candidate(&a, None, &d, &s);
    assert!(before.eligible, "{:?}", before.exclusions);
    m["liquid_mechanics"]["qualification"]["material_report_sha256"] = json!("f".repeat(64));
    assert!(admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_err());
    s["motion"]["result"] = m;
    let after = policy.candidate(&a, None, &d, &s);
    assert_eq!(
        serde_json::to_value(before).unwrap(),
        serde_json::to_value(after).unwrap()
    );
}

#[test]
fn stale_or_unqualified_coupling_never_enters_thermal_decisions() {
    for fault in 0..7 {
        let (a, mut d, s, mut m) = coupled_case();
        let mut q = m["liquid_mechanics"]["qualification"].clone();
        match fault {
            0 => q["allow_estimates"] = json!(false),
            1 => q["material_report_sha256"] = json!("f".repeat(64)),
            2 => q["bindings"]["front"]["material_sha256"] = json!("e".repeat(64)),
            3 => q["bindings"]["front"]["temperature_range_c"] = json!([0., 80.]),
            4 => q["bindings"]["front"]["maximum_liquid_loading_kg_m2"] = json!(1e-6),
            5 => q["source"]["note"] = json!(""),
            _ => {
                q["bindings"].as_object_mut().unwrap().remove("back");
            }
        }
        m["initial_sample"]["liquid_mechanics"]["qualification"] = q.clone();
        m["liquid_mechanics"]["qualification"] = q.clone();
        let h = &mut m["transient_surface"]["result"];
        h["request"]["mechanical_coupling"] = q.clone();
        h["mechanical_coupling"]["qualification"] = q;
        d["scenario"]["motion"]["transient_surface"] = h["request"].clone();
        assert!(
            admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_err(),
            "fault {fault}"
        );
    }
}

#[test]
fn coupled_reports_require_complete_initial_final_and_sampled_inventory() {
    for fault in 0..11 {
        let (a, d, s, mut m) = coupled_case();
        match fault {
            0 => m["initial_sample"]["liquid_mechanics"] = Value::Null,
            1 => m["liquid_mechanics"] = Value::Null,
            2 => m["transient_surface"]["result"]["mechanical_coupling"] = Value::Null,
            3 => m["transient_surface"]["result"]["trace"][0]["mechanical_coupling"] = Value::Null,
            4 => {
                m["initial_sample"]["liquid_mechanics"]["exported_momentum_kg_m_s"] =
                    json!([1., 0., 0.])
            }
            _ => {
                let (key, value) = match fault {
                    5 => ("total_inertial_mass_kg", json!(0.001)),
                    6 => ("exported_kinetic_energy_j", json!(-1.)),
                    7 => ("exported_gravitational_energy_j", Value::Null),
                    8 => ("transfer_momentum_residual_kg_m_s", json!([0., 0.])),
                    9 => ("mass_balance_residual_kg", json!(0.001)),
                    _ => ("remaining_liquid_kg", json!(1e-9)),
                };
                let full = if key == "remaining_liquid_kg" {
                    "remaining_liquid_mass_kg"
                } else {
                    key
                };
                m["liquid_mechanics"][full] = value.clone();
                let h = &mut m["transient_surface"]["result"];
                h["mechanical_coupling"][full] = value.clone();
                h["trace"][0]["mechanical_coupling"][key] = value;
            }
        }
        assert!(
            admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_err(),
            "fault {fault}"
        );
    }
    let (a, mut d, s, mut m) = coupled_case();
    let h = &mut m["transient_surface"]["result"];
    h["request"]
        .as_object_mut()
        .unwrap()
        .remove("mechanical_coupling");
    d["scenario"]["motion"]["transient_surface"] = h["request"].clone();
    assert!(admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_err());
    let (a, d, s, m) = case();
    assert!(admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_ok());
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

fn exchanging_case(condensation: bool) -> (Alternative, Value, Value, Value) {
    let (a, mut d, mut s, mut m) = if condensation {
        condensing_case()
    } else {
        supplied_case()
    };
    let (_, _, _, prototype) = coupled_case();
    let h = &mut m["transient_surface"]["result"];
    let mut q = prototype["liquid_mechanics"]["qualification"].clone();
    q["schema"] = json!("ftx-coupled-liquid-mechanics/1");
    q["model"] = json!("co_moving_mass_exchange_frozen_mechanics/1");
    for binding in q["bindings"].as_object_mut().unwrap().values_mut() {
        binding["maximum_liquid_loading_kg_m2"] =
            h["request"]["surface_water"]["maximum_liquid_loading_kg_m2"].clone();
    }
    s["drape"]["mass_kg"] = json!(0.0004);
    let initial_mass = h["surface_water"]["initial_liquid_kg"].as_f64().unwrap();
    let remaining = h["surface_water"]["remaining_liquid_kg"].as_f64().unwrap();
    let evaporated = h["surface_water"]["evaporated_liquid_kg"].as_f64().unwrap();
    let incoming = h["surface_water"]["supplied_liquid_kg"].as_f64().unwrap()
        + if condensation {
            h["surface_water"]["condensed_liquid_kg"].as_f64().unwrap()
        } else {
            0.
        };
    let mut initial = prototype["initial_sample"]["liquid_mechanics"].clone();
    initial["schema"] = json!("ftx-coupled-liquid-mechanics-result/1");
    initial["qualification"] = q.clone();
    initial["exchange"] = json!({"incoming_mass_kg":0.,"outgoing_mass_kg":0.,
        "initial_momentum_kg_m_s":[initial_mass,0.,0.],"initial_kinetic_energy_j":0.5*initial_mass,
        "initial_gravitational_energy_j":3.*initial_mass,"incoming_momentum_kg_m_s":[0.,0.,0.],
        "incoming_kinetic_energy_j":0.,"incoming_gravitational_energy_j":0.});
    let mut final_report = initial.clone();
    final_report["remaining_liquid_mass_kg"] = json!(remaining);
    final_report["evaporated_liquid_mass_kg"] = json!(evaporated);
    final_report["total_inertial_mass_kg"] = json!(0.0004 + remaining);
    final_report["exported_momentum_kg_m_s"] = json!([evaporated, 0., 0.]);
    final_report["exported_kinetic_energy_j"] = json!(0.5 * evaporated);
    final_report["exported_gravitational_energy_j"] = json!(3. * evaporated);
    final_report["exchange"]["incoming_mass_kg"] = json!(incoming);
    final_report["exchange"]["outgoing_mass_kg"] = json!(evaporated);
    final_report["exchange"]["incoming_momentum_kg_m_s"] = json!([incoming, 0., 0.]);
    final_report["exchange"]["incoming_kinetic_energy_j"] = json!(0.5 * incoming);
    final_report["exchange"]["incoming_gravitational_energy_j"] = json!(3. * incoming);
    let mut summary = final_report.as_object().unwrap().clone();
    for key in ["schema", "qualification", "scope"] {
        summary.remove(key);
    }
    for (full, short) in [
        ("initial_liquid_mass_kg", "initial_liquid_kg"),
        ("remaining_liquid_mass_kg", "remaining_liquid_kg"),
        ("evaporated_liquid_mass_kg", "evaporated_liquid_kg"),
    ] {
        let value = summary.remove(full).unwrap();
        summary.insert(short.into(), value);
    }
    h["request"]["mechanical_coupling"] = q;
    h["mechanical_coupling"] = final_report.clone();
    h["trace"][0]["mechanical_coupling"] = Value::Object(summary);
    d["scenario"]["motion"]["transient_surface"] = h["request"].clone();
    m["initial_sample"]["liquid_mechanics"] = initial;
    m["liquid_mechanics"] = final_report;
    (a, d, s, m)
}

fn draining_case(condensation: bool) -> (Alternative, Value, Value, Value) {
    let (a, mut d, s, mut m) = exchanging_case(condensation);
    let h = &mut m["transient_surface"]["result"];
    let policy = &mut h["request"]["surface_water"];
    policy["schema"] = json!("ftx-textile-surface-water/4");
    policy["drainage"] = json!({"schema":"ftx-textile-liquid-drainage/1",
        "bindings":{"front":{"rate_kg_m2_s":[4e-6]},"back":{"rate_kg_m2_s":[0.]}},
        "source":{"reference":"test:prescribed-drainage","accessed_on":"2026-10-31",
            "note":"Synthetic drainage accounting, not empirical runoff evidence"},
        "evidence":"estimate","allow_estimates":true,"method":"Prescribed synthetic liquid export",
        "qualification":"Stock-limited co-moving drainage at old inner-cell temperature; proportional demand sharing on empty cloth"});
    h["surface_water"]["request"] = h["request"]["surface_water"].clone();
    // Reclassify part of the independently balanced outgoing mass as liquid. Its sensible
    // enthalpy is unchanged, but it has no evaporative latent term.
    let drained = 4e-6 * 0.001;
    let evaporated = h["surface_water"]["evaporated_liquid_kg"].as_f64().unwrap() - drained;
    let sensible = drained * 4200. * 20.;
    let net = h["net_boundary_energy_j"].as_f64().unwrap() - drained * 2500000.;
    for path in ["/surface_water", "/trace/0/surface_water"] {
        let row = h.pointer_mut(path).unwrap();
        row["schema"] = json!("ftx-textile-surface-water-result/4");
        row["drained_liquid_kg"] = json!(drained);
        row["exported_liquid_sensible_energy_j"] = json!(sensible);
        row["evaporated_liquid_kg"] = json!(evaporated);
        row["latent_energy_j"] = json!(evaporated * 2500000.);
        row["exported_vapor_sensible_energy_j"] = json!(evaporated * 4200. * 20.);
    }
    for (index, patch) in h["surface_water"]["patches"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        patch["drained_liquid_kg"] = json!(if index == 0 { drained } else { 0. });
        patch["exported_liquid_sensible_energy_j"] = json!(if index == 0 { sensible } else { 0. });
        patch["evaporated_liquid_kg"] = json!(if index == 0 { evaporated } else { 0. });
    }
    h["net_boundary_energy_j"] = json!(net);
    h["outside_energy_j"] = json!(-net);
    h["trace"][0]["net_boundary_energy_j"] = json!(net);
    h["trace"][0]["outside_energy_j"] = json!(-net);
    h["trace"][0]["mechanical_coupling"]["drained_liquid_kg"] = json!(drained);
    h["trace"][0]["mechanical_coupling"]["evaporated_liquid_kg"] = json!(evaporated);
    d["scenario"]["motion"]["transient_surface"] = h["request"].clone();
    for path in [
        "/liquid_mechanics",
        "/transient_surface/result/mechanical_coupling",
    ] {
        let report = m.pointer_mut(path).unwrap();
        report["drained_liquid_mass_kg"] = json!(drained);
        report["evaporated_liquid_mass_kg"] = json!(evaporated);
    }
    m["initial_sample"]["liquid_mechanics"]["drained_liquid_mass_kg"] = json!(0.);
    (a, d, s, m)
}

fn finite_reservoir_case(draining: bool, mechanical: bool) -> (Alternative, Value, Value, Value) {
    let (a, mut d, s, mut m) = if draining {
        draining_case(true)
    } else if mechanical {
        exchanging_case(true)
    } else {
        condensing_case()
    };
    let h = &mut m["transient_surface"]["result"];
    let policy = &mut h["request"]["surface_water"];
    policy["schema"] = json!("ftx-textile-surface-water/5");
    policy
        .as_object_mut()
        .unwrap()
        .remove("outside_vapor_pressure_pa");
    policy["vapor_reservoir"] = json!({
        "schema":"ftx-textile-vapor-reservoir/1",
        "model":"isothermal_constant_vapor_capacitance/1",
        "vapor_capacitance_kg_pa":1e-9,"initial_vapor_mass_kg":1e-6,"temperature_c":40.,
        "pressure_domain_pa":[0.,5000.],"max_iterations":96,"mass_tolerance_kg":1e-16,
        "source":{"reference":"test:finite-vapor-admission","accessed_on":"2026-11-01",
            "note":"Synthetic inventory and effective-enthalpy identities; not empirical gas data"},
        "evidence":"estimate","allow_estimates":true,"method":"Synthetic finite storage",
        "qualification":"Explicit constant capacitance, isothermal storage and effective vapor enthalpy"});
    h["surface_water"]["request"] = h["request"]["surface_water"].clone();
    let w = &h["surface_water"];
    let received =
        w["evaporated_liquid_kg"].as_f64().unwrap() - w["condensed_liquid_kg"].as_f64().unwrap();
    let mass = 1e-6 + received;
    let residual = mass - 1e-6 - received;
    let change = (2500000. + 4200. * 40.) * (mass - 1e-6);
    let received_j = w["latent_energy_j"].as_f64().unwrap()
        + w["exported_vapor_sensible_energy_j"].as_f64().unwrap()
        - w["condensation_latent_energy_j"].as_f64().unwrap()
        - w["incoming_vapor_sensible_energy_j"].as_f64().unwrap();
    let removed_j = received_j - change;
    let reservoir = json!({"schema":"ftx-textile-vapor-reservoir-result/1",
        "initial_vapor_mass_kg":1e-6,"remaining_vapor_mass_kg":mass,
        "initial_vapor_pressure_pa":1e-6/1e-9,"vapor_pressure_pa":mass/1e-9,
        "net_vapor_received_kg":received,"mass_balance_error_kg":residual,
        "vapor_enthalpy_change_j":change,"net_vapor_enthalpy_received_j":received_j,
        "isothermal_heat_removed_j":removed_j,"enthalpy_balance_error_j":change+removed_j-received_j,
        "elapsed_seconds":1.,"pressure_trials":64,"maximum_step_mass_residual_kg":residual.abs()});
    for path in ["/surface_water", "/trace/0/surface_water"] {
        let row = h.pointer_mut(path).unwrap();
        row["schema"] = json!("ftx-textile-surface-water-result/5");
        row["vapor_reservoir"] = reservoir.clone();
    }
    // Two patches with two cells each, two heated substeps and 64 actual pressure probes.
    h["cell_steps"] = json!(4 * (2 * 129 + 64));
    h["request"]["max_cell_steps"] = json!(2000);
    d["scenario"]["motion"]["transient_surface"] = h["request"].clone();
    (a, d, s, m)
}

#[test]
fn finite_vapor_inventory_admits_with_optional_drainage_and_existing_mechanical_exchange() {
    for (draining, mechanical) in [(false, false), (false, true), (true, true)] {
        let (a, d, s, m) = finite_reservoir_case(draining, mechanical);
        admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).unwrap_or_else(|error| {
            panic!("drainage={draining}, mechanics={mechanical}: {error:?}")
        });
    }
    let (a, mut d, s, mut m) = finite_reservoir_case(false, false);
    let h = &mut m["transient_surface"]["result"];
    h["request"]["surface_water"]["outside_vapor_pressure_pa"] = json!([]);
    h["surface_water"]["request"] = h["request"]["surface_water"].clone();
    d["scenario"]["motion"]["transient_surface"] = h["request"].clone();
    assert!(admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_ok());
}

#[test]
fn finite_vapor_retry_work_remains_admissible_only_when_fully_charged() {
    let (a, mut d, s, mut m) = finite_reservoir_case(false, false);
    let h = &mut m["transient_surface"]["result"];
    // Failed attempts can spend more probes than the two committed steps' iteration limits.
    // Their physical histories rolled back, but the same whole-history work budget still pays.
    h["surface_water"]["vapor_reservoir"]["pressure_trials"] = json!(500);
    h["trace"][0]["surface_water"]["vapor_reservoir"]["pressure_trials"] = json!(500);
    h["cell_steps"] = json!(4 * (2 * 129 + 500));
    h["request"]["max_cell_steps"] = json!(3500);
    d["scenario"]["motion"]["transient_surface"] = h["request"].clone();
    assert!(admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_ok());
    for actual_work in [4 * (2 * 129 + 64), 3501] {
        let mut changed = m.clone();
        changed["transient_surface"]["result"]["cell_steps"] = json!(actual_work);
        assert!(admit_heat(&a, &d, &s, &changed, Consumer::TransientSurface).is_err());
    }
}

#[test]
fn every_retained_finite_vapor_field_is_required_and_checked() {
    let (a, d, s, m) = finite_reservoir_case(true, true);
    for path in [
        "/surface_water/vapor_reservoir",
        "/trace/0/surface_water/vapor_reservoir",
    ] {
        for field in [
            "initial_vapor_mass_kg",
            "remaining_vapor_mass_kg",
            "initial_vapor_pressure_pa",
            "vapor_pressure_pa",
            "net_vapor_received_kg",
            "mass_balance_error_kg",
            "vapor_enthalpy_change_j",
            "net_vapor_enthalpy_received_j",
            "isothermal_heat_removed_j",
            "enthalpy_balance_error_j",
            "elapsed_seconds",
            "pressure_trials",
            "maximum_step_mass_residual_kg",
        ] {
            for missing in [false, true] {
                let mut changed = m.clone();
                let report = changed["transient_surface"]["result"]
                    .pointer_mut(path)
                    .unwrap();
                if missing {
                    report.as_object_mut().unwrap().remove(field);
                } else if field == "pressure_trials" {
                    report[field] = json!(65);
                } else {
                    report[field] = json!(report[field].as_f64().unwrap() + 1.);
                }
                assert!(
                    admit_heat(&a, &d, &s, &changed, Consumer::TransientSurface).is_err(),
                    "{path}.{field}, missing={missing}"
                );
            }
        }
    }
}

#[test]
fn finite_vapor_reconciles_external_water_and_work_instead_of_only_internal_equalities() {
    let (a, d, s, m) = finite_reservoir_case(false, false);
    for mutation in 0..4 {
        let mut changed = m.clone();
        let h = &mut changed["transient_surface"]["result"];
        for path in [
            "/surface_water/vapor_reservoir",
            "/trace/0/surface_water/vapor_reservoir",
        ] {
            let r = h.pointer_mut(path).unwrap();
            match mutation {
                0 => {
                    // Reconcile every internal reservoir identity after inventing incoming vapor.
                    // The independently retained textile transfer must still reject this stock.
                    let mass = r["remaining_vapor_mass_kg"].as_f64().unwrap() + 1e-7;
                    let change = (2500000. + 4200. * 40.) * (mass - 1e-6);
                    r["remaining_vapor_mass_kg"] = json!(mass);
                    r["vapor_pressure_pa"] = json!(mass / 1e-9);
                    r["net_vapor_received_kg"] = json!(mass - 1e-6);
                    r["mass_balance_error_kg"] = json!(0.);
                    r["vapor_enthalpy_change_j"] = json!(change);
                    r["isothermal_heat_removed_j"] =
                        json!(r["net_vapor_enthalpy_received_j"].as_f64().unwrap() - change);
                    r["enthalpy_balance_error_j"] = json!(0.);
                }
                1 => {
                    r["vapor_enthalpy_change_j"] =
                        json!(r["vapor_enthalpy_change_j"].as_f64().unwrap() + 1.);
                    r["isothermal_heat_removed_j"] =
                        json!(r["isothermal_heat_removed_j"].as_f64().unwrap() - 1.);
                }
                2 => r["pressure_trials"] = json!(0),
                _ => r["pressure_trials"] = json!(65),
            }
        }
        assert!(
            admit_heat(&a, &d, &s, &changed, Consumer::TransientSurface).is_err(),
            "mutation {mutation}"
        );
    }
}

#[test]
fn finite_vapor_requires_its_exact_source_domain_and_single_pressure_authority() {
    let (a, d, s, m) = finite_reservoir_case(false, false);
    for (path, value) in [
        (
            "/vapor_reservoir/schema",
            json!("ftx-textile-vapor-reservoir/2"),
        ),
        ("/vapor_reservoir/model", json!("ideal_gas")),
        ("/vapor_reservoir/vapor_capacitance_kg_pa", json!(0.)),
        ("/vapor_reservoir/initial_vapor_mass_kg", json!(-1.)),
        ("/vapor_reservoir/temperature_c", json!(39.)),
        ("/vapor_reservoir/pressure_domain_pa", json!([0., 900.])),
        ("/vapor_reservoir/pressure_domain_pa", json!([5000., 0.])),
        ("/vapor_reservoir/max_iterations", json!(0)),
        ("/vapor_reservoir/max_iterations", json!(129)),
        ("/vapor_reservoir/mass_tolerance_kg", json!(0.)),
        ("/vapor_reservoir/source/reference", json!("")),
        ("/vapor_reservoir/source/note", json!("")),
        ("/vapor_reservoir/source/accessed_on", json!("missing")),
        ("/vapor_reservoir/method", json!("")),
        ("/vapor_reservoir/qualification", json!("")),
        ("/vapor_reservoir/allow_estimates", json!(false)),
        ("/supply", Value::Null),
        ("/condensation", Value::Null),
        ("/vapor_reservoir", Value::Null),
    ] {
        let mut changed = m.clone();
        let mut descriptor = d.clone();
        let h = &mut changed["transient_surface"]["result"];
        *h["request"]["surface_water"].pointer_mut(path).unwrap() = value;
        h["surface_water"]["request"] = h["request"]["surface_water"].clone();
        descriptor["scenario"]["motion"]["transient_surface"] = h["request"].clone();
        assert!(
            admit_heat(&a, &descriptor, &s, &changed, Consumer::TransientSurface).is_err(),
            "{path}"
        );
    }
    for ambient in [json!([1000.]), Value::Null] {
        let mut changed = m.clone();
        let mut descriptor = d.clone();
        let h = &mut changed["transient_surface"]["result"];
        h["request"]["surface_water"]["outside_vapor_pressure_pa"] = ambient;
        h["surface_water"]["request"] = h["request"]["surface_water"].clone();
        descriptor["scenario"]["motion"]["transient_surface"] = h["request"].clone();
        assert!(admit_heat(&a, &descriptor, &s, &changed, Consumer::TransientSurface).is_err());
    }
    let mut changed = m.clone();
    changed["transient_surface"]["result"]["surface_water"]["request"]["vapor_reservoir"]["source"]
        ["reference"] = json!("test:other-source");
    assert!(admit_heat(&a, &d, &s, &changed, Consumer::TransientSurface).is_err());
}

#[test]
fn legacy_water_schemas_do_not_accept_an_unrequested_vapor_owner() {
    let (_, _, _, finite_case) = finite_reservoir_case(false, false);
    for (a, d, s, m) in [
        case(),
        supplied_case(),
        condensing_case(),
        draining_case(true),
    ] {
        assert!(admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_ok());
        for policy_field in [false, true] {
            for value in [
                Value::Null,
                json!({"schema":"ftx-textile-vapor-reservoir-result/1"}),
            ] {
                let mut changed = m.clone();
                let mut descriptor = d.clone();
                let h = &mut changed["transient_surface"]["result"];
                if policy_field {
                    h["request"]["surface_water"]["vapor_reservoir"] = finite_case["transient_surface"]["result"]["request"]["surface_water"]["vapor_reservoir"].clone();
                    h["surface_water"]["request"] = h["request"]["surface_water"].clone();
                    descriptor["scenario"]["motion"]["transient_surface"] = h["request"].clone();
                } else {
                    h["surface_water"]["vapor_reservoir"] = value.clone();
                    h["trace"][0]["surface_water"]["vapor_reservoir"] = value;
                }
                assert!(
                    admit_heat(&a, &descriptor, &s, &changed, Consumer::TransientSurface).is_err()
                );
            }
        }
    }
}

#[test]
fn drainage_retains_separate_liquid_enthalpy_and_both_mechanical_outflow_channels() {
    for condensation in [false, true] {
        for incoming_model in 0..4 {
            let (a, mut d, s, mut m) = draining_case(condensation);
            match incoming_model {
                1 => prescribe_incoming_velocity(&mut d, &mut m),
                2 => schedule_incoming_velocity(&mut d, &mut m),
                3 => qualify_incoming_velocities(&mut d, &mut m, true),
                _ => {}
            }
            let heat = admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).unwrap_or_else(|r| {
                panic!("condensation={condensation} incoming_model={incoming_model}: {r:?}")
            });
            let admitted = Numerical {
                cutting_program: None,
                finite_stock_program: None,
                cutting_job_program: None,
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
            let metric = Metric::TextileWaterDrainedMassKg;
            assert_eq!(metric.consumer(), Consumer::TransientSurface);
            assert_eq!(metric.unit(None), "kg");
            assert!(!metric.supports_bounds());
            assert_eq!(metric.value(&a, &admitted), Some(4e-6 * 0.001));
            assert_eq!(
                serde_json::to_value(metric).unwrap(),
                "textile_water_drained_mass_kg"
            );
            let water = &m["transient_surface"]["result"]["surface_water"];
            let mechanics = &m["liquid_mechanics"];
            assert_eq!(
                mechanics["drained_liquid_mass_kg"],
                water["drained_liquid_kg"]
            );
            assert!(near(
                mechanics["exchange"]["outgoing_mass_kg"].as_f64().unwrap(),
                water["evaporated_liquid_kg"].as_f64().unwrap()
                    + water["drained_liquid_kg"].as_f64().unwrap(),
                1e-8
            ));
            assert_eq!(
                water["latent_energy_j"].as_f64().unwrap(),
                water["evaporated_liquid_kg"].as_f64().unwrap() * 2500000.
            );
        }
    }
}

#[test]
fn drainage_requires_source_complete_schedules_and_consistent_phase_ledgers() {
    for mutation in 0..14 {
        let (a, mut d, s, mut m) = draining_case(true);
        let h = &mut m["transient_surface"]["result"];
        match mutation {
            0 => h["request"]["surface_water"]["drainage"]["source"] = Value::Null,
            1 => h["request"]["surface_water"]["drainage"]["qualification"] = json!(" "),
            2 => h["request"]["surface_water"]["drainage"]["allow_estimates"] = json!(false),
            3 => {
                h["request"]["surface_water"]["drainage"]["bindings"]
                    .as_object_mut()
                    .unwrap()
                    .remove("back");
            }
            4 => {
                h["request"]["surface_water"]["drainage"]["bindings"]["front"]["rate_kg_m2_s"] =
                    json!([4e-6, 4e-6])
            }
            5 => {
                h["request"]["surface_water"]["drainage"]["bindings"]["front"]["rate_kg_m2_s"] =
                    json!([-1.])
            }
            6 => h["request"]["surface_water"]["schema"] = json!("ftx-textile-surface-water/3"),
            7 => {
                h["request"]["surface_water"]["drainage"]["bindings"]["front"]["rate_kg_m2_s"] =
                    json!([1e-6])
            }
            8 => h["surface_water"]["patches"][0]["evaporated_liquid_kg"] = json!(1e-8),
            9 => h["surface_water"]["patches"][0]["exported_liquid_sensible_energy_j"] = json!(1.),
            10 => {
                h["trace"][0]["surface_water"]
                    .as_object_mut()
                    .unwrap()
                    .remove("drained_liquid_kg");
            }
            11 => h["mechanical_coupling"]["drained_liquid_mass_kg"] = json!(0.),
            12 => h["mechanical_coupling"]["evaporated_liquid_mass_kg"] = json!(1e-8),
            13 => h["mechanical_coupling"]["exchange"]["outgoing_mass_kg"] = json!(6e-9),
            _ => unreachable!(),
        }
        h["surface_water"]["request"] = h["request"]["surface_water"].clone();
        d["scenario"]["motion"]["transient_surface"] = h["request"].clone();
        assert!(
            admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_err(),
            "mutation={mutation}"
        );
    }
}

#[test]
fn drainage_demand_respects_reference_area_and_every_physical_interval() {
    let (_, _, _, m) = draining_case(false);
    let water = &m["transient_surface"]["result"]["request"]["surface_water"];
    let mut request = water["drainage"].clone();
    request["bindings"]["front"]["rate_kg_m2_s"] = json!([2., 0., 4.]);
    request["bindings"]["back"]["rate_kg_m2_s"] = json!([0., 3., 1.]);
    let motion = json!({"duration_s":6.,"keyframes":[{"time_s":1.},{"time_s":3.},{"time_s":6.}]});
    let drainage =
        Drainage::new(&request, water["bindings"].as_object().unwrap(), &motion).unwrap();
    let areas = BTreeMap::from([("front", 0.25), ("back", 0.5)]);
    assert_eq!(drainage.integral("front", 0., 6.).unwrap(), 14.);
    assert_eq!(drainage.integral("back", 0., 6.).unwrap(), 9.);
    assert_eq!(drainage.demand(&areas, 0., 6.).unwrap(), 8.);
    assert_eq!(drainage.demand(&areas, 0.5, 4.).unwrap(), 4.75);
    assert_eq!(drainage.demand(&areas, 1., 3.).unwrap(), 3.);
    assert_eq!(drainage.demand(&areas, 6., 6.).unwrap(), 0.);
    for (from, to) in [
        (-1., 1.),
        (2., 1.),
        (0., 7.),
        (f64::NAN, 1.),
        (0., f64::INFINITY),
    ] {
        assert!(drainage.demand(&areas, from, to).is_err());
    }
}

fn prescribe_incoming_velocity(d: &mut Value, m: &mut Value) {
    qualify_incoming_velocities(d, m, false);
}

fn qualify_incoming_velocities(d: &mut Value, m: &mut Value, phase_specific: bool) {
    let mut qualification = m["liquid_mechanics"]["qualification"].clone();
    qualification["schema"] = json!(if phase_specific {
        "ftx-coupled-liquid-mechanics/4"
    } else {
        "ftx-coupled-liquid-mechanics/2"
    });
    qualification["model"] = json!(if phase_specific {
        "phase_specific_incoming_velocity_mass_exchange_frozen_mechanics/1"
    } else {
        "prescribed_incoming_velocity_mass_exchange_frozen_mechanics/1"
    });
    for binding in qualification["bindings"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        // The independent accounting fixture already carries unit-x incoming momentum per kg.
        if phase_specific {
            binding["incoming_stream_velocities_world_m_s"] =
                json!({"supply":[1.,0.,0.],"condensation":[1.,0.,0.]});
        } else {
            binding["incoming_velocity_world_m_s"] = json!([1., 0., 0.]);
        }
    }
    for path in [
        "/initial_sample/liquid_mechanics",
        "/liquid_mechanics",
        "/transient_surface/result/mechanical_coupling",
    ] {
        let report = m.pointer_mut(path).unwrap();
        report["schema"] = json!(if phase_specific {
            "ftx-coupled-liquid-mechanics-result/4"
        } else {
            "ftx-coupled-liquid-mechanics-result/2"
        });
        report["qualification"] = qualification.clone();
        report["exchange"]["mixing_dissipation_j"] = json!(0.);
    }
    let heat = &mut m["transient_surface"]["result"];
    heat["request"]["mechanical_coupling"] = qualification;
    heat["trace"][0]["mechanical_coupling"]["exchange"]["mixing_dissipation_j"] = json!(0.);
    d["scenario"]["motion"]["transient_surface"] = heat["request"].clone();
}

fn schedule_incoming_velocity(d: &mut Value, m: &mut Value) {
    prescribe_incoming_velocity(d, m);
    let mut qualification = m["liquid_mechanics"]["qualification"].clone();
    qualification["schema"] = json!("ftx-coupled-liquid-mechanics/3");
    qualification["model"] = json!("scheduled_incoming_velocity_mass_exchange_frozen_mechanics/1");
    for binding in qualification["bindings"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        binding
            .as_object_mut()
            .unwrap()
            .remove("incoming_velocity_world_m_s");
        binding["incoming_velocity_world_m_s_by_interval"] = json!([[1., 0., 0.]]);
    }
    for path in [
        "/initial_sample/liquid_mechanics",
        "/liquid_mechanics",
        "/transient_surface/result/mechanical_coupling",
    ] {
        let report = m.pointer_mut(path).unwrap();
        report["schema"] = json!("ftx-coupled-liquid-mechanics-result/3");
        report["qualification"] = qualification.clone();
    }
    let heat = &mut m["transient_surface"]["result"];
    heat["request"]["mechanical_coupling"] = qualification;
    d["scenario"]["motion"]["transient_surface"] = heat["request"].clone();
}

fn replace_mechanical_qualification(d: &mut Value, m: &mut Value, qualification: Value) {
    for path in [
        "/initial_sample/liquid_mechanics",
        "/liquid_mechanics",
        "/transient_surface/result/mechanical_coupling",
    ] {
        m.pointer_mut(path).unwrap()["qualification"] = qualification.clone();
    }
    let heat = &mut m["transient_surface"]["result"];
    heat["request"]["mechanical_coupling"] = qualification;
    d["scenario"]["motion"]["transient_surface"] = heat["request"].clone();
}

fn schedule_phase_velocities(d: &mut Value, m: &mut Value, recoil: bool) {
    qualify_incoming_velocities(d, m, true);
    let mut qualification = m["liquid_mechanics"]["qualification"].clone();
    qualification["schema"] = json!(if recoil {
        "ftx-coupled-liquid-mechanics/6"
    } else {
        "ftx-coupled-liquid-mechanics/5"
    });
    qualification["model"] = json!(if recoil {
        "scheduled_phase_specific_incoming_and_outgoing_velocity_mass_exchange_frozen_mechanics/1"
    } else {
        "scheduled_phase_specific_incoming_velocity_mass_exchange_frozen_mechanics/1"
    });
    for binding in qualification["bindings"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        let velocities = binding
            .as_object_mut()
            .unwrap()
            .remove("incoming_stream_velocities_world_m_s")
            .unwrap();
        binding["incoming_stream_velocities_world_m_s_by_interval"] = json!([velocities]);
        if recoil {
            // Zero relative outflow retains the independent accounting fixture's unit-x velocity.
            binding["outgoing_stream_velocities_relative_to_cloth_m_s_by_interval"] =
                json!([{"evaporation":[0.,0.,0.],"drainage":[0.,0.,0.]}]);
        }
    }
    for path in [
        "/initial_sample/liquid_mechanics",
        "/liquid_mechanics",
        "/transient_surface/result/mechanical_coupling",
    ] {
        let report = m.pointer_mut(path).unwrap();
        report["schema"] = json!(if recoil {
            "ftx-coupled-liquid-mechanics-result/6"
        } else {
            "ftx-coupled-liquid-mechanics-result/5"
        });
        if recoil {
            report["exchange"]["outgoing_separation_work_j"] = json!(0.);
        }
    }
    if recoil {
        m["transient_surface"]["result"]["trace"][0]["mechanical_coupling"]["exchange"]["outgoing_separation_work_j"] =
            json!(0.);
    }
    replace_mechanical_qualification(d, m, qualification);
}

#[test]
fn scheduled_phases_and_recoil_admit_the_same_independent_thermal_objectives() {
    for condensation in [false, true] {
        for draining in [false, true] {
            for recoil in [false, true] {
                let (a, mut d, s, mut m) = if draining {
                    draining_case(condensation)
                } else {
                    exchanging_case(condensation)
                };
                let h = &m["transient_surface"]["result"];
                let expected = admit(&a, &s, &m, h, &h["request"], false, &[3., 3.]).unwrap();
                schedule_phase_velocities(&mut d, &mut m, recoil);
                assert!(admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_ok());
                let h = &m["transient_surface"]["result"];
                assert_eq!(
                    admit(&a, &s, &m, h, &h["request"], false, &[3., 3.]).unwrap(),
                    expected
                );
            }
        }
    }
}

#[test]
fn scheduled_phase_mechanics_need_exact_intervals_phases_sources_and_exclusive_fields() {
    let field = "incoming_stream_velocities_world_m_s_by_interval";
    for recoil in [false, true] {
        let (a, mut d, s, mut m) = draining_case(true);
        schedule_phase_velocities(&mut d, &mut m, recoil);
        assert!(admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_ok());
        for fault in 0..18 {
            let (mut changed, mut descriptor) = (m.clone(), d.clone());
            let mut q = changed["liquid_mechanics"]["qualification"].clone();
            let binding = &mut q["bindings"]["front"];
            match fault {
                0 => {
                    binding.as_object_mut().unwrap().remove(field);
                }
                1 => binding[field] = Value::Null,
                2 => binding[field] = json!([]),
                3 => binding[field]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"supply":[1.,0.,0.],"condensation":[1.,0.,0.]})),
                4 => binding[field][0]["supply"] = json!([1., 0.]),
                5 => binding[field][0]["condensation"] = json!([1., null, 0.]),
                6 => {
                    binding[field][0].as_object_mut().unwrap().remove("supply");
                }
                7 => binding[field][0]["extra"] = json!([0., 0., 0.]),
                8 => {
                    binding[field][0]
                        .as_object_mut()
                        .unwrap()
                        .remove("condensation");
                    binding[field][0]["drainage"] = json!([0., 0., 0.]);
                }
                9 => binding["incoming_velocity_world_m_s"] = Value::Null,
                10 => {
                    binding["incoming_stream_velocities_world_m_s"] =
                        json!({"supply":[1.,0.,0.],"condensation":[1.,0.,0.]})
                }
                11 => binding["incoming_velocity_world_m_s_by_interval"] = json!([[1., 0., 0.]]),
                12 => {
                    q["schema"] = json!(if recoil {
                        "ftx-coupled-liquid-mechanics/5"
                    } else {
                        "ftx-coupled-liquid-mechanics/6"
                    })
                }
                13 => {
                    q["model"] =
                        json!("phase_specific_incoming_velocity_mass_exchange_frozen_mechanics/1")
                }
                14 => binding["material_sha256"] = json!("f".repeat(64)),
                15 => q["material_report_sha256"] = json!("f".repeat(64)),
                16 => q["allow_estimates"] = json!(false),
                _ => q["qualification"] = json!(""),
            }
            replace_mechanical_qualification(&mut descriptor, &mut changed, q);
            assert!(
                admit_heat(&a, &descriptor, &s, &changed, Consumer::TransientSurface).is_err(),
                "recoil {recoil}, fault {fault}"
            );
        }
        let mut stale = m.clone();
        stale["initial_sample"]["liquid_mechanics"]["schema"] =
            json!("ftx-coupled-liquid-mechanics-result/4");
        assert!(admit_heat(&a, &d, &s, &stale, Consumer::TransientSurface).is_err());
        let mut wrong_intervals = m.clone();
        wrong_intervals["keyframes"]
            .as_array_mut()
            .unwrap()
            .push(json!({"time_s":2.}));
        let h = &wrong_intervals["transient_surface"]["result"];
        assert!(
            admit_mechanics(
                &s,
                &wrong_intervals,
                h,
                &h["request"],
                false,
                &h["request"]["surface_water"],
                &h["request"]["mechanical_coupling"],
            )
            .is_err()
        );
    }
}

#[test]
fn outgoing_recoil_requires_both_qualified_phases_on_every_interval() {
    let field = "outgoing_stream_velocities_relative_to_cloth_m_s_by_interval";
    let (a, mut d, s, mut m) = draining_case(true);
    schedule_phase_velocities(&mut d, &mut m, true);
    for fault in 0..9 {
        let (mut changed, mut descriptor) = (m.clone(), d.clone());
        let mut q = changed["liquid_mechanics"]["qualification"].clone();
        let binding = &mut q["bindings"]["back"];
        match fault {
            0 => {
                binding.as_object_mut().unwrap().remove(field);
            }
            1 => binding[field] = Value::Null,
            2 => binding[field] = json!([]),
            3 => binding[field]
                .as_array_mut()
                .unwrap()
                .push(json!({"evaporation":[0.,0.,0.],"drainage":[0.,0.,0.]})),
            4 => binding[field][0]["evaporation"] = json!([0., 0.]),
            5 => binding[field][0]["drainage"] = json!([0., null, 0.]),
            6 => {
                binding[field][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("drainage");
            }
            7 => binding[field][0]["extra"] = json!([0., 0., 0.]),
            _ => {
                binding[field][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("evaporation");
                binding[field][0]["supply"] = json!([0., 0., 0.]);
            }
        }
        replace_mechanical_qualification(&mut descriptor, &mut changed, q);
        assert!(
            admit_heat(&a, &descriptor, &s, &changed, Consumer::TransientSurface).is_err(),
            "fault {fault}"
        );
    }
}

#[test]
fn recoil_work_requires_complete_monotone_accounting_and_actual_outgoing_mass() {
    let (a, mut d, s, mut m) = draining_case(true);
    schedule_phase_velocities(&mut d, &mut m, true);
    // A nonzero qualification keeps these receipt checks independent of the zero-recoil control.
    let mut q = m["liquid_mechanics"]["qualification"].clone();
    q["bindings"]["front"]["outgoing_stream_velocities_relative_to_cloth_m_s_by_interval"][0]["drainage"] =
        json!([0., 0., 1.]);
    replace_mechanical_qualification(&mut d, &mut m, q);
    for fault in 0..5 {
        let mut changed = m.clone();
        if fault == 0 {
            changed["initial_sample"]["liquid_mechanics"]["exchange"]["outgoing_separation_work_j"] =
                json!(1.);
        } else {
            for path in [
                "/liquid_mechanics/exchange",
                "/transient_surface/result/mechanical_coupling/exchange",
                "/transient_surface/result/trace/0/mechanical_coupling/exchange",
            ] {
                let flow = changed.pointer_mut(path).unwrap();
                match fault {
                    1 => {
                        flow.as_object_mut()
                            .unwrap()
                            .remove("outgoing_separation_work_j");
                    }
                    2 => flow["outgoing_separation_work_j"] = Value::Null,
                    3 => flow["outgoing_separation_work_j"] = json!(-1.),
                    _ => flow["mixing_dissipation_j"] = Value::Null,
                }
            }
        }
        assert!(
            admit_heat(&a, &d, &s, &changed, Consumer::TransientSurface).is_err(),
            "fault {fault}"
        );
    }
    // Isolate cumulative receipt admission: a second retained sample with unchanged outgoing
    // mass cannot manufacture work, and a later sample cannot erase already supplied work.
    for (first, last) in [(0., 1.), (1., 0.)] {
        let mut changed = m.clone();
        for path in [
            "/liquid_mechanics/exchange",
            "/transient_surface/result/mechanical_coupling/exchange",
            "/transient_surface/result/trace/0/mechanical_coupling/exchange",
        ] {
            changed.pointer_mut(path).unwrap()["outgoing_separation_work_j"] = json!(last);
        }
        let h = &mut changed["transient_surface"]["result"];
        let mut earlier = h["trace"][0].clone();
        earlier["mechanical_coupling"]["exchange"]["outgoing_separation_work_j"] = json!(first);
        h["trace"].as_array_mut().unwrap().insert(0, earlier);
        let h = &changed["transient_surface"]["result"];
        assert!(
            admit_mechanics(
                &s,
                &changed,
                h,
                &h["request"],
                false,
                &h["request"]["surface_water"],
                &h["request"]["mechanical_coupling"],
            )
            .is_err(),
            "{first} -> {last}"
        );
    }
}

#[test]
fn zero_relative_outflow_forbids_work_without_inventing_flow_from_an_inactive_phase() {
    let (a, mut d, s, mut m) = exchanging_case(false);
    schedule_phase_velocities(&mut d, &mut m, true);
    assert!(admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_ok());
    let mut changed = m.clone();
    for path in [
        "/liquid_mechanics/exchange",
        "/transient_surface/result/mechanical_coupling/exchange",
        "/transient_surface/result/trace/0/mechanical_coupling/exchange",
    ] {
        changed.pointer_mut(path).unwrap()["outgoing_separation_work_j"] = json!(1e-12);
    }
    // Actual evaporation is positive, so an outgoing-mass check alone cannot reject invented work.
    assert!(
        m["liquid_mechanics"]["exchange"]["outgoing_mass_kg"]
            .as_f64()
            .unwrap()
            > 0.
    );
    assert!(admit_heat(&a, &d, &s, &changed, Consumer::TransientSurface).is_err());
    let mut q = m["liquid_mechanics"]["qualification"].clone();
    q["bindings"]["front"]["outgoing_stream_velocities_relative_to_cloth_m_s_by_interval"][0]["drainage"] =
        json!([0., 0., 1.]);
    replace_mechanical_qualification(&mut d, &mut m, q);
    // This water policy has no drainage; a qualified nonzero velocity does not create its mass/work.
    assert!(admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_ok());
}

#[test]
fn older_mechanical_models_cannot_acquire_new_velocity_fields_or_recoil_work() {
    for model in 1..=5 {
        let (a, mut d, s, mut m) = exchanging_case(false);
        match model {
            2 => prescribe_incoming_velocity(&mut d, &mut m),
            3 => schedule_incoming_velocity(&mut d, &mut m),
            4 => qualify_incoming_velocities(&mut d, &mut m, true),
            5 => schedule_phase_velocities(&mut d, &mut m, false),
            _ => {}
        }
        assert!(admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_ok());
        let mut changed = m.clone();
        for path in [
            "/liquid_mechanics/exchange",
            "/transient_surface/result/mechanical_coupling/exchange",
            "/transient_surface/result/trace/0/mechanical_coupling/exchange",
        ] {
            changed.pointer_mut(path).unwrap()["outgoing_separation_work_j"] = json!(0.);
        }
        assert!(
            admit_heat(&a, &d, &s, &changed, Consumer::TransientSurface).is_err(),
            "work on /{model}"
        );
        for field in [
            "incoming_stream_velocities_world_m_s_by_interval",
            "outgoing_stream_velocities_relative_to_cloth_m_s_by_interval",
        ] {
            if model == 5 && field == "incoming_stream_velocities_world_m_s_by_interval" {
                continue;
            }
            let (mut changed, mut descriptor) = (m.clone(), d.clone());
            let mut q = changed["liquid_mechanics"]["qualification"].clone();
            q["bindings"]["front"][field] = Value::Null;
            replace_mechanical_qualification(&mut descriptor, &mut changed, q);
            assert!(
                admit_heat(&a, &descriptor, &s, &changed, Consumer::TransientSurface).is_err(),
                "{field} on /{model}"
            );
        }
    }
}

#[test]
fn scheduled_incoming_velocity_admission_requires_exact_complete_source_intervals() {
    for condensation in [false, true] {
        let (a, mut d, s, mut m) = exchanging_case(condensation);
        let h = &m["transient_surface"]["result"];
        let expected = admit(&a, &s, &m, h, &h["request"], false, &[3., 3.]).unwrap();
        schedule_incoming_velocity(&mut d, &mut m);
        assert!(admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_ok());
        let h = &m["transient_surface"]["result"];
        assert_eq!(
            admit(&a, &s, &m, h, &h["request"], false, &[3., 3.]).unwrap(),
            expected
        );
        for fault in 0..13 {
            let mut changed = m.clone();
            let mut descriptor = d.clone();
            let mut q = changed["liquid_mechanics"]["qualification"].clone();
            let binding = &mut q["bindings"]["front"];
            match fault {
                0 => binding["incoming_velocity_world_m_s_by_interval"] = json!([]),
                1 => {
                    binding["incoming_velocity_world_m_s_by_interval"] =
                        json!([[1., 0., 0.], [1., 0., 0.]])
                }
                2 => binding["incoming_velocity_world_m_s_by_interval"] = json!([[1., 0.]]),
                3 => binding["incoming_velocity_world_m_s_by_interval"] = json!([[1., null, 0.]]),
                4 => {
                    binding
                        .as_object_mut()
                        .unwrap()
                        .remove("incoming_velocity_world_m_s_by_interval");
                }
                5 => binding["incoming_velocity_world_m_s_by_interval"] = Value::Null,
                6 => binding["incoming_velocity_world_m_s"] = json!([1., 0., 0.]),
                7 => q["schema"] = json!("ftx-coupled-liquid-mechanics/2"),
                8 => {
                    q["model"] =
                        json!("prescribed_incoming_velocity_mass_exchange_frozen_mechanics/1")
                }
                9 => binding["incoming_velocity_world_m_s"] = Value::Null,
                10 => q["qualification"] = json!(""),
                12 => {
                    binding["incoming_stream_velocities_world_m_s"] =
                        json!({"supply":[0.,0.,0.],"condensation":[0.,0.,0.]})
                }
                _ => binding["incoming_velocity_world_m_s_by_interval"] = json!([1., 0., 0.]),
            }
            for path in [
                "/initial_sample/liquid_mechanics",
                "/liquid_mechanics",
                "/transient_surface/result/mechanical_coupling",
            ] {
                changed.pointer_mut(path).unwrap()["qualification"] = q.clone();
            }
            let heat = &mut changed["transient_surface"]["result"];
            heat["request"]["mechanical_coupling"] = q;
            descriptor["scenario"]["motion"]["transient_surface"] = heat["request"].clone();
            assert!(
                admit_heat(&a, &descriptor, &s, &changed, Consumer::TransientSurface).is_err(),
                "fault {fault}"
            );
        }
        let mut stale = m.clone();
        stale["initial_sample"]["liquid_mechanics"]["schema"] =
            json!("ftx-coupled-liquid-mechanics-result/2");
        assert!(admit_heat(&a, &d, &s, &stale, Consumer::TransientSurface).is_err());
    }
}

#[test]
fn scheduled_mechanical_policy_changes_only_motion_comparison_protocols() {
    let (a, mut d, s, mut m) = exchanging_case(false);
    schedule_incoming_velocity(&mut d, &mut m);
    let admitted = Numerical {
        cutting_program: None,
        finite_stock_program: None,
        cutting_job_program: None,
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
        transient_surface: None,
        transient_coverage: None,
    };
    for (metric, moving) in [
        ("motion_max_absolute_edge_strain_pct", true),
        ("max_absolute_edge_strain_pct", false),
        ("assembly_mass_kg", false),
    ] {
        let policy: Policy = serde_json::from_value(json!({"basis":"nominal",
            "objectives":[{"metric":metric,"direction":"minimize"}]}))
        .unwrap();
        let original = policy.protocol(&a, &d, &admitted).unwrap();
        for changed_key in ["velocity", "source"] {
            let mut changed = d.clone();
            let q = &mut changed["scenario"]["motion"]["transient_surface"]["mechanical_coupling"];
            if changed_key == "velocity" {
                q["bindings"]["front"]["incoming_velocity_world_m_s_by_interval"] =
                    json!([[0., 1., 0.]]);
            } else {
                q["source"]["reference"] = json!("test:independent-inflow-qualification");
            }
            let actual = policy.protocol(&a, &changed, &admitted).unwrap();
            assert_eq!(original != actual, moving, "{metric}: {changed_key}");
        }
    }
}

#[test]
fn prescribed_incoming_velocity_keeps_thermal_objectives_independent_of_bulk_mixing() {
    for (condensation, phase_specific) in
        [(false, false), (true, false), (false, true), (true, true)]
    {
        let (a, mut d, s, mut m) = exchanging_case(condensation);
        let heat = &m["transient_surface"]["result"];
        let expected = admit(&a, &s, &m, heat, &heat["request"], false, &[3., 3.]).unwrap();
        qualify_incoming_velocities(&mut d, &mut m, phase_specific);
        assert!(admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_ok());
        let heat = &m["transient_surface"]["result"];
        let actual = admit(&a, &s, &m, heat, &heat["request"], false, &[3., 3.]).unwrap();
        assert_eq!(actual, expected);
    }
}

#[test]
fn phase_specific_velocity_requires_both_streams_exact_schema_and_mixing_ledger() {
    for fault in 0..12 {
        let (a, mut d, s, mut m) = exchanging_case(true);
        qualify_incoming_velocities(&mut d, &mut m, true);
        let mut q = m["liquid_mechanics"]["qualification"].clone();
        for binding in q["bindings"].as_object_mut().unwrap().values_mut() {
            let streams = &mut binding["incoming_stream_velocities_world_m_s"];
            match fault {
                0 => {
                    streams.as_object_mut().unwrap().remove("supply");
                }
                1 => {
                    streams.as_object_mut().unwrap().remove("condensation");
                }
                2 => *streams = Value::Null,
                3 => streams["supply"] = json!([1., 0.]),
                4 => streams["condensation"] = json!([1., null, 0.]),
                5 => streams["extra"] = json!([0., 0., 0.]),
                6 => binding["incoming_velocity_world_m_s"] = json!([1., 0., 0.]),
                10 => binding["incoming_velocity_world_m_s_by_interval"] = json!([[0., 0., 0.]]),
                _ => {}
            }
        }
        if fault == 7 {
            q["schema"] = json!("ftx-coupled-liquid-mechanics/2");
            q["model"] = json!("prescribed_incoming_velocity_mass_exchange_frozen_mechanics/1");
        }
        if fault == 11 {
            q["model"] = json!("scheduled_incoming_velocity_mass_exchange_frozen_mechanics/1");
        }
        for path in [
            "/initial_sample/liquid_mechanics",
            "/liquid_mechanics",
            "/transient_surface/result/mechanical_coupling",
        ] {
            let report = m.pointer_mut(path).unwrap();
            report["qualification"] = q.clone();
            if fault == 8 {
                report["schema"] = json!("ftx-coupled-liquid-mechanics-result/2");
            }
            if fault == 9 {
                report["exchange"]
                    .as_object_mut()
                    .unwrap()
                    .remove("mixing_dissipation_j");
            }
        }
        let heat = &mut m["transient_surface"]["result"];
        heat["request"]["mechanical_coupling"] = q;
        d["scenario"]["motion"]["transient_surface"] = heat["request"].clone();
        assert!(
            admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_err(),
            "fault {fault}"
        );
    }
}

#[test]
fn incomplete_velocity_qualification_and_mixing_accounting_refuse_decision_admission() {
    for fault in 0..8 {
        let (a, mut d, s, mut m) = exchanging_case(true);
        prescribe_incoming_velocity(&mut d, &mut m);
        if fault < 3 || fault == 7 {
            let mut qualification = m["liquid_mechanics"]["qualification"].clone();
            for binding in qualification["bindings"]
                .as_object_mut()
                .unwrap()
                .values_mut()
            {
                match fault {
                    0 => {
                        binding
                            .as_object_mut()
                            .unwrap()
                            .remove("incoming_velocity_world_m_s");
                    }
                    1 => binding["incoming_velocity_world_m_s"] = json!([1., 0.]),
                    2 => binding["incoming_velocity_world_m_s"] = json!([1., null, 0.]),
                    _ => {}
                }
            }
            if fault == 7 {
                qualification["schema"] = json!("ftx-coupled-liquid-mechanics/1");
                qualification["model"] = json!("co_moving_mass_exchange_frozen_mechanics/1");
            }
            for path in [
                "/initial_sample/liquid_mechanics",
                "/liquid_mechanics",
                "/transient_surface/result/mechanical_coupling",
            ] {
                m.pointer_mut(path).unwrap()["qualification"] = qualification.clone();
            }
            let heat = &mut m["transient_surface"]["result"];
            heat["request"]["mechanical_coupling"] = qualification;
            d["scenario"]["motion"]["transient_surface"] = heat["request"].clone();
        } else if fault == 3 {
            m["liquid_mechanics"]["schema"] = json!("ftx-coupled-liquid-mechanics-result/1");
            m["transient_surface"]["result"]["mechanical_coupling"]["schema"] =
                json!("ftx-coupled-liquid-mechanics-result/1");
        } else if fault == 6 {
            m["initial_sample"]["liquid_mechanics"]["exchange"]["mixing_dissipation_j"] = json!(1.);
        } else {
            for path in [
                "/liquid_mechanics/exchange",
                "/transient_surface/result/mechanical_coupling/exchange",
                "/transient_surface/result/trace/0/mechanical_coupling/exchange",
            ] {
                let flow = m.pointer_mut(path).unwrap();
                if fault == 4 {
                    flow.as_object_mut().unwrap().remove("mixing_dissipation_j");
                } else {
                    flow["mixing_dissipation_j"] = json!(-1.);
                }
            }
        }
        assert!(
            admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_err(),
            "fault {fault}"
        );
    }
}

#[test]
fn supplied_and_condensed_mechanical_mass_admit_the_same_independent_thermal_objectives() {
    for condensation in [false, true] {
        let (a, d, s, m) = exchanging_case(condensation);
        assert!(admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_ok());
        let h = &m["transient_surface"]["result"];
        let actual = admit(&a, &s, &m, h, &h["request"], false, &[3., 3.]).unwrap();
        let (a, _, s, m) = if condensation {
            condensing_case()
        } else {
            supplied_case()
        };
        let h = &m["transient_surface"]["result"];
        let expected = admit(&a, &s, &m, h, &h["request"], false, &[3., 3.]).unwrap();
        assert_eq!(actual, expected);
    }
}

#[test]
fn missing_or_invented_incoming_transfer_cannot_enter_coupled_decisions() {
    for fault in 0..6 {
        let (a, d, s, mut m) = exchanging_case(true);
        let (key, value) = match fault {
            0 => ("incoming_mass_kg", json!(0.)),
            1 => ("outgoing_mass_kg", json!(0.)),
            2 => ("incoming_momentum_kg_m_s", json!([0., 0.])),
            3 => ("incoming_kinetic_energy_j", json!(-1.)),
            4 => ("initial_gravitational_energy_j", json!(1.)),
            _ => ("incoming_gravitational_energy_j", Value::Null),
        };
        m["liquid_mechanics"]["exchange"][key] = value.clone();
        let h = &mut m["transient_surface"]["result"];
        h["mechanical_coupling"]["exchange"][key] = value.clone();
        h["trace"][0]["mechanical_coupling"]["exchange"][key] = value;
        assert!(
            admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_err(),
            "fault {fault}"
        );
    }
}

#[test]
fn no_incoming_mass_cannot_carry_invented_momentum_or_bulk_energy() {
    let (a, mut d, s, mut m) = coupled_case();
    let mut q = m["liquid_mechanics"]["qualification"].clone();
    q["schema"] = json!("ftx-coupled-liquid-mechanics/1");
    q["model"] = json!("co_moving_mass_exchange_frozen_mechanics/1");
    for path in [
        "/initial_sample/liquid_mechanics",
        "/liquid_mechanics",
        "/transient_surface/result/mechanical_coupling",
    ] {
        let report = m.pointer_mut(path).unwrap();
        report["schema"] = json!("ftx-coupled-liquid-mechanics-result/1");
        report["qualification"] = q.clone();
        report["exchange"] = json!({"incoming_mass_kg":0.,
            "outgoing_mass_kg":report["evaporated_liquid_mass_kg"],
            "initial_momentum_kg_m_s":[0.,0.,0.],"initial_kinetic_energy_j":0.,
            "initial_gravitational_energy_j":0.,"incoming_momentum_kg_m_s":[0.,0.,0.],
            "incoming_kinetic_energy_j":0.,"incoming_gravitational_energy_j":0.});
    }
    let flow = m["liquid_mechanics"]["exchange"].clone();
    let heat = &mut m["transient_surface"]["result"];
    heat["request"]["mechanical_coupling"] = q;
    heat["trace"][0]["mechanical_coupling"]["exchange"] = flow;
    d["scenario"]["motion"]["transient_surface"] = heat["request"].clone();
    assert!(admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_ok());
    for (key, value) in [
        ("incoming_momentum_kg_m_s", json!([1., 0., 0.])),
        ("incoming_kinetic_energy_j", json!(1.)),
        ("incoming_gravitational_energy_j", json!(-1.)),
    ] {
        let mut changed = m.clone();
        for path in [
            "/liquid_mechanics/exchange",
            "/transient_surface/result/mechanical_coupling/exchange",
            "/transient_surface/result/trace/0/mechanical_coupling/exchange",
        ] {
            changed.pointer_mut(path).unwrap()[key] = value.clone();
        }
        assert!(
            admit_heat(&a, &d, &s, &changed, Consumer::TransientSurface).is_err(),
            "invented {key}"
        );
    }
    prescribe_incoming_velocity(&mut d, &mut m);
    assert!(admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_ok());
    for path in [
        "/liquid_mechanics/exchange",
        "/transient_surface/result/mechanical_coupling/exchange",
        "/transient_surface/result/trace/0/mechanical_coupling/exchange",
    ] {
        m.pointer_mut(path).unwrap()["mixing_dissipation_j"] = json!(1.);
    }
    assert!(admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_err());
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

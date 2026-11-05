//! Synthetic receipt consistency, not a replacement physical solver or empirical heat evidence.
use super::*;
use crate::material_study::{Metrics, Outcome};
use ftx_pattern::{material_binding::Changes, material_catalog::Selection, materials::Catalog};

pub(super) fn case(combined: bool) -> (Alternative, Value, Value, Value) {
    let mut records = vec![];
    let mut motion = json!({"duration_s":1.,"path_sha256":"a".repeat(64),
        "initial_positions_sha256":"b".repeat(64),"triangles":[[0,1,2],[3,4,5]],
        "initial_sample":{"time_s":0.},"keyframes":[{"sample_index":0}],
        "samples":[{"time_s":1.,"positions_m":[[0.,0.,0.]],"body_positions_m":[[0.,0.,-1.]]}]});
    let mut request = json!({"schema":"ftx-motion-surface-heat/1","initial_temperature_c":20.,
        "boundaries":[{}],"cells_per_layer":2,"max_cell_steps":1000});
    let mut heat = json!({"schema":"ftx-motion-surface-heat-result/1","duration_s":1.,
        "path_sha256":motion["path_sha256"],"initial_positions_sha256":motion["initial_positions_sha256"],
        "thermal_steps":1,"cell_steps":258,"inside_energy_j":2.,"outside_energy_j":3.,
        "net_boundary_energy_j":-1.,"stored_change_j":-1.,"balance_error_j":0.,
        "absolute_step_balance_error_j":0.,"trace":[{"sample_index":0,"interval_index":0,
            "time_s":1.,"positions_sha256":hash(&motion["samples"][0]["positions_m"]).unwrap(),
            "thermal_substeps":1,"temperature_range_c":[20.,21.],"inside_energy_j":2.,
            "outside_energy_j":3.,"net_boundary_energy_j":-1.,"stored_change_j":-1.,"balance_error_j":0.}]});
    for (index, fixture, policy, property, array, initial, final_history, total, baseline) in [
        (
            0,
            include_str!("../../../../../tests/fixtures/materials/synthetic-relaxation-heat.json"),
            "relaxation_dissipation_heat",
            "membrane_relaxation_heat_fraction",
            "membrane_relaxation_dissipation_j",
            "initial_membrane_history",
            "membrane_history",
            "viscous_dissipation_j",
            "initial_viscous_dissipation_j",
        ),
        (
            1,
            include_str!(
                "../../../../../tests/fixtures/materials/synthetic-membrane-plasticity.json"
            ),
            "plastic_dissipation_heat",
            "membrane_plastic_heat_fraction",
            "membrane_plastic_dissipation_j",
            "initial_membrane_plasticity_history",
            "membrane_plasticity_history",
            "plastic_dissipation_j",
            "initial_plastic_dissipation_j",
        ),
    ] {
        if index == 1 && !combined {
            continue;
        }
        let mut raw: Value = serde_json::from_str(fixture).unwrap();
        if index == 1 {
            raw["record"]["properties"][property] = json!({"value":0.5,"unit":"1",
                "source":"test","evidence":"estimate","method":"Synthetic conversion",
                "conditions":{"specimen_id":"artificial-stretch-A","conditioning":"synthetic constant dry state",
                    "work_basis":"irreversible_membrane_plastic_dissipation","temperature_min_c":"-20","temperature_max_c":"80"}});
        }
        let material = Catalog::builtin()
            .unwrap()
            .resolve(&serde_json::from_value::<Selection>(raw).unwrap())
            .unwrap();
        let (fraction, range, law) = if index == 0 {
            let f = material
                .membrane_relaxation_heat_fraction()
                .unwrap()
                .unwrap();
            (
                f.fraction,
                f.temperature_range_c,
                material
                    .membrane_relaxation()
                    .unwrap()
                    .unwrap()
                    .fingerprint()
                    .to_owned(),
            )
        } else {
            let f = material.membrane_plastic_heat_fraction().unwrap().unwrap();
            (
                f.fraction,
                f.temperature_range_c,
                material
                    .membrane_plasticity()
                    .unwrap()
                    .unwrap()
                    .fingerprint(),
            )
        };
        let history_total = if index == 0 {
            "viscous_step_work_j"
        } else {
            total
        };
        let mut before = [0.; 2];
        before[index] = 2.;
        let mut after = before;
        after[index] = 6.;
        motion["initial_sample"][array] = json!(before);
        motion["samples"][0][array] = json!(after);
        motion[initial] = json!({history_total:2.,"elements":[{"triangle":index,"law_sha256":law,
            "vertices":motion["triangles"][index],"plastic_dissipation_j":2.}]});
        if !combined {
            let other = json!({"triangle":1,"law_sha256":law,
                "vertices":motion["triangles"][1],"plastic_dissipation_j":0.});
            motion[initial]["elements"]
                .as_array_mut()
                .unwrap()
                .push(other);
        }
        motion[final_history] = json!({history_total:6.});
        let work = json!({"distribution":"uniform_reference_thickness",
            "temporal_distribution":"uniform_over_accepted_interval",total:4.,baseline:2.,
            "supplied_heat_j":fraction*4.,"unconverted_dissipation_j":(1.-fraction)*4.});
        heat[policy] = work.clone();
        heat["trace"][0][policy] = work;
        heat[policy]["material_fractions"] = json!([{"material_sha256":material.sha256(),
            "law_sha256":law,"fraction":fraction,"temperature_range_c":range,
            "observation":material.authored().properties[property]}]);
        heat["stored_change_j"] = json!(heat["stored_change_j"].as_f64().unwrap() + fraction * 4.);
        heat["trace"][0]["stored_change_j"] = heat["stored_change_j"].clone();
        request[policy] = json!("uniform_reference_thickness");
        records.push(material.frozen_selection());
    }
    heat["request"] = request.clone();
    motion["transient_surface"] = json!({"status":"available","result":heat});
    let changes: Changes = serde_json::from_value(json!({"schema":"ftx-material-bindings/1",
        "default":records[0],"pieces":if combined {json!({"back":records[1]})} else {json!({})}}))
    .unwrap();
    let mut alternative = Alternative {
        choices: BTreeMap::new(),
        key: "synthetic".into(),
        design: None,
        binding: Outcome::NotRequested,
        frozen_changes: Some(changes),
        cutting: Outcome::NotRequested,
        finite_stock: Outcome::NotRequested,
        cutting_job: Outcome::NotRequested,
        sewing: Outcome::NotRequested,
        sewing_selection: Outcome::NotRequested,
        purchasing: Outcome::NotRequested,
        cut_and_sew_purchasing: Outcome::NotRequested,
        assembly_mass: Outcome::NotRequested,
        vapor: Outcome::NotRequested,
        metrics: Metrics::default(),
        cutting_effort: Default::default(),
    };
    let simulation = source(&mut alternative, &mut motion);
    (
        alternative,
        json!({"scenario":{"motion":{"transient_surface":request}}}),
        simulation,
        motion,
    )
}

// A complete retained source makes the receipt tests exercise the same material/owner mapping
// as production, without treating a synthetic energy ledger as an independent thermal solution.
fn source(alternative: &mut Alternative, motion: &mut Value) -> Value {
    let pattern = ftx_pattern::Pattern::from_json(include_str!(
        "../../../../../tests/fixtures/materials/synthetic-sewing-pattern.json"
    ))
    .unwrap();
    let changes = alternative.frozen_changes.as_ref().unwrap();
    alternative.binding = Outcome::Available {
        result: changes.apply(&pattern).unwrap(),
    };
    let catalog = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: vec![],
    };
    let mut materials: Vec<ResolvedMaterial> = Vec::new();
    let mut assignment = Vec::new();
    let mut report = json!({"basis":"per_piece_characterization",
        "instances":[],"triangle_instance":[0,1],"records":{}});
    for instance in ["front", "back"] {
        let material = catalog
            .resolve(
                changes
                    .pieces
                    .get(instance)
                    .or(changes.default.as_ref())
                    .unwrap(),
            )
            .unwrap();
        report["instances"]
            .as_array_mut()
            .unwrap()
            .push(json!({"instance":instance,"record_sha256":material.sha256()}));
        report["records"][material.sha256()] = json!({"record":material});
        let column = materials
            .iter()
            .position(|other| other.sha256() == material.sha256())
            .unwrap_or_else(|| {
                materials.push(material);
                materials.len() - 1
            });
        assignment.push(column);
    }
    let positions = json!([
        [0., 0., 0.],
        [0.02, 0., 0.],
        [0., 0.1, 0.],
        [0., 0.2, 0.],
        [0.02, 0.2, 0.],
        [0., 0.3, 0.]
    ]);
    let patches = json!([
        {"vertices":[0,1,2],"reference_area_m2":0.001,"column":assignment[0]},
        {"vertices":[3,4,5],"reference_area_m2":0.001,"column":assignment[1]}]);
    let model = |cells: usize| {
        let columns: Vec<_> = materials
            .iter()
            .map(|material| {
                let law = ftx_pattern::material_thermal::ThermalLaw::from_material(
                    material.material(),
                    material.allows_estimates(),
                )
                .unwrap();
                let capacity = law.capacity_at(20.).unwrap() / cells as f64;
                let resistance = material.thermal_resistance().unwrap() / cells as f64;
                json!({"schema":"ftx-dry-thermal-model/1","layers":[material],
                    "cells_per_layer":cells,"interface_resistance_m2_k_w":[],
                    "cell_capacity_j_m2_k":vec![capacity;cells],
                    "cell_resistance_m2_k_w":vec![resistance;cells]})
            })
            .collect();
        let mut model = json!({"schema":"ftx-thermal-surface/1","vertex_count":6,
            "columns":columns,"patches":patches});
        model["sha256"] = json!(hash(&model).unwrap());
        model
    };
    let mut frame = json!({"schema":"ftx-drape-thermal-surface/1","available":true,
        "model":model(4),"instances":["front","back"],"triangle_instance":[0,1],
        "positions_m":positions,"material_report_sha256":hash(&report).unwrap()});
    frame["sha256"] = json!(hash(&frame).unwrap());
    motion["initial_positions_sha256"] = json!(hash(&positions).unwrap());
    motion["samples"][0]["positions_m"] = positions.clone();
    for name in ["transient_surface", "transient_coverage"] {
        if motion[name]["status"] != "available" {
            continue;
        }
        let heat = &mut motion[name]["result"];
        heat["initial_positions_sha256"] = json!(hash(&positions).unwrap());
        heat["trace"][0]["positions_sha256"] = json!(hash(&positions).unwrap());
        if name == "transient_surface" {
            heat["model"] = model(2);
        } else {
            heat["checkpoint"] = json!({"frame":{"source":model(2)}});
        }
    }
    json!({"positions_m":positions,"positions_sha256":hash(&positions).unwrap(),
        "triangles":motion["triangles"],"drape":{"material":report,"thermal_surface":frame}})
}

#[test]
fn qualified_maxwell_and_disjoint_plastic_sources_reconcile_storage_once() {
    for combined in [false, true] {
        let (a, d, s, mut m) = case(combined);
        assert!(admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_ok());
        // Boundary energy is -1 J, Maxwell heat is 3 J, optional plastic heat is 2 J.
        assert_eq!(
            m["transient_surface"]["result"]["stored_change_j"],
            if combined { 4. } else { 2. }
        );
        m["transient_surface"]["result"]["stored_change_j"] = json!(5.);
        assert!(admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_err());
    }
}

#[test]
fn incomplete_surface_water_cannot_pass_the_dry_history_comparison_ledger() {
    for location in 0..3 {
        let (a, mut descriptor, s, mut motion) = case(false);
        match location {
            0 => {
                descriptor["scenario"]["motion"]["transient_surface"]["surface_water"] =
                    json!({"schema":"ftx-textile-surface-water/1"});
                motion["transient_surface"]["result"]["request"] =
                    descriptor["scenario"]["motion"]["transient_surface"].clone();
            }
            1 => motion["transient_surface"]["result"]["surface_water"] = json!({}),
            _ => motion["transient_surface"]["result"]["trace"][0]["surface_water"] = json!({}),
        }
        let refusal = admit_heat(&a, &descriptor, &s, &motion, Consumer::TransientSurface)
            .err()
            .unwrap();
        assert!(refusal.message.contains("liquid and vapor enthalpy ledger"));
        assert_eq!(motion["transient_surface"]["status"], "available");
    }
}

#[test]
fn inconsistent_maxwell_evidence_or_work_cannot_qualify_a_heat_objective() {
    for fault in 0..15 {
        let (a, d, s, mut m) = case(true);
        match fault {
            0 => m["initial_membrane_history"]["viscous_step_work_j"] = json!(0.),
            1 => m["initial_membrane_history"]["elements"][0]["law_sha256"] = json!("stale"),
            2 => m["initial_membrane_history"]["elements"][0]["triangle"] = json!(2),
            3 => m["samples"][0]["membrane_relaxation_dissipation_j"] = json!([1., 0.]),
            4 => m["samples"][0]["membrane_relaxation_dissipation_j"] = Value::Null,
            5 => m["samples"][0]["membrane_relaxation_dissipation_j"] = json!([6., 1.]),
            6 => m["membrane_history"]["viscous_step_work_j"] = json!(7.),
            _ => {
                let h = &mut m["transient_surface"]["result"];
                match fault {
                    7 => {
                        h["relaxation_dissipation_heat"]["material_fractions"][0]["fraction"] =
                            json!(1.)
                    }
                    8 => {
                        h["relaxation_dissipation_heat"]["material_fractions"][0]["observation"]["method"] =
                            json!("changed")
                    }
                    9 => h["relaxation_dissipation_heat"]["material_fractions"] = json!([]),
                    10 => {
                        h["trace"][0]["relaxation_dissipation_heat"]["supplied_heat_j"] = json!(4.)
                    }
                    11 => {
                        h["relaxation_dissipation_heat"]["initial_viscous_dissipation_j"] =
                            json!(0.)
                    }
                    12 => {
                        h["relaxation_dissipation_heat"]["unconverted_dissipation_j"] = json!(-1.)
                    }
                    13 => h["trace"][0]["stored_change_j"] = json!(2.), // omit the second source
                    _ => {
                        h["trace"][0]["relaxation_dissipation_heat"]["temporal_distribution"] =
                            json!("instantaneous")
                    }
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
fn maxwell_outputs_need_explicit_domain_policy_and_bound_source_identity() {
    let (mut a, mut d, s, mut m) = case(false);
    d["scenario"]["motion"]["transient_surface"]["relaxation_dissipation_heat"] = Value::Null;
    m["transient_surface"]["result"]["request"] =
        d["scenario"]["motion"]["transient_surface"].clone();
    assert!(admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_err());
    let (_, d, s, m) = case(false);
    a.frozen_changes
        .as_mut()
        .unwrap()
        .default
        .as_mut()
        .unwrap()
        .expected_sha256 = Some("unrelated".into());
    assert!(admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_err());
}

fn coverage_case(combined: bool) -> (Alternative, Value, Value, Value) {
    let (a, mut d, s, mut m) = case(combined);
    let mut request = d["scenario"]["motion"]["transient_surface"].clone();
    request["schema"] = json!("ftx-motion-coverage-heat/1");
    request["settings"] = json!({"cells_per_layer":2});
    request["exposed_boundaries"] = json!([null]);
    let mut heat = m["transient_surface"]["result"].clone();
    heat["schema"] = json!("ftx-motion-coverage-heat-result/1");
    let model = heat.as_object_mut().unwrap().remove("model").unwrap();
    heat["checkpoint"] = json!({"frame":{"source":model}});
    heat["request"] = request.clone();
    heat["physical_stored_change_j"] = heat
        .as_object_mut()
        .unwrap()
        .remove("stored_change_j")
        .unwrap();
    for field in [
        "bare_energy_j",
        "signed_remap_residual_j",
        "absolute_remap_residual_j",
    ] {
        heat[field] = json!(0.);
    }
    for (i, trace) in heat["trace"].as_array_mut().unwrap().iter_mut().enumerate() {
        trace["physical_stored_change_j"] = trace
            .as_object_mut()
            .unwrap()
            .remove("stored_change_j")
            .unwrap();
        trace["body_positions_sha256"] = json!(hash(&m["samples"][i]["body_positions_m"]).unwrap());
    }
    d["scenario"]["motion"]["transient_coverage"] = request;
    m["transient_coverage"] = json!({"status":"available","result":heat});
    (a, d, s, m)
}

#[test]
fn coverage_maxwell_and_mixed_sources_use_the_same_accepted_work_admission() {
    for combined in [false, true] {
        let (a, d, s, mut m) = coverage_case(combined);
        assert!(admit_heat(&a, &d, &s, &m, Consumer::TransientCoverage).is_ok());
        assert_eq!(
            m["transient_coverage"]["result"]["physical_stored_change_j"],
            if combined { 4. } else { 2. }
        );
        // An existing valid independent-sheet history cannot excuse a changed coverage source.
        m["transient_coverage"]["result"]["relaxation_dissipation_heat"]["material_fractions"][0]
            ["observation"]["method"] = json!("unrelated source");
        assert!(admit_heat(&a, &d, &s, &m, Consumer::TransientCoverage).is_err());
        assert!(admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_ok());
    }
}

fn same_law_case(second_work: f64) -> (Alternative, Value, Value, Value) {
    let (mut a, d, _, mut m) = coverage_case(false);
    let original = a.frozen_changes.as_ref().unwrap().default.as_ref().unwrap();
    let mut materials = Vec::new();
    for fraction in [0.25, 0.75] {
        let mut selection = serde_json::to_value(original).unwrap();
        selection["expected_sha256"] = Value::Null;
        selection["record"]["properties"]["membrane_relaxation_heat_fraction"]["value"] =
            json!(fraction);
        materials.push(
            Catalog::builtin()
                .unwrap()
                .resolve(&serde_json::from_value(selection).unwrap())
                .unwrap(),
        );
    }
    assert_ne!(materials[0].sha256(), materials[1].sha256());
    let law = materials[0].membrane_relaxation().unwrap().unwrap();
    assert_eq!(
        law.fingerprint(),
        materials[1]
            .membrane_relaxation()
            .unwrap()
            .unwrap()
            .fingerprint()
    );
    let changes = a.frozen_changes.as_mut().unwrap();
    changes.default = Some(materials[0].frozen_selection());
    changes
        .pieces
        .insert("back".into(), materials[1].frozen_selection());
    let work = 4. + second_work;
    let supplied = 0.25 * 4. + 0.75 * second_work;
    m["initial_sample"]["membrane_relaxation_dissipation_j"] = json!([2., 3.]);
    m["samples"][0]["membrane_relaxation_dissipation_j"] = json!([6., 3. + second_work]);
    m["initial_membrane_history"] = json!({"viscous_step_work_j":5.,"elements":[
        {"triangle":0,"law_sha256":law.fingerprint()},
        {"triangle":1,"law_sha256":law.fingerprint()}]});
    m["membrane_history"]["viscous_step_work_j"] = json!(5. + work);
    let receipts: Vec<_> = materials
        .iter()
        .map(|material| {
            let fraction = material
                .membrane_relaxation_heat_fraction()
                .unwrap()
                .unwrap();
            json!({"material_sha256":material.sha256(),"law_sha256":law.fingerprint(),
                "fraction":fraction.fraction,"temperature_range_c":fraction.temperature_range_c,
                "observation":material.authored().properties["membrane_relaxation_heat_fraction"]})
        })
        .collect();
    for (name, stored) in [
        ("transient_surface", "stored_change_j"),
        ("transient_coverage", "physical_stored_change_j"),
    ] {
        let heat = &mut m[name]["result"];
        let receipt = json!({"distribution":"uniform_reference_thickness",
            "temporal_distribution":"uniform_over_accepted_interval",
            "viscous_dissipation_j":work,"initial_viscous_dissipation_j":5.,
            "supplied_heat_j":supplied,"unconverted_dissipation_j":work-supplied});
        heat["relaxation_dissipation_heat"] = receipt.clone();
        heat["relaxation_dissipation_heat"]["material_fractions"] = json!(receipts);
        heat["trace"][0]["relaxation_dissipation_heat"] = receipt;
        heat[stored] = json!(-1. + supplied);
        heat["trace"][0][stored] = heat[stored].clone();
    }
    let s = source(&mut a, &mut m);
    (a, d, s, m)
}

fn rehash(value: &mut Value) {
    value.as_object_mut().unwrap().remove("sha256");
    value["sha256"] = json!(hash(value).unwrap());
}

#[test]
fn shared_maxwell_law_preserves_distinct_material_conversion_and_column_order() {
    let (a, d, mut s, mut m) = same_law_case(10.);
    for reordered in [false, true] {
        if reordered {
            let frame = &mut s["drape"]["thermal_surface"];
            let model = &mut frame["model"];
            model["columns"].as_array_mut().unwrap().reverse();
            model["patches"][0]["column"] = json!(1);
            model["patches"][1]["column"] = json!(0);
            rehash(model);
            rehash(frame);
            for name in ["transient_surface", "transient_coverage"] {
                let heat = &mut m[name]["result"];
                heat["relaxation_dissipation_heat"]["material_fractions"]
                    .as_array_mut()
                    .unwrap()
                    .reverse();
                let model = if name == "transient_surface" {
                    &mut heat["model"]
                } else {
                    &mut heat["checkpoint"]["frame"]["source"]
                };
                model["columns"].as_array_mut().unwrap().reverse();
                model["patches"][0]["column"] = json!(1);
                model["patches"][1]["column"] = json!(0);
                rehash(model);
            }
        }
        for consumer in [Consumer::TransientSurface, Consumer::TransientCoverage] {
            let heat = admit_heat(&a, &d, &s, &m, consumer).unwrap();
            // 4 J and 10 J of separate material work convert at 0.25 and 0.75, respectively.
            assert_eq!(
                heat.result["relaxation_dissipation_heat"]["supplied_heat_j"],
                8.5
            );
        }
    }
}

#[test]
fn same_law_sources_cannot_borrow_another_materials_qualification() {
    for (name, consumer, other) in [
        (
            "transient_surface",
            Consumer::TransientSurface,
            Consumer::TransientCoverage,
        ),
        (
            "transient_coverage",
            Consumer::TransientCoverage,
            Consumer::TransientSurface,
        ),
    ] {
        for fault in [
            "missing",
            "duplicate",
            "fraction",
            "foreign",
            "swapped_heat",
        ] {
            let (a, d, s, mut m) = same_law_case(10.);
            let heat = &mut m[name]["result"];
            let receipts = &mut heat["relaxation_dissipation_heat"]["material_fractions"];
            match fault {
                "missing" => {
                    receipts.as_array_mut().unwrap().pop();
                }
                "duplicate" => {
                    let first = receipts[0].clone();
                    receipts.as_array_mut().unwrap().push(first);
                }
                "fraction" => receipts[1]["fraction"] = json!(0.25),
                "foreign" => receipts[1]["material_sha256"] = json!("a".repeat(64)),
                "swapped_heat" => {
                    // Swapping the two fractions would produce 5.5 J, not the real 8.5 J.
                    let work = &mut heat["trace"][0]["relaxation_dissipation_heat"];
                    work["supplied_heat_j"] = json!(5.5);
                    work["unconverted_dissipation_j"] = json!(8.5);
                    let work = &mut heat["relaxation_dissipation_heat"];
                    work["supplied_heat_j"] = json!(5.5);
                    work["unconverted_dissipation_j"] = json!(8.5);
                }
                _ => unreachable!(),
            }
            assert!(
                admit_heat(&a, &d, &s, &m, consumer).is_err(),
                "{name}/{fault}"
            );
            assert!(admit_heat(&a, &d, &s, &m, other).is_ok(), "{name}/{fault}");
        }
    }
    for missing_receipt in [false, true] {
        let (a, d, s, mut m) = same_law_case(0.);
        m["initial_sample"]["membrane_relaxation_dissipation_j"][1] = json!(0.);
        m["samples"][0]["membrane_relaxation_dissipation_j"][1] = json!(0.);
        m["initial_membrane_history"]["viscous_step_work_j"] = json!(2.);
        m["membrane_history"]["viscous_step_work_j"] = json!(6.);
        for name in ["transient_surface", "transient_coverage"] {
            let heat = &mut m[name]["result"];
            heat["relaxation_dissipation_heat"]["initial_viscous_dissipation_j"] = json!(2.);
            heat["trace"][0]["relaxation_dissipation_heat"]["initial_viscous_dissipation_j"] =
                json!(2.);
        }
        assert!(admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_ok());
        if missing_receipt {
            m["transient_surface"]["result"]["relaxation_dissipation_heat"]["material_fractions"]
                .as_array_mut()
                .unwrap()
                .pop();
        } else {
            m["initial_membrane_history"]["elements"]
                .as_array_mut()
                .unwrap()
                .pop();
        }
        assert!(admit_heat(&a, &d, &s, &m, Consumer::TransientSurface).is_err());
    }
}

#[test]
fn rehashed_heat_sources_must_keep_actual_mechanical_and_frozen_instance_assignments() {
    for fault in ["columns", "owners", "record", "uniform", "frozen"] {
        let (mut a, d, mut s, mut m) = same_law_case(10.);
        match fault {
            "columns" => {
                // Rehash both thermal sources consistently while keeping actual mechanics fixed.
                let frame_model = &mut s["drape"]["thermal_surface"]["model"];
                frame_model["patches"][0]["column"] = json!(1);
                frame_model["patches"][1]["column"] = json!(0);
                rehash(frame_model);
                for name in ["transient_surface", "transient_coverage"] {
                    let heat = &mut m[name]["result"];
                    let model = if name == "transient_surface" {
                        &mut heat["model"]
                    } else {
                        &mut heat["checkpoint"]["frame"]["source"]
                    };
                    model["patches"][0]["column"] = json!(1);
                    model["patches"][1]["column"] = json!(0);
                    rehash(model);
                }
            }
            "owners" => s["drape"]["thermal_surface"]["triangle_instance"] = json!([1, 0]),
            "record" | "uniform" => {
                let report = &mut s["drape"]["material"];
                let first = report["instances"][0]["record_sha256"]
                    .as_str()
                    .unwrap()
                    .to_string();
                let second = report["instances"][1]["record_sha256"]
                    .as_str()
                    .unwrap()
                    .to_string();
                let record = report["records"][first.as_str()]["record"].clone();
                if fault == "record" {
                    report["records"][second.as_str()]["record"] = record;
                } else {
                    *report = json!({"basis":"characterized_record",
                        "record_sha256":first,"record":record});
                }
            }
            "frozen" => {
                let changes = a.frozen_changes.as_mut().unwrap();
                changes.default = changes.pieces.get("back").cloned();
            }
            _ => unreachable!(),
        }
        s["drape"]["thermal_surface"]["material_report_sha256"] =
            json!(hash(&s["drape"]["material"]).unwrap());
        rehash(&mut s["drape"]["thermal_surface"]);
        for consumer in [Consumer::TransientSurface, Consumer::TransientCoverage] {
            assert!(admit_heat(&a, &d, &s, &m, consumer).is_err(), "{fault}");
        }
    }
}

#[test]
fn coverage_heat_refuses_missing_work_partial_sources_and_unrequested_conversion() {
    for fault in 0..7 {
        let (a, mut d, s, mut m) = coverage_case(true);
        match fault {
            0 => m["samples"][0]["membrane_relaxation_dissipation_j"] = Value::Null,
            1 => m["samples"][0]["membrane_relaxation_dissipation_j"] = json!([1., 0.]),
            2 => {
                m["transient_coverage"]["result"]["trace"][0]["relaxation_dissipation_heat"] =
                    Value::Null
            }
            3 => m["transient_coverage"]["result"]["physical_stored_change_j"] = json!(2.),
            4 => {
                m["transient_coverage"]["result"]["relaxation_dissipation_heat"]["initial_viscous_dissipation_j"] =
                    json!(0.)
            }
            5 => {
                m["transient_coverage"]["result"]["relaxation_dissipation_heat"]["material_fractions"] =
                    json!([])
            }
            _ => {
                d["scenario"]["motion"]["transient_coverage"]["relaxation_dissipation_heat"] =
                    Value::Null;
                m["transient_coverage"]["result"]["request"] =
                    d["scenario"]["motion"]["transient_coverage"].clone();
            }
        }
        assert!(
            admit_heat(&a, &d, &s, &m, Consumer::TransientCoverage).is_err(),
            "fault {fault}"
        );
    }
}

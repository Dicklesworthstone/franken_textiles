//! Synthetic receipt consistency, not a replacement physical solver or empirical heat evidence.
use super::*;
use crate::material_study::{Metrics, Outcome};
use ftx_pattern::{material_binding::Changes, material_catalog::Selection, materials::Catalog};

fn case(combined: bool) -> (Alternative, Value, Value) {
    let mut records = vec![];
    let mut motion = json!({"duration_s":1.,"path_sha256":"a".repeat(64),
        "initial_positions_sha256":"b".repeat(64),"triangles":[[0,1,2],[3,4,5]],
        "initial_sample":{"time_s":0.},"keyframes":[{"sample_index":0}],
        "samples":[{"time_s":1.,"positions_m":[[0.,0.,0.]],"body_positions_m":[[0.,0.,-1.]]}]});
    let mut request = json!({"schema":"ftx-motion-surface-heat/1","initial_temperature_c":20.,
        "boundaries":[{}],"max_cell_steps":1000});
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
    let alternative = Alternative {
        choices: BTreeMap::new(),
        key: "synthetic".into(),
        binding: Outcome::NotRequested,
        frozen_changes: Some(changes),
        cutting: Outcome::NotRequested,
        finite_stock: Outcome::NotRequested,
        cutting_job: Outcome::NotRequested,
        sewing: Outcome::NotRequested,
        sewing_selection: Outcome::NotRequested,
        purchasing: Outcome::NotRequested,
        assembly_mass: Outcome::NotRequested,
        vapor: Outcome::NotRequested,
        metrics: Metrics::default(),
    };
    (
        alternative,
        json!({"scenario":{"motion":{"transient_surface":request}}}),
        motion,
    )
}

#[test]
fn qualified_maxwell_and_disjoint_plastic_sources_reconcile_storage_once() {
    for combined in [false, true] {
        let (a, d, mut m) = case(combined);
        assert!(admit_heat(&a, &d, &m, Consumer::TransientSurface).is_ok());
        // Boundary energy is -1 J, Maxwell heat is 3 J, optional plastic heat is 2 J.
        assert_eq!(
            m["transient_surface"]["result"]["stored_change_j"],
            if combined { 4. } else { 2. }
        );
        m["transient_surface"]["result"]["stored_change_j"] = json!(5.);
        assert!(admit_heat(&a, &d, &m, Consumer::TransientSurface).is_err());
    }
}

#[test]
fn inconsistent_maxwell_evidence_or_work_cannot_qualify_a_heat_objective() {
    for fault in 0..15 {
        let (a, d, mut m) = case(true);
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
            admit_heat(&a, &d, &m, Consumer::TransientSurface).is_err(),
            "fault {fault}"
        );
    }
}

#[test]
fn maxwell_outputs_need_explicit_domain_policy_and_bound_source_identity() {
    let (mut a, mut d, mut m) = case(false);
    d["scenario"]["motion"]["transient_surface"]["relaxation_dissipation_heat"] = Value::Null;
    m["transient_surface"]["result"]["request"] =
        d["scenario"]["motion"]["transient_surface"].clone();
    assert!(admit_heat(&a, &d, &m, Consumer::TransientSurface).is_err());
    let (_, d, m) = case(false);
    a.frozen_changes
        .as_mut()
        .unwrap()
        .default
        .as_mut()
        .unwrap()
        .expected_sha256 = Some("unrelated".into());
    assert!(admit_heat(&a, &d, &m, Consumer::TransientSurface).is_err());
}

fn coverage_case(combined: bool) -> (Alternative, Value, Value) {
    let (a, mut d, mut m) = case(combined);
    let mut request = d["scenario"]["motion"]["transient_surface"].clone();
    request["schema"] = json!("ftx-motion-coverage-heat/1");
    request["exposed_boundaries"] = json!([null]);
    let mut heat = m["transient_surface"]["result"].clone();
    heat["schema"] = json!("ftx-motion-coverage-heat-result/1");
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
    (a, d, m)
}

#[test]
fn coverage_maxwell_and_mixed_sources_use_the_same_accepted_work_admission() {
    for combined in [false, true] {
        let (a, d, mut m) = coverage_case(combined);
        assert!(admit_heat(&a, &d, &m, Consumer::TransientCoverage).is_ok());
        assert_eq!(
            m["transient_coverage"]["result"]["physical_stored_change_j"],
            if combined { 4. } else { 2. }
        );
        // An existing valid independent-sheet history cannot excuse a changed coverage source.
        m["transient_coverage"]["result"]["relaxation_dissipation_heat"]["material_fractions"][0]
            ["observation"]["method"] = json!("unrelated source");
        assert!(admit_heat(&a, &d, &m, Consumer::TransientCoverage).is_err());
        assert!(admit_heat(&a, &d, &m, Consumer::TransientSurface).is_ok());
    }
}

#[test]
fn coverage_heat_refuses_missing_work_partial_sources_and_unrequested_conversion() {
    for fault in 0..7 {
        let (a, mut d, mut m) = coverage_case(true);
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
            admit_heat(&a, &d, &m, Consumer::TransientCoverage).is_err(),
            "fault {fault}"
        );
    }
}

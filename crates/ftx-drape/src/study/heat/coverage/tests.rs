//! Exact-area shared-consumer regression; all geometry and coefficients are synthetic.
use super::*;
use crate::thermal::{DryThermalModel, surface::Patch};
use ftx_pattern::{material_catalog::Selection, materials::Catalog};

fn body(offset: f64) -> Vec<Vec3> {
    vec![
        Vec3::new(offset, 0., 0.),
        Vec3::new(offset + 1., 0., 0.),
        Vec3::new(offset, 1., 0.),
    ]
}

fn history() -> (History, Vec<Vec3>) {
    let mut fixture: Value = serde_json::from_str(include_str!(
        "../../../../../../tests/fixtures/materials/synthetic-exterior-exchange.json"
    ))
    .unwrap();
    for (key, value, unit) in [
        ("areal_density", 0.2, "kg/m2"),
        ("specific_heat", 1000., "J/(kg*K)"),
    ] {
        fixture["layers"][0]["record"]["properties"][key] = json!({
            "value":value,"unit":unit,"source":"synthetic","evidence":"estimate",
            "method":"Declared synthetic heat-storage coefficient"});
    }
    let selection: Selection = serde_json::from_value(fixture["layers"][0].clone()).unwrap();
    let material = Catalog::builtin().unwrap().resolve(&selection).unwrap();
    let source = Surface::new(
        vec![DryThermalModel::new(vec![material], vec![], 1).unwrap()],
        3,
        vec![Patch {
            vertices: [0, 1, 2],
            reference_area_m2: 0.5,
            column: 0,
        }],
    )
    .unwrap();
    let cloth = vec![
        Vec3::new(0., 0., 0.01),
        Vec3::new(1., 0., 0.01),
        Vec3::new(0., 1., 0.01),
    ];
    let triangles = vec![[0, 1, 2]];
    let projection: projection::Request = serde_json::from_value(json!({
        "schema":"ftx-thermal-projection/1","cloth_layers":["shell"],
        "settings":{"max_distance_m":0.1,"min_normal_cosine":0.9,
            "separation_tolerance_m":1e-9,"cells_per_layer":1,"interfaces":[]}
    }))
    .unwrap();
    let frame = Frame::new(
        source.clone(),
        cloth.clone(),
        body(0.),
        &triangles,
        projection.clone(),
    )
    .unwrap();
    let film = Film {
        inside_c: 35.,
        outside_c: 5.,
        inside_resistance_m2_k_w: 0.1,
        outside_resistance_m2_k_w: 0.2,
    };
    let exposed = Film {
        outside_resistance_m2_k_w: 0.,
        ..film
    };
    let exterior: Exterior = serde_json::from_value(fixture["exterior_exchange"].clone()).unwrap();
    let request = Request {
        schema: "ftx-motion-coverage-heat/1".into(),
        body_triangle_ids: vec![0],
        instance_layers: BTreeMap::from([("sheet".into(), "shell".into())]),
        settings: projection.settings.clone(),
        initial_temperature_c: 10.,
        initial_instance_temperatures_c: None,
        initial_instance_temperature_profiles_c: None,
        prescribed_compression: None,
        boundaries: vec![film; 4],
        exterior_exchange: None,
        bare_boundaries: None,
        exposed_boundaries: vec![Some(exposed); 4],
        exposed_exterior_exchange: Some(vec![Some(exterior); 4]),
        plastic_dissipation_heat: None,
        relaxation_dissipation_heat: None,
        bending_plastic_dissipation_heat: None,
        thermal_bridges: None,
        max_step_s: 0.25,
        max_cell_steps: 1000,
        max_geometry_operations: 100_000,
    };
    let history = History {
        request,
        source,
        triangles,
        projection,
        initial_frame_sha256: frame.sha256().to_owned(),
        initial_condition: None,
        compression: None,
        geometry_operations: frame.projection().geometry_operations,
        state: frame.initial(10.).unwrap(),
        frame,
        time_s: 0.,
        cell_steps: 0,
        thermal_steps: 0,
        inside_j: 0.,
        outside_j: 0.,
        covered_film_j: 0.,
        exposed_film_j: 0.,
        covered_convective_j: 0.,
        covered_radiative_j: 0.,
        exposed_convective_j: 0.,
        exposed_radiative_j: 0.,
        net_j: 0.,
        stored_j: 0.,
        bare_j: 0.,
        signed_remap_j: 0.,
        absolute_remap_j: 0.,
        absolute_step_error_j: 0.,
        trace: Vec::new(),
        final_field: Value::Null,
        plastic: None,
        relaxation: None,
        bending: None,
        bridge_authority: None,
        bridge_j: 0.,
    };
    (history, cloth)
}

fn two_instance_history() -> (History, Vec<Vec3>) {
    let (mut history, mut cloth) = history();
    let mut source = history.source.snapshot();
    source["vertex_count"] = json!(6);
    source["patches"].as_array_mut().unwrap().push(json!({
        "vertices":[3,4,5],"reference_area_m2":0.5,"column":0}));
    source.as_object_mut().unwrap().remove("sha256");
    source["sha256"] = json!(hash(&source).unwrap());
    history.source = Surface::from_snapshot(&source).unwrap();
    let shell = cloth
        .iter()
        .map(|p| Vec3::new(p.x, p.y, 0.02))
        .collect::<Vec<_>>();
    cloth.extend(shell);
    history.projection.cloth_layers = vec!["lining".into(), "shell".into()];
    history.projection.settings.interfaces = vec![projection::Interface {
        inner: "lining".into(),
        outer: "shell".into(),
        resistance_m2_k_w: 0.05,
        basis: "synthetic independent layer interface".into(),
        stagnant_gap: None,
    }];
    history.request.settings = history.projection.settings.clone();
    history.frame = Frame::new(
        history.source.clone(),
        cloth.clone(),
        body(0.),
        &history.triangles,
        history.projection.clone(),
    )
    .unwrap();
    history.initial_frame_sha256 = history.frame.sha256().to_owned();
    history.geometry_operations = history.frame.projection().geometry_operations;
    history.state = history.frame.initial(10.).unwrap();
    (history, cloth)
}

#[test]
fn coverage_instance_initial_temperatures_keep_shared_material_owners_and_reference_mass() {
    let (mut history, cloth) = two_instance_history();
    history.request.initial_temperature_c = 999.;
    history.request.initial_instance_temperatures_c = Some(BTreeMap::from([
        ("cold:0".into(), 5.),
        ("hot:0".into(), 35.),
    ]));
    history.request.validate_initial_temperatures().unwrap();
    let instances = ["hot:0".into(), "cold:0".into(), "unused:0".into()];
    (history.state, history.initial_condition, _) = history
        .request
        .initial_state(&history.frame, &history.source, &instances, &[1, 0])
        .unwrap();
    assert_eq!(history.state.temperature_c(), &[vec![5., 35.]]);
    assert!(
        history
            .state
            .exposed_temperature_c()
            .iter()
            .all(Vec::is_empty)
    );
    let initial_frame = history.initial_frame_sha256.clone();
    history
        .advance(&cloth, &body(0.), 0.25, 0, 0, None, None, None)
        .unwrap();
    let row = &history.state.temperature_c()[0];
    let stored = 100. * (row[0] - 5. + row[1] - 35.);
    assert!((history.stored_j - stored).abs() < 1e-9);
    assert!((history.stored_j - history.net_j).abs() < 1e-9);
    let result = history.finish("path", "initial", 0.25).unwrap();
    let receipt = &result["initial_condition"];
    assert_eq!(receipt["schema"], "ftx-motion-coverage-initial-condition/1");
    assert_eq!(
        receipt["model_sha256"],
        result["checkpoint"]["frame"]["source"]["sha256"]
    );
    assert_eq!(receipt["frame_sha256"], initial_frame);
    assert_eq!(receipt["frame_sha256"], result["initial_frame_sha256"]);
    assert_eq!(receipt["instances"], json!(instances));
    assert_eq!(receipt["triangle_instance"], json!([1, 0]));
    assert_eq!(receipt["instance_temperature_c"], json!([35., 5., 999.]));
    assert_eq!(receipt["temperature_range_c"], json!([5., 35.]));
    assert_eq!(result["request"]["initial_temperature_c"], 999.);
}

#[test]
fn coverage_instance_initial_temperatures_require_retained_owners_and_admit_heating_policies() {
    let (history, _) = two_instance_history();
    let instances = ["hot:0".into(), "cold:0".into(), "unused:0".into()];
    let mut request = history.request.clone();
    request.initial_instance_temperatures_c = Some(BTreeMap::from([("hot:0".into(), 35.)]));
    request.validate_initial_temperatures().unwrap();
    let (state, receipt, patch_temperatures) = request
        .initial_state(&history.frame, &history.source, &instances, &[1, 0])
        .unwrap();
    assert_eq!(state.temperature_c(), &[vec![10., 35.]]);
    assert_eq!(receipt.unwrap()["temperature_range_c"], json!([10., 35.]));
    assert_eq!(patch_temperatures.unwrap(), vec![10., 35.]);
    for name in ["missing:0", "unused:0"] {
        let mut bad = request.clone();
        bad.initial_instance_temperatures_c = Some(BTreeMap::from([(name.into(), 20.)]));
        assert!(
            bad.initial_state(&history.frame, &history.source, &instances, &[1, 0])
                .is_err()
        );
    }
    for (names, owners) in [
        (vec![], vec![1, 0]),
        (vec!["hot:0".into(), "hot:0".into()], vec![1, 0]),
        (vec!["hot:0".into(), "".into()], vec![1, 0]),
        (instances.to_vec(), vec![0]),
        (instances.to_vec(), vec![1, 3]),
    ] {
        assert!(
            request
                .initial_state(&history.frame, &history.source, &names, &owners)
                .is_err()
        );
    }
    for values in [
        BTreeMap::new(),
        BTreeMap::from([("".into(), 20.)]),
        BTreeMap::from([("hot:0".into(), -274.)]),
        BTreeMap::from([("hot:0".into(), f64::NAN)]),
    ] {
        let mut bad = request.clone();
        bad.initial_instance_temperatures_c = Some(values);
        assert!(bad.validate_initial_temperatures().is_err());
    }
    for channel in [
        "plastic_dissipation_heat",
        "relaxation_dissipation_heat",
        "bending_plastic_dissipation_heat",
    ] {
        let mut heated = json!(request);
        heated[channel] = json!("uniform_reference_thickness");
        serde_json::from_value::<Request>(heated)
            .unwrap()
            .validate_initial_temperatures()
            .unwrap();
    }
    assert!(
        !json!(history.request)
            .as_object()
            .unwrap()
            .contains_key("initial_instance_temperatures_c")
    );
    let (_, receipt, patch_temperatures) = history
        .request
        .initial_state(&history.frame, &history.source, &[], &[])
        .unwrap();
    assert!(receipt.is_none());
    assert!(patch_temperatures.is_none());
}

#[test]
fn coverage_initial_temperatures_qualify_applied_values_without_using_an_unused_fallback() {
    let (mut history, cloth) = two_instance_history();
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../../../tests/fixtures/materials/synthetic-variable-thermal.json"
    ))
    .unwrap();
    let selection: Selection = serde_json::from_value(fixture["layers"][0].clone()).unwrap();
    let material = Catalog::builtin().unwrap().resolve(&selection).unwrap();
    history.source = Surface::new(
        vec![DryThermalModel::new(vec![material], vec![], 1).unwrap()],
        6,
        history.source.patches().to_vec(),
    )
    .unwrap();
    history.frame = Frame::new(
        history.source.clone(),
        cloth,
        body(0.),
        &history.triangles,
        history.projection.clone(),
    )
    .unwrap();
    // This source's characterized interval is [0, 60], but the explicit fallback is not applied.
    history.request.initial_temperature_c = -20.;
    history.request.initial_instance_temperatures_c = Some(BTreeMap::from([
        ("cold:0".into(), 5.),
        ("hot:0".into(), 35.),
    ]));
    let instances = ["hot:0".into(), "cold:0".into()];
    let (state, receipt, patch_temperatures) = history
        .request
        .initial_state(&history.frame, &history.source, &instances, &[1, 0])
        .unwrap();
    assert_eq!(state.temperature_c(), &[vec![5., 35.]]);
    assert_eq!(receipt.unwrap()["temperature_range_c"], json!([5., 35.]));
    assert_eq!(patch_temperatures.unwrap(), vec![5., 35.]);
    history
        .request
        .initial_instance_temperatures_c
        .as_mut()
        .unwrap()
        .remove("cold:0");
    assert!(
        history
            .request
            .initial_state(&history.frame, &history.source, &instances, &[1, 0])
            .is_err()
    );
    history
        .request
        .initial_instance_temperatures_c
        .as_mut()
        .unwrap()
        .insert("cold:0".into(), 61.);
    assert!(
        history
            .request
            .initial_state(&history.frame, &history.source, &instances, &[1, 0])
            .is_err()
    );
    history.request.initial_instance_temperatures_c = None;
    assert!(
        history
            .request
            .initial_state(&history.frame, &history.source, &instances, &[1, 0])
            .is_err()
    );
}

#[test]
fn coverage_initial_profiles_keep_exact_owners_cells_and_storage_through_motion() {
    let (mut history, cloth) = two_instance_history();
    history.request.settings.cells_per_layer = 2;
    history.projection.settings.cells_per_layer = 2;
    history.frame = Frame::new(
        history.source.clone(),
        cloth.clone(),
        body(0.),
        &history.triangles,
        history.projection.clone(),
    )
    .unwrap();
    history.initial_frame_sha256 = history.frame.sha256().to_owned();
    history.request.initial_temperature_c = 999.;
    history.request.initial_instance_temperature_profiles_c = Some(BTreeMap::from([
        ("hot:0".into(), vec![35., 25.]),
        ("cold:0".into(), vec![5., 15.]),
    ]));
    let instances = ["hot:0".into(), "cold:0".into(), "unused:0".into()];
    (history.state, history.initial_condition, _) = history
        .request
        .initial_state(&history.frame, &history.source, &instances, &[1, 0])
        .unwrap();
    assert_eq!(history.state.temperature_c(), &[vec![5., 15., 35., 25.]]);
    history
        .advance(&cloth, &body(0.5), 0.25, 0, 0, None, None, None)
        .unwrap();
    let mut allocated = [0.; 2];
    let initial = [[5., 15.], [35., 25.]];
    let mut stored = 0.;
    for (region, row) in history
        .frame
        .projection()
        .layout
        .regions
        .iter()
        .zip(history.state.temperature_c())
    {
        for (layer, values) in region.layers.iter().zip(row.as_chunks::<2>().0) {
            allocated[layer.cloth_triangle] += layer.fraction;
            stored += 50.
                * layer.fraction
                * values
                    .iter()
                    .zip(initial[layer.cloth_triangle])
                    .map(|(after, before)| after - before)
                    .sum::<f64>();
        }
    }
    for (i, row) in history.state.exposed_temperature_c().iter().enumerate() {
        stored += 50.
            * (1. - allocated[i])
            * row
                .iter()
                .zip(initial[i])
                .map(|(after, before)| after - before)
                .sum::<f64>();
    }
    assert!((stored - history.stored_j - history.signed_remap_j).abs() < 1e-9);
    let result = history.finish("path", "initial", 0.25).unwrap();
    assert_eq!(
        result["initial_condition"]["schema"],
        "ftx-motion-coverage-initial-condition/2"
    );
    assert_eq!(
        result["initial_condition"]["cell_order"],
        "inside_to_outside"
    );
    assert_eq!(
        result["initial_condition"]["temperature_range_c"],
        json!([5., 35.])
    );
    assert_eq!(
        result["initial_condition"]["instance_temperature_profiles_c"],
        result["request"]["initial_instance_temperature_profiles_c"]
    );
    let request: Request = serde_json::from_value(result["request"].clone()).unwrap();
    assert_eq!(json!(request), result["request"]);
}

#[test]
fn coverage_profile_maps_refuse_partial_extra_conflicting_and_unphysical_initial_states() {
    let (history, _) = two_instance_history();
    let instances = ["hot:0".into(), "cold:0".into(), "unused:0".into()];
    for fault in 0..7 {
        let mut request = history.request.clone();
        request.initial_instance_temperature_profiles_c = Some(BTreeMap::from([
            ("hot:0".into(), vec![35.]),
            ("cold:0".into(), vec![5.]),
        ]));
        let profiles = request
            .initial_instance_temperature_profiles_c
            .as_mut()
            .unwrap();
        match fault {
            0 => {
                profiles.remove("cold:0");
            }
            1 => {
                profiles.insert("unused:0".into(), vec![10.]);
            }
            2 => {
                profiles.insert("missing:0".into(), vec![10.]);
            }
            3 => {
                profiles.insert("hot:0".into(), vec![]);
            }
            4 => {
                profiles.insert("hot:0".into(), vec![-274.]);
            }
            5 => {
                profiles.insert("hot:0".into(), vec![f64::NAN]);
            }
            _ => {
                request.initial_instance_temperatures_c =
                    Some(BTreeMap::from([("hot:0".into(), 35.)]))
            }
        }
        assert!(
            request
                .initial_state(&history.frame, &history.source, &instances, &[1, 0])
                .is_err(),
            "fault {fault}"
        );
    }
    assert!(
        !json!(history.request)
            .as_object()
            .unwrap()
            .contains_key("initial_instance_temperature_profiles_c")
    );
}

fn bridge_history() -> (History, Vec<Vec3>) {
    use crate::thermal::steady::{SteadyConductionModel, surface::SteadySurface};
    let (mut history, cloth) = history();
    history.request.exposed_exterior_exchange = None;
    history.request.exposed_boundaries.fill(None);
    let snapshot = history.source.snapshot();
    let retained = &snapshot["columns"][0]["layers"][0];
    let selection: Selection = serde_json::from_value(json!({"id":retained["authored"]["id"],
        "record":retained["authored"],"allow_estimates":true,"expected_sha256":retained["sha256"]}))
    .unwrap();
    let material = Catalog::builtin().unwrap().resolve(&selection).unwrap();
    let source = SteadySurface::new(
        vec![SteadyConductionModel::new(vec![material], vec![]).unwrap()],
        3,
        serde_json::from_value(snapshot["patches"].clone()).unwrap(),
    )
    .unwrap();
    let authority = bridges::Authority::new(source, vec!["sheet".into()], vec![0]).unwrap();
    let q = json!({"id":"synthetic-linear-excess", "surface_model_sha256":authority.model_sha256(),
        "material_sha256":[retained["sha256"]],"interface_resistance_m2_k_w":[],
        "layer_area_ratio_ranges":[[0.01,100.]],"inside_resistance_m2_k_w":0.1,
        "outside_resistance_m2_k_w":0.2,"temperature_range_c":[-20.,80.],"length_range_m":[0.01,10.],
        "excess_linear_conductance":{"value":0.4,"unit":"W/(m*K)","source":"math","evidence":"estimate",
            "method":"Synthetic exact linear path","conditions":{"specimen_id":"synthetic-edge","conditioning":"synthetic dry state"}},
        "sources":{"math":{"reference":"test:synthetic-memoryless-bridge","accessed_on":"2026-10-28",
            "note":"Numerical fixture only; no physical material measurement"}},"allow_estimates":true,
        "reference_basis":"excess_over_locked_unbridged_stack_same_reservoir_films",
        "qualification":"Synthetic exact excess conductance over unchanged films and source material"});
    let q = serde_json::to_value(
        serde_json::from_value::<crate::thermal::steady::coverage::bridges::Qualification>(q)
            .unwrap(),
    )
    .unwrap();
    history.request.thermal_bridges = Some(serde_json::from_value(json!({
        "schema":"ftx-motion-thermal-bridges/1","model":"qualified_zero_storage_centerline_exchange/1",
        "reference":{"schema":"ftx-steady-thermal-bridges/1","model":"qualified_excess_centerline_conductance/1",
            "paths":[{"id":"edge","instance":"sheet","vertices":[0,1],"qualifications":[q]}]},
        "qualifications":[{"id":"synthetic-instantaneous","path_id":"edge","steady_qualification_sha256":hash(&q).unwrap(),
            "time_range_s":[0.,10.],"sample_interval_range_s":[0.001,1.],"temperature_range_c":[-20.,80.],
            "zero_storage":true,"evidence":"estimate","allow_estimates":true,"source":"math","sources":q["sources"],
            "method":"Synthetic exact memoryless law","specimen_id":"synthetic-edge","conditioning":"synthetic dry state",
            "qualification":"Declared numerical zero-storage law over the specified actual intervals; no empirical claim"}]
    })).unwrap());
    history
        .request
        .thermal_bridges
        .as_ref()
        .unwrap()
        .validate()
        .unwrap();
    history.bridge_authority = Some(authority);
    (history, cloth)
}

#[test]
fn moving_zero_storage_bridges_integrate_actual_lengths_signs_and_leave_storage_unchanged() {
    let (mut bridged, mut cloth) = bridge_history();
    let (mut baseline, _) = bridge_history();
    baseline.request.thermal_bridges = None;
    baseline.bridge_authority = None;
    let mut expected = 0.;
    for (i, (length, inside, outside)) in
        [(1., 35., 5.), (0.5, 35., 5.), (1., 5., 35.), (1., 20., 20.)]
            .into_iter()
            .enumerate()
    {
        cloth[1].x = length;
        for history in [&mut baseline, &mut bridged] {
            history.request.boundaries[i].inside_c = inside;
            history.request.boundaries[i].outside_c = outside;
            history
                .advance(
                    &cloth,
                    &body(0.),
                    (i + 1) as f64 * 0.5,
                    i,
                    i,
                    None,
                    None,
                    None,
                )
                .unwrap();
        }
        expected += 0.4 * length * (inside - outside) * 0.5;
        assert!((bridged.bridge_j - expected).abs() < 1e-12);
        assert_eq!(
            bridged.state.temperature_c(),
            baseline.state.temperature_c()
        );
        assert_eq!(
            bridged.state.exposed_temperature_c(),
            baseline.state.exposed_temperature_c()
        );
        assert_eq!(bridged.stored_j, baseline.stored_j);
        assert_eq!(bridged.net_j, baseline.net_j);
        assert_eq!(bridged.signed_remap_j, baseline.signed_remap_j);
        assert_eq!(bridged.inside_j, baseline.inside_j);
        assert_eq!(bridged.outside_j, baseline.outside_j);
        let sample = &bridged.trace[i]["thermal_bridges"];
        assert_eq!(sample["bridge_stored_change_j"], 0.);
        assert!(
            (sample["total_inside_energy_j"].as_f64().unwrap() - baseline.inside_j - expected)
                .abs()
                < 1e-12
        );
        assert_eq!(sample["interval"]["paths"][0]["length_m"], length);
    }
    // The shorter triangle leaves unrelated bare body while the entire bridge stays covered.
    assert!(bridged.trace[1]["bare_energy_j"].as_f64().unwrap() > 0.);
    let result = bridged.finish("path", "initial", 2.).unwrap();
    let unbridged = baseline.finish("path", "initial", 2.).unwrap();
    assert_eq!(result["checkpoint"], unbridged["checkpoint"]);
    assert_eq!(result["thermal_bridges"]["bridge_energy_j"], expected);
    assert_eq!(
        result["inside_energy_j"],
        result["thermal_bridges"]["total_inside_energy_j"]
    );
    assert_eq!(
        result["outside_energy_j"],
        result["thermal_bridges"]["total_outside_energy_j"]
    );
    assert!(unbridged.get("thermal_bridges").is_none());
}

#[test]
fn moving_bridge_qualifies_live_stored_temperatures_and_actual_mechanical_time() {
    let failures = [
        "initial-temperature",
        "elapsed-start",
        "elapsed-end",
        "sample-duration",
        "estimate",
        "steady-source",
        "film",
        "missing-body",
    ];
    for failure in failures {
        let (mut history, cloth) = bridge_history();
        let policy = history.request.thermal_bridges.as_mut().unwrap();
        let temporal = &mut policy.qualifications[0];
        let mut body_positions = body(0.);
        match failure {
            // Reservoirs and any steady solution are inside the source range, but the actual
            // retained material temperature is not. It must never be replaced by a steady proxy.
            "initial-temperature" => history.state = history.frame.initial(90.).unwrap(),
            "elapsed-start" => temporal.time_range_s = [0.1, 10.],
            "elapsed-end" => temporal.time_range_s = [0., 0.25],
            "sample-duration" => temporal.sample_interval_range_s = [0.001, 0.25],
            "estimate" => temporal.allow_estimates = false,
            "steady-source" => {
                policy.reference.paths[0].qualifications[0].surface_model_sha256 = "a".repeat(64)
            }
            "film" => history.request.boundaries[0].outside_resistance_m2_k_w = 0.3,
            "missing-body" => body_positions = body(0.5),
            _ => unreachable!(),
        }
        assert!(
            history
                .advance(&cloth, &body_positions, 0.5, 0, 0, None, None, None)
                .is_err(),
            "{failure}"
        );
    }
    let (mut history, cloth) = bridge_history();
    history
        .request
        .thermal_bridges
        .as_mut()
        .unwrap()
        .qualifications[0]
        .time_range_s = [0., 0.75];
    history
        .advance(&cloth, &body(0.), 0.5, 0, 0, None, None, None)
        .unwrap();
    assert!(
        history
            .advance(&cloth, &body(0.), 1., 1, 1, None, None, None)
            .is_err()
    );
    assert!(history.finish("path", "initial", 1.).is_err());
}

#[test]
fn moving_bridge_requires_separate_source_qualification_and_charges_shared_geometry_budget() {
    let (mut history, cloth) = bridge_history();
    let policy = history.request.thermal_bridges.as_ref().unwrap();
    for (field, value) in [
        ("zero_storage", json!(false)),
        ("steady_qualification_sha256", json!("b".repeat(64))),
        ("method", json!("")),
        ("time_range_s", json!([0., 0.])),
    ] {
        let mut changed = serde_json::to_value(policy).unwrap();
        changed["qualifications"][0][field] = value;
        let changed: bridges::Request = serde_json::from_value(changed).unwrap();
        assert!(changed.validate().is_err(), "{field}");
    }
    history
        .advance(&cloth, &body(0.), 0.5, 0, 0, None, None, None)
        .unwrap();
    let used = history.geometry_operations;
    let bridge_work = history.trace[0]["thermal_bridges"]["interval"]["geometry_operations"]
        .as_u64()
        .unwrap() as usize;
    assert!(bridge_work > 0 && used > bridge_work);
    let (mut bounded, _) = bridge_history();
    bounded.request.max_geometry_operations = used - 1;
    assert_eq!(
        bounded
            .advance(&cloth, &body(0.), 0.5, 0, 0, None, None, None)
            .unwrap_err()
            .code,
        ftx_core::RefusalCode::BudgetExceeded
    );
    let (mut baseline, _) = bridge_history();
    baseline.request.thermal_bridges = None;
    baseline.bridge_authority = None;
    baseline.request.max_geometry_operations = used - 1;
    baseline
        .advance(&cloth, &body(0.), 0.5, 0, 0, None, None, None)
        .unwrap();
}

#[test]
fn bridge_temperature_envelope_retains_material_heat_preload_before_boundary_cooling() {
    let (history, _) = bridge_history();
    let boundary = vec![boundary(history.request.boundaries[0]).unwrap()];
    let step = history
        .frame
        .advance_with_exchange_and_heat(&history.state, 1., &boundary, None, None, &[10_000.])
        .unwrap();
    let ranges = step.bridge_temperature_ranges();
    assert!(ranges[0][1] >= 109.999999999);
    assert!(step.heat().state.temperature_c()[0][0] < ranges[0][1]);
}

#[test]
fn declared_exterior_survives_full_partial_full_partial_coverage_with_heat_intact() {
    let (mut history, cloth) = history();
    for (i, offset) in [0., 0.5, 0., 0.5].into_iter().enumerate() {
        let previous = [history.exposed_convective_j, history.exposed_radiative_j];
        history
            .advance(
                &cloth,
                &body(offset),
                (i + 1) as f64 * 0.5,
                i,
                i,
                None,
                None,
                None,
            )
            .unwrap();
        let exchange = &history.final_field["exposed_exterior_exchange"];
        let trace = &history.trace[i]["exposed_exterior_exchange"];
        assert!(exchange.is_object());
        assert!(trace.is_object());
        if offset == 0. {
            assert!(!history.frame.has_exposed_cloth());
            assert_eq!(exchange["triangles"], json!([]));
            for key in [
                "convective_heat_w",
                "radiative_heat_w",
                "balance_residual_w",
            ] {
                assert_eq!(exchange[key], 0.);
                assert_eq!(trace[key], 0.);
            }
            assert_eq!(history.exposed_convective_j, previous[0]);
            assert_eq!(history.exposed_radiative_j, previous[1]);
        } else {
            assert!(history.frame.has_exposed_cloth());
            assert_eq!(exchange["triangles"].as_array().unwrap().len(), 1);
            assert!(history.exposed_convective_j != previous[0]);
            assert!(history.exposed_radiative_j > previous[1]);
        }
        assert_eq!(trace["convective_energy_j"], history.exposed_convective_j);
        assert_eq!(trace["radiative_energy_j"], history.exposed_radiative_j);
        // Triangle intersection is exactly 1/4 of the reference sheet after a half-unit shift.
        // The whole sheet has 100 J/K capacity, independent of which portion covers the body.
        let fraction = if offset == 0. { 1. } else { 0.25 };
        let mut stored = 100. * fraction * (history.state.temperature_c()[0][0] - 10.);
        if offset != 0. {
            stored += 100. * (1. - fraction) * (history.state.exposed_temperature_c()[0][0] - 10.);
        }
        assert!((stored - history.stored_j - history.signed_remap_j).abs() < 1e-8);
    }
    let out = history
        .finish("synthetic-path", "synthetic-start", 2.)
        .unwrap();
    assert_eq!(out["trace"].as_array().unwrap().len(), 4);
    assert_eq!(
        out["request"]["exposed_exterior_exchange"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert!(
        out["exposed_exterior_exchange"]["radiative_energy_j"]
            .as_f64()
            .unwrap()
            > 0.
    );
    let (frame, state) = Frame::from_checkpoint(&out["checkpoint"]).unwrap();
    assert_eq!(frame.checkpoint(&state).unwrap(), out["checkpoint"]);
}

// Original pressure acquisition, two physical layers sharing one source, and an independently
// declared stagnant gas. All coefficients and geometry below are synthetic known answers.
fn compression_history() -> (History, Vec<Vec3>) {
    compression_history_cells(2)
}

fn compression_history_cells(cells: usize) -> (History, Vec<Vec3>) {
    use ftx_pattern::material_compression::{self, stack};
    let (mut history, mut cloth) = two_instance_history();
    let mut preparation: material_compression::Input = serde_json::from_str(include_str!(
        "../../../../../../tests/fixtures/materials/synthetic-compression.json"
    ))
    .unwrap();
    preparation.pressure_pa = 0.;
    let material = Catalog::builtin()
        .unwrap()
        .resolve(&Selection {
            id: preparation.material.id.clone(),
            record: Some(preparation.material.clone()),
            allow_estimates: true,
            ..Selection::default()
        })
        .unwrap();
    let names = ["lining:0".to_string(), "shell:0".to_string()];
    let policy = compression::Request {
        schema: "ftx-motion-surface-compression/1".into(),
        thermal_evidence: "Synthetic fixed mass/Cp, pressure/film and gas closure; no physical qualification or normal-work heating.".into(),
        bindings: names
            .iter()
            .enumerate()
            .map(|(index, id)| {
                (id.clone(), compression::Binding {
                    material_sha256: material.sha256().into(),
                    preparation: preparation.clone(),
                    direction: stack::Direction::Loading,
                    pressure_pa: Some(if index == 0 {
                        vec![500.; 3]
                    } else {
                        vec![250., 500., 500.]
                    }),
                    controls: None,
                    branch_changes: None,
                })
            })
            .collect(),
    };
    let (source, compressed) = compression::Compression::from_retained(
        &policy,
        &[material],
        &names,
        &[0, 1],
        vec![
            Patch {
                vertices: [0, 1, 2],
                reference_area_m2: 0.5,
                column: 0,
            },
            Patch {
                vertices: [3, 4, 5],
                reference_area_m2: 0.5,
                column: 0,
            },
        ],
        6,
        cells,
        "a".repeat(64),
    )
    .unwrap();
    for (index, point) in cloth.iter_mut().enumerate() {
        point.z = if index < 3 { 0.03 } else { 0.07 };
    }
    history.projection.settings.cells_per_layer = cells;
    history.projection.settings.interfaces[0].stagnant_gap = Some(
        serde_json::from_value(json!({
            "conductivity_w_m_k":0.025,
            "source":{"reference":"test:synthetic-gas","accessed_on":"2026-11-12",
                "note":"Declared arithmetic fixture, not measured atmosphere"},
            "conditions":"Synthetic constant gas; no convection or radiation",
            "gap_range_m":[0.005,0.05],"relative_tolerance":1e-4,
            "allow_conduction_only_approximation":true
        }))
        .unwrap(),
    );
    history.request.settings = history.projection.settings.clone();
    history.request.instance_layers = BTreeMap::from([
        (names[0].clone(), "lining".into()),
        (names[1].clone(), "shell".into()),
    ]);
    history.request.initial_temperature_c = 20.;
    let film = Film {
        inside_c: 20.,
        outside_c: 20.,
        inside_resistance_m2_k_w: 0.1,
        outside_resistance_m2_k_w: 0.2,
    };
    history.request.boundaries = vec![film; 3];
    history.request.exposed_boundaries = vec![Some(film); 3];
    history.request.exposed_exterior_exchange = None;
    history.request.prescribed_compression = Some(policy);
    history.request.validate_compression(3).unwrap();
    history.source = source;
    history.frame = Frame::new(
        history.source.clone(),
        cloth.clone(),
        body(0.),
        &history.triangles,
        history.projection.clone(),
    )
    .unwrap();
    history.initial_frame_sha256 = history.frame.sha256().to_owned();
    history.geometry_operations = history.frame.projection().geometry_operations;
    history.state = history.frame.initial(20.).unwrap();
    history.compression = Some(compressed);
    (history, cloth)
}

#[test]
fn coverage_compression_changes_actual_gap_once_and_keeps_normal_work_out_of_heat() {
    let (mut history, cloth) = compression_history();
    let original = &history.frame.projection().gap_interfaces[0];
    assert!((original.surface_gap_m[0] - 0.02).abs() < 1e-12);
    history
        .advance(&cloth, &body(0.), 0.1, 0, 0, None, None, None)
        .unwrap();
    let gap = &history.frame.projection().gap_interfaces[0];
    assert!((gap.surface_gap_m[0] - 0.0275).abs() < 1e-12);
    assert!((gap.resistance_m2_k_w - 1.1).abs() < 1e-10);
    let first = history.compression.as_ref().unwrap().finish();
    assert_eq!(first["updates"][0]["time_s"], 0.);
    // Each half-square-metre layer uses its own complete pressure traverse.
    assert!((first["cumulative_normal_work_j"].as_f64().unwrap() - 1.5625).abs() < 1e-12);
    assert_eq!(first["final_instances"]["lining:0"]["pressure_pa"], 500.);
    assert_eq!(first["final_instances"]["shell:0"]["pressure_pa"], 250.);
    history
        .advance(&cloth, &body(0.), 0.2, 0, 1, None, None, None)
        .unwrap();
    assert_eq!(history.compression.as_ref().unwrap().finish(), first);
    // The same material becomes wholly exposed, then returns to covered layered storage.
    history
        .advance(&cloth, &body(2.), 0.3, 1, 2, None, None, None)
        .unwrap();
    assert!(history.frame.has_exposed_cloth());
    history
        .advance(&cloth, &body(0.), 0.4, 2, 3, None, None, None)
        .unwrap();
    assert!(!history.frame.has_exposed_cloth());
    let report = history.finish("path", "initial", 0.4).unwrap();
    assert_eq!(report["model"], report["checkpoint"]["frame"]["source"]);
    assert_eq!(
        report["prescribed_compression"]["updates"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        report["prescribed_compression"]["updates"][1]["time_s"],
        0.2
    );
    assert_eq!(
        report["prescribed_compression"]["updates"][2]["time_s"],
        0.3
    );
    assert!(
        (report["prescribed_compression"]["cumulative_normal_work_j"]
            .as_f64()
            .unwrap()
            - 2.5)
            .abs()
            < 1e-12
    );
    assert_eq!(
        report["prescribed_compression"]["normal_work_to_heat_j"],
        0.
    );
    assert!(report["physical_stored_change_j"].as_f64().unwrap().abs() < 1e-9);
    assert!(report["net_boundary_energy_j"].as_f64().unwrap().abs() < 1e-9);
    assert!(report["balance_error_j"].as_f64().unwrap().abs() < 1e-9);
    for row in report["trace"].as_array().unwrap() {
        for temperature in row["temperature_range_c"].as_array().unwrap() {
            assert!((temperature.as_f64().unwrap() - 20.).abs() < 1e-10);
        }
    }
}

#[test]
fn conditioned_coverage_transfer_preserves_depth_profiles_without_relaxing_ordinary_remap() {
    let (mut history, cloth) = compression_history();
    let state = history
        .frame
        .initial_profiles(&[vec![10., 30.], vec![40., 20.]])
        .unwrap();
    let source = history
        .compression
        .as_mut()
        .unwrap()
        .condition_source(&history.source, &cloth, 0, 0, 0.1)
        .unwrap()
        .unwrap();
    let next = Frame::new(
        source,
        cloth,
        body(0.),
        &history.triangles,
        history.projection.clone(),
    )
    .unwrap();
    assert!(next.remap(&history.frame, &state).is_err());
    let transfer = next.remap_reconditioned(&history.frame, &state).unwrap();
    for (actual, expected) in transfer
        .state
        .temperature_c()
        .iter()
        .flatten()
        .zip(state.temperature_c().iter().flatten())
    {
        assert!((actual - expected).abs() < 1e-10);
    }
    assert!(transfer.balance.residual_j.abs() < 1e-10);
    assert!(transfer.balance.absolute_residual_j < 1e-10);
}

#[test]
fn compression_coverage_rolls_back_pressure_geometry_temperature_and_work_on_late_failure() {
    let (mut history, cloth) = compression_history();
    let checkpoint = history.frame.checkpoint(&history.state).unwrap();
    let source = history.source.snapshot();
    let compression = history.compression.as_ref().unwrap().finish();
    let operations = history.geometry_operations;
    history.request.max_step_s = 1e-9;
    assert!(
        history
            .advance(&cloth, &body(0.), 0.1, 0, 0, None, None, None)
            .is_err()
    );
    assert_eq!(
        history.frame.checkpoint(&history.state).unwrap(),
        checkpoint
    );
    assert_eq!(history.source.snapshot(), source);
    assert_eq!(history.compression.as_ref().unwrap().finish(), compression);
    assert_eq!(history.geometry_operations, operations);
    assert_eq!(history.time_s, 0.);
    assert!(history.trace.is_empty());
    history.request.max_step_s = 0.25;
    history
        .advance(&cloth, &body(0.), 0.1, 0, 0, None, None, None)
        .unwrap();
    assert_eq!(
        history.compression.as_ref().unwrap().finish()["updates"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn compression_coverage_requires_independent_storage_and_dry_qualification() {
    let (history, _) = compression_history();
    let mut request = history.request.clone();
    request.exterior_exchange = Some(vec![None; 3]);
    request.validate_compression(3).unwrap();
    assert!(!request.compression_exterior());
    request = history.request.clone();
    request
        .prescribed_compression
        .as_mut()
        .unwrap()
        .bindings
        .get_mut("lining:0")
        .unwrap()
        .preparation
        .invariants
        .remove("specific_heat");
    assert!(request.validate_compression(3).is_err());
    // Equal mass*Cp cannot conceal an unqualified exchange of reference material.
    let snapshot = history.source.snapshot();
    let mut record: ftx_pattern::materials::Material =
        serde_json::from_value(snapshot["columns"][0]["layers"][0]["authored"].clone()).unwrap();
    record.properties.get_mut("areal_density").unwrap().value *= 2.;
    record.properties.get_mut("specific_heat").unwrap().value *= 0.5;
    let changed = Catalog::builtin()
        .unwrap()
        .resolve(&Selection {
            id: record.id.clone(),
            record: Some(record),
            allow_estimates: true,
            ..Selection::default()
        })
        .unwrap();
    let next = DryThermalModel::new(vec![changed], vec![], 2).unwrap();
    assert!(
        history
            .source
            .reconditioned_columns(vec![(0, next)])
            .is_err()
    );
}

fn convection_compression_history(cells: usize) -> (History, Vec<Vec3>) {
    let (mut history, cloth) = compression_history_cells(cells);
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../../../tests/fixtures/materials/synthetic-exterior-exchange.json"
    ))
    .unwrap();
    let mut exterior: Exterior =
        serde_json::from_value(fixture["exterior_exchange"].clone()).unwrap();
    exterior.radiation = None;
    exterior.convection_w_m2_k = 5.;
    exterior.convection_conditions =
        "Synthetic constant coefficient for explicit covered/exposed arithmetic controls".into();
    let film = Film {
        inside_c: 35.,
        outside_c: 5.,
        inside_resistance_m2_k_w: 0.1,
        outside_resistance_m2_k_w: 0.2,
    };
    let convective = Film {
        outside_resistance_m2_k_w: 0.,
        ..film
    };
    history.request.boundaries = vec![convective; 3];
    history.request.exposed_boundaries = vec![Some(convective); 3];
    history.request.bare_boundaries = Some(vec![Some(film); 3]);
    history.request.exterior_exchange = Some(vec![Some(exterior.clone()); 3]);
    history.request.exposed_exterior_exchange = Some(vec![Some(exterior); 3]);
    history.request.validate_compression(3).unwrap();
    (history, cloth)
}

#[test]
fn coverage_compression_convection_matches_independent_two_cell_rc_answer() {
    let (mut history, cloth) = convection_compression_history(1);
    history
        .advance(&cloth, &body(0.), 0.1, 0, 0, None, None, None)
        .unwrap();
    // One cell per layer: each has 100 J/(m² K) reference storage. The original acquisition
    // gives R=0.25 and 0.375 at 500 and 250 Pa; the actual face gap is 0.0275 m. Solve the
    // independent two-by-two backward-Euler RC equations, including half-layer resistances.
    let inside_r = 0.1 + 0.25 / 2.;
    let middle_r = 0.25 / 2. + 0.05 + 0.0275 / 0.025 + 0.375 / 2.;
    let outside_r = 0.375 / 2. + 1. / 5.;
    let capacity_rate = 100. / 0.1;
    let conductance = 1. / middle_r;
    let a = capacity_rate + 1. / inside_r + conductance;
    let d = capacity_rate + 1. / outside_r + conductance;
    let f = capacity_rate * 20. + 35. / inside_r;
    let g = capacity_rate * 20. + 5. / outside_r;
    let determinant = a * d - conductance * conductance;
    let temperatures = [
        (f * d + conductance * g) / determinant,
        (a * g + conductance * f) / determinant,
    ];
    for (&actual, expected) in history.state.temperature_c()[0].iter().zip(temperatures) {
        assert!((actual - expected).abs() < 1e-10, "{actual} != {expected}");
    }
    let expected_outside = 0.5 * (temperatures[1] - 5.) / outside_r;
    let expected_stored = 50. * (temperatures[0] + temperatures[1] - 40.);
    assert!((history.outside_j - 0.1 * expected_outside).abs() < 1e-10);
    assert!((history.stored_j - expected_stored).abs() < 1e-10);
    let report = history.finish("path", "initial", 0.1).unwrap();
    assert_eq!(report["outside_energy_paths"]["bare_j"], 0.);
    assert_eq!(report["outside_energy_paths"]["exposed_convection_j"], 0.);
    assert_eq!(report["outside_energy_paths"]["covered_film_j"], 0.);
    assert_eq!(report["outside_energy_paths"]["exposed_film_j"], 0.);
    assert!(
        (report["prescribed_compression"]["cumulative_normal_work_j"]
            .as_f64()
            .unwrap()
            - 1.5625)
            .abs()
            < 1e-12
    );
    assert_eq!(
        report["prescribed_compression"]["normal_work_to_heat_j"],
        0.
    );
}

#[test]
fn coverage_compression_convection_matches_films_through_partial_exposure_and_return() {
    let (mut convection, cloth) = convection_compression_history(2);
    let mut films = convection.clone();
    films.request.exterior_exchange = None;
    films.request.exposed_exterior_exchange = None;
    films.request.bare_boundaries = None;
    for boundary in &mut films.request.boundaries {
        boundary.outside_resistance_m2_k_w = 0.2;
    }
    for boundary in films.request.exposed_boundaries.iter_mut().flatten() {
        boundary.outside_resistance_m2_k_w = 0.2;
    }
    films.request.validate_compression(3).unwrap();
    for (sample, (offset, interval)) in [(0., 0), (0.5, 1), (2., 2), (0., 2)]
        .into_iter()
        .enumerate()
    {
        let time = (sample + 1) as f64 * 0.1;
        for history in [&mut convection, &mut films] {
            history
                .advance(
                    &cloth,
                    &body(offset),
                    time,
                    interval,
                    sample,
                    None,
                    None,
                    None,
                )
                .unwrap();
        }
        assert_eq!(convection.frame.sha256(), films.frame.sha256());
        for (&actual, &expected) in convection
            .state
            .temperature_c()
            .iter()
            .chain(convection.state.exposed_temperature_c())
            .flatten()
            .zip(
                films
                    .state
                    .temperature_c()
                    .iter()
                    .chain(films.state.exposed_temperature_c())
                    .flatten(),
            )
        {
            assert!((actual - expected).abs() < 1e-10);
        }
        assert!((convection.stored_j - films.stored_j).abs() < 1e-9);
        assert!((convection.inside_j - films.inside_j).abs() < 1e-9);
        assert!((convection.outside_j - films.outside_j).abs() < 1e-9);
        assert_eq!(
            convection.compression.as_ref().unwrap().finish(),
            films.compression.as_ref().unwrap().finish()
        );
        let row = convection.trace.last().unwrap();
        let energy_sum: f64 = row["outside_energy_paths"]
            .as_object()
            .unwrap()
            .values()
            .map(|value| value.as_f64().unwrap())
            .sum();
        let power_sum: f64 = row["outside_power_paths"]
            .as_object()
            .unwrap()
            .values()
            .map(|value| value.as_f64().unwrap())
            .sum();
        assert!((energy_sum - row["outside_energy_j"].as_f64().unwrap()).abs() < 1e-9);
        assert!((power_sum - row["outside_heat_w"].as_f64().unwrap()).abs() < 1e-9);
        assert_eq!(row["outside_energy_paths"]["bare_j"], row["bare_energy_j"]);
        if offset == 0. {
            assert_eq!(row["outside_power_paths"]["exposed_convection_w"], 0.);
            assert_eq!(row["outside_power_paths"]["bare_w"], 0.);
        } else {
            assert!(
                row["outside_power_paths"]["exposed_convection_w"]
                    .as_f64()
                    .unwrap()
                    > 0.
            );
            assert!(row["outside_power_paths"]["bare_w"].as_f64().unwrap() > 0.);
        }
        if offset == 2. {
            assert_eq!(row["outside_power_paths"]["covered_convection_w"], 0.);
        }
    }
    let convective = convection.finish("path", "initial", 0.4).unwrap();
    let film_only = films.finish("path", "initial", 0.4).unwrap();
    assert!(film_only.get("outside_energy_paths").is_none());
    assert!(
        film_only["final_field"]
            .get("outside_power_paths")
            .is_none()
    );
    assert!(
        film_only["trace"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row.get("outside_power_paths").is_none())
    );
    assert_eq!(
        convective["outside_energy_paths"],
        convective["trace"][3]["outside_energy_paths"]
    );
    assert!(convective["balance_error_j"].as_f64().unwrap().abs() < 1e-8);
    assert!(convective["absolute_remap_residual_j"].as_f64().unwrap() < 1e-8);
}

#[test]
fn coverage_compression_mixed_convection_keeps_signed_path_ledgers_after_last_film_interval() {
    let (mut history, cloth) = convection_compression_history(2);
    history.request.exterior_exchange.as_mut().unwrap()[1] = None;
    history.request.exterior_exchange.as_mut().unwrap()[2] = None;
    history.request.exposed_exterior_exchange.as_mut().unwrap()[0] = None;
    history.request.exposed_exterior_exchange.as_mut().unwrap()[2] = None;
    for interval in [1, 2] {
        history.request.boundaries[interval].outside_resistance_m2_k_w = 0.2;
        history.request.bare_boundaries.as_mut().unwrap()[interval] = None;
    }
    for interval in [0, 2] {
        history.request.exposed_boundaries[interval]
            .as_mut()
            .unwrap()
            .outside_resistance_m2_k_w = 0.2;
    }
    history.request.exposed_exterior_exchange.as_mut().unwrap()[1]
        .as_mut()
        .unwrap()
        .air_c = 35.;
    history.request.exposed_boundaries[1]
        .as_mut()
        .unwrap()
        .outside_c = 35.;
    history.request.validate_compression(3).unwrap();
    let mut previous = BTreeMap::<String, f64>::new();
    for interval in 0..3 {
        history
            .advance(
                &cloth,
                &body(0.5),
                (interval + 1) as f64 * 0.1,
                interval,
                interval,
                None,
                None,
                None,
            )
            .unwrap();
        let row = history.trace.last().unwrap();
        for name in [
            "covered_film",
            "exposed_film",
            "bare",
            "covered_convection",
            "exposed_convection",
        ] {
            let energy = row["outside_energy_paths"][format!("{name}_j")]
                .as_f64()
                .unwrap();
            let power = row["outside_power_paths"][format!("{name}_w")]
                .as_f64()
                .unwrap();
            let old = previous.insert(name.into(), energy).unwrap_or(0.);
            assert!(
                (energy - old - 0.1 * power).abs() < 1e-10,
                "{name}, interval {interval}"
            );
        }
        assert_eq!(
            row["outside_power_paths"]["covered_convection_w"] == 0.,
            interval != 0
        );
        assert_eq!(
            row["outside_power_paths"]["exposed_convection_w"] == 0.,
            interval != 1
        );
    }
    let report = history.finish("path", "initial", 3. * 0.1).unwrap();
    assert!(
        report["outside_energy_paths"]["covered_convection_j"]
            .as_f64()
            .unwrap()
            > 0.
    );
    assert!(
        report["outside_energy_paths"]["exposed_convection_j"]
            .as_f64()
            .unwrap()
            < 0.
    );
    assert_eq!(
        report["exterior_exchange"]["convective_energy_j"],
        report["outside_energy_paths"]["covered_convection_j"]
    );
    assert_eq!(
        report["exposed_exterior_exchange"]["convective_energy_j"],
        report["outside_energy_paths"]["exposed_convection_j"]
    );
    assert!(report["trace"][2].get("exterior_exchange").is_none());
    assert!(
        report["trace"][2]
            .get("exposed_exterior_exchange")
            .is_none()
    );
    assert_eq!(
        report["final_field"]["outside_power_paths"]["covered_convection_w"],
        0.
    );
    assert_eq!(
        report["final_field"]["outside_power_paths"]["exposed_convection_w"],
        0.
    );
}

#[test]
fn compression_coverage_admits_complete_convection_sources_and_rejects_radiation_before_exposure() {
    let (history, _) = convection_compression_history(2);
    let radiation = serde_json::from_str::<Value>(include_str!(
        "../../../../../../tests/fixtures/materials/synthetic-exterior-exchange.json"
    ))
    .unwrap()["exterior_exchange"]["radiation"]
        .clone();
    for covered in [true, false] {
        for mutation in 0..8 {
            let mut request = history.request.clone();
            let schedules = if covered {
                &mut request.exterior_exchange
            } else {
                &mut request.exposed_exterior_exchange
            };
            let exterior = schedules.as_mut().unwrap()[2].as_mut().unwrap();
            match mutation {
                0 => exterior.radiation = Some(serde_json::from_value(radiation.clone()).unwrap()),
                1 => exterior.convection_source.reference.clear(),
                2 => exterior.convection_source.note.clear(),
                3 => exterior.convection_conditions.clear(),
                4 => exterior.allow_estimates = false,
                5 => exterior.convection_w_m2_k = -1.,
                6 => exterior.surface_temperature_range_c = [30., 20.],
                7 => exterior.air_c = f64::INFINITY,
                _ => unreachable!(),
            }
            // Admission checks the entire declared schedule before any geometry can hide a path.
            assert!(
                request.validate_compression(3).is_err(),
                "covered={covered}, mutation={mutation}"
            );
        }
    }
    for mutation in 0..5 {
        let mut request = history.request.clone();
        match mutation {
            0 => request.bare_boundaries = None,
            1 => request.bare_boundaries.as_mut().unwrap()[2] = None,
            _ => {
                let bare = request.bare_boundaries.as_mut().unwrap()[2]
                    .as_mut()
                    .unwrap();
                match mutation {
                    2 => bare.inside_c += 1.,
                    3 => {
                        bare.inside_resistance_m2_k_w = 0.;
                        bare.outside_resistance_m2_k_w = 0.;
                    }
                    4 => {
                        bare.inside_resistance_m2_k_w = f64::MAX;
                        bare.outside_resistance_m2_k_w = f64::MAX;
                    }
                    _ => unreachable!(),
                }
            }
        }
        assert!(
            request.validate_compression(3).is_err(),
            "bare mutation={mutation}"
        );
    }
}

#[test]
fn coverage_compression_late_exposed_domain_failure_restores_pressure_heat_and_path_ledgers() {
    let (mut history, cloth) = convection_compression_history(2);
    history
        .advance(&cloth, &body(0.5), 0.1, 0, 0, None, None, None)
        .unwrap();
    let mut control = history.clone();
    history.request.exposed_exterior_exchange.as_mut().unwrap()[1]
        .as_mut()
        .unwrap()
        .surface_temperature_range_c = [-20., 0.];
    history.request.validate_compression(3).unwrap();
    let before = history.clone().finish("path", "initial", 0.1).unwrap();
    // The shell would advance 250 -> 500 Pa and all material would become exposed before its
    // actual temperature fails the separately qualified exterior domain.
    assert!(
        history
            .advance(&cloth, &body(2.), 0.2, 1, 1, None, None, None)
            .is_err()
    );
    assert_eq!(
        history.clone().finish("path", "initial", 0.1).unwrap(),
        before
    );
    history.request = control.request.clone();
    for run in [&mut history, &mut control] {
        run.advance(&cloth, &body(2.), 0.2, 1, 1, None, None, None)
            .unwrap();
    }
    assert_eq!(
        history.finish("path", "initial", 0.2).unwrap(),
        control.finish("path", "initial", 0.2).unwrap()
    );
}

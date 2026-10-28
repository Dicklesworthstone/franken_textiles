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
        geometry_operations: frame.projection().geometry_operations,
        state: frame.initial(10.).unwrap(),
        frame,
        time_s: 0.,
        cell_steps: 0,
        thermal_steps: 0,
        inside_j: 0.,
        outside_j: 0.,
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

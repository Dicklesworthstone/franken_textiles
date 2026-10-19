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
    };
    (history, cloth)
}

#[test]
fn declared_exterior_survives_full_partial_full_partial_coverage_with_heat_intact() {
    let (mut history, cloth) = history();
    for (i, offset) in [0., 0.5, 0., 0.5].into_iter().enumerate() {
        let previous = [history.exposed_convective_j, history.exposed_radiative_j];
        history
            .advance(&cloth, &body(offset), (i + 1) as f64 * 0.5, i, i)
            .unwrap();
        let exchange = &history.final_field["exposed_exterior_exchange"];
        let trace = &history.trace[i]["exposed_exterior_exchange"];
        assert!(exchange.is_object());
        assert!(trace.is_object());
        if offset == 0. {
            assert!(!history.frame.has_exposed_cloth());
            assert_eq!(exchange["triangles"], json!([]));
            for key in ["convective_heat_w", "radiative_heat_w", "balance_residual_w"] {
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
    let out = history.finish("synthetic-path", "synthetic-start", 2.).unwrap();
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

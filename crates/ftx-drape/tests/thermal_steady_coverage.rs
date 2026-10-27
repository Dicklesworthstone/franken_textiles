//! Shared coverage topology with exact conduction and no invented storage observations.
use ftx_core::Vec3;
use ftx_drape::thermal::{
    Boundary, DryThermalModel,
    steady::{SteadyConductionModel, coverage::SteadyCoverage, surface::SteadySurface},
    surface::{
        Patch,
        coverage::{Coverage, Layout},
    },
};
use ftx_pattern::{material_catalog::Selection, materials::Catalog};
use serde_json::{Value, json};

fn selection(r: f64) -> Selection {
    serde_json::from_value(json!({"id":"test-sheet","allow_estimates":true,"record":{
        "id":"test-sheet","name":"Synthetic sheet, no storage data","form":"woven",
        "sources":{"test":{"reference":"test:steady-coverage","accessed_on":"2026-10-15",
            "note":"Synthetic numerical fixture, not measured textile data"}},
        "properties":{"thermal_resistance":{"value":r,"unit":"m2*K/W","evidence":"estimate",
            "source":"test","method":"Synthetic constant resistance"}}
    }}))
    .unwrap()
}
fn source_with(a: Selection, b: Selection) -> SteadySurface {
    let catalog = Catalog::builtin().unwrap();
    let columns = [a, b]
        .iter()
        .map(|s| {
            let material = catalog.resolve(s).unwrap();
            assert!(DryThermalModel::new(vec![material.clone()], vec![], 1).is_err());
            SteadyConductionModel::new(vec![material], vec![]).unwrap()
        })
        .collect();
    SteadySurface::new(
        columns,
        6,
        vec![
            Patch {
                vertices: [0, 1, 2],
                reference_area_m2: 0.5,
                column: 0,
            },
            Patch {
                vertices: [3, 4, 5],
                reference_area_m2: 0.5,
                column: 1,
            },
        ],
    )
    .unwrap()
}
fn source() -> SteadySurface {
    source_with(selection(0.2), selection(0.4))
}
fn body() -> Vec<Vec3> {
    vec![
        Vec3::ZERO,
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
    ]
}
fn cloth() -> Vec<Vec3> {
    vec![
        Vec3::new(0.0, 0.0, 0.1),
        Vec3::new(1.0, 0.0, 0.1),
        Vec3::new(0.0, 1.0, 0.1),
        Vec3::new(0.0, 0.0, 0.2),
        Vec3::new(2.0, 0.0, 0.2),
        Vec3::new(0.0, 1.0, 0.2),
    ]
}
fn layout() -> Layout {
    serde_json::from_value(json!({"schema":"ftx-thermal-coverage-layout/1",
        "mapping_basis":"Synthetic disjoint full-body patch, two separately allocated sheets",
        "body_vertex_count":3,"body_triangles":[[0,1,2]],"cells_per_layer":1,
        "regions":[{"id":"covered","body_triangle":0,"body_fraction":1.0,
            "layers":[{"layer":"lining","cloth_triangle":0,"fraction":1.0},
                      {"layer":"shell","cloth_triangle":1,"fraction":1.0}],
            "interface_resistance_m2_k_w":[0.1]}]}))
    .unwrap()
}
fn boundary() -> Boundary {
    Boundary {
        inside_c: 30.0,
        outside_c: 0.0,
        inside_resistance: 0.1,
        outside_resistance: 0.2,
    }
}
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-7, "{a} != {b}");
}

fn bridge_digest(value: &Value) -> String {
    ftx_core::receipt::sha256_hex(&serde_json::to_vec(value).unwrap())
}

fn bridge_input(resistance: f64) -> Value {
    let material = Catalog::builtin()
        .unwrap()
        .resolve(&selection(resistance))
        .unwrap();
    let hash = material.sha256().to_owned();
    let report = json!({"basis":"per_piece_characterization", "instances":[
        {"instance":"front","record_sha256":hash},{"instance":"back","record_sha256":hash}],
        "records":{hash:{"record":material}}});
    let source = SteadySurface::new(
        vec![SteadyConductionModel::new(vec![material.clone()], vec![]).unwrap()],
        4,
        vec![
            Patch {
                vertices: [0, 1, 2],
                reference_area_m2: 0.5,
                column: 0,
            },
            Patch {
                vertices: [1, 3, 2],
                reference_area_m2: 0.5,
                column: 0,
            },
        ],
    )
    .unwrap();
    let mut frame = json!({"schema":"ftx-drape-steady-thermal-surface/1","available":true,
        "model":source.snapshot().unwrap(), "positions_m":[[0.,0.,0.],[1.,0.,0.],[0.,1.,0.],[1.,1.,0.]],
        "material_report_sha256":bridge_digest(&report),"triangle_instance":[0,1],"instances":["front","back"]});
    frame["sha256"] = json!(bridge_digest(&frame));
    json!({"schema":"ftx-steady-coverage-request/1","report":{"material":report,"steady_thermal_surface":frame},
        "map":serde_json::from_str::<Value>(include_str!("../../../tests/fixtures/materials/steady-coverage-map.json")).unwrap(),
        "conditions":serde_json::from_str::<Value>(include_str!("../../../tests/fixtures/materials/steady-coverage-conditions.json")).unwrap()})
}

fn with_bridge(mut input: Value, vertices: &[usize], psi: f64) -> Value {
    let hash = &input["report"]["material"]["instances"][0]["record_sha256"];
    input["thermal_bridges"] = json!({"schema":"ftx-steady-thermal-bridges/1",
        "model":"qualified_excess_centerline_conductance/1", "paths":[{
        "id":"synthetic-seam","instance":"front","vertices":vertices,"qualifications":[{
            "id":"qualified-test-reference","surface_model_sha256":input["report"]["steady_thermal_surface"]["model"]["sha256"],
            "material_sha256":[hash],"interface_resistance_m2_k_w":[],"layer_area_ratio_ranges":[[0.5,2.]],
            "inside_resistance_m2_k_w":0.1,"outside_resistance_m2_k_w":0.2,
            "temperature_range_c":[-10.,50.],"length_range_m":[0.1,4.],
            "excess_linear_conductance":{"value":psi,"unit":"W/(m*K)","evidence":"estimate",
                "source":"test","method":"Synthetic paired bridged-minus-unbridged reference",
                "conditions":{"specimen_id":"synthetic-seam-coupon","conditioning":"synthetic dry test"}},
            "sources":{"test":{"reference":"test:thermal-bridge","accessed_on":"2026-10-27",
                "note":"Synthetic software verification; no measured seam performance"}},
            "allow_estimates":true,"reference_basis":"excess_over_locked_unbridged_stack_same_reservoir_films",
            "qualification":"Synthetic linear centerline excess with independently declared geometry and reference domains"}]}]});
    input
}

fn bridge_evaluate(value: &Value) -> Result<Value, ftx_core::Refusal> {
    serde_json::from_value::<ftx_drape::thermal::steady::coverage::request::Request>(value.clone())
        .map_err(|e| ftx_core::Refusal::new(ftx_core::RefusalCode::InvalidDocument, e.to_string()))?
        .evaluate()
}

#[test]
fn qualified_centerline_adds_only_signed_excess_and_preserves_frozen_reference() {
    let plain_input = bridge_input(0.4);
    let plain = bridge_evaluate(&plain_input).unwrap();
    assert!(plain["steady"]["baseline_power_w"].is_null());
    assert!(plain["thermal_bridges"].is_null());
    let input = with_bridge(plain_input, &[0, 1], 0.2);
    let original = input.clone();
    let result = bridge_evaluate(&input).unwrap();
    assert_eq!(input, original);
    near(
        result["steady"]["baseline_power_w"].as_f64().unwrap(),
        plain["steady"]["power_w"].as_f64().unwrap(),
    );
    near(result["steady"]["bridge_power_w"].as_f64().unwrap(), 6.);
    near(
        result["steady"]["power_w"].as_f64().unwrap(),
        30. / 0.7 + 6.,
    );
    assert_eq!(result["steady"]["regions"], plain["steady"]["regions"]);
    assert_eq!(
        result["thermal_bridges"]["inside_excess_power_w"],
        result["thermal_bridges"]["outside_excess_power_w"]
    );
    assert_eq!(result, bridge_evaluate(&result["request"]).unwrap());
    for (inside, outside, expected) in [(0., 30., -6.), (15., 15., 0.)] {
        let mut reversed = input.clone();
        reversed["conditions"]["boundaries"][0]["inside_c"] = json!(inside);
        reversed["conditions"]["boundaries"][0]["outside_c"] = json!(outside);
        let result = bridge_evaluate(&reversed).unwrap();
        near(
            result["steady"]["bridge_power_w"].as_f64().unwrap(),
            expected,
        );
        let sum: f64 = result["steady"]["body"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["power_w"].as_f64().unwrap())
            .sum();
        near(sum, result["steady"]["power_w"].as_f64().unwrap());
    }
}

#[test]
fn shared_seam_and_body_boundaries_own_each_current_length_once() {
    let input = with_bridge(bridge_input(0.4), &[1, 2], 0.2);
    let result = bridge_evaluate(&input).unwrap();
    near(
        result["steady"]["bridge_power_w"].as_f64().unwrap(),
        6. * 2_f64.sqrt(),
    );
    let pieces = result["thermal_bridges"]["paths"][0]["contributions"]
        .as_array()
        .unwrap();
    near(
        pieces.iter().map(|p| p["length_m"].as_f64().unwrap()).sum(),
        2_f64.sqrt(),
    );
    assert_eq!(pieces.len(), 1); // Both source triangles/body faces share the same physical line.

    let mut crossed = input.clone();
    crossed["map"]["body_triangles"] = json!([[0, 1, 3], [0, 3, 2]]);
    let split = bridge_evaluate(&crossed).unwrap();
    near(
        split["steady"]["bridge_power_w"].as_f64().unwrap(),
        6. * 2_f64.sqrt(),
    );
    let parts = split["thermal_bridges"]["paths"][0]["contributions"]
        .as_array()
        .unwrap();
    assert_eq!(parts.len(), 2);
    near(
        parts.iter().map(|p| p["length_m"].as_f64().unwrap()).sum(),
        2_f64.sqrt(),
    );

    let mut moved = input.clone();
    for vertex in moved["report"]["steady_thermal_surface"]["positions_m"]
        .as_array_mut()
        .unwrap()
    {
        vertex[0] = json!(2. * vertex[0].as_f64().unwrap());
    }
    for vertex in moved["map"]["body_positions_m"].as_array_mut().unwrap() {
        vertex[0] = json!(2. * vertex[0].as_f64().unwrap());
    }
    let frame = &mut moved["report"]["steady_thermal_surface"];
    frame.as_object_mut().unwrap().remove("sha256");
    frame["sha256"] = json!(bridge_digest(frame));
    let result = bridge_evaluate(&moved).unwrap();
    near(
        result["steady"]["bridge_power_w"].as_f64().unwrap(),
        6. * 5_f64.sqrt(),
    );
    assert_eq!(
        result["thermal_bridges"]["surface_model_sha256"],
        input["report"]["steady_thermal_surface"]["model"]["sha256"]
    );

    let mut heterogeneous = input;
    heterogeneous["report"] = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-steady-surface-report.json"
    ))
    .unwrap();
    let model_hash = heterogeneous["report"]["steady_thermal_surface"]["model"]["sha256"].clone();
    let material_hash =
        heterogeneous["report"]["material"]["instances"][0]["record_sha256"].clone();
    let q = &mut heterogeneous["thermal_bridges"]["paths"][0]["qualifications"][0];
    q["surface_model_sha256"] = model_hash;
    q["material_sha256"][0] = material_hash;
    assert!(bridge_evaluate(&heterogeneous).is_err());
}

#[test]
fn bridge_sources_domains_geometry_and_double_counting_refuse_without_partial_power() {
    let input = with_bridge(bridge_input(0.4), &[0, 1], 0.2);
    let path = "/thermal_bridges/paths/0/qualifications/0/";
    for (field, value) in [
        ("allow_estimates", json!(false)),
        ("surface_model_sha256", json!("0".repeat(64))),
        ("material_sha256", json!(["0".repeat(64)])),
        ("reference_basis", json!("whole_seam_including_area")),
        ("length_range_m", json!([0.1, 0.9])),
        ("temperature_range_c", json!([0., 20.])),
        ("layer_area_ratio_ranges", json!([[1.1, 2.]])),
        ("inside_resistance_m2_k_w", json!(0.2)),
    ] {
        let mut bad = input.clone();
        *bad.pointer_mut(&format!("{path}{field}")).unwrap() = value;
        assert!(bridge_evaluate(&bad).is_err(), "{field}");
    }
    let mut duplicate = input.clone();
    let mut extra = duplicate["thermal_bridges"]["paths"][0].clone();
    extra["id"] = json!("duplicate-physical-seam");
    duplicate["thermal_bridges"]["paths"]
        .as_array_mut()
        .unwrap()
        .push(extra);
    assert!(bridge_evaluate(&duplicate).is_err());
    let mut unmapped = input.clone();
    for vertex in unmapped["map"]["body_positions_m"].as_array_mut().unwrap() {
        vertex[0] = json!(0.5 * vertex[0].as_f64().unwrap());
    }
    assert!(bridge_evaluate(&unmapped).is_err());
    let mut unknown = input.clone();
    unknown["thermal_bridges"]["paths"][0]["vertices"] = json!([0, 3]);
    assert!(bridge_evaluate(&unknown).is_err());
    let mut exterior = input.clone();
    exterior["conditions"]["exterior_exchange"] = json!([{"air_c":0.,"convection_w_m2_k":5.}]);
    assert!(bridge_evaluate(&exterior).is_err());
    let mut replaced = with_bridge(bridge_input(0.8), &[0, 1], 0.4);
    let mut obsolete = input["thermal_bridges"]["paths"][0]["qualifications"][0].clone();
    obsolete["id"] = json!("obsolete-qualified-reference");
    replaced["thermal_bridges"]["paths"][0]["qualifications"]
        .as_array_mut()
        .unwrap()
        .push(obsolete);
    near(
        bridge_evaluate(&replaced).unwrap()["steady"]["bridge_power_w"]
            .as_f64()
            .unwrap(),
        12.,
    );
    replaced["thermal_bridges"] = input["thermal_bridges"].clone();
    assert!(bridge_evaluate(&replaced).is_err());
}

#[test]
fn unequal_area_layers_match_series_resistances_without_storage() {
    let a = SteadyCoverage::new(source(), layout()).unwrap();
    let q = a.solve(&cloth(), &body(), &[boundary()]).unwrap();
    // Covered-body area .5; current lining .5, shell 1.0. Films + interface stay per body area.
    let expected_flux = 30.0 / (0.1 + 0.2 + 0.1 + 0.4 * 0.5 + 0.2);
    near(q.power_w, 0.5 * expected_flux);
    near(q.body_area_m2, 0.5);
    near(q.covered_area_m2, 0.5);
    near(q.bare_power_w, 0.0);
    near(q.unassigned_cloth_reference_area_m2, 0.0);
    assert_eq!(q.regions[0].layer_area_m2, vec![0.5, 1.0]);
    let faces = &q.regions[0].conduction.layer_faces_c;
    near((faces[0][0] - faces[0][1]) / 0.2 * 0.5, q.power_w);
    near((faces[1][0] - faces[1][1]) / 0.4 * 1.0, q.power_w);
    near(faces[0][1] - faces[1][0], expected_flux * 0.1);
    assert!(Coverage::from_snapshot(&a.snapshot().unwrap()).is_err());
}

#[test]
fn partial_coverage_parallel_bare_path_and_unused_cloth_are_counted_once() {
    let mut l = layout();
    l.regions[0].body_fraction = 0.5;
    l.regions[0].layers[0].fraction = 0.25;
    l.regions[0].layers[1].fraction = 0.5;
    let q = SteadyCoverage::new(source(), l)
        .unwrap()
        .solve(&cloth(), &body(), &[boundary()])
        .unwrap();
    near(q.covered_area_m2, 0.25);
    near(q.bare_power_w, 25.0);
    near(
        q.regions[0].power_w,
        0.25 * 30.0 / (0.3 + 0.1 + 0.2 * 2.0 + 0.4 * 0.5),
    );
    near(q.power_w, 32.5);
    near(q.unassigned_cloth_reference_area_m2, 0.625);
    near(q.body[0].power_w, q.power_w);
}

#[test]
fn partition_refinement_rigid_motion_and_signed_boundary_reversal() {
    let a = SteadyCoverage::new(source(), layout()).unwrap();
    let expected = a.solve(&cloth(), &body(), &[boundary()]).unwrap();
    let mut split = layout();
    let r = split.regions[0].clone();
    split.regions.clear();
    for i in 0..4 {
        let mut r = r.clone();
        r.id = format!("quarter-{i}");
        r.body_fraction = 0.25;
        for p in &mut r.layers {
            p.fraction = 0.25;
        }
        split.regions.push(r);
    }
    let q = SteadyCoverage::new(source(), split)
        .unwrap()
        .solve(&cloth(), &body(), &[boundary()])
        .unwrap();
    near(q.power_w, expected.power_w);
    near(q.covered_area_m2, expected.covered_area_m2);
    let moved = |x: Vec<Vec3>| {
        x.into_iter()
            .map(|p| Vec3::new(1.0 + p.z, 4.0 + p.x, -2.0 + p.y))
            .collect::<Vec<_>>()
    };
    near(
        a.solve(&moved(cloth()), &moved(body()), &[boundary()])
            .unwrap()
            .power_w,
        expected.power_w,
    );
    let reverse = Boundary {
        inside_c: 0.0,
        outside_c: 30.0,
        ..boundary()
    };
    near(
        a.solve(&cloth(), &body(), &[reverse]).unwrap().power_w,
        -expected.power_w,
    );
    // Scaling both cloth and body by two preserves flux and quadruples power.
    let bigger = |x: Vec<Vec3>| x.into_iter().map(|p| p * 2.0).collect::<Vec<_>>();
    near(
        a.solve(&bigger(cloth()), &bigger(body()), &[boundary()])
            .unwrap()
            .power_w,
        4.0 * expected.power_w,
    );
}

#[test]
fn nonlinear_conductivity_faces_satisfy_independent_integral_on_unequal_areas() {
    let input: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-variable-thermal.json"
    ))
    .unwrap();
    let mut variable: Selection = serde_json::from_value(input["layers"][0].clone()).unwrap();
    let m = variable.record.as_mut().unwrap();
    m.properties.remove("areal_density");
    let response = m.thermal_response.as_mut().unwrap();
    response.specific_heat = None;
    // k(T) = .0025 + .00005*T; thickness .001. Integral from 30 to 40 is .0425 W/m.
    response.conductivity.as_mut().unwrap().points[1].mean = 0.0055;
    let mut l = layout();
    l.regions[0].interface_resistance_m2_k_w[0] = 0.0;
    let a = SteadyCoverage::new(source_with(variable, selection(20.0 / 42.5)), l).unwrap();
    let b = Boundary {
        inside_c: 40.0,
        outside_c: 20.0,
        inside_resistance: 0.0,
        outside_resistance: 0.0,
    };
    let q = a.solve(&cloth(), &body(), &[b]).unwrap();
    near(q.regions[0].conduction.flux_w_m2, 42.5);
    near(q.regions[0].conduction.layer_faces_c[0][1], 30.0);
    near(q.regions[0].conduction.layer_faces_c[1][0], 30.0);
    near(q.power_w, 21.25);
    assert!(
        a.solve(
            &cloth(),
            &body(),
            &[Boundary {
                inside_c: 80.0,
                ..b
            }]
        )
        .is_err()
    );
}

#[test]
fn material_swap_and_pose_updates_never_change_source_observations() {
    let a = SteadyCoverage::new(source(), layout()).unwrap();
    let snapshot = a.snapshot().unwrap();
    let restored = SteadyCoverage::from_snapshot(&snapshot).unwrap();
    let q = a.solve(&cloth(), &body(), &[boundary()]).unwrap();
    assert_eq!(
        serde_json::to_value(&q).unwrap(),
        serde_json::to_value(restored.solve(&cloth(), &body(), &[boundary()]).unwrap()).unwrap()
    );
    let b = SteadyCoverage::new(source_with(selection(0.2), selection(0.8)), layout()).unwrap();
    assert_ne!(snapshot["sha256"], b.snapshot().unwrap()["sha256"]);
    assert!(b.solve(&cloth(), &body(), &[boundary()]).unwrap().power_w < q.power_w);
    let mut stretched = cloth();
    stretched[4].x = 4.0;
    assert!(a.solve(&stretched, &body(), &[boundary()]).unwrap().power_w > q.power_w);
    assert_eq!(a.snapshot().unwrap(), snapshot);
    for field in ["layout", "source", "unknown"] {
        let mut bad = snapshot.clone();
        bad[field] = json!(false);
        assert!(SteadyCoverage::from_snapshot(&bad).is_err());
    }
    let mut bad = snapshot;
    bad["source"]["columns"][0]["layers"][0]["record"]["properties"]["thermal_resistance"]["value"] =
        json!(10.0);
    assert!(SteadyCoverage::from_snapshot(&bad).is_err());
}

#[test]
fn allocation_order_missing_interfaces_and_invalid_geometries_refuse() {
    let mut variants = Vec::new();
    let mut l = layout();
    l.regions.push(l.regions[0].clone());
    variants.push(l);
    let mut l = layout();
    l.regions[0].layers[0].fraction = 1.01;
    variants.push(l);
    let mut l = layout();
    l.regions[0].layers[0].cloth_triangle = 2;
    variants.push(l);
    let mut l = layout();
    l.regions[0].layers[1].layer = "lining".into();
    variants.push(l);
    let mut l = layout();
    l.regions[0].interface_resistance_m2_k_w.clear();
    variants.push(l);
    let mut l = layout();
    l.regions[0].interface_resistance_m2_k_w[0] = f64::NAN;
    variants.push(l);
    let mut l = layout();
    l.regions[0].body_fraction = 0.5;
    for p in &mut l.regions[0].layers {
        p.fraction = 0.5;
    }
    let mut r = l.regions[0].clone();
    r.id = "reversed".into();
    r.layers.reverse();
    l.regions.push(r);
    variants.push(l);
    for l in variants {
        assert!(SteadyCoverage::new(source(), l).is_err());
    }
    let a = SteadyCoverage::new(source(), layout()).unwrap();
    assert!(a.solve(&cloth(), &[], &[boundary()]).is_err());
    assert!(a.solve(&cloth(), &body(), &[]).is_err());
    assert!(a.solve(&[Vec3::ZERO; 6], &body(), &[boundary()]).is_err());
    assert!(
        a.solve(
            &cloth(),
            &body(),
            &[Boundary {
                inside_c: f64::NAN,
                ..boundary()
            }]
        )
        .is_err()
    );
    let mut l = layout();
    l.regions.clear();
    let bare = SteadyCoverage::new(source(), l).unwrap();
    near(
        bare.solve(&cloth(), &body(), &[boundary()])
            .unwrap()
            .power_w,
        50.0,
    );
    assert!(
        bare.solve(
            &cloth(),
            &body(),
            &[Boundary {
                inside_resistance: 0.0,
                outside_resistance: 0.0,
                ..boundary()
            }]
        )
        .is_err()
    );
}

#[test]
fn source_qualified_hfm_layers_reach_coverage_without_storage_or_estimation_of_cp() {
    let catalog = Catalog::builtin().unwrap();
    let id = "fsri/hfm/cotton-sheet/dry/220330-r1";
    let s: Selection = serde_json::from_value(json!({"id":id,"allow_estimates":true})).unwrap();
    let m = catalog.resolve(&s).unwrap();
    assert!(DryThermalModel::new(vec![m.clone()], vec![], 1).is_err());
    let column = SteadyConductionModel::new(vec![m], vec![]).unwrap();
    let b = Boundary {
        inside_c: 40.0,
        outside_c: 20.0,
        inside_resistance: 0.0,
        outside_resistance: 0.0,
    };
    let expected = column.solve(b).unwrap().flux_w_m2;
    let source = SteadySurface::new(
        vec![column],
        3,
        vec![Patch {
            vertices: [0, 1, 2],
            reference_area_m2: 0.5,
            column: 0,
        }],
    )
    .unwrap();
    let mut l = layout();
    l.regions[0].layers.truncate(1);
    l.regions[0].interface_resistance_m2_k_w.clear();
    let a = SteadyCoverage::new(source, l).unwrap();
    near(
        a.solve(&body(), &body(), &[b]).unwrap().power_w,
        0.5 * expected,
    );
}

fn exterior_selection(r: f64, eps: f64) -> Selection {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-exterior-exchange.json"
    ))
    .unwrap();
    let mut s = selection(r);
    let mut q = fixture["layers"][0]["record"]["properties"]["emissivity"].clone();
    q["value"] = json!(eps);
    q["source"] = json!("test");
    s.record
        .as_mut()
        .unwrap()
        .properties
        .insert("emissivity".into(), serde_json::from_value(q).unwrap());
    s
}
fn environment(radiation: bool) -> ftx_drape::thermal::steady::exterior::Exterior {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-exterior-exchange.json"
    ))
    .unwrap();
    let mut e = fixture["exterior_exchange"].clone();
    e["air_c"] = json!(0.);
    e["convection_w_m2_k"] = json!(5.);
    if !radiation {
        e.as_object_mut().unwrap().remove("radiation");
    }
    serde_json::from_value(e).unwrap()
}
fn exterior_boundary() -> Boundary {
    Boundary {
        outside_resistance: 0.,
        ..boundary()
    }
}

#[test]
fn coverage_exterior_scales_exchange_by_outer_cloth_area_not_body_or_all_layers() {
    let a = SteadyCoverage::new(source(), layout()).unwrap();
    let solved = a
        .solve_exterior(
            &cloth(),
            &body(),
            &[exterior_boundary()],
            &[environment(false)],
            &[],
        )
        .unwrap();
    // Inner film .1, lining .2, contact .1, shell .4*.5, exterior (1/5)*.5 per BODY area.
    near(solved.steady.power_w, 0.5 * 30. / 0.7);
    near(solved.convective_power_w, solved.steady.power_w);
    near(solved.regions[0].outer_area_per_model_area, 2.);
    near(solved.regions[0].surface_c * 5. * 1., solved.steady.power_w);
    near(solved.radiative_power_w, 0.);
    near(solved.balance_residual_w, 0.);
    let equivalent = Boundary {
        outside_resistance: 0.1,
        ..boundary()
    };
    near(
        a.solve(&cloth(), &body(), &[equivalent]).unwrap().power_w,
        solved.steady.power_w,
    );
    let moved = |v: Vec3| Vec3::new(v.z, v.x, v.y) + Vec3::new(0.3, -0.2, 0.7);
    let m = a
        .solve_exterior(
            &cloth().into_iter().map(moved).collect::<Vec<_>>(),
            &body().into_iter().map(moved).collect::<Vec<_>>(),
            &[exterior_boundary()],
            &[environment(false)],
            &[],
        )
        .unwrap();
    near(m.steady.power_w, solved.steady.power_w);
}

#[test]
fn partial_exterior_coverage_requires_separate_bare_films_and_one_body_temperature() {
    let mut l = layout();
    l.regions[0].body_fraction = 0.5;
    l.regions[0].layers[0].fraction = 0.25;
    l.regions[0].layers[1].fraction = 0.5;
    let a = SteadyCoverage::new(source(), l).unwrap();
    let b = [exterior_boundary()];
    let e = [environment(false)];
    let bare = [Boundary {
        inside_resistance: 0.2,
        outside_resistance: 0.4,
        ..boundary()
    }];
    assert!(a.solve_exterior(&cloth(), &body(), &b, &e, &[]).is_err());
    let solved = a.solve_exterior(&cloth(), &body(), &b, &e, &bare).unwrap();
    near(solved.steady.covered_area_m2, 0.25);
    near(solved.steady.bare_power_w, 12.5);
    near(solved.convective_power_w, 0.25 * 30. / 0.9);
    near(solved.steady.power_w, 12.5 + 0.25 * 30. / 0.9);
    near(solved.steady.unassigned_cloth_reference_area_m2, 0.625);
    assert!(
        a.solve_exterior(
            &cloth(),
            &body(),
            &b,
            &e,
            &[Boundary {
                inside_c: 31.,
                ..bare[0]
            }]
        )
        .is_err()
    );
    assert!(
        a.solve_exterior(
            &cloth(),
            &body(),
            &b,
            &e,
            &[Boundary {
                inside_resistance: 0.,
                outside_resistance: 0.,
                ..bare[0]
            }]
        )
        .is_err()
    );
    assert!(a.solve_exterior(&cloth(), &body(), &b, &[], &bare).is_err());
    assert!(
        a.solve_exterior(&cloth(), &body(), &b, &e, &[bare[0]; 2])
            .is_err()
    );
    // Entirely bare geometry has no covered optical exchange and still requires explicit films.
    let mut l = layout();
    l.regions.clear();
    let a = SteadyCoverage::new(source(), l).unwrap();
    let solved = a.solve_exterior(&cloth(), &body(), &b, &e, &bare).unwrap();
    assert!(solved.regions.is_empty());
    near(solved.steady.power_w, 25.);
    near(solved.convective_power_w, 0.);
}

#[test]
fn only_exposed_finish_affects_layered_radiation_and_frozen_material_replay() {
    use ftx_drape::thermal::steady::exterior::STEFAN_BOLTZMANN;
    let e = environment(true);
    // Independent fourth powers manufacture a known outer face temperature on unequal areas.
    let t: f64 = 10.;
    let fourth = |c: f64| (c + 273.15).powi(4);
    let rad = 0.8 * STEFAN_BOLTZMANN * (fourth(t) - fourth(-10.));
    let outer_flux = 5. * t + rad;
    let body_flux = 2. * outer_flux;
    let b = Boundary {
        inside_c: t + body_flux * 0.6,
        ..exterior_boundary()
    };
    let mut values = Vec::new();
    for (inner_eps, outer_eps) in [(0.1, 0.8), (0.9, 0.8), (0.9, 0.05)] {
        let a = SteadyCoverage::new(
            source_with(
                exterior_selection(0.2, inner_eps),
                exterior_selection(0.4, outer_eps),
            ),
            layout(),
        )
        .unwrap();
        let solved = a
            .solve_exterior(&cloth(), &body(), &[b], std::slice::from_ref(&e), &[])
            .unwrap();
        let replay = SteadyCoverage::from_snapshot(&a.snapshot().unwrap())
            .unwrap()
            .solve_exterior(&cloth(), &body(), &[b], std::slice::from_ref(&e), &[])
            .unwrap();
        assert_eq!(
            serde_json::to_value(&solved).unwrap(),
            serde_json::to_value(replay).unwrap()
        );
        values.push(solved);
    }
    near(values[0].regions[0].surface_c, t);
    near(values[0].steady.power_w, 0.5 * body_flux);
    near(values[0].radiative_power_w, rad);
    near(values[0].convective_power_w, 50.);
    assert_eq!(values[0].steady.power_w, values[1].steady.power_w);
    assert!(values[2].steady.power_w < values[0].steady.power_w);
    let mut split = layout();
    split.regions[0].body_fraction = 0.5;
    for p in &mut split.regions[0].layers {
        p.fraction = 0.5;
    }
    let mut second = split.regions[0].clone();
    second.id = "second".into();
    split.regions.push(second);
    let refined = SteadyCoverage::new(
        source_with(exterior_selection(0.2, 0.1), exterior_selection(0.4, 0.8)),
        split,
    )
    .unwrap()
    .solve_exterior(&cloth(), &body(), &[b], &[e], &[])
    .unwrap();
    near(refined.steady.power_w, values[0].steady.power_w);
    near(refined.convective_power_w, values[0].convective_power_w);
    near(refined.radiative_power_w, values[0].radiative_power_w);
}

//! Independently calculated series/area answers; synthetic geometry is not empirical validation.

use ftx_core::{Vec3, receipt::sha256_hex};
use ftx_drape::{thermal::surface::Patch, vapor::coverage::*};
use ftx_pattern::{material_catalog::Selection, material_vapor, materials::Catalog};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn catalog() -> Catalog {
    Catalog {
        schema: "ftx-textile-catalog/1".into(),
        materials: vec![],
    }
}

fn material(id: &str, resistance: f64) -> Selection {
    let s: Selection = serde_json::from_value(json!({
        "id":id,"allow_estimates":true,"record":{"id":id,"name":id,"form":"woven",
            "properties":{"vapor_resistance":{"value":resistance,"unit":"m2*Pa/W",
                "evidence":"estimate","source":"synthetic","method":"analytic resistor",
                "conditions":{"scope":"synthetic software test"}}},
            "sources":{"synthetic":{"reference":"local:analytic-vapor-coverage",
                "accessed_on":"2026-10-22","note":"Synthetic resistance only; no textile measurement"}}}
    })).unwrap();
    catalog().resolve(&s).unwrap().frozen_selection()
}

fn request() -> Request {
    Request {
        schema: SCHEMA.into(),
        source: Source {
            schema: "ftx-vapor-coverage-source/1".into(),
            positions_m: vec![
                Vec3::new(0., 0., 0.01),
                Vec3::new(1., 0., 0.01),
                Vec3::new(0., 1., 0.01),
                Vec3::new(0., 0., 0.02),
                Vec3::new(1., 0., 0.02),
                Vec3::new(0., 1., 0.02),
            ],
            patches: vec![
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
            materials: vec![material("lining", 2.), material("shell", 3.)],
            instances: vec!["lining".into(), "shell".into()],
            triangle_instance: vec![0, 1],
            material_report_sha256: sha256_hex(b"declared synthetic source; not a drape receipt"),
        },
        map: Map {
            body_positions_m: vec![Vec3::ZERO, Vec3::new(1., 0., 0.), Vec3::new(0., 1., 0.)],
            body_triangles: vec![[0, 1, 2]],
            instance_layers: BTreeMap::from([
                ("lining".into(), "lining".into()),
                ("shell".into(), "shell".into()),
            ]),
            settings: Settings {
                max_distance_m: 1.,
                min_normal_cosine: 0.5,
                separation_tolerance_m: 1e-6,
                interfaces: vec![Interface {
                    inner: "lining".into(),
                    outer: "shell".into(),
                    resistance_m2_pa_w: 2.,
                    basis: "explicit synthetic interface assumption".into(),
                }],
            },
        },
        conditions: Conditions {
            boundaries: vec![Boundary {
                inside_vapor_pressure_pa: 2000.,
                outside_vapor_pressure_pa: 1000.,
                inside_film_resistance_m2_pa_w: 1.,
                outside_film_resistance_m2_pa_w: 4.,
                latent_heat_j_kg: 2_500_000.,
            }],
            body_mesh_sha256: None,
            required_conditions: BTreeMap::new(),
            required_method: None,
            allow_mixed_protocols: false,
        },
    }
}

fn close(actual: &Value, expected: f64) {
    let actual = actual.as_f64().unwrap();
    assert!(
        (actual - expected).abs() < 1e-12 * expected.abs().max(1e-10),
        "{actual} != {expected}"
    );
}

#[test]
fn overlapping_layers_count_body_once_and_match_independent_series_answer_without_dry_properties() {
    let input = request();
    let r = input.evaluate().unwrap();
    close(&r["coverage"]["body_area_m2"], 0.5);
    close(&r["coverage"]["covered_area_m2"], 0.5);
    assert_eq!(r["coverage"]["uncovered_area_m2"], 0.0);
    close(
        &r["coverage"]["covered_potential_heat_flow_w"],
        1000.0 / 12.0 * 0.5,
    );
    close(
        &r["coverage"]["covered_potential_mass_flow_kg_s"],
        1000.0 / 12.0 * 0.5 / 2_500_000.0,
    );
    let region = &r["coverage"]["regions"][0];
    assert_eq!(region["layer_area_m2"], json!([0.5, 0.5]));
    assert_eq!(region["vapor"]["layers"].as_array().unwrap().len(), 2);
    assert_eq!(region["vapor"]["interfaces"][0]["resistance_m2_pa_w"], 2.0);
    assert_eq!(r["dry_thermal_properties_required"], false);
    assert_eq!(r["physical_validation_claim"], false);
    assert!(
        r["projection"]["regions"][0]
            .get("interface_resistance_m2_k_w")
            .is_none()
    );
    let replay: Request = serde_json::from_str(r["replay_request_json"].as_str().unwrap()).unwrap();
    assert_eq!(replay.evaluate().unwrap(), r);
    let standalone: material_vapor::Request =
        serde_json::from_str(region["vapor"]["replay_request_json"].as_str().unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(standalone.evaluate(&catalog()).unwrap()).unwrap(),
        region["vapor"]
    );
}

#[test]
fn actual_sloped_cloth_area_changes_transport_without_changing_ret_or_reference_area() {
    let mut input = request();
    let baseline = input.evaluate().unwrap();
    input.source.positions_m[4].z = 0.77;
    let changed = input.evaluate().unwrap();
    let region = &changed["coverage"]["regions"][0];
    close(&region["layer_area_m2"][1], 0.625);
    close(&region["vapor"]["layers"][1]["observation"]["value"], 3.0);
    close(
        &region["vapor"]["layers"][1]["effective_resistance_m2_pa_w"],
        2.4,
    );
    close(
        &changed["coverage"]["covered_potential_heat_flow_w"],
        1000.0 / 11.4 * 0.5,
    );
    assert_ne!(changed["source_sha256"], baseline["source_sha256"]);
    assert_ne!(changed["request_sha256"], baseline["request_sha256"]);
    assert_eq!(
        changed["request"]["source"]["materials"],
        baseline["request"]["source"]["materials"]
    );
    assert_eq!(
        changed["request"]["source"]["patches"],
        baseline["request"]["source"]["patches"]
    );
}

#[test]
fn partial_coverage_retains_unknown_body_fraction_and_unallocated_cloth() {
    let mut input = request();
    input.map.body_positions_m.push(Vec3::new(1., 1., 0.));
    input.map.body_triangles.push([1, 3, 2]);
    let r = input.evaluate().unwrap();
    close(&r["coverage"]["body_area_m2"], 1.0);
    close(&r["coverage"]["covered_area_m2"], 0.5);
    close(&r["coverage"]["uncovered_area_m2"], 0.5);
    close(
        &r["coverage"]["covered_potential_heat_flow_w"],
        1000.0 / 12.0 * 0.5,
    );
    assert_eq!(r["coverage"]["body"][1]["uncovered_area_m2"], 0.5);
    assert!(
        r["coverage"]
            .get("total_body_potential_heat_flow_w")
            .is_none()
    );
    // Narrowing the declared body boundary consumes only one quarter of each retained sheet.
    input.map.body_triangles.truncate(1);
    input.map.body_positions_m[1].x = 0.5;
    input.map.body_positions_m[2].y = 0.5;
    let small = input.evaluate().unwrap();
    close(&small["coverage"]["covered_area_m2"], 0.125);
    close(
        &small["coverage"]["unassigned_cloth_reference_area_m2"],
        0.75,
    );
}

#[test]
fn material_substitution_changes_only_its_qualified_network_and_preserves_exact_replay() {
    let mut input = request();
    let a = input.evaluate().unwrap();
    input.source.materials[1] = material("shell2", 7.);
    let b = input.evaluate().unwrap();
    close(
        &b["coverage"]["covered_potential_heat_flow_w"],
        1000.0 / 16.0 * 0.5,
    );
    assert_ne!(a["source_sha256"], b["source_sha256"]);
    assert_eq!(
        a["request"]["source"]["positions_m"],
        b["request"]["source"]["positions_m"]
    );
    assert_eq!(a["projection"]["footprints"], b["projection"]["footprints"]);
    let replay: Request = serde_json::from_str(b["replay_request_json"].as_str().unwrap()).unwrap();
    assert_eq!(replay.evaluate().unwrap(), b);
    input.source.materials[1].allow_estimates = false;
    assert!(input.evaluate().is_err());
}

#[test]
fn missing_interfaces_source_data_and_inconsistent_geometry_refuse_without_guessing() {
    let mut input = request();
    input.map.settings.interfaces.clear();
    assert!(
        input
            .evaluate()
            .unwrap_err()
            .message
            .contains("missing explicit vapor interface")
    );
    input = request();
    input.map.settings.interfaces[0].resistance_m2_pa_w = 0.;
    close(
        &input.evaluate().unwrap()["coverage"]["covered_potential_heat_flow_w"],
        50.0,
    );
    input = request();
    input
        .map
        .instance_layers
        .insert("shell".into(), "lining".into());
    input.map.settings.interfaces.clear();
    assert!(input.evaluate().is_err());
    input = request();
    input.source.positions_m[3].z = 0.; // Shell crosses the inner layer over the body polygon.
    assert!(input.evaluate().is_err());
    input = request();
    input.map.body_triangles.push([2, 1, 0]);
    assert!(input.evaluate().is_err());
    input = request();
    input.source.triangle_instance[1] = 0; // One source instance cannot own two materials.
    assert!(input.evaluate().is_err());
    input = request();
    input.source.materials[0].expected_sha256 = None;
    assert!(input.evaluate().is_err());
    input = request();
    let record = input.source.materials[0].record.as_mut().unwrap();
    record.properties.remove("vapor_resistance");
    let mut selection = input.source.materials[0].clone();
    selection.expected_sha256 = None;
    input.source.materials[0] = catalog().resolve(&selection).unwrap().frozen_selection();
    assert!(input.evaluate().is_err());
}

#[test]
fn source_protocol_filters_and_body_boundaries_are_enforced_for_replay() {
    let mut input = request();
    input.conditions.required_method = Some("a different acquisition".into());
    assert!(input.evaluate().is_err());
    input = request();
    input.map.body_positions_m.push(Vec3::new(1., 1., 0.));
    input.map.body_triangles.push([1, 3, 2]);
    input
        .conditions
        .boundaries
        .push(input.conditions.boundaries[0]);
    assert!(input.evaluate().is_err());
    let body =
        json!({"positions_m":input.map.body_positions_m,"triangles":input.map.body_triangles});
    input.conditions.body_mesh_sha256 = Some(sha256_hex(&serde_json::to_vec(&body).unwrap()));
    assert!(input.evaluate().is_ok());
    input.map.body_positions_m[1].x = 0.9;
    assert!(input.evaluate().is_err());
}

#[test]
fn protocols_must_match_across_disjoint_regions_even_when_each_path_has_one_layer() {
    let mut input = request();
    // Two disjoint body patches, each with one layer: neither local column can discover a
    // protocol mismatch in the other path. The coverage total must enforce that policy.
    for vertex in &mut input.source.positions_m[3..] {
        vertex.x += 2.0;
    }
    input.map.body_positions_m.extend([
        Vec3::new(2., 0., 0.),
        Vec3::new(3., 0., 0.),
        Vec3::new(2., 1., 0.),
    ]);
    input.map.body_triangles.push([3, 4, 5]);
    let mut different = input.source.materials[1].clone();
    different.expected_sha256 = None;
    different
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("vapor_resistance")
        .unwrap()
        .conditions
        .insert("scope".into(), "second synthetic protocol".into());
    input.source.materials[1] = catalog().resolve(&different).unwrap().frozen_selection();
    let refusal = input.evaluate().unwrap_err();
    assert!(refusal.message.contains("across regions"));
    input.conditions.allow_mixed_protocols = true;
    let admitted = input.evaluate().unwrap();
    assert_eq!(admitted["mixed_protocols"], true);
    assert_eq!(admitted["coverage"]["regions"].as_array().unwrap().len(), 2);
    close(&admitted["coverage"]["covered_area_m2"], 1.0);
    close(
        &admitted["coverage"]["covered_potential_heat_flow_w"],
        0.5 * 1000.0 / (1.0 + 2.0 + 4.0) + 0.5 * 1000.0 / (1.0 + 3.0 + 4.0),
    );
}

#[test]
fn entirely_uncovered_body_stays_unknown_and_does_not_demand_unused_material_ret() {
    let mut input = request();
    for vertex in &mut input.source.positions_m {
        vertex.x += 2.0;
    }
    for selection in &mut input.source.materials {
        selection.expected_sha256 = None;
        selection.record.as_mut().unwrap().properties.clear();
        *selection = catalog().resolve(selection).unwrap().frozen_selection();
    }
    let report = input.evaluate().unwrap();
    close(&report["coverage"]["body_area_m2"], 0.5);
    close(&report["coverage"]["uncovered_area_m2"], 0.5);
    close(
        &report["coverage"]["unassigned_cloth_reference_area_m2"],
        1.0,
    );
    assert_eq!(report["coverage"]["covered_area_m2"], 0.0);
    assert_eq!(report["coverage"]["covered_potential_heat_flow_w"], 0.0);
    assert_eq!(report["coverage"]["regions"], json!([]));
    let replay: Request =
        serde_json::from_str(report["replay_request_json"].as_str().unwrap()).unwrap();
    assert_eq!(replay.evaluate().unwrap(), report);
}

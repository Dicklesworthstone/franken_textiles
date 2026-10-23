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

fn with_bounds(mut input: Request) -> Request {
    for (selection, bounds) in input.source.materials.iter_mut().zip([[1., 3.], [2., 4.]]) {
        set_bounds(selection, Some(bounds));
    }
    input
}

fn set_bounds(selection: &mut Selection, bounds: Option<[f64; 2]>) {
    selection.expected_sha256 = None;
    selection
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("vapor_resistance")
        .unwrap()
        .bounds = bounds;
    *selection = catalog().resolve(selection).unwrap().frozen_selection();
}

fn close(actual: &Value, expected: f64) {
    let actual = actual.as_f64().unwrap();
    assert!(
        (actual - expected).abs() < 1e-12 * expected.abs().max(1e-10),
        "{actual} != {expected}"
    );
}

fn history_request(initial: Request) -> history::Request {
    let mut translated_cloth = initial.source.positions_m.clone();
    let mut translated_body = initial.map.body_positions_m.clone();
    for p in translated_cloth.iter_mut().chain(&mut translated_body) {
        p.x += 0.2;
    }
    let mut stretched_cloth = translated_cloth.clone();
    let mut stretched_body = translated_body.clone();
    for p in stretched_cloth.iter_mut().chain(&mut stretched_body) {
        p.x *= 2.0;
    }
    history::Request {
        schema: history::SCHEMA.into(),
        policy: history::Policy {
            schema: "ftx-motion-vapor-coverage/1".into(),
            integration: history::Integration::RightEndpoint,
            max_geometry_operations: 1_000_000,
        },
        initial,
        samples: vec![
            history::Sample {
                time_s: 0.1,
                positions_m: translated_cloth,
                body_positions_m: translated_body,
            },
            history::Sample {
                time_s: 0.25,
                positions_m: stretched_cloth,
                body_positions_m: stretched_body,
            },
        ],
        path_sha256: Some(sha256_hex(b"declared synthetic prescribed path")),
    }
}

#[test]
fn moving_history_matches_independent_area_time_integrals_and_exact_frozen_replay() {
    let input = history_request(request());
    let r = input.evaluate().unwrap();
    assert_eq!(r["integration"], "right_endpoint");
    assert_eq!(r["duration_s"], 0.25);
    // First interval translates 0.5 m² rigidly; the second has 1 m² current boundary/layer area.
    let energy = 1000.0 / 12.0 * (0.5 * 0.1 + 1.0 * 0.15);
    close(&r["totals"]["potential_energy_j"], energy);
    close(&r["totals"]["potential_mass_kg"], energy / 2_500_000.0);
    close(
        &r["samples"][0]["potential_energy_j"],
        1000.0 / 12.0 * 0.5 * 0.1,
    );
    close(
        &r["samples"][1]["result"]["coverage"]["covered_area_m2"],
        1.0,
    );
    for (sample, row) in input.samples.iter().zip(r["samples"].as_array().unwrap()) {
        assert_eq!(
            row["result"]["request"]["source"]["positions_m"],
            json!(sample.positions_m)
        );
        assert_eq!(
            row["result"]["request"]["source"]["patches"],
            json!(input.initial.source.patches)
        );
        assert_eq!(
            row["result"]["request"]["source"]["materials"],
            json!(input.initial.source.materials)
        );
        assert_ne!(
            row["result"]["body_mesh_sha256"],
            r["initial"]["result"]["body_mesh_sha256"]
        );
    }
    assert_eq!(r["latent_heat_coupled"], false);
    assert_eq!(r["dry_thermal_properties_required"], false);
    let replay: history::Request =
        serde_json::from_str(r["replay_request_json"].as_str().unwrap()).unwrap();
    assert_eq!(replay.evaluate().unwrap(), r);
}

#[test]
fn complete_ret_ranges_propagate_through_coverage_and_moving_area_time_integrals() {
    let initial = with_bounds(request());
    let frame = initial.evaluate().unwrap();
    let bounds = &frame["coverage"]["reported_bounds"];
    // Fixed films + interface = 7; summed layer Ret range = [3,7].
    close(&bounds["covered_potential_heat_flow_w"][0], 500. / 14.);
    close(&bounds["covered_potential_heat_flow_w"][1], 500. / 10.);
    close(
        &bounds["covered_potential_mass_flow_kg_s"][0],
        500. / 14. / 2_500_000.,
    );
    assert_eq!(frame["coverage"]["bounds_unavailable"], json!([]));
    assert_eq!(
        frame["coverage"]["regions"][0]["vapor"]["reported_bounds"]["potential_heat_flow_w"],
        bounds["covered_potential_heat_flow_w"]
    );
    let input = history_request(initial);
    let result = input.evaluate().unwrap();
    let bounds = &result["totals"]["reported_bounds"];
    for (endpoint, resistance) in [14., 10.].into_iter().enumerate() {
        let energy = 1000. / resistance * (0.5 * 0.1 + 1. * 0.15);
        close(&bounds["potential_energy_j"][endpoint], energy);
        close(&bounds["potential_mass_kg"][endpoint], energy / 2_500_000.);
        close(
            &result["samples"][0]["reported_bounds"]["potential_energy_j"][endpoint],
            1000. / resistance * 0.5 * 0.1,
        );
        close(
            &result["samples"][1]["reported_bounds"]["potential_energy_j"][endpoint],
            1000. / resistance * 1. * 0.15,
        );
    }
    assert_eq!(result["totals"]["bounds_unavailable"], json!([]));
    let replay: history::Request =
        serde_json::from_str(result["replay_request_json"].as_str().unwrap()).unwrap();
    assert_eq!(replay.evaluate().unwrap(), result);
}

#[test]
fn a_late_unbounded_layer_invalidates_integral_ranges_but_preserves_nominal_history() {
    let mut initial = with_bounds(request());
    set_bounds(&mut initial.source.materials[1], None);
    // The unbounded shell initially lies outside the body domain and is not consumed.
    for p in &mut initial.source.positions_m[3..] {
        p.x += 2.;
    }
    let mut input = history_request(initial);
    // It covers the body only in the last interval; both early ranges must remain inspectable.
    for i in 0..3 {
        input.samples[1].positions_m[3 + i] =
            input.samples[1].positions_m[i] + Vec3::new(0., 0., 0.01);
    }
    let result = input.evaluate().unwrap();
    assert!(result["initial"]["result"]["coverage"]["reported_bounds"].is_object());
    assert!(result["samples"][0]["reported_bounds"].is_object());
    assert!(result["samples"][1]["reported_bounds"].is_null());
    assert!(result["totals"]["reported_bounds"].is_null());
    assert!(
        !result["totals"]["bounds_unavailable"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    close(
        &result["totals"]["potential_energy_j"],
        1000. * (0.5 * 0.1 / 7. + 1. * 0.15 / 12.),
    );
}

#[test]
fn an_unbounded_time_zero_layer_does_not_invalidate_later_complete_integral_ranges() {
    let mut initial = with_bounds(request());
    set_bounds(&mut initial.source.materials[1], None);
    let mut input = history_request(initial);
    for sample in &mut input.samples {
        for p in &mut sample.positions_m[3..] {
            p.x += 4.;
        }
    }
    let result = input.evaluate().unwrap();
    assert!(result["initial"]["result"]["coverage"]["reported_bounds"].is_null());
    assert!(result["samples"][0]["reported_bounds"].is_object());
    assert!(result["samples"][1]["reported_bounds"].is_object());
    // Only bounded lining contributes over positive duration; films 5 + lining [1,3].
    close(
        &result["totals"]["reported_bounds"]["potential_energy_j"][0],
        1000. * (0.5 * 0.1 + 1. * 0.15) / 8.,
    );
    close(
        &result["totals"]["reported_bounds"]["potential_energy_j"][1],
        1000. * (0.5 * 0.1 + 1. * 0.15) / 6.,
    );
}

#[test]
fn composite_trapezoid_ret_ranges_include_both_distinct_endpoints_and_replay_exactly() {
    let mut input = trapezoid_area_history(2, |t| 1.0 + t);
    input.initial = with_bounds(input.initial);
    let result = input.evaluate().unwrap();
    // Areas 0.5, 0.75 and 1 m² give interval area-time integrals 0.3125 and 0.4375 m²s.
    for (endpoint, resistance) in [14., 10.].into_iter().enumerate() {
        let energy = 1000. * 0.75 / resistance;
        close(
            &result["totals"]["reported_bounds"]["potential_energy_j"][endpoint],
            energy,
        );
        close(
            &result["totals"]["reported_bounds"]["potential_mass_kg"][endpoint],
            energy / 2_500_000.,
        );
        for (sample, area_time) in [0.3125, 0.4375].into_iter().enumerate() {
            let bounds = &result["samples"][sample]["reported_bounds"];
            close(
                &bounds["potential_energy_j"][endpoint],
                1000. * area_time / resistance,
            );
            close(
                &bounds["potential_mass_kg"][endpoint],
                1000. * area_time / resistance / 2_500_000.,
            );
            assert_eq!(result["samples"][sample]["bounds_unavailable"], json!([]));
        }
    }
    assert_eq!(result["totals"]["bounds_unavailable"], json!([]));
    let replay: history::Request =
        serde_json::from_str(result["replay_request_json"].as_str().unwrap()).unwrap();
    assert_eq!(replay.evaluate().unwrap(), result);
}

#[test]
fn composite_trapezoid_missing_time_zero_bounds_invalidates_first_interval_and_total_range() {
    let mut initial = with_bounds(request());
    set_bounds(&mut initial.source.materials[1], None);
    let mut input = history_request(initial);
    input.policy.integration = history::Integration::CompositeTrapezoid;
    for sample in &mut input.samples {
        for p in &mut sample.positions_m[3..] {
            p.x += 4.;
        }
    }
    let result = input.evaluate().unwrap();
    assert!(result["initial"]["result"]["coverage"]["reported_bounds"].is_null());
    assert!(result["samples"][0]["reported_bounds"].is_null());
    assert!(
        result["samples"][0]["bounds_unavailable"][0]
            .as_str()
            .unwrap()
            .starts_with("sample 0 start:")
    );
    assert!(result["samples"][1]["reported_bounds"].is_object());
    assert!(result["totals"]["reported_bounds"].is_null());
    assert!(
        !result["totals"]["bounds_unavailable"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    // The first interval still integrates the nominal two-layer initial potential.
    let energy = 1000. * ((0.5 / 12. + 0.5 / 7.) * 0.05 + (0.5 / 7. + 1. / 7.) * 0.075);
    close(&result["totals"]["potential_energy_j"], energy);
    close(&result["totals"]["potential_mass_kg"], energy / 2_500_000.);
    for (endpoint, resistance) in [8., 6.].into_iter().enumerate() {
        let bounds = &result["samples"][1]["reported_bounds"];
        close(
            &bounds["potential_energy_j"][endpoint],
            1000. * (0.5 + 1.) * 0.075 / resistance,
        );
        close(
            &bounds["potential_mass_kg"][endpoint],
            1000. * (0.5 + 1.) * 0.075 / resistance / 2_500_000.,
        );
    }
    let replay: history::Request =
        serde_json::from_str(result["replay_request_json"].as_str().unwrap()).unwrap();
    assert_eq!(replay.evaluate().unwrap(), result);
}

#[test]
fn moving_nonuniform_pressures_follow_verified_original_body_triangles() {
    let mut initial = request();
    initial.map.body_positions_m.push(Vec3::new(0.5, 0.5, 0.0));
    initial.map.body_triangles = vec![[0, 1, 3], [0, 3, 2]];
    let mut other = initial.conditions.boundaries[0];
    other.inside_vapor_pressure_pa = 3000.0;
    initial.conditions.boundaries.push(other);
    initial.conditions.body_mesh_sha256 = Some(sha256_hex(
        &serde_json::to_vec(&json!({
            "positions_m":initial.map.body_positions_m,"triangles":initial.map.body_triangles,
        }))
        .unwrap(),
    ));
    let mut input = history_request(initial);
    input.samples.truncate(1);
    let report = input.evaluate().unwrap();
    // Two 0.25 m² domains keep their respective 1000 Pa and 2000 Pa driving pressures.
    close(
        &report["totals"]["potential_energy_j"],
        0.1 * (1000.0 + 2000.0) * 0.25 / 12.0,
    );
    assert_eq!(
        report["samples"][0]["result"]["request"]["conditions"]["boundaries"],
        json!(input.initial.conditions.boundaries)
    );
    input.initial.conditions.body_mesh_sha256 = Some("a".repeat(64));
    assert!(
        input
            .evaluate()
            .unwrap_err()
            .message
            .contains("body_mesh_sha256")
    );
    assert!(
        input
            .initial
            .with_positions(
                &input.samples[0].positions_m,
                &input.samples[0].body_positions_m
            )
            .is_err()
    );
    input.initial.conditions.body_mesh_sha256 = None;
    assert!(input.evaluate().is_err());
}

#[test]
fn moving_history_refuses_last_sample_without_publishing_partial_integrals() {
    let mut input = history_request(request());
    input.samples[1].positions_m[3].z = 0.01;
    input.samples[1].positions_m[4].z = 0.01;
    input.samples[1].positions_m[5].z = 0.01;
    assert!(
        input.evaluate().is_err(),
        "coincident different layers must refuse the whole history"
    );
    let mut input = history_request(request());
    input.samples[1].time_s = input.samples[0].time_s;
    assert!(input.evaluate().is_err());
    input.samples[1].time_s = 0.25;
    input.samples[1].positions_m.pop();
    assert!(input.evaluate().is_err());
}

#[test]
fn moving_history_charges_initial_and_every_projection_and_refuses_within_remaining_work() {
    let mut input = history_request(request());
    let report = input.evaluate().unwrap();
    let work = report["geometry_operations"].as_u64().unwrap() as usize;
    let counted: usize = std::iter::once(&report["initial"])
        .chain(report["samples"].as_array().unwrap())
        .map(|row| {
            row["result"]["projection"]["geometry_operations"]
                .as_u64()
                .unwrap() as usize
        })
        .sum();
    assert_eq!(work, counted);
    assert!(work > 1);
    input.policy.max_geometry_operations = work;
    assert!(input.evaluate().is_ok());
    input.policy.max_geometry_operations = work - 1;
    assert_eq!(
        input.evaluate().unwrap_err().code,
        ftx_core::RefusalCode::BudgetExceeded
    );
    input.policy.max_geometry_operations = 1;
    assert_eq!(
        input.evaluate().unwrap_err().code,
        ftx_core::RefusalCode::BudgetExceeded
    );
}

#[test]
fn moving_history_keeps_partial_coverage_unknown_and_source_substitution_effects_separate() {
    let mut initial = request();
    initial.map.body_positions_m.push(Vec3::new(1.0, 1.0, 0.0));
    initial.map.body_triangles.push([1, 3, 2]);
    let mut input = history_request(initial);
    let baseline = input.evaluate().unwrap();
    close(
        &baseline["samples"][0]["result"]["coverage"]["uncovered_area_m2"],
        0.5,
    );
    close(
        &baseline["samples"][1]["result"]["coverage"]["uncovered_area_m2"],
        1.0,
    );
    input.initial.source.materials[1] = material("higher-ret-shell", 15.0);
    let changed = input.evaluate().unwrap();
    close(
        &changed["totals"]["potential_energy_j"],
        baseline["totals"]["potential_energy_j"].as_f64().unwrap() / 2.0,
    );
    assert_ne!(changed["request_sha256"], baseline["request_sha256"]);
    assert_eq!(
        changed["request"]["samples"],
        baseline["request"]["samples"]
    );
}

#[test]
fn right_endpoint_potential_converges_to_independent_linear_area_time_integral() {
    // A(t)=0.5*(1+t) m² for t in [0,1], with the complete two-layer resistance fixed at 12.
    let exact = 1000.0 / 12.0 * 0.75;
    let mut errors = Vec::new();
    for n in [2, 4, 8] {
        let mut input = history_request(request());
        input.samples = (1..=n)
            .map(|i| {
                let time = i as f64 / n as f64;
                let scale = |p: &Vec3| Vec3::new(p.x * (1.0 + time), p.y, p.z);
                history::Sample {
                    time_s: time,
                    positions_m: input.initial.source.positions_m.iter().map(scale).collect(),
                    body_positions_m: input
                        .initial
                        .map
                        .body_positions_m
                        .iter()
                        .map(scale)
                        .collect(),
                }
            })
            .collect();
        let report = input.evaluate().unwrap();
        let error = report["totals"]["potential_energy_j"].as_f64().unwrap() - exact;
        assert!(error > 0.0);
        assert!((error - 1000.0 / 12.0 * 0.25 / n as f64).abs() < 1e-10);
        errors.push(error);
    }
    assert!((errors[0] / errors[1] - 2.0).abs() < 1e-10);
    assert!((errors[1] / errors[2] - 2.0).abs() < 1e-10);
}

fn trapezoid_area_history(n: usize, scale: impl Fn(f64) -> f64) -> history::Request {
    let mut input = history_request(request());
    input.policy.integration = history::Integration::CompositeTrapezoid;
    input.samples = (1..=n)
        .map(|i| {
            let time = i as f64 / n as f64;
            let factor = scale(time);
            let stretch = |p: &Vec3| Vec3::new(p.x * factor, p.y, p.z);
            history::Sample {
                time_s: time,
                positions_m: input
                    .initial
                    .source
                    .positions_m
                    .iter()
                    .map(stretch)
                    .collect(),
                body_positions_m: input
                    .initial
                    .map
                    .body_positions_m
                    .iter()
                    .map(stretch)
                    .collect(),
            }
        })
        .collect();
    input
}

#[test]
fn composite_trapezoid_integrates_linear_area_exactly_and_replays_the_same_samples() {
    // A(t)=0.5*(1+t) m² integrates to 0.75 m²s without another geometric evaluation.
    let exact = 1000.0 / 12.0 * 0.75;
    for n in [1, 3, 8] {
        let input = trapezoid_area_history(n, |t| 1.0 + t);
        let report = input.evaluate().unwrap();
        assert_eq!(report["integration"], "composite_trapezoid");
        close(&report["totals"]["potential_energy_j"], exact);
        close(&report["totals"]["potential_mass_kg"], exact / 2_500_000.0);
        let mut previous = 0.0;
        for (sample, row) in input
            .samples
            .iter()
            .zip(report["samples"].as_array().unwrap())
        {
            let t = sample.time_s;
            // Integrate the prescribed linear area analytically on this exact interval.
            let area_time = 0.5 * (t - previous) + 0.25 * (t * t - previous * previous);
            close(&row["potential_energy_j"], 1000.0 / 12.0 * area_time);
            close(
                &row["potential_mass_kg"],
                1000.0 / 12.0 * area_time / 2_500_000.0,
            );
            previous = t;
        }
        let frozen: history::Request =
            serde_json::from_str(report["replay_request_json"].as_str().unwrap()).unwrap();
        assert_eq!(frozen.evaluate().unwrap(), report);

        let mut right = input.clone();
        right.policy.integration = history::Integration::RightEndpoint;
        let baseline = right.evaluate().unwrap();
        assert_eq!(
            baseline["geometry_operations"],
            report["geometry_operations"]
        );
        assert_eq!(baseline["request"]["samples"], report["request"]["samples"]);
        assert_ne!(baseline["request_sha256"], report["request_sha256"]);
    }
}

#[test]
fn composite_trapezoid_refines_quadratically_for_nonlinear_area() {
    // A(t)=0.5*(1+t²) m²: its independent integral is 2/3 m²s. The sampled rule's
    // error is exactly 1/(12*n²) m²s, so doubling the samples reduces it by four.
    let exact = 1000.0 / 12.0 * (2.0 / 3.0);
    let mut errors = Vec::new();
    for n in [2, 4, 8] {
        let report = trapezoid_area_history(n, |t| 1.0 + t * t)
            .evaluate()
            .unwrap();
        let energy = report["totals"]["potential_energy_j"].as_f64().unwrap();
        let error = energy - exact;
        assert!(error > 0.0);
        assert!((error - 1000.0 / (144.0 * (n * n) as f64)).abs() < 1e-10);
        close(&report["totals"]["potential_mass_kg"], energy / 2_500_000.0);
        errors.push(error);
    }
    assert!((errors[0] / errors[1] - 4.0).abs() < 1e-10);
    assert!((errors[1] / errors[2] - 4.0).abs() < 1e-10);
}

#[test]
fn composite_trapezoid_keeps_initial_flow_when_later_coverage_vanishes() {
    let mut input = history_request(request());
    input.policy.integration = history::Integration::CompositeTrapezoid;
    let away: Vec<_> = input
        .initial
        .source
        .positions_m
        .iter()
        .map(|p| *p + Vec3::new(2.0, 0.0, 0.0))
        .collect();
    input.samples = [1.0, 3.0]
        .into_iter()
        .map(|time_s| history::Sample {
            time_s,
            positions_m: away.clone(),
            body_positions_m: input.initial.map.body_positions_m.clone(),
        })
        .collect();
    let report = input.evaluate().unwrap();
    // Initial potential is 1000*0.5/12 W; only its half of the first interval contributes.
    let energy = 1000.0 * 0.5 / 12.0 * 0.5;
    close(&report["totals"]["potential_energy_j"], energy);
    close(&report["totals"]["potential_mass_kg"], energy / 2_500_000.0);
    close(&report["samples"][0]["potential_energy_j"], energy);
    assert_eq!(report["samples"][1]["potential_energy_j"], 0.0);
    assert_eq!(report["samples"][1]["potential_mass_kg"], 0.0);
    for row in report["samples"].as_array().unwrap() {
        assert_eq!(row["result"]["coverage"]["uncovered_area_m2"], 0.5);
        assert_eq!(
            row["result"]["coverage"]["covered_potential_heat_flow_w"],
            0.0
        );
    }
    assert_eq!(report["latent_heat_coupled"], false);
    input.policy.integration = history::Integration::RightEndpoint;
    assert_eq!(
        input.evaluate().unwrap()["totals"]["potential_energy_j"],
        0.0
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

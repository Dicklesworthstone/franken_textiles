//! Source-qualified moving bridge decisions through the actual selected-model native and portable
//! paths. All observations, including the temporal approximation, are synthetic numerical fixtures.
use super::*;
use ftx_core::{Vec2, Vec3};
use ftx_drape::study::fitted::{BodyRequest, FittedBody};
use ftx_pattern::Pattern;
use ftx_wasm::materials::scenario::on_model;

fn keyed<'a>(rows: &'a Value, key: &str) -> &'a Value {
    rows.as_array()
        .unwrap()
        .iter()
        .find(|row| row["key"] == key)
        .unwrap()
}

#[test]
fn qualified_moving_bridges_reach_real_heat_objectives_native_portable_and_atomic_refusals() {
    let bytes = model_bytes_with_rig(true);
    let digest = sha256_hex(&bytes);
    let model =
        ftx_body::AnnyModel::from_data(ftx_body::AnnyData::from_bytes(&bytes, true).unwrap())
            .unwrap();
    let body = json!({"schema":"ftx-scenario-body/1","accepted_body_fit":receipt(&digest),"arm_abduction_deg":35.});
    let selected =
        FittedBody::prepare(&model, &digest, &BodyRequest::read(&body).unwrap()).unwrap();
    let body_ids: Vec<_> = (0..selected.body().collision.1.len()).collect();
    let mut pattern: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/materials/synthetic-scenario-pattern.json"
    ))
    .unwrap();
    let width = std::f64::consts::TAU * 180.;
    pattern["pieces"][0]["vertices"]["b"][0] = json!(width);
    pattern["pieces"][0]["vertices"]["c"][0] = json!(width);
    pattern["pieces"][0]["placement"]["radius_mm"] = json!(180.);
    pattern["pieces"][0]["placement"]["x0"] = json!(width / 2.);
    let mut a = pattern["fabric"]["material"].clone();
    a["record"]["properties"]["specific_heat"] = json!({"value":1000.,"unit":"J/(kg*K)","source":"test",
        "evidence":"estimate","method":"Synthetic source-qualified moving bridge decision"});
    let mut b = a.clone();
    b["record"]["properties"]["thermal_conductivity"]["value"] = json!(0.1);
    let study = json!({"schema":"ftx-material-study/1","variants":[
        {"key":"a","changes":{"schema":"ftx-material-bindings/1","default":a}},
        {"key":"b","changes":{"schema":"ftx-material-bindings/1","default":b}}],
        "cutting":{"copies":1,"gap_mm":1.,"edge_margin_mm":0.,"end_margin_mm":0.,"seam_allowance_mm":0.},
        "comparison":{"basis":"nominal","allow_mixed_protocols":true,"required_consumers":["transient_coverage"],
            "objectives":[{"metric":"transient_coverage_inside_energy_j","direction":"minimize"}]}});
    let mut single: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/materials/synthetic-scenario.json"
    ))
    .unwrap();
    single["options"]["spacing"] = json!(100.);
    single["options"]["sew_steps"] = json!(120);
    single["options"]["drape_steps"] = json!(0);
    let film = single["coverage"]["conditions"]["boundaries"][0].clone();
    let settings = json!({"max_distance_m":0.15,"min_normal_cosine":0.9,
        "separation_tolerance_m":0.00001,"cells_per_layer":1,"interfaces":[]});
    single["coverage"]["body_triangle_ids"] = json!(body_ids);
    single["coverage"]["settings"] = settings.clone();
    single["motion"] = json!({"schema":"ftx-drape-motion/1","keyframes":[{"time_s":0.001,"arm_abduction_deg":35.}],
        "max_step_s":0.0005,"max_trials":8,"max_pairs":100000,"max_sample_vertices":100000,
        "transient_coverage":{"schema":"ftx-motion-coverage-heat/1","body_triangle_ids":body_ids,
            "instance_layers":{"band":"shell"},"settings":settings,"initial_temperature_c":20.,
            "boundaries":[film],"exposed_boundaries":[null],"max_step_s":0.00025,
            "max_cell_steps":100000,"max_geometry_operations":500000}});
    let evaluate = |scenario: &Value| -> Value {
        serde_json::from_str(
            &on_model(
                &model,
                &digest,
                &pattern.to_string(),
                &study.to_string(),
                &scenario.to_string(),
                &body.to_string(),
                |_| {},
            )
            .unwrap(),
        )
        .unwrap()
    };
    let baseline = evaluate(&single);
    assert_eq!(
        baseline["comparison"]["frontier"],
        json!(["a"]),
        "{} / {}",
        baseline["comparison"],
        outcomes(&baseline)
    );
    let edge = covered_edge(&baseline);
    let mut reference: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/materials/synthetic-steady-thermal-bridges.json"
    ))
    .unwrap();
    reference["paths"][0]["instance"] = json!("band");
    reference["paths"][0]["vertices"] = json!(edge);
    let template = reference["paths"][0]["qualifications"][0].clone();
    let mut observations = Vec::new();
    let mut temporal = Vec::new();
    for (i, key) in ["a", "b"].into_iter().enumerate() {
        let source = &keyed(&baseline["simulations"], key)["evaluation"]["result"]["drape"]["steady_thermal_surface"];
        let mut q = template.clone();
        q["id"] = json!(format!("synthetic-material-{key}"));
        q["surface_model_sha256"] = source["model"]["sha256"].clone();
        q["material_sha256"][0] =
            source["model"]["columns"][0]["layers"][0]["expected_sha256"].clone();
        q["layer_area_ratio_ranges"] = json!([[0.01, 100.]]);
        q["length_range_m"] = json!([1e-9, 10.]);
        q["excess_linear_conductance"]["value"] = json!(if i == 0 { 1. } else { 0. });
        let q = serde_json::to_value(
            serde_json::from_value::<ftx_drape::thermal::steady::coverage::bridges::Qualification>(
                q,
            )
            .unwrap(),
        )
        .unwrap();
        temporal.push(json!({"id":format!("synthetic-time-{key}"),"path_id":reference["paths"][0]["id"],
            "steady_qualification_sha256":sha256_hex(&serde_json::to_vec(&q).unwrap()),"time_range_s":[0.,0.01],
            "sample_interval_range_s":[1e-8,0.001],"temperature_range_c":[-10.,50.],"zero_storage":true,
            "source":"test","sources":q["sources"],"evidence":"estimate","allow_estimates":true,
            "method":"Synthetic instantaneous exchange at accepted mechanical endpoints","specimen_id":"synthetic-band-bridge",
            "conditioning":"synthetic dry state","qualification":"Numerical zero-storage law; no empirical seam performance claim"}));
        observations.push(q);
    }
    reference["paths"][0]["qualifications"] = json!(observations);
    single["motion"]["transient_coverage"]["thermal_bridges"] = json!({"schema":"ftx-motion-thermal-bridges/1",
        "model":"qualified_zero_storage_centerline_exchange/1","reference":reference,"qualifications":temporal});
    let mut late = single.clone();
    late["motion"]["transient_coverage"]["thermal_bridges"]["qualifications"][0]["time_range_s"] =
        json!([0., 0.00075]);
    let matrix = json!({"schema":"ftx-drape-scenario-matrix/1","max_evaluations":4,
        "cases":[{"key":"qualified","scenario":single},{"key":"late-domain-refusal","scenario":late}]});
    let dir = setup();
    std::fs::write(dir.join("model.ftxbody"), &bytes).unwrap();
    save(&dir.join("pattern.json"), &pattern);
    save(&dir.join("study.json"), &study);
    save(&dir.join("body-input.json"), &body);
    let capture = std::env::var_os("FTX_MOVING_BRIDGES_REPLAY").map(PathBuf::from);
    if let Some(capture) = &capture {
        std::fs::create_dir_all(capture).unwrap();
        std::fs::write(capture.join("model.ftxbody"), &bytes).unwrap();
        save(&capture.join("pattern.json"), &pattern);
        save(&capture.join("study.json"), &study);
        save(&capture.join("body-input.json"), &body);
    }
    for (name, scenario) in [("single", single), ("matrix", matrix)] {
        let report = evaluate(&scenario);
        assert_eq!(
            report["comparison"]["frontier"],
            json!(["b"]),
            "{} / {}",
            report["comparison"],
            outcomes(&report)
        );
        if name == "single" {
            for key in ["a", "b"] {
                let result = &keyed(&report["simulations"], key)["evaluation"]["result"];
                let old = &keyed(&baseline["simulations"], key)["evaluation"]["result"];
                let heat = &result["motion"]["result"]["transient_coverage"];
                assert_eq!(heat["status"], "available", "{heat}");
                let heat = &heat["result"];
                let unbridged = &old["motion"]["result"]["transient_coverage"]["result"];
                assert_eq!(heat["checkpoint"], unbridged["checkpoint"]);
                assert_eq!(
                    heat["physical_stored_change_j"],
                    unbridged["physical_stored_change_j"]
                );
                assert_eq!(
                    heat["thermal_bridges"]["baseline_inside_energy_j"],
                    unbridged["inside_energy_j"]
                );
                assert_eq!(
                    heat["inside_energy_j"],
                    heat["thermal_bridges"]["total_inside_energy_j"]
                );
                assert_eq!(
                    keyed(&report["comparison"]["alternatives"], key)["readings"]["transient_coverage_inside_energy_j"]
                        ["value"],
                    heat["inside_energy_j"]
                );
                assert_eq!(result["drape"], old["drape"]);
            }
            rejects_inconsistent_bridge_history(&pattern, &study, &report);
        } else {
            let late = &keyed(&report["cases"], "late-domain-refusal");
            let refused = &keyed(&late["simulations"], "a")["evaluation"]["result"];
            assert_eq!(refused["motion"]["status"], "available");
            assert_eq!(
                refused["motion"]["result"]["transient_coverage"]["status"],
                "refused"
            );
            assert!(
                refused["motion"]["result"]["transient_coverage"]
                    .get("result")
                    .is_none()
            );
            assert_eq!(
                keyed(&report["manufacturing"]["alternatives"], "a")["cutting"]["status"],
                "available"
            );
        }
        save(&dir.join("scenario.json"), &scenario);
        for (binary, prefix) in [
            (
                env!("CARGO_BIN_EXE_ftx"),
                &["--json", "materials", "scenario-study"][..],
            ),
            (env!("CARGO_BIN_EXE_ftx-materials"), &["scenario-study"][..]),
        ] {
            let output = Command::new(binary)
                .current_dir(&dir)
                .args(prefix)
                .args([
                    "pattern.json",
                    "study.json",
                    "scenario.json",
                    "body-input.json",
                    "model.ftxbody",
                ])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let native: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(native.get("result").unwrap_or(&native), &report);
        }
        if let Some(capture) = &capture {
            save(&capture.join(format!("{name}-scenario.json")), &scenario);
            save(&capture.join(format!("{name}-report.json")), &report);
        }
    }
}

fn covered_edge(baseline: &Value) -> [usize; 2] {
    use ftx_drape::thermal::surface::{Surface, coverage::motion::Frame, projection};
    let result = &keyed(&baseline["simulations"], "a")["evaluation"]["result"];
    let steady = &result["drape"]["steady_thermal_surface"];
    let positions: Vec<Vec3> = serde_json::from_value(steady["positions_m"].clone()).unwrap();
    let checkpoint = &result["motion"]["result"]["transient_coverage"]["result"]["checkpoint"];
    let source = Surface::from_snapshot(&checkpoint["frame"]["source"]).unwrap();
    let triangles: Vec<[usize; 3]> = serde_json::from_value(
        checkpoint["frame"]["projection"]["layout"]["body_triangles"].clone(),
    )
    .unwrap();
    let mapping: projection::Request =
        serde_json::from_value(checkpoint["frame"]["request"].clone()).unwrap();
    let frames: Vec<_> = baseline["simulations"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|row| {
            row["evaluation"]["result"]["motion"]["result"]["samples"]
                .as_array()
                .unwrap()
        })
        .map(|sample| {
            Frame::new(
                source.clone(),
                serde_json::from_value(sample["positions_m"].clone()).unwrap(),
                serde_json::from_value(sample["body_positions_m"].clone()).unwrap(),
                &triangles,
                mapping.clone(),
            )
            .unwrap()
        })
        .collect();
    let patches: Vec<[usize; 3]> = steady["model"]["patches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| serde_json::from_value(p["vertices"].clone()).unwrap())
        .collect();
    let contains = |polygon: &[Vec2], point: Vec2| {
        polygon.iter().enumerate().all(|(i, &a)| {
            (polygon[(i + 1) % polygon.len()] - a).cross(point - a) >= -512. * f64::EPSILON
        })
    };
    let axial_error = |edge: &[usize; 2]| {
        let d = positions[edge[1]] - positions[edge[0]];
        (d.x * d.x + d.y * d.y) / (d.z * d.z)
    };
    // Test fixture selection only: require both endpoints in one canonical convex footprint at
    // every real accepted pose. The production bridge still independently owns/admit its path.
    patches
        .iter()
        .flat_map(|v| [[v[0], v[1]], [v[1], v[2]], [v[2], v[0]]])
        .filter(|e| (positions[e[1]].z - positions[e[0]].z).abs() > 1e-6)
        .filter(|edge| {
            frames.iter().all(|frame| {
                frame.projection().footprints.iter().any(|footprint| {
                    footprint.hits.iter().any(|hit| {
                        let vertices = patches[hit.cloth_triangle];
                        let local = |v| {
                            vertices
                                .iter()
                                .position(|&i| i == v)
                                .map(|i| [Vec2::ZERO, Vec2::new(1., 0.), Vec2::new(0., 1.)][i])
                        };
                        match (local(edge[0]), local(edge[1])) {
                            (Some(a), Some(b)) => {
                                contains(&hit.cloth_polygon, a) && contains(&hit.cloth_polygon, b)
                            }
                            _ => false,
                        }
                    })
                })
            })
        })
        .min_by(|a, b| axial_error(a).total_cmp(&axial_error(b)))
        .expect(
            "synthetic band needs an actual fully covered retained edge at every accepted sample",
        )
}

fn outcomes(report: &Value) -> Value {
    json!(report["simulations"].as_array().map(|rows| rows.iter().map(|row| {
        let motion=&row["evaluation"]["result"]["motion"];
        let heat=&motion["result"]["transient_coverage"];
        json!({"key":row["key"],"evaluation":row["evaluation"]["status"],"motion":motion["status"],
            "motion_refusal":motion["refusal"],"heat":heat["status"],"heat_refusal":heat["refusal"]})
    }).collect::<Vec<_>>()))
}

fn rejects_inconsistent_bridge_history(pattern: &Value, study: &Value, original: &Value) {
    let pattern = Pattern::from_json(&pattern.to_string()).unwrap();
    let request: ftx_export::material_study::Request =
        serde_json::from_value(study.clone()).unwrap();
    for fault in [
        "missing",
        "contribution",
        "duration",
        "source",
        "checkpoint",
        "body",
        "stored",
        "cumulative",
        "edge-partition",
        "temperature",
    ] {
        let result=request.evaluate_with(&pattern,&original["scenario"],|key,_| {
            let mut simulated=keyed(&original["simulations"],key)["evaluation"]["result"].clone();
            if fault=="source" {
                simulated["drape"]["steady_thermal_surface"]["model"]["patches"][0]["reference_area_m2"]=json!(9.);
            } else {
                let heat=&mut simulated["motion"]["result"]["transient_coverage"]["result"];
                match fault {
                    "missing"=>heat["trace"][0]["thermal_bridges"]=Value::Null,
                    "contribution"=>heat["trace"][0]["thermal_bridges"]["interval"]["paths"][0]["contributions"][0]["power_w"]=json!(999.),
                    "duration"=>heat["trace"][0]["thermal_bridges"]["interval"]["time_range_s"][0]=json!(0.0001),
                    "checkpoint"=>heat["checkpoint"]["temperature_c"][0][0]=json!(21.),
                    "body"=>heat["final_field"]["body"][0]["bridge_power_w"]=json!(999.),
                    "stored"=>heat["physical_stored_change_j"]=json!(999.),
                    "cumulative"=>heat["thermal_bridges"]["bridge_energy_j"]=json!(999.),
                    "edge-partition"=>heat["trace"][0]["thermal_bridges"]["interval"]["paths"][0]["contributions"][0]["edge_fraction"][0]=json!(0.5),
                    "temperature"=>{
                        for domain in heat["trace"][0]["thermal_bridges"]["interval"]["region_domains"].as_array_mut().unwrap() {
                            domain["temperature_range_c"]=json!([-10.,99.]);
                        }
                    },
                    _=>unreachable!(),
                }
            }
            Ok(simulated)
        }).unwrap();
        let result = serde_json::to_value(result).unwrap();
        assert_eq!(
            result["comparison"]["frontier"],
            json!([]),
            "{fault}: {result}"
        );
        for candidate in result["comparison"]["alternatives"].as_array().unwrap() {
            assert!(
                !candidate["readings"]["transient_coverage_inside_energy_j"].is_object(),
                "{fault}: {candidate}"
            );
        }
    }
}

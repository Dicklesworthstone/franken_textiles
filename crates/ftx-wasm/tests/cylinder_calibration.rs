//! Browser adapter integration against the real 3D cylindrical-holder forward and inverse solvers;
//! no browser or solver doubles. Synthetic acquisitions verify numerical and wire plumbing, never
//! measured textile characterization or garment fit.

use std::collections::BTreeMap;

use ftx_core::{RefusalCode, receipt::sha256_hex};
use ftx_drape::cylinder::{
    self, Controls, Fixture, REQUEST_SCHEMA, Request, Waypoint,
    fit::{
        Channel, Experiment, INPUT_SCHEMA, Input, Observation, Policy, SensitivityMode, TargetKind,
        TargetParameter,
    },
};
use ftx_pattern::{
    material_calibration::Partition,
    material_catalog::Selection,
    materials::{Evidence, Material},
    textile_import,
};
use ftx_wasm::materials;
use serde_json::{Value, json};

fn base_material() -> Material {
    serde_json::from_value(json!({
        "id": "synthetic/wasm-cylinder-coupon",
        "name": "Synthetic wasm cylinder coupon",
        "form": "woven",
        "sources": {
            "spec": {
                "reference": "synthetic:wasm-cylinder-spec",
                "accessed_on": "2026-11-21",
                "note": "Synthetic baseline mass, weft/shear/Poisson and presser-foot thickness"
            },
            "fit_acq": {
                "reference": "synthetic:wasm-cylinder-fit",
                "accessed_on": "2026-11-21",
                "note": "Synthetic static FIT cylindrical-holder acquisitions"
            },
            "check_acq": {
                "reference": "synthetic:wasm-cylinder-check",
                "accessed_on": "2026-11-21",
                "note": "Independent synthetic static CHECK cylindrical-holder acquisition"
            }
        },
        "properties": {
            "areal_density": {
                "value": 0.21,
                "unit": "kg/m2",
                "evidence": "measured",
                "source": "spec",
                "method": "synthetic-coupon-mass"
            },
            "weft_stiffness": {
                "value": 620.0,
                "unit": "N/m",
                "evidence": "measured",
                "source": "spec",
                "method": "synthetic-weft-baseline"
            },
            "poisson_warp_weft": {
                "value": 0.25,
                "unit": "1",
                "evidence": "measured",
                "source": "spec",
                "method": "synthetic-biaxial-baseline"
            },
            "shear_stiffness": {
                "value": 180.0,
                "unit": "N/m",
                "evidence": "measured",
                "source": "spec",
                "method": "synthetic-shear-baseline"
            },
            "reported_thickness": {
                "value": 0.00055,
                "unit": "m",
                "evidence": "measured",
                "source": "spec",
                "method": "synthetic-presser-foot-gauge",
                "conditions": {"pressure_pa": "presser_foot_unspecified"}
            }
        }
    }))
    .unwrap()
}

fn truth_material(warp_n_m: f64, bending_nm: f64) -> Material {
    let mut mat = base_material();
    mat.id = "synthetic/wasm-cylinder-truth".into();
    mat.properties.insert(
        "warp_stiffness".into(),
        serde_json::from_value(json!({
            "value": warp_n_m,
            "unit": "N/m",
            "evidence": "measured",
            "source": "spec",
            "method": "synthetic-truth"
        }))
        .unwrap(),
    );
    mat.properties.insert(
        "bending_rigidity".into(),
        serde_json::from_value(json!({
            "value": bending_nm,
            "unit": "N*m",
            "evidence": "measured",
            "source": "spec",
            "method": "synthetic-truth"
        }))
        .unwrap(),
    );
    mat
}

fn small_fixture(warp_axis: [f64; 2]) -> Fixture {
    Fixture {
        rotating_holder: Default::default(),
        holder_radius_m: 0.040,
        specimen_width_m: 0.260,
        specimen_height_m: 0.110,
        nominal_free_height_m: 0.050,
        pretension_displacement_m: 0.0005,
        tare_force_n: 0.12,
        tare_torque_nm: -0.002,
        warp_axis,
        cells: [12, 4],
        self_contact: false,
        grip_slip_force_limit_n: None,
        refuse_unmodeled_slip: false,
    }
}

fn build_static_calibration_input() -> (Input, Request) {
    let truth = truth_material(900.0, 2.5e-3);
    let controls = Controls {
        static_tolerance_m: 1e-10,
        velocity_tolerance_m_s: 1e-9,
        ..Controls::default()
    };
    let make_exp = |id: &str, axial_m: f64, rot_deg: f64, partition: Partition| -> Experiment {
        let fixture = small_fixture([0.0, 1.0]);
        let mut refined = fixture.clone();
        refined.cells = refined.cells.map(|n| n * 2);
        let loading = vec![Waypoint {
            time_s: 0.0,
            axial_displacement_m: axial_m,
            rotation_deg: rot_deg,
            clamp_slip_m: 0.0,
        }];
        let states = cylinder::evaluate(&Request {
            schema: REQUEST_SCHEMA.into(),
            material: Selection {
                id: truth.id.clone(),
                record: Some(truth.clone()),
                ..Selection::default()
            },
            fixture: refined,
            loading: loading.clone(),
            nominal_protocol: None,
            controls: controls.clone(),
        })
        .unwrap()
        .states;
        let s = &states[0];
        Experiment {
            id: id.into(),
            fixture,
            loading,
            nominal_protocol: None,
            source: if partition == Partition::Fit {
                "fit_acq"
            } else {
                "check_acq"
            }
            .into(),
            evidence: Evidence::Measured,
            partition,
            observations: vec![Observation {
                id: format!("{id}-obs"),
                waypoint_index: 0,
                force_n: Some(Channel {
                    value: s.tare_adjusted_force_n,
                    scale: 1.0,
                }),
                torque_nm: Some(Channel {
                    value: s.tare_adjusted_torque_nm,
                    scale: 0.03,
                }),
                mean_mid_radius_m: Some(Channel {
                    value: s.mid_radius_range_m[2],
                    scale: 1e-3,
                }),
            }],
        }
    };

    let forward_request = Request {
        schema: REQUEST_SCHEMA.into(),
        material: Selection {
            id: truth.id.clone(),
            record: Some(truth.clone()),
            ..Selection::default()
        },
        fixture: small_fixture([0.0, 1.0]),
        loading: vec![Waypoint {
            time_s: 0.0,
            axial_displacement_m: 0.0020,
            rotation_deg: 6.0,
            clamp_slip_m: 0.0,
        }],
        nominal_protocol: None,
        controls: controls.clone(),
    };

    let input = Input {
        schema: INPUT_SCHEMA.into(),
        material: base_material(),
        method: "Synthetic wasm static 2-target cylindrical-holder adjoint calibration".into(),
        conditions: BTreeMap::from([("temperature_c".into(), "21".into())]),
        fitted_on: "2026-11-21".into(),
        targets: vec![
            TargetParameter {
                parameter: TargetKind::WarpStiffness,
                bounds: [500.0, 1400.0],
                branch_id: None,
            },
            TargetParameter {
                parameter: TargetKind::BendingRigidity,
                bounds: [1.0e-3, 5.0e-3],
                branch_id: None,
            },
        ],
        experiments: vec![
            make_exp("fit-warp-ext", 0.0020, 6.0, Partition::Fit),
            make_exp("fit-warp-comp", -0.0010, -4.0, Partition::Fit),
            make_exp("check-warp", 0.0006, 2.0, Partition::Check),
        ],
        admit_cylindrical_shell_model: true,
        allow_estimated_observations: false,
        replace_existing: false,
        allow_estimates: false,
        controls,
        policy: Policy {
            max_rms_scaled: 0.35,
            max_peak_scaled: 0.6,
            max_mesh_scaled: 0.6,
            max_time_scaled: None,
            min_excitation_pivot: 1e-4,
            max_iterations: 40,
            max_forward_solves: 2048,
            sensitivity: SensitivityMode::Adjoint,
        },
    };
    (input, forward_request)
}

#[test]
fn portable_cylinder_forward_and_adjoint_calibration_match_shared_engine() {
    let (input, forward_request) = build_static_calibration_input();

    // 1. Browser forward cylindrical-holder adapter matches native engine byte-for-byte.
    let raw_forward_req = serde_json::to_string(&forward_request).unwrap();
    let expected_forward =
        serde_json::to_string(&cylinder::evaluate(&forward_request).unwrap()).unwrap();
    let wasm_forward = materials::cylinder_experiment(&raw_forward_req).unwrap();
    assert_eq!(wasm_forward, expected_forward);

    // 2. Browser opt-in adjoint inverse matches native engine and freezes the selection without
    //    fabricating unloaded thickness or friction_skin.
    let raw_input = serde_json::to_string(&input).unwrap();
    let mut expected_input = input.clone();
    expected_input.allow_estimates = true;
    let expected_report = cylinder::fit::calibrate(&expected_input).unwrap();
    let response: Value =
        serde_json::from_str(&materials::calibrate_cylinder(&raw_input, true).unwrap()).unwrap();
    let calibration = &response["calibration"];
    assert_eq!(
        calibration,
        &serde_json::to_value(&expected_report).unwrap()
    );
    assert_eq!(calibration["accepted"], true, "{response}");
    assert_eq!(calibration["domain_replay_verified"], true);
    assert!(
        calibration["adjoint_solves"].as_u64().unwrap() > 0,
        "adjoint sensitivity must record positive adjoint_solves: {calibration}"
    );
    assert_eq!(response["fit_claim"], false);
    assert_eq!(response["empirical_validation_claim"], false);
    assert_eq!(response["simulation_ready"], false);
    assert!(response["mechanics_refusal"].is_object());

    let expected_hash = sha256_hex(&serde_json::to_vec(&expected_input).unwrap());
    assert_eq!(calibration["input_sha256"], expected_hash);

    let selection_json = response["selection_json"].as_str().unwrap();
    assert_eq!(
        selection_json,
        expected_report.selection_json.as_deref().unwrap()
    );
    let selection: Selection = serde_json::from_str(selection_json).unwrap();
    assert!(selection.allow_estimates);
    let record = selection.record.as_ref().unwrap();
    assert!(record.properties.contains_key("reported_thickness"));
    assert!(
        !record.properties.contains_key("thickness"),
        "presser-foot reported_thickness must never be promoted to unloaded contact thickness"
    );
    for key in ["warp_stiffness", "bending_rigidity"] {
        assert_eq!(
            serde_json::to_value(record.properties[key].evidence).unwrap(),
            "estimate"
        );
    }
    let detail: Value = serde_json::from_str(&materials::inspect(selection_json).unwrap()).unwrap();
    assert_eq!(detail, response["material"]);

    // 3. Replaying the published CHECK experiment through `materials::cylinder_experiment` with
    //    the frozen selection matches the calibration's CHECK state exactly.
    let check_exp = expected_report
        .input
        .experiments
        .iter()
        .find(|e| e.partition == Partition::Check)
        .unwrap();
    let mut check_fixture = check_exp.fixture.clone();
    check_fixture.cells = check_fixture.cells.map(|n| n * 2);
    let replay_req = Request {
        schema: REQUEST_SCHEMA.into(),
        material: selection,
        fixture: check_fixture,
        loading: check_exp.loading.clone(),
        nominal_protocol: None,
        controls: expected_report.input.controls.clone(),
    };
    let replay: Value = serde_json::from_str(
        &materials::cylinder_experiment(&serde_json::to_string(&replay_req).unwrap()).unwrap(),
    )
    .unwrap();
    let check_pred = expected_report
        .predictions
        .iter()
        .find(|p| p.experiment == check_exp.id)
        .unwrap();
    assert_eq!(
        replay["states"][0],
        serde_json::to_value(&check_pred.state).unwrap()
    );

    // 4. `allow_estimates = false` keeps the returned selection gated against downstream mechanics.
    let gated: Value =
        serde_json::from_str(&materials::calibrate_cylinder(&raw_input, false).unwrap()).unwrap();
    assert_eq!(gated["calibration"]["accepted"], true);
    assert_eq!(gated["selection"]["allow_estimates"], false);
    assert_eq!(gated["simulation_ready"], false);
    let gated_sel: Selection =
        serde_json::from_str(gated["selection_json"].as_str().unwrap()).unwrap();
    let resolved = textile_import::catalog()
        .unwrap()
        .resolve(&gated_sel)
        .unwrap();
    assert!(resolved.mechanics_for_contacts(false).is_err());
}

#[test]
fn cylinder_api_keeps_rejected_report_and_refuses_invalid_adjoint_scope() {
    let (input, _) = build_static_calibration_input();

    // 1. Unconverged (max_iterations = 1): accepted == false, no selection published.
    let mut stalled = serde_json::to_value(&input).unwrap();
    stalled["policy"]["max_iterations"] = json!(1);
    let response: Value =
        serde_json::from_str(&materials::calibrate_cylinder(&stalled.to_string(), true).unwrap())
            .unwrap();
    assert_eq!(response["calibration"]["accepted"], false, "{response}");
    assert!(
        response["calibration"]["reasons"]
            .as_array()
            .is_some_and(|r| !r.is_empty())
    );
    for field in ["selection", "selection_json", "material"] {
        assert!(response[field].is_null(), "{field}: {response}");
    }
    assert_eq!(response["simulation_ready"], false);
    assert_eq!(response["fit_claim"], false);
    assert_eq!(response["empirical_validation_claim"], false);

    // 2. Forward solve budget exhaustion refuses fail-closed with BudgetExceeded.
    let mut tiny_budget = serde_json::to_value(&input).unwrap();
    tiny_budget["policy"]["max_forward_solves"] = json!(1);
    let err = materials::calibrate_cylinder(&tiny_budget.to_string(), true).unwrap_err();
    assert_eq!(err.code, RefusalCode::BudgetExceeded);

    // 3. Missing explicit cylindrical-shell model consent refuses fail-closed.
    let mut no_consent = serde_json::to_value(&input).unwrap();
    no_consent["admit_cylindrical_shell_model"] = json!(false);
    let err = materials::calibrate_cylinder(&no_consent.to_string(), true).unwrap_err();
    assert_eq!(err.code, RefusalCode::InvalidDocument);

    // 4. Adjoint mode refuses multi-waypoint dynamic FIT trajectories fail-closed.
    let mut dynamic = serde_json::to_value(&input).unwrap();
    dynamic["experiments"][0]["loading"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "time_s": 0.25,
            "axial_displacement_m": 0.0025,
            "rotation_deg": 7.0,
            "clamp_slip_m": 0.0
        }));
    let err = materials::calibrate_cylinder(&dynamic.to_string(), true).unwrap_err();
    assert_eq!(err.code, RefusalCode::InvalidDocument);
    assert!(
        err.message.contains("static single-waypoint"),
        "unexpected refusal message: {err}"
    );
}

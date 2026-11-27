//! Explicit synthetic knit records through the ordinary sewn-garment and motion paths.
//! These tests establish software binding and transactional behavior, not physical calibration.
use ftx_core::{RefusalCode, Vec3};
use ftx_drape::material::resolve;
use ftx_drape::motion::{MotionOptions, advance_body};
use ftx_drape::run::{DrapeBody, DrapeOptions, DrapeRun, drape};
use ftx_drape::scene::PartFrames;
use ftx_drape::{Body, PartFrame, TimeIntegrator};
use ftx_pattern::Pattern;
use ftx_pattern::material_bending::NORMAL_MODEL;
use ftx_pattern::material_catalog::Selection;
use ftx_pattern::materials::Form;
use serde_json::{Value, json};

fn selection() -> Selection {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-coupled-knit.json"
    ))
    .unwrap()
}

fn lining() -> Selection {
    let mut selected = selection();
    selected.id = "synthetic-woven-lining".into();
    let record = selected.record.as_mut().unwrap();
    record.id = selected.id.clone();
    record.name = "Synthetic woven lining with ordinary tensor bending".into();
    record.form = Form::Woven;
    record.knit_response = None;
    record
        .structure
        .insert("bending_model".into(), NORMAL_MODEL.into());
    let template = record.properties["bending_rigidity"].clone();
    for (key, value, unit) in [
        ("warp_stiffness", 1000.0, "N/m"),
        ("weft_stiffness", 500.0, "N/m"),
        ("poisson_warp_weft", 0.1, "1"),
        ("shear_stiffness", 200.0, "N/m"),
        ("bending_warp", 0.00004, "N*m"),
        ("bending_weft", 0.00002, "N*m"),
        ("bending_coupling", 0.000004, "N*m"),
        ("bending_twist", 0.00001, "N*m"),
    ] {
        let mut quantity = template.clone();
        quantity.value = value;
        quantity.unit = unit.into();
        quantity.method = "Explicit synthetic lining coefficient, not measured data".into();
        record.properties.insert(key.into(), quantity);
    }
    selected
}

struct SyntheticBody;

impl PartFrames for SyntheticBody {
    fn frame(&self, _: &str) -> Option<PartFrame> {
        Some(PartFrame {
            origin: Vec3::ZERO,
            up: Vec3::new(0.0, 0.0, 1.0),
            front: Vec3::new(0.0, -1.0, 0.0),
            lat: Vec3::new(1.0, 0.0, 0.0),
        })
    }

    fn landmark_h(&self, _: &str, _: &str) -> Option<f64> {
        Some(40.0)
    }

    fn section_radius(&self, _: &str, _: f64) -> f64 {
        30.0
    }
}

fn pattern() -> Pattern {
    let width = 40.0 * std::f64::consts::PI;
    let mut pieces: Vec<Value> = ["front", "back"]
        .into_iter()
        .enumerate()
        .map(|(i, id)| {
            json!({
                "id": id, "cut": {"qty": 1},
                "vertices": {"a":[0,0], "b":[width,0], "c":[width,40], "d":[0,40]},
                "edges": [
                    {"id":"bottom", "from":"a", "to":"b"},
                    {"id":"right", "from":"b", "to":"c"},
                    {"id":"top", "from":"c", "to":"d"},
                    {"id":"left", "from":"d", "to":"a"}
                ],
                "grain": [[0,0],[0,40]],
                "placement": {
                    "part":"torso", "radius_mm":40, "x0":width/2.0, "theta_deg":180*i
                }
            })
        })
        .collect();
    pieces[1]["fabric"] = json!({"material":lining()});
    Pattern::new(json!({
        "schema":"fabric-pattern/1", "units":"mm",
        "fabric":{"material":selection()},
        "pieces":pieces,
        "seams":[
            {"id":"join1","a":"front/right","b":"back/left"},
            {"id":"join2","a":"back/right","b":"front/left"}
        ]
    }))
}

fn sewn_run() -> DrapeRun {
    let surface = Body::new(
        vec![
            Vec3::new(-1.0, -1.0, -1.0),
            Vec3::new(1.0, -1.0, -1.0),
            Vec3::new(0.0, 1.0, -1.0),
        ],
        vec![[0, 1, 2]],
        0.1,
    )
    .unwrap();
    let body = DrapeBody {
        frames: &SyntheticBody,
        collision: (&surface.vertices, &surface.triangles),
        display: (&surface.vertices, &surface.triangles),
        arm_deg: 35.0,
        arm_pose: None,
    };
    drape(
        &pattern(),
        &json!({}),
        &body,
        &DrapeOptions {
            spacing: 12.0,
            sew_steps: 3,
            drape_steps: 3,
            step: 0.0001,
            ..DrapeOptions::default()
        },
        |_| {},
    )
    .unwrap()
}

#[test]
fn ordinary_sewn_scene_preserves_knit_and_woven_triangle_and_patch_ownership() {
    let run = sewn_run();
    let model = &run.scene.solver.model;
    assert_eq!(run.report["material"]["coupled_knit_piece_count"], 1);
    assert_eq!(
        run.report["material"]["asymmetric_knit_bending_piece_count"],
        1
    );
    assert_eq!(run.report["material"]["knit_hysteresis_piece_count"], 1);
    assert_eq!(run.report["material"]["fit_claim"], false);
    let is_knit =
        |triangle: usize| run.scene.mesh.instances[run.scene.triangle_owner[triangle]] == "front";
    let mut knit_count = 0;
    let mut woven_count = 0;
    for triangle in 0..model.membranes.len() {
        assert_eq!(
            model.coupled_knit_membrane(triangle).is_some(),
            is_knit(triangle)
        );
        if is_knit(triangle) {
            knit_count += 1;
        } else {
            woven_count += 1;
        }
    }
    assert!(knit_count > 0 && woven_count > 0);
    assert_eq!(model.panel_bending_patches().len(), model.membranes.len());
    for (patch, &triangle) in model.panel_bending_triangles().iter().enumerate() {
        assert_eq!(model.panel_knit_bending(patch).is_some(), is_knit(triangle));
    }
    assert!(
        model
            .hinges
            .iter()
            .enumerate()
            .any(|(i, _)| model.hinge_rigidity(i) > 0.0),
        "separate scalar seam hinges must remain active"
    );
    let history = run.scene.solver.knit_hysteresis_report().unwrap().unwrap();
    assert_eq!(history["accepted_steps"], 6);
    let elements = history["elements"].as_array().unwrap();
    assert_eq!(elements.len(), knit_count);
    for entry in elements {
        assert!(is_knit(entry["triangle"].as_u64().unwrap() as usize));
    }

    // Exercise the ordinary report consumer on this actual mixed-material sewn state.
    // Missing optical observations must remain unavailable, and model indicators must not
    // become claims of physical knitting, a complete assessment, machine permission or fit.
    let selected: Vec<_> = (0..model.membranes.len()).filter(|&i| is_knit(i)).collect();
    let knit_report = &run.report["knit_shell"];
    assert_eq!(knit_report["material_triangles"], json!(selected));
    assert_eq!(knit_report["asymmetric_bending_patches"], knit_count);
    assert_eq!(knit_report["hysteresis"], history);
    assert_eq!(
        knit_report["unavailable_assessments"],
        json!(["rib_transition_snap", "pore_grin_through"])
    );
    for claim in [
        "complete_knit_assessment",
        "hardware_execution_authorized",
        "physical_knitting_validated",
        "fit_claimed",
    ] {
        assert_eq!(knit_report[claim], false);
    }
    assert_eq!(run.report["panel_bending"]["model"], "per_patch");
    assert_eq!(
        run.report["panel_bending"]["models"],
        json!(["asymmetric_knit", NORMAL_MODEL])
    );
    let minimum_margin = selected.iter().fold(1.0_f64, |margin, &i| {
        let strain = model.membranes[i].strain(&run.scene.solver.x);
        let law = model.coupled_knit_membrane(i).unwrap();
        margin.min(
            1.0 - (strain[0] / law.e1_jam).powi(2)
                - (strain[1] / law.e2_jam).powi(2)
                - (strain[2] / law.gamma_jam).powi(2),
        )
    });
    assert!(
        (knit_report["minimum_physical_jamming_margin"]
            .as_f64()
            .unwrap()
            - minimum_margin)
            .abs()
            < 1e-12
    );
    let curvature_indicator = knit_report["maximum_curvature_indicator_rad"]
        .as_f64()
        .unwrap();
    assert!(curvature_indicator.is_finite() && curvature_indicator >= 0.0);
    let flags = knit_report["flags"].as_array().unwrap();
    for (flag, active) in [
        ("knit_jamming", minimum_margin < 0.05),
        ("knit_edge_curl", curvature_indicator > 0.35),
        (
            "knit_hysteresis_drift",
            history["max_backstrain"].as_f64().unwrap() > 0.02,
        ),
    ] {
        assert_eq!(flags.contains(&json!(flag)), active);
        if active {
            assert!(
                run.report["flags"]
                    .as_array()
                    .unwrap()
                    .contains(&json!(flag))
            );
            assert!(
                run.report["gating"]
                    .as_array()
                    .unwrap()
                    .contains(&json!(flag))
            );
        }
    }
    assert!(flags.iter().all(|flag| {
        matches!(
            flag.as_str(),
            Some("knit_jamming" | "knit_edge_curl" | "knit_hysteresis_drift")
        )
    }));

    // Removing only the actually bound coupled field changes nonlinear energy while preserving
    // the admitted origin tensor, mass, topology and both bending fields.
    let stretched: Vec<_> = run.scene.solver.x.iter().map(|p| *p * 1.1).collect();
    model.check_membrane_domains(&stretched).unwrap();
    let mut linear = model.clone();
    linear
        .set_coupled_knit_membranes(&vec![None; model.membranes.len()])
        .unwrap();
    assert_eq!(linear.mass, model.mass);
    assert!(model.elastic_energy(&stretched) > linear.elastic_energy(&stretched) + 1e-8);
    let jammed: Vec<_> = run.scene.solver.x.iter().map(|p| *p * 4.0).collect();
    assert_eq!(
        model.check_membrane_domains(&jammed).unwrap_err().code,
        RefusalCode::OutOfRange
    );
}

#[test]
fn ordinary_knit_history_continues_through_motion_and_rejected_requests_are_atomic() {
    let mut solver = sewn_run().scene.solver;
    let initial = solver.knit_hysteresis_report().unwrap().unwrap();
    let target = solver.contact.as_ref().unwrap().body.vertices.clone();
    let options = MotionOptions {
        max_step_s: 0.0001,
        max_trials: 16,
        ..MotionOptions::default()
    };
    let moved = advance_body(&mut solver, &target, 0.0002, options).unwrap();
    let accepted = solver.knit_hysteresis_report().unwrap().unwrap();
    assert_eq!(
        accepted["accepted_steps"].as_u64().unwrap(),
        initial["accepted_steps"].as_u64().unwrap() + moved.samples.len() as u64
    );
    assert!(
        accepted["dissipated_work_j"].as_f64().unwrap()
            >= initial["dissipated_work_j"].as_f64().unwrap()
    );

    let max_newton = solver.max_newton;
    solver.max_newton = 0;
    let before = format!("{solver:?}");
    assert!(
        advance_body(
            &mut solver,
            &target,
            0.0001,
            MotionOptions {
                max_trials: 1,
                ..options
            },
        )
        .is_err()
    );
    assert_eq!(format!("{solver:?}"), before);
    assert_eq!(solver.knit_hysteresis_report().unwrap().unwrap(), accepted);
    solver.max_newton = max_newton;
    let mut replay = solver.clone();
    advance_body(&mut solver, &target, 0.0001, options).unwrap();
    advance_body(&mut replay, &target, 0.0001, options).unwrap();
    assert_eq!(solver.x, replay.x);
    assert_eq!(solver.v, replay.v);
    assert_eq!(
        solver.knit_hysteresis_report().unwrap(),
        replay.knit_hysteresis_report().unwrap()
    );
    assert!(solver.static_solve().is_err());

    solver
        .model
        .set_coupled_knit_membranes(&vec![None; solver.model.membranes.len()])
        .unwrap();
    assert!(solver.knit_hysteresis_report().is_err());
    assert!(solver.step(0.0001).is_err());
}

#[test]
fn shared_material_admission_requires_evidence_and_refuses_unsupported_knit_options() {
    let selected = selection();
    let input = |selected: Selection| Pattern::new(json!({"fabric":{"material":selected}}));
    assert!(resolve(&input(selected.clone()), &DrapeOptions::default()).is_ok());
    let mut no_consent = selected.clone();
    no_consent.allow_estimates = false;
    assert!(resolve(&input(no_consent), &DrapeOptions::default()).is_err());
    let mut no_thickness = selected.clone();
    no_thickness
        .record
        .as_mut()
        .unwrap()
        .properties
        .remove("thickness");
    assert!(resolve(&input(no_thickness), &DrapeOptions::default()).is_err());

    let mut no_explicit_model = selected.clone();
    no_explicit_model.record.as_mut().unwrap().knit_response = None;
    assert!(resolve(&input(no_explicit_model), &DrapeOptions::default()).is_err());
    let generalized_alpha = DrapeOptions {
        integrator: TimeIntegrator::GeneralizedAlpha { rho_inf: 0.8 },
        ..DrapeOptions::default()
    };
    assert!(resolve(&input(selected.clone()), &generalized_alpha).is_err());
    let mut no_history = selected.clone();
    no_history
        .record
        .as_mut()
        .unwrap()
        .knit_response
        .as_mut()
        .unwrap()
        .hysteresis = None;
    assert!(resolve(&input(no_history), &generalized_alpha).is_ok());

    let mut damping = DrapeOptions::default();
    damping.damping.rayleigh_bending_s = 0.01;
    assert!(resolve(&input(selected.clone()), &damping).is_err());
    let shells = DrapeOptions {
        bending_model: ftx_drape::BendingModel::DiscreteShells,
        ..DrapeOptions::default()
    };
    assert!(resolve(&input(selected), &shells).is_err());
}

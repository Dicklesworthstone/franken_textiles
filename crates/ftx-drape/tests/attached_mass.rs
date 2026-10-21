//! Synthetic point-load mechanics through real source materials, meshing, seam welding and motion.
//! These numerical checks do not qualify a real accessory, its attachment strength or garment fit.
use ftx_core::{Refusal, Vec3, det};
use ftx_drape::motion::{MotionOptions, advance_body};
use ftx_drape::run::{DrapeBody, DrapeOptions, DrapeRun, drape};
use ftx_drape::scene::PartFrames;
use ftx_drape::thermal::surface::Surface;
use ftx_drape::{Body, PartFrame, Solver};
use ftx_pattern::Pattern;
use serde_json::{Value, json};

fn sheet(density: f64) -> Value {
    let mut properties = serde_json::Map::new();
    for (name, value, unit) in [
        ("areal_density", density, "g/m2"),
        ("thickness", 0.6, "mm"),
        ("warp_stiffness", 100.0, "N/m"),
        ("weft_stiffness", 40.0, "N/m"),
        ("shear_stiffness", 10.0, "N/m"),
        ("poisson_warp_weft", 0.2, "1"),
        ("bending_rigidity", 3e-5, "N*m"),
        ("friction_skin", 0.2, "1"),
        ("friction_self", 0.3, "1"),
        ("thermal_resistance", 0.2, "m2*K/W"),
        ("specific_heat", 1000.0, "J/(kg*K)"),
    ] {
        properties.insert(
            name.into(),
            json!({"value":value,"unit":unit,"source":"test",
            "evidence":"estimate","method":"Synthetic software mechanics and heat input"}),
        );
    }
    json!({"id":"synthetic-sheet","allow_estimates":true,"record":{
        "id":"synthetic-sheet","name":"Synthetic sheet","form":"woven","properties":properties,
        "sources":{"test":{"reference":"test:attached-mass","accessed_on":"2026-10-20","note":"Synthetic numerical input"}}}})
}

fn band() -> Value {
    let w = 40.0 * std::f64::consts::PI;
    let pieces: Vec<_> = ["front", "back"]
        .into_iter()
        .enumerate()
        .map(|(i, id)| {
            json!({
                "id":id,"cut":{"qty":1},"vertices":{"a":[0,0],"b":[w,0],"c":[w,20],"d":[0,20]},
                "edges":[{"id":"bottom","from":"a","to":"b"},{"id":"right","from":"b","to":"c"},
                    {"id":"top","from":"c","to":"d"},{"id":"left","from":"d","to":"a"}],
                "notches":[],"grain":[[0,0],[0,20]],
                "placement":{"part":"torso","radius_mm":40,"x0":w/2.0,"theta_deg":180*i}
            })
        })
        .collect();
    json!({"schema":"fabric-pattern/1","units":"mm","fabric":{"material":sheet(200.)},
        "pieces":pieces,"seams":[{"id":"right_join","a":"front/right","b":"back/left"},
        {"id":"left_join","a":"back/right","b":"front/left"}]})
}

fn extra() -> Value {
    json!({"id":"closure-set","mass":{"value":0.03,"unit":"kg","bounds":[0.028,0.032],
        "evidence":"estimate","source":"test","method":"Synthetic total retained group"},
        "sources":{"test":{"reference":"test:attached-mass","accessed_on":"2026-10-20","note":"Synthetic numerical input"}},
        "allow_estimates":true})
}

fn loaded() -> Value {
    let mut raw = band();
    raw["attached_mass"] = json!({"schema":"ftx-attached-mass/1","groups":[{"extra":extra(),
        "targets":[{"instance":"front","vertex":"b","fraction":0.25},
            {"instance":"back","vertex":"a","fraction":0.75}]}]});
    raw
}

struct Form;
impl PartFrames for Form {
    fn frame(&self, _: &str) -> Option<PartFrame> {
        Some(PartFrame {
            origin: Vec3::ZERO,
            up: Vec3::new(0., 0., 1.),
            front: Vec3::new(0., -1., 0.),
            lat: Vec3::new(1., 0., 0.),
        })
    }
    fn landmark_h(&self, _: &str, _: &str) -> Option<f64> {
        Some(20.)
    }
    fn section_radius(&self, _: &str, _: f64) -> f64 {
        30.
    }
}

fn body() -> Body {
    let mut vertices = Vec::new();
    for z in [-0.02, 0.12] {
        for i in 0..16 {
            let (s, c) = det::sin_cos(i as f64 * std::f64::consts::TAU / 16.);
            vertices.push(Vec3::new(0.03 * c, 0.03 * s, z));
        }
    }
    let mut tris = Vec::new();
    for i in 0..16 {
        let j = (i + 1) % 16;
        tris.extend([[i, j, j + 16], [i, j + 16, i + 16]]);
    }
    Body::new(vertices, tris, 0.01).unwrap()
}

fn run(raw: Value) -> Result<DrapeRun, Refusal> {
    let body = body();
    drape(
        &Pattern::new(raw),
        &json!({}),
        &DrapeBody {
            frames: &Form,
            collision: (&body.vertices, &body.triangles),
            display: (&body.vertices, &body.triangles),
            arm_deg: 35.,
        },
        &DrapeOptions {
            spacing: 24.,
            sew_steps: 3,
            drape_steps: 3,
            step: 0.001,
            ..DrapeOptions::default()
        },
        |_| {},
    )
}

#[test]
fn source_group_welds_once_changes_sewn_dynamics_and_keeps_cloth_heat_storage() {
    let plain = run(band()).unwrap();
    let raw = loaded();
    let loaded = run(raw.clone()).unwrap();
    let report = loaded
        .scene
        .solver
        .model
        .attached_mass_report()
        .unwrap()
        .unwrap();
    assert_eq!(report.input, raw["attached_mass"]);
    assert_eq!(report.attached_mass_kg, 0.03);
    assert_eq!(
        report.cloth_mass_kg,
        plain.scene.solver.model.mass.iter().sum::<f64>()
    );
    assert!(report.mass_balance_residual_kg.abs() < 1e-16);
    assert_eq!(loaded.report["mass_kg"], plain.report["mass_kg"]);
    assert_eq!(
        loaded.report["attached_mass"],
        serde_json::to_value(report).unwrap()
    );
    assert_eq!(loaded.scene.mesh.faces, plain.scene.mesh.faces);
    assert_eq!(loaded.scene.mesh.verts2d, plain.scene.mesh.verts2d);
    assert_eq!(loaded.scene.weld, plain.scene.weld);
    assert_eq!(
        loaded.scene.solver.model.membranes,
        plain.scene.solver.model.membranes
    );
    let targets = &report.groups[0].targets;
    assert_ne!(targets[0].mesh_vertex, targets[1].mesh_vertex);
    assert_eq!(targets[0].solver_vertex, targets[1].solver_vertex);
    assert_eq!(targets[0].mass_kg, 0.0075);
    assert_eq!(targets[1].mass_kg, 0.0225);
    for (i, (&a, &b)) in loaded
        .scene
        .solver
        .model
        .mass
        .iter()
        .zip(&plain.scene.solver.model.mass)
        .enumerate()
    {
        assert!(
            (a - b
                - if i == targets[0].solver_vertex {
                    0.03
                } else {
                    0.
                })
            .abs()
                < 1e-17
        );
    }
    assert_ne!(loaded.scene.solver.x, plain.scene.solver.x);
    let (plain_surface, _) = Surface::from_drape_report(&plain.report).unwrap();
    let (loaded_surface, x) = Surface::from_drape_report(&loaded.report).unwrap();
    assert_eq!(x, loaded.scene.solver.x);
    // The source sheet is unchanged. Point mass supplies no guessed accessory heat capacity/area.
    assert_eq!(plain_surface.snapshot(), loaded_surface.snapshot());
    let replay = run(raw).unwrap();
    assert_eq!(loaded.report, replay.report);
    assert_eq!(loaded.scene.solver.x, replay.scene.solver.x);
}

fn one_dof(run: &DrapeRun, target: usize, x: Vec<Vec3>, gravity: Vec3) -> Solver {
    let mut fixed = vec![[true; 3]; x.len()];
    fixed[target][2] = false;
    let mut solver = Solver::new(run.scene.solver.model.clone(), x, &fixed, gravity);
    solver.tol = 1e-10;
    solver
}

#[test]
fn exact_static_weight_and_implicit_inertia_include_the_attached_group() {
    let plain = run(band()).unwrap();
    let loaded = run(loaded()).unwrap();
    let target = loaded
        .scene
        .solver
        .model
        .attached_mass_report()
        .unwrap()
        .unwrap()
        .groups[0]
        .targets[0]
        .solver_vertex;
    let start = plain.scene.solver.x.clone();
    let mut heavy = one_dof(&loaded, target, start.clone(), Vec3::new(0., 0., -9.81));
    heavy.static_solve().unwrap();
    let force = heavy.checked_elastic_forces().unwrap()[target].z;
    assert!((force - heavy.model.mass[target] * 9.81).abs() < 1e-7);
    let mut light = one_dof(&plain, target, start.clone(), Vec3::new(0., 0., -9.81));
    light.static_solve().unwrap();
    assert!(heavy.x[target].z < light.x[target].z);

    let mut disturbed = start;
    disturbed[target].z += 0.002;
    let mut heavy = one_dof(&loaded, target, disturbed.clone(), Vec3::ZERO);
    let mut light = one_dof(&plain, target, disturbed.clone(), Vec3::ZERO);
    let h = 0.001;
    for solver in [&mut heavy, &mut light] {
        solver.step(h).unwrap();
        let elastic = solver.checked_elastic_forces().unwrap()[target].z;
        let inertia =
            solver.model.mass[target] * (solver.x[target].z - disturbed[target].z) / (h * h);
        assert!((elastic - inertia).abs() < 1e-7);
    }
    assert!(
        (heavy.x[target].z - disturbed[target].z).abs()
            < (light.x[target].z - disturbed[target].z).abs()
    );
}

#[test]
fn material_swap_rebuilds_sheet_mass_preserving_group_source_and_exact_mapping() {
    let raw = loaded();
    let original = run(raw.clone()).unwrap();
    let mut replacement = raw;
    replacement["fabric"]["material"] = sheet(400.);
    let swapped = run(replacement).unwrap();
    let a = &original.report["attached_mass"];
    let b = &swapped.report["attached_mass"];
    assert_eq!(a["input"], b["input"]);
    assert_eq!(a["input_sha256"], b["input_sha256"]);
    assert_eq!(a["groups"], b["groups"]);
    assert_eq!(a["attached_mass_kg"], b["attached_mass_kg"]);
    assert_eq!(
        a["cloth_mass_kg"].as_f64().unwrap() * 2.,
        b["cloth_mass_kg"]
    );
    assert_eq!(original.scene.mesh.faces, swapped.scene.mesh.faces);
    assert_eq!(original.scene.weld, swapped.scene.weld);
    assert_ne!(original.scene.solver.x, swapped.scene.solver.x);
    let mut bound = original.scene.solver.model.clone();
    let before = bound.mass.clone();
    assert!(
        bound
            .set_materials(&[])
            .unwrap_err()
            .message
            .contains("attached mass")
    );
    assert_eq!(before, bound.mass);
    assert!(bound.attached_mass_report().unwrap().is_some());
}

#[test]
fn mirrored_instances_receive_only_their_declared_shares() {
    let mut raw = loaded();
    raw["pieces"].as_array_mut().unwrap().truncate(1);
    raw["pieces"][0]["cut"] = json!({"qty":2});
    raw["seams"] = json!([]);
    raw["attached_mass"]["groups"][0]["targets"] = json!([
        {"instance":"front.R","vertex":"a","fraction":0.25},
        {"instance":"front.L","vertex":"a","fraction":0.75}]);
    let result = run(raw).unwrap();
    let report = result
        .scene
        .solver
        .model
        .attached_mass_report()
        .unwrap()
        .unwrap();
    let targets = &report.groups[0].targets;
    assert_eq!(targets[0].instance, "front.R");
    assert_eq!(targets[1].instance, "front.L");
    assert_eq!(targets[0].source_piece, targets[1].source_piece);
    assert_ne!(targets[0].solver_vertex, targets[1].solver_vertex);
    assert_eq!(report.attached_mass_kg, 0.03);
}

#[test]
fn absent_or_empty_extension_preserves_the_existing_run_and_bad_mass_refuses() {
    let plain = run(band()).unwrap();
    let mut empty = band();
    empty["attached_mass"] = json!({"schema":"ftx-attached-mass/1","groups":[]});
    let empty = run(empty).unwrap();
    assert_eq!(plain.report, empty.report);
    assert_eq!(plain.scene.solver.x, empty.scene.solver.x);
    assert_eq!(plain.scene.solver.model.mass, empty.scene.solver.model.mass);
    assert!(
        plain
            .scene
            .solver
            .model
            .attached_mass_report()
            .unwrap()
            .is_none()
    );
    assert!(plain.report.get("attached_mass").is_none());
    for extra in [
        {
            let mut e = extra();
            e["mass"] = Value::Null;
            e
        },
        {
            let mut e = extra();
            e["allow_estimates"] = json!(false);
            e
        },
        {
            let mut e = extra();
            e["mass"]["unit"] = json!("g");
            e
        },
        {
            let mut e = extra();
            e["mass"]["value"] = json!(0.);
            e
        },
    ] {
        let mut raw = loaded();
        raw["attached_mass"]["groups"][0]["extra"] = extra;
        assert!(run(raw).is_err());
    }
    let mut missing = loaded();
    missing["attached_mass"]["groups"][0]["targets"][0]["vertex"] = json!("missing");
    assert!(run(missing).is_err());
    let mut omitted = loaded();
    omitted["pieces"][0]["sim"] = json!(false);
    assert!(run(omitted).is_err());
}

#[test]
fn prescribed_motion_retains_inertia_and_late_failure_rolls_back_every_visible_state() {
    let result = run(loaded()).unwrap();
    let mut solver = result.scene.solver.clone();
    let mass = solver.model.mass.clone();
    let receipt = serde_json::to_value(solver.model.attached_mass_report().unwrap()).unwrap();
    let target: Vec<_> = solver
        .contact
        .as_ref()
        .unwrap()
        .body
        .vertices
        .iter()
        .map(|p| *p + Vec3::new(0.00001, 0., 0.))
        .collect();
    let options = MotionOptions {
        max_step_s: 0.001,
        ..MotionOptions::default()
    };
    let report = advance_body(&mut solver, &target, 0.002, options).unwrap();
    assert_eq!(report.samples.len(), 2);
    assert_eq!(solver.model.mass, mass);
    assert_eq!(
        serde_json::to_value(solver.model.attached_mass_report().unwrap()).unwrap(),
        receipt
    );
    assert_ne!(solver.x, result.scene.solver.x);

    let mut refused = result.scene.solver.clone();
    let before = format!("{refused:?}");
    let one_sample = refused.x.len() + target.len();
    let error = advance_body(
        &mut refused,
        &target,
        0.002,
        MotionOptions {
            max_sample_vertices: one_sample,
            ..options
        },
    )
    .unwrap_err();
    assert!(error.message.contains("sample budget"));
    assert_eq!(format!("{refused:?}"), before);
}

#[test]
fn stale_inertia_or_reference_cannot_step_publish_a_receipt_or_partially_mutate_history() {
    let result = run(loaded()).unwrap();
    for reference_edit in [false, true] {
        let mut stale = result.scene.solver.clone();
        if reference_edit {
            stale.model.rest[0].x += 0.001;
        } else {
            stale.model.mass[0] += 0.001;
        }
        let before = format!("{stale:?}");
        assert!(stale.model.attached_mass_report().is_err());
        assert!(
            stale
                .step(0.001)
                .unwrap_err()
                .message
                .contains("attached mass")
        );
        assert!(
            stale
                .static_solve()
                .unwrap_err()
                .message
                .contains("attached mass")
        );
        let target = stale.contact.as_ref().unwrap().body.vertices.clone();
        assert!(
            advance_body(&mut stale, &target, 0.001, MotionOptions::default())
                .unwrap_err()
                .message
                .contains("attached mass")
        );
        assert_eq!(format!("{stale:?}"), before);
    }
}

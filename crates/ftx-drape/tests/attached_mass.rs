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

fn wet_sheet() -> Value {
    let mut selected = sheet(200.);
    let record = &mut selected["record"];
    record["structure"]["mechanical_mass_model"] =
        json!(ftx_pattern::material_mass::RETAINED_LIQUID_MODEL);
    let conditions = json!({"specimen_id":"synthetic-retained-sheet", "conditioning":"closed wet test",
        "retained_liquid_kg_m2":"0.1"});
    for q in record["properties"].as_object_mut().unwrap().values_mut() {
        q["conditions"] = conditions.clone();
    }
    record["properties"]["areal_density"]["conditions"]["reference_area_basis"] =
        json!("dry_textile_reference_area");
    record["properties"]["areal_density"]["conditions"]["mass_basis"] =
        json!("dry_textile_excluding_retained_liquid");
    record["properties"]["retained_liquid_areal_density"] = json!({"value":0.1,"unit":"kg/m2",
        "evidence":"estimate","source":"test","method":"Independent synthetic retained-liquid mass",
        "conditions":{"specimen_id":"synthetic-retained-sheet","conditioning":"closed wet test",
            "reference_area_basis":"dry_textile_reference_area", "mass_basis":"additional_retained_liquid",
            "retention_basis":"closed_co_moving_no_exchange","qualification":"Synthetic no-exchange co-moving liquid experiment"}});
    selected
}

#[test]
fn fixed_retained_liquid_changes_sewn_weight_and_inertia_without_double_counting_extras_or_heat() {
    let plain = run(band()).unwrap();
    let mut wet = band();
    wet["fabric"]["material"] = wet_sheet();
    let result = run(wet.clone()).unwrap();
    let dry_mass: f64 = plain.scene.solver.model.mass.iter().sum();
    let wet_mass: f64 = result.scene.solver.model.mass.iter().sum();
    assert!((wet_mass - 1.5 * dry_mass).abs() < 1e-15);
    assert_eq!(plain.scene.mesh.faces, result.scene.mesh.faces);
    assert_eq!(plain.scene.mesh.verts2d, result.scene.mesh.verts2d);
    assert_eq!(plain.scene.weld, result.scene.weld);
    for (a, b) in plain
        .scene
        .solver
        .model
        .mass
        .iter()
        .zip(&result.scene.solver.model.mass)
    {
        assert!((b - 1.5 * a).abs() < 1e-17);
    }
    assert_ne!(plain.scene.solver.x, result.scene.solver.x);
    assert_eq!(
        result.report["material"]["retained_liquid_mass"]["mass"]["dry_textile_kg_m2"],
        0.2
    );
    assert_eq!(
        result.report["material"]["retained_liquid_mass"]["mass"]["retained_liquid_kg_m2"],
        0.1
    );
    assert!(Surface::from_drape_report(&result.report).is_err());
    assert_eq!(result.report, run(wet).unwrap().report);
    let mut with_extra = loaded();
    with_extra["fabric"]["material"] = wet_sheet();
    let extra_result = run(with_extra).unwrap();
    let receipt = extra_result
        .scene
        .solver
        .model
        .attached_mass_report()
        .unwrap()
        .unwrap();
    assert!((receipt.total_inertial_mass_kg - wet_mass - 0.03).abs() < 1e-15);
    assert_eq!(receipt.attached_mass_kg, 0.03);
    assert_eq!(receipt.cloth_mass_kg, wet_mass);

    let target = 0;
    let mut wet_static = one_dof(
        &result,
        target,
        plain.scene.solver.x.clone(),
        Vec3::new(0., 0., -9.81),
    );
    wet_static.static_solve().unwrap();
    assert!(
        (wet_static.checked_elastic_forces().unwrap()[target].z
            - wet_static.model.mass[target] * 9.81)
            .abs()
            < 1e-7
    );
    let mut disturbed = plain.scene.solver.x.clone();
    disturbed[target].z += 0.002;
    let mut wet_dynamic = one_dof(&result, target, disturbed.clone(), Vec3::ZERO);
    wet_dynamic.step(0.001).unwrap();
    let expected =
        wet_dynamic.model.mass[target] * (wet_dynamic.x[target].z - disturbed[target].z) / 1e-6;
    assert!((wet_dynamic.checked_elastic_forces().unwrap()[target].z - expected).abs() < 1e-7);
}

#[test]
fn retained_liquid_follows_actual_piece_ownership_and_survives_motion_without_reset_or_exchange() {
    let plain = run(band()).unwrap();
    let mut raw = band();
    raw["pieces"][0]["fabric"] = json!({"material":wet_sheet()});
    let result = run(raw).unwrap();
    let model = &result.scene.solver.model;
    let mut expected = plain.scene.solver.model.mass.clone();
    for (element, owner) in model.membranes.iter().zip(&result.scene.triangle_owner) {
        if result.scene.mesh.instances[*owner] == "front" {
            for v in element.v {
                expected[v] += 0.1 * element.area / 3.;
            }
        }
    }
    for (a, b) in model.mass.iter().zip(&expected) {
        assert!((a - b).abs() < 1e-17);
    }
    let mut moving = result.scene.solver.clone();
    let mass = moving.model.mass.clone();
    let target: Vec<_> = moving
        .contact
        .as_ref()
        .unwrap()
        .body
        .vertices
        .iter()
        .map(|p| *p + Vec3::new(0.00001, 0., 0.))
        .collect();
    let motion = advance_body(
        &mut moving,
        &target,
        0.002,
        MotionOptions {
            max_step_s: 0.001,
            ..MotionOptions::default()
        },
    )
    .unwrap();
    assert_eq!(motion.samples.len(), 2);
    assert_eq!(moving.model.mass, mass);
    assert_ne!(moving.x, result.scene.solver.x);
    let mut bad = band();
    bad["fabric"]["material"] = wet_sheet();
    bad["fabric"]["material"]["record"]["properties"]["friction_skin"]["conditions"]["retained_liquid_kg_m2"] =
        json!("0");
    assert!(run(bad).err().unwrap().message.contains("wet mechanics"));
}

#[test]
fn timed_strip_consumes_the_same_fixed_liquid_density_and_frozen_source() {
    let input = |material: Value| {
        serde_json::from_value::<ftx_drape::strip::history::Request>(json!({
        "schema":"ftx-strip-history/1", "material":material,
        "fixture":{"gauge_length_m":0.1,"width_m":0.06,"warp_axis":[0.,1.],"cells":[2,2]},
        "loading":[{"time_s":0.,"extension_fraction":0.},
            {"time_s":0.01,"extension_fraction":0.01},{"time_s":0.02,"extension_fraction":0.01}],
        "controls":{"max_step_s":0.002}
    })).unwrap()
    };
    let dry = ftx_drape::strip::history::evaluate(&input(sheet(200.))).unwrap();
    let wet = ftx_drape::strip::history::evaluate(&input(wet_sheet())).unwrap();
    assert!((wet.reference_mass_kg - 1.5 * dry.reference_mass_kg).abs() < 1e-15);
    assert_eq!(wet.reference_positions_m, dry.reference_positions_m);
    assert_eq!(wet.triangles, dry.triangles);
    assert_eq!(wet.retained_liquid.unwrap().retained_liquid_kg_m2, 0.1);
    assert_ne!(wet.states[1].positions_m, dry.states[1].positions_m);
    let replay = ftx_drape::strip::history::evaluate(
        &serde_json::from_str(&wet.replay_request_json).unwrap(),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(wet).unwrap(),
        serde_json::to_value(replay).unwrap()
    );
}

#[test]
fn both_static_strip_consumers_require_the_selected_wet_response_but_not_dry_mass() {
    let check = |selected: Value| {
        let material = ftx_pattern::textile_import::catalog()
            .unwrap()
            .resolve(&serde_json::from_value(selected.clone()).unwrap())
            .unwrap();
        let a = ftx_drape::strip::simulate(
            &material,
            &ftx_drape::strip::Geometry {
                gauge_m: 0.1,
                width_m: 0.06,
                warp_angle_deg: 0.,
                length_cells: 2,
                width_cells: 2,
            },
            &[0.01],
            &ftx_drape::strip::Options::default(),
        );
        let request: ftx_drape::strip::Request = serde_json::from_value(json!({
            "schema":"ftx-strip-experiment/1","material":selected,
            "fixture":{"gauge_length_m":0.1,"width_m":0.06,"warp_axis":[0.,1.],"cells":[2,2]},
            "extensions":[0.01]
        }))
        .unwrap();
        let b = ftx_drape::strip::evaluate(&request);
        (a, b)
    };
    let (a, b) = check(wet_sheet());
    assert!(a.is_ok() && b.is_ok());
    let mut stale = wet_sheet();
    stale["record"]["properties"]["warp_stiffness"]["conditions"]["retained_liquid_kg_m2"] =
        json!("0");
    let (a, b) = check(stale);
    assert!(a.unwrap_err().message.contains("wet mechanics"));
    assert!(b.unwrap_err().message.contains("wet mechanics"));
    let mut dry = sheet(200.);
    dry["record"]["properties"]
        .as_object_mut()
        .unwrap()
        .retain(|key, _| {
            [
                "warp_stiffness",
                "weft_stiffness",
                "poisson_warp_weft",
                "shear_stiffness",
            ]
            .contains(&key.as_str())
        });
    let (a, b) = check(dry);
    assert!(a.is_ok() && b.is_ok());
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

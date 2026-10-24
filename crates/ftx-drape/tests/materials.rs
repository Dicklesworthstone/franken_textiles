//! Synthetic characterization tests: arithmetic/causal integration, not measured fabric validation.

use ftx_core::{Vec2, Vec3};
use ftx_drape::material::{Fabric, resolve};
use ftx_drape::run::{DrapeBody, DrapeOptions, drape};
use ftx_drape::scene::PartFrames;
use ftx_drape::{Body, ClothModel, MembraneElement, PartFrame};
use ftx_pattern::Pattern;
use serde_json::{Value, json};

fn record(scale: f64) -> Value {
    let mut properties = serde_json::Map::new();
    for (name, value, unit) in [
        ("areal_density", 200.0 * scale, "g/m2"),
        ("thickness", 0.6 * scale, "mm"),
        ("warp_stiffness", 100.0 * scale * scale, "N/m"),
        ("weft_stiffness", 50.0 * scale * scale, "N/m"),
        ("shear_stiffness", 10.0 * scale * scale, "N/m"),
        ("poisson_warp_weft", 0.2, "1"),
        ("bending_rigidity", 3e-5 * scale, "N*m"),
        ("friction_skin", 0.2 * scale, "1"),
        ("friction_self", 0.3 * scale, "1"),
        ("thermal_conductivity", 0.04, "W/(m*K)"),
    ] {
        properties.insert(
            name.into(),
            json!({"value":value,"unit":unit,"source":"test",
            "evidence":"estimate","method":"Synthetic test declaration, not an actual textile"}),
        );
    }
    json!({"id":"synthetic", "allow_estimates":true, "record":{
        "id":"synthetic","name":"Synthetic characterization","form":"woven",
        "properties":properties,
        "sources":{"test":{"reference":"test:materials","note":"Independent synthetic inputs","accessed_on":"2026-10-13"}}}})
}

fn band(fabric: Value) -> Pattern {
    let w = 40.0 * std::f64::consts::TAU;
    Pattern::new(
        json!({"schema":"fabric-pattern/1","units":"mm","fabric":fabric,
        "pieces":[{"id":"band","cut":{"qty":1},
            "vertices":{"a":[0,0],"b":[w,0],"c":[w,20],"d":[0,20]},
            "edges":[{"id":"bottom","from":"a","to":"b"},{"id":"right","from":"b","to":"c"},
                {"id":"top","from":"c","to":"d"},{"id":"left","from":"d","to":"a"}],
            "notches":[],"grain":[[0,0],[0,20]],
            "placement":{"part":"torso","radius_mm":40,"x0":w/2.0}}],
        "seams":[{"id":"join","a":"band/right","b":"band/left"}]}),
    )
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
        Some(20.0)
    }
    fn section_radius(&self, _: &str, _: f64) -> f64 {
        30.0
    }
}

fn body() -> Body {
    let mut vertices = Vec::new();
    for z in [-0.02, 0.12] {
        for i in 0..16 {
            let (s, c) = ftx_core::det::sin_cos(i as f64 * std::f64::consts::TAU / 16.0);
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

#[test]
fn material_controls_membrane_mass_bending_and_both_contacts() {
    let p = band(json!({"material":record(1.0)}));
    let opts = DrapeOptions {
        self_contact: Some(DrapeOptions::default().contact),
        ..DrapeOptions::default()
    };
    let m = resolve(&p, &opts).unwrap();
    assert_eq!(m.options.contact.thickness, 0.0003);
    assert_eq!(m.options.contact.mu, 0.2);
    assert_eq!(m.options.self_contact.unwrap().thickness, 0.0006);
    assert_eq!(m.options.self_contact.unwrap().mu, 0.3);
    assert_eq!(m.options.contact.dhat, opts.contact.dhat);
    assert_eq!(m.options.contact.kappa, opts.contact.kappa);
    assert_eq!(m.options.contact.eps_v, opts.contact.eps_v);
    let rest = vec![Vec2::ZERO, Vec2::new(1., 0.), Vec2::new(0., 1.)];
    let model = ClothModel::new(
        rest.clone(),
        &[[0, 1, 2]],
        m.fabric.membrane(1.0),
        m.fabric.bending_nm,
        m.fabric.areal_kg_m2,
    )
    .unwrap();
    assert!((model.mass.iter().sum::<f64>() - 0.1).abs() < 1e-15);
    assert_eq!(model.bending, 3e-5);
    let element = MembraneElement::new([0, 1, 2], [rest[0], rest[1], rest[2]]).unwrap();
    let warp = [Vec3::ZERO, Vec3::new(1.01, 0., 0.), Vec3::new(0., 1., 0.)];
    let weft = [Vec3::ZERO, Vec3::new(1., 0., 0.), Vec3::new(0., 1.01, 0.)];
    assert!(
        (element.energy(&warp, &model.membrane) / element.energy(&weft, &model.membrane) - 2.)
            .abs()
            < 1e-12
    );
    assert_eq!(
        m.report["thermal_basis"],
        "dry_slab_thickness_over_conductivity"
    );
    assert!((m.report["dry_material_resistance_m2_k_w"].as_f64().unwrap() - 0.015).abs() < 1e-15);
}

#[test]
fn the_real_drape_changes_when_only_material_observations_change() {
    let surface = body();
    let body = DrapeBody {
        frames: &Form,
        collision: (&surface.vertices, &surface.triangles),
        display: (&surface.vertices, &surface.triangles),
        arm_deg: 35.0,
    };
    let opts = DrapeOptions {
        spacing: 24.0,
        sew_steps: 3,
        drape_steps: 3,
        step: 1e-3,
        ..DrapeOptions::default()
    };
    let a = band(json!({"material":record(1.0)}));
    let b = band(json!({"material":record(2.0)}));
    assert_eq!(a.raw()["pieces"], b.raw()["pieces"]);
    let a = drape(&a, &json!({}), &body, &opts, |_| {}).unwrap();
    let b = drape(&b, &json!({}), &body, &opts, |_| {}).unwrap();
    assert_eq!(a.scene.mesh.verts2d, b.scene.mesh.verts2d);
    assert_eq!(a.scene.weld, b.scene.weld);
    let ma: f64 = a.scene.solver.model.mass.iter().sum();
    let mb: f64 = b.scene.solver.model.mass.iter().sum();
    assert!((mb / ma - 2.0).abs() < 1e-12);
    assert_eq!(
        b.scene.solver.model.bending,
        2.0 * a.scene.solver.model.bending
    );
    assert_ne!(a.scene.solver.x, b.scene.solver.x);
    assert_ne!(
        a.report["material"]["record_sha256"],
        b.report["material"]["record_sha256"]
    );
    assert_eq!(a.report["mass_kg"], ma);
    assert_eq!(a.report["material"]["fit_claim"], false);
    for run in [&a, &b] {
        assert_eq!(run.sew.samples.len(), opts.sew_steps);
        assert_eq!(run.drape.samples.len(), opts.drape_steps);
        for sample in run.sew.samples.iter().chain(&run.drape.samples) {
            assert!(sample.solve.converged);
            assert!(sample.solve.residual.is_finite());
            assert!(sample.solve.last_step / opts.step < opts.tol);
            assert!(sample.contact.as_ref().unwrap().body.is_some());
            assert!(sample.contact.as_ref().unwrap().self_contact.is_none());
        }
        let reactions = &run.report["contact_diagnostics"]["final_reactions"];
        assert_eq!(reactions["status"], "available");
        assert_eq!(
            reactions["result"]["positions_m"],
            json!(run.scene.solver.x)
        );
        assert!(reactions["result"]["pressure_pa"].is_null());
        assert_eq!(
            run.report["contact_diagnostics"]["accepted_samples"],
            opts.sew_steps + opts.drape_steps
        );
        assert_eq!(
            reactions["result"]["material_report_sha256"],
            ftx_core::receipt::sha256_hex(&serde_json::to_vec(&run.report["material"]).unwrap())
        );
        let speed = run
            .scene
            .solver
            .v
            .iter()
            .map(|v| v.norm())
            .fold(0.0, f64::max);
        assert_eq!(run.drape.samples.last().unwrap().max_speed_m_s, speed);
        assert_eq!(run.report["numerical"]["drape"]["max_speed"], speed);
        assert_eq!(run.report["numerical"]["settled"], false);
        assert!(
            run.gating()
                .iter()
                .any(|s| s.starts_with("not numerically settled"))
        );
    }

    let mut scene = a.scene;
    scene.solver.max_newton = 0;
    scene.solver.contact.as_mut().unwrap().params.thickness = 0.0;
    let accepted = format!("{:?}", scene.solver);
    let mut frames = 0;
    let failure = ftx_drape::scene::run_phase(&mut scene, 1, opts.step, 9.81, |_, _| frames += 1)
        .unwrap_err();
    assert_eq!(failure.code, ftx_core::RefusalCode::NotConverged);
    assert_eq!(frames, 0);
    assert_eq!(format!("{:?}", scene.solver), accepted);
}

#[test]
fn identity_and_marketing_do_not_choose_hidden_physics() {
    let p = band(json!({"material":record(1.0)}));
    let a = resolve(&p, &DrapeOptions::default()).unwrap();
    let mut raw = p.raw().clone();
    raw["fabric"]["material"]["record"]["name"] = json!("Totally different marketing name");
    let b = resolve(&Pattern::new(raw), &DrapeOptions::default()).unwrap();
    assert_eq!(a.fabric, b.fabric);
    assert_eq!(a.options, b.options);
    assert_ne!(a.report["record_sha256"], b.report["record_sha256"]);
}

#[test]
fn missing_or_malformed_material_refuses_before_geometry() {
    let opts = DrapeOptions::default();
    let p = Pattern::new(json!({"fabric":{"material_id":"fs_il019_natural"}}));
    assert!(
        resolve(&p, &opts)
            .err()
            .unwrap()
            .message
            .contains("missing textile property")
    );
    for f in [
        json!(3),
        json!({"tensile_N_m":"fast"}),
        json!({"bending_Nm":-1}),
        json!({"material_id":"absent"}),
    ] {
        assert!(resolve(&band(f), &opts).is_err());
    }
    let mut p = band(json!({})).raw().clone();
    p["pieces"][0]["material_id"] = json!("fs_il019_natural");
    assert!(resolve(&Pattern::new(p), &opts).is_err());
}

#[test]
fn legacy_coefficients_and_options_remain_exact() {
    let opts = DrapeOptions::default();
    let p = band(json!({}));
    let m = resolve(&p, &opts).unwrap();
    assert_eq!(m.fabric, Fabric::default());
    assert_eq!(m.options, opts);
    assert_eq!(
        m.fabric.membrane(1.0).q,
        [[8000., 0., 0.], [0., 8000., 0.], [0., 0., 1600.]]
    );
    assert_eq!(m.report["basis"], "legacy_uncertain");
    assert!(Fabric::of(&band(json!({"material":record(1.0)}))).is_err());
}

#[test]
fn explicit_estimate_consent_is_enforced_in_the_real_drape_boundary() {
    let mut selection = record(1.0);
    selection["allow_estimates"] = json!(false);
    let error = resolve(
        &band(json!({"material":selection})),
        &DrapeOptions::default(),
    )
    .err()
    .unwrap();
    assert!(error.message.contains("explicit opt-in"));
}

/// A piece's `material` string is a cutting label (the skill's rounds put one on every piece, the exporters
/// print it), so it drapes in pattern.fabric as the reference does; a characterized selection in its place
/// (an object, or `material_id`) is refused and pointed at `piece.fabric`.
#[test]
fn piece_material_labels_drape_and_misplaced_selections_are_refused() {
    let opts = DrapeOptions::default();
    let labelled = |piece: Value| {
        let mut p = band(json!({"id":"shell","tensile_N_m":8000.0}))
            .raw()
            .clone();
        let band = p["pieces"][0].as_object_mut().unwrap();
        for (k, v) in piece.as_object().unwrap() {
            band.insert(k.clone(), v.clone());
        }
        Pattern::new(p)
    };
    assert!(resolve(&labelled(json!({"material":"shell"})), &opts).is_ok());
    assert!(resolve(&labelled(json!({"material":"lining"})), &opts).is_ok());
    for piece in [
        json!({"material":record(1.0)}),
        json!({"material_id":"fs_il019_natural"}),
    ] {
        let e = resolve(&labelled(piece.clone()), &opts).err().unwrap();
        assert!(
            e.message.contains("piece.fabric.material"),
            "{piece}: {}",
            e.message
        );
    }
}

#[test]
fn swatch_identification_reaches_real_sewn_band_dynamics_without_pattern_edits() {
    use ftx_pattern::material_calibration::{Input, calibrate};
    use ftx_pattern::material_catalog::Selection;
    use ftx_pattern::textile_import::catalog;

    let input: Input = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-swatch.json"
    ))
    .unwrap();
    let surface = body();
    let body = DrapeBody {
        frames: &Form,
        collision: (&surface.vertices, &surface.triangles),
        display: (&surface.vertices, &surface.triangles),
        arm_deg: 35.0,
    };
    let opts = DrapeOptions {
        spacing: 24.0,
        sew_steps: 3,
        drape_steps: 3,
        step: 1e-3,
        ..DrapeOptions::default()
    };
    let solve = |scale: f64| {
        let mut input = input.clone();
        for s in &mut input.samples {
            for p in s.nominal_n_m.iter_mut().flatten().flatten() {
                *p *= scale;
            }
        }
        let identified = calibrate(&input).unwrap();
        let selected = catalog()
            .unwrap()
            .resolve(&Selection {
                id: identified.material.id.clone(),
                record: Some(identified.material),
                allow_estimates: true,
                ..Selection::default()
            })
            .unwrap();
        let pattern = band(json!({"material":selected.frozen_selection()}));
        let run = drape(&pattern, &json!({}), &body, &opts, |_| {}).unwrap();
        let domain = resolve(&pattern, &opts)
            .unwrap()
            .fabric
            .strain_domain()
            .unwrap();
        assert_eq!(
            run.report["material"]["fitted_strain_range"],
            json!(domain.ranges())
        );
        for (i, element) in run.scene.solver.model.membranes.iter().enumerate() {
            assert_eq!(run.scene.solver.model.membrane_domain(i), Some(domain));
            assert!(domain.contains(element.strain(&run.scene.solver.x)));
        }
        (pattern, run)
    };
    let (pa, a) = solve(1.0);
    let (pb, b) = solve(0.25);
    assert_eq!(pa.raw()["pieces"], pb.raw()["pieces"]);
    assert_eq!(pa.raw()["seams"], pb.raw()["seams"]);
    assert_eq!(a.scene.mesh.verts2d, b.scene.mesh.verts2d);
    assert_eq!(a.scene.weld, b.scene.weld);
    assert_eq!(a.scene.solver.model.mass, b.scene.solver.model.mass);
    assert_eq!(a.scene.solver.model.bending, b.scene.solver.model.bending);
    for (qa, qb) in a
        .scene
        .solver
        .model
        .membrane
        .q
        .iter()
        .flatten()
        .zip(b.scene.solver.model.membrane.q.iter().flatten())
    {
        assert!((qb - qa * 0.25).abs() < 1e-8);
    }
    assert_ne!(a.scene.solver.x, b.scene.solver.x);
    assert_ne!(
        a.report["material"]["record_sha256"],
        b.report["material"]["record_sha256"]
    );
}

#[test]
fn narrow_swatch_cannot_silently_extrapolate_during_sewn_scene_construction() {
    use ftx_pattern::material_calibration::{Input, calibrate};
    let mut input: Input = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-swatch.json"
    ))
    .unwrap();
    // Preserve the original acquisition's three FIT states: no negative shear was observed.
    input.samples.truncate(4);
    let identified = calibrate(&input).unwrap();
    assert_eq!(identified.fitted_strain_range[2][0], 0.);
    let pattern = band(json!({"material":{
        "id":identified.material.id,"record":identified.material,"allow_estimates":true
    }}));
    let surface = body();
    let body = DrapeBody {
        frames: &Form,
        collision: (&surface.vertices, &surface.triangles),
        display: (&surface.vertices, &surface.triangles),
        arm_deg: 35.,
    };
    let options = DrapeOptions {
        spacing: 24.,
        sew_steps: 3,
        drape_steps: 3,
        step: 1e-3,
        ..DrapeOptions::default()
    };
    let refusal = drape(&pattern, &json!({}), &body, &options, |_| {})
        .err()
        .unwrap();
    assert_eq!(refusal.code, ftx_core::RefusalCode::OutOfRange);
    assert!(
        refusal.message.contains("outside fitted ranges"),
        "{refusal}"
    );
}

#[test]
fn nonlinear_identification_changes_actual_sewn_dynamics_without_geometry_or_mass_edits() {
    use ftx_pattern::material_calibration::nonlinear::{Input, calibrate};
    use ftx_pattern::material_catalog::Selection;

    let input: Input = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-nonlinear-swatch.json"
    ))
    .unwrap();
    let surface = body();
    let body = DrapeBody {
        frames: &Form,
        collision: (&surface.vertices, &surface.triangles),
        display: (&surface.vertices, &surface.triangles),
        arm_deg: 35.0,
    };
    let opts = DrapeOptions {
        spacing: 24.0,
        sew_steps: 3,
        drape_steps: 3,
        step: 1e-3,
        ..DrapeOptions::default()
    };
    let solve = |scale: f64| {
        let mut input = input.clone();
        for force in input
            .samples
            .iter_mut()
            .flat_map(|s| s.nominal_n_m.iter_mut().flatten().flatten())
        {
            *force *= scale;
        }
        let identified = calibrate(&input).unwrap();
        let selected = ftx_pattern::textile_import::catalog()
            .unwrap()
            .resolve(&Selection {
                id: identified.material.id.clone(),
                record: Some(identified.material),
                allow_estimates: true,
                ..Selection::default()
            })
            .unwrap();
        let pattern = band(json!({"material":selected.frozen_selection()}));
        let run = drape(&pattern, &json!({}), &body, &opts, |_| {}).unwrap();
        (pattern, run)
    };
    let (pa, a) = solve(1.0);
    let (pb, b) = solve(0.25);
    assert_eq!(pa.raw()["pieces"], pb.raw()["pieces"]);
    assert_eq!(pa.raw()["seams"], pb.raw()["seams"]);
    assert_eq!(a.scene.mesh.verts2d, b.scene.mesh.verts2d);
    assert_eq!(a.scene.weld, b.scene.weld);
    assert_eq!(a.scene.solver.model.mass, b.scene.solver.model.mass);
    assert_eq!(a.scene.solver.model.bending, b.scene.solver.model.bending);
    assert_ne!(a.scene.solver.x, b.scene.solver.x);
    assert_ne!(
        a.report["material"]["record_sha256"],
        b.report["material"]["record_sha256"]
    );
    assert!(a.report["material"]["nonlinear_strain_range"].is_array());
}

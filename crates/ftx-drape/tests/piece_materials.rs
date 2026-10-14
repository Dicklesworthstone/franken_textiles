//! Real meshing/welding/Newton known answers with synthetic sheets, not empirical garment evidence.
use ftx_core::{Vec3, det};
use ftx_drape::material::resolve;
use ftx_drape::run::{DrapeBody, DrapeOptions, DrapeRun, drape};
use ftx_drape::scene::{PartFrames, build_scene};
use ftx_drape::{Body, PartFrame, build_garment};
use ftx_pattern::Pattern;
use serde_json::{Value, json};

fn sheet(scale: f64) -> Value {
    let mut properties = serde_json::Map::new();
    for (name, value, unit) in [
        ("areal_density", 200.0 * scale, "g/m2"),
        ("thickness", 0.6, "mm"),
        ("warp_stiffness", 100.0 * scale * scale, "N/m"),
        ("weft_stiffness", 40.0 * scale * scale, "N/m"),
        ("shear_stiffness", 10.0 * scale * scale, "N/m"),
        ("poisson_warp_weft", 0.2, "1"),
        ("bending_rigidity", 3e-5 * scale, "N*m"),
        ("friction_skin", 0.2, "1"),
        ("friction_self", 0.3, "1"),
    ] {
        properties.insert(
            name.into(),
            json!({"value":value,"unit":unit,"source":"test",
            "evidence":"estimate","method":"Synthetic known answer, not an actual textile"}),
        );
    }
    json!({"id":"test-sheet", "allow_estimates":true, "record":{
        "id":"test-sheet", "name":"Synthetic sheet", "form":"woven", "properties":properties,
        "sources":{"test":{"reference":"test:piece-materials","note":"Synthetic inputs", "accessed_on":"2026-10-13"}}}})
}

fn band() -> Pattern {
    let w = 40.0 * std::f64::consts::PI;
    let pieces: Vec<_> = ["front", "back"]
        .into_iter()
        .enumerate()
        .map(|(i, id)| {
            json!({
                "id":id, "material":"cutting label only", "cut":{"qty":1},
                "vertices":{"a":[0,0],"b":[w,0],"c":[w,20],"d":[0,20]},
                "edges":[{"id":"bottom","from":"a","to":"b"},{"id":"right","from":"b","to":"c"},
                    {"id":"top","from":"c","to":"d"},{"id":"left","from":"d","to":"a"}],
                "notches":[],"grain":[[0,0],[0,20]],
                "placement":{"part":"torso","radius_mm":40,"x0":w/2.0,"theta_deg":180*i}
            })
        })
        .collect();
    Pattern::new(
        json!({"schema":"fabric-pattern/1","units":"mm", "fabric":{"material":sheet(1.0)},
        "pieces":pieces,"seams":[{"id":"right_join","a":"front/right","b":"back/left"},
        {"id":"left_join","a":"back/right","b":"front/left"}]}),
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
fn options() -> DrapeOptions {
    DrapeOptions {
        spacing: 24.0,
        sew_steps: 3,
        drape_steps: 3,
        step: 0.001,
        ..DrapeOptions::default()
    }
}
fn run(p: &Pattern, opts: DrapeOptions) -> DrapeRun {
    let b = body();
    drape(
        p,
        &json!({}),
        &DrapeBody {
            frames: &Form,
            collision: (&b.vertices, &b.triangles),
            display: (&b.vertices, &b.triangles),
            arm_deg: 35.,
        },
        &opts,
        |_| {},
    )
    .unwrap()
}
fn assigned(scale: f64) -> Pattern {
    let mut p = band().raw().clone();
    p["pieces"][1]["fabric"] = json!({"material":sheet(scale)});
    Pattern::new(p)
}

#[test]
fn piece_laws_drive_the_actual_sewn_garment_without_changing_its_pattern() {
    let plain = run(&band(), options());
    let mixed = run(&assigned(2.0), options());
    assert_eq!(plain.scene.mesh.faces, mixed.scene.mesh.faces);
    assert_eq!(plain.scene.mesh.verts2d, mixed.scene.mesh.verts2d);
    assert_eq!(plain.scene.weld, mixed.scene.weld);
    assert_eq!(plain.scene.start.relaxed, mixed.scene.start.relaxed);
    assert_ne!(plain.scene.solver.x, mixed.scene.solver.x);
    let model = &mixed.scene.solver.model;
    assert!(model.is_heterogeneous());
    let mut mass = vec![0.0; model.mass.len()];
    for (e, &owner) in model.membranes.iter().zip(&mixed.scene.triangle_owner) {
        let density = if owner == 0 { 0.2 } else { 0.4 };
        for &v in &e.v {
            mass[v] += density * e.area / 3.0;
        }
    }
    assert_eq!(mass, model.mass);
    assert!(
        mixed.scene.weld.len() > model.mass.len(),
        "fixture must actually weld seams"
    );
    let mut seen = [false; 2];
    for (i, &owner) in mixed.scene.triangle_owner.iter().enumerate() {
        seen[owner] = true;
        let q = model.material(i).q;
        let original = plain.scene.solver.model.material(i).q;
        assert_eq!(
            q,
            original.map(|r| r.map(|v| v * if owner == 0 { 1.0 } else { 4.0 }))
        );
    }
    assert_eq!(seen, [true; 2]);
    assert!(
        (model.mass.iter().sum::<f64>() / plain.scene.solver.model.mass.iter().sum::<f64>() - 1.5)
            .abs()
            < 1e-12
    );
    assert_eq!(mixed.report["fabric"]["basis"], "per_piece");
    assert_eq!(
        mixed.report["material"]["triangle_instance"],
        json!(mixed.scene.triangle_owner)
    );
    assert_ne!(
        mixed.report["material"]["pieces"][0]["record_sha256"],
        mixed.report["material"]["pieces"][1]["record_sha256"]
    );
    assert_eq!(
        mixed.report["material"]["pieces"][0]["inherited_default"],
        true
    );
    assert_eq!(
        mixed.report["material"]["pieces"][1]["inherited_default"],
        false
    );
    let contact = mixed.scene.solver.contact.as_ref().unwrap();
    assert_eq!(contact.params.thickness, 0.0003);
    assert_eq!(contact.params.mu, 0.2);
}

#[test]
fn homogeneous_assignment_preserves_positions_velocities_and_animation_exactly() {
    let opts = DrapeOptions {
        self_contact: Some(DrapeOptions::default().contact),
        ..options()
    };
    let plain = run(&band(), opts);
    let declared = run(&assigned(1.0), opts);
    assert!(!declared.scene.solver.model.is_heterogeneous());
    assert_eq!(
        plain.scene.solver.model.mass,
        declared.scene.solver.model.mass
    );
    assert_eq!(plain.scene.solver.x, declared.scene.solver.x);
    assert_eq!(plain.scene.solver.v, declared.scene.solver.v);
    assert_eq!(plain.fold.frames, declared.fold.frames);
    assert_eq!(format!("{:?}", plain.sew), format!("{:?}", declared.sew));
    assert_eq!(
        format!("{:?}", plain.drape),
        format!("{:?}", declared.drape)
    );
}

#[test]
fn identity_survives_grain_changes_mesh_refinement_and_repeated_runs() {
    let mut p = assigned(2.0).raw().clone();
    p["pieces"][1]["grain"] = json!([[0, 0], [20, 0]]);
    let p = Pattern::new(p);
    let coarse = run(&p, options());
    let fine = run(
        &p,
        DrapeOptions {
            spacing: 12.0,
            ..options()
        },
    );
    assert_ne!(
        coarse.scene.triangle_owner.len(),
        fine.scene.triangle_owner.len()
    );
    assert_eq!(
        coarse.report["material"]["assignment_sha256"],
        fine.report["material"]["assignment_sha256"]
    );
    let repeated = run(&p, options());
    assert_eq!(repeated.report, coarse.report);
    assert_eq!(repeated.fold.frames, coarse.fold.frames);
    for r in [&coarse, &fine] {
        for (i, &owner) in r.scene.triangle_owner.iter().enumerate() {
            let e = &r.scene.solver.model.membranes[i];
            assert_eq!(
                r.scene.solver.model.material(i).q[0][0] / r.scene.solver.model.material(i).q[1][1],
                2.5
            );
            // Physical rest coordinates were rotated to each piece's own grain, not to its neighbor.
            assert!(e.area > 0.0 && owner < 2);
        }
        let mass: f64 = r.report["material"]["instances"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["mass_kg"].as_f64().unwrap())
            .sum();
        assert!((mass - r.scene.solver.model.mass.iter().sum::<f64>()).abs() < 1e-15);
    }
    assert!(
        (coarse.report["mass_kg"].as_f64().unwrap() - fine.report["mass_kg"].as_f64().unwrap())
            .abs()
            < 1e-12
    );
}

#[test]
fn unsupported_interfaces_partial_assignments_and_bad_sources_refuse_before_meshing() {
    for change in [
        json!({}),
        json!({"tensile_N_m":20}),
        json!({"layers":[]}),
        json!("lining"),
        json!({"material_id":"unknown"}),
    ] {
        let mut p = band().raw().clone();
        p["pieces"][1]["fabric"] = change;
        assert!(resolve(&Pattern::new(p), &options()).is_err());
    }
    for (name, value) in [("thickness", 1.2), ("friction_skin", 0.5)] {
        let mut p = assigned(2.0).raw().clone();
        p["pieces"][1]["fabric"]["material"]["record"]["properties"][name]["value"] = json!(value);
        assert!(resolve(&Pattern::new(p), &options()).is_err());
    }
    let opts = DrapeOptions {
        self_contact: Some(DrapeOptions::default().contact),
        ..options()
    };
    assert!(
        resolve(&assigned(2.0), &opts)
            .err()
            .unwrap()
            .message
            .contains("pairwise friction")
    );
    let mut p = assigned(2.0).raw().clone();
    p["pieces"][1]["fabric"]["material"]["allow_estimates"] = json!(false);
    assert!(resolve(&Pattern::new(p), &options()).is_err());
    let mut p = assigned(2.0).raw().clone();
    p["fabric"] = json!({});
    assert!(resolve(&Pattern::new(p), &options()).is_err());
    let p = assigned(2.0);
    let b = body();
    let mesh = build_garment(&p, 24.0).unwrap();
    assert!(
        build_scene(
            &p,
            mesh,
            &Form,
            &b.vertices,
            &b.triangles,
            resolve(&p, &options()).unwrap().fabric,
            1.0,
            options().contact,
            None
        )
        .is_err(),
        "low-level uniform entry must not erase assignment"
    );
}

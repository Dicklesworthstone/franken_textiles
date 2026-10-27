//! Real meshing/welding/Newton known answers with synthetic sheets, not empirical garment evidence.
use ftx_core::{Vec3, det};
use ftx_drape::material::resolve;
use ftx_drape::run::{DrapeBody, DrapeOptions, DrapeRun, drape};
use ftx_drape::scene::{PartFrames, build_scene};
use ftx_drape::{Body, PartFrame, build_garment};
use ftx_pattern::{Pattern, material_binding::Changes};
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

fn panel_sheet(warp: f64, weft: f64) -> Value {
    let mut m = sheet(1.0);
    m["record"]["structure"] = json!({"bending_model":ftx_pattern::material_bending::MODEL});
    for (key, value) in [
        ("bending_warp", warp),
        ("bending_weft", weft),
        ("bending_coupling", 1e-5),
        ("bending_twist", 2e-5),
    ] {
        m["record"]["properties"][key] = json!({"value":value,"unit":"N*m","source":"test",
            "evidence":"estimate","method":"Synthetic orthotropic panel bending coefficient"});
    }
    m
}

#[test]
fn panel_bending_selection_reaches_sewn_dynamics_with_fixed_geometry_and_membrane() {
    let mut raw = band().raw().clone();
    raw["fabric"]["material"] = panel_sheet(2e-4, 5e-5);
    let p = Pattern::new(raw.clone());
    let opts = DrapeOptions {
        spacing: 8.0,
        ..options()
    };
    let a = run(&p, opts);
    raw["fabric"]["material"] = panel_sheet(5e-5, 2e-4);
    let b = run(&Pattern::new(raw), opts);
    assert_eq!(a.scene.mesh.verts2d, b.scene.mesh.verts2d);
    assert_eq!(a.scene.mesh.faces, b.scene.mesh.faces);
    assert_eq!(a.scene.weld, b.scene.weld);
    assert_eq!(a.scene.start.relaxed, b.scene.start.relaxed);
    assert_eq!(a.scene.solver.model.mass, b.scene.solver.model.mass);
    assert_eq!(
        a.scene.solver.model.membranes,
        b.scene.solver.model.membranes
    );
    assert_eq!(a.scene.solver.model.membrane, b.scene.solver.model.membrane);
    assert_ne!(a.scene.solver.x, b.scene.solver.x);
    let m = &a.scene.solver.model;
    assert_eq!(m.panel_bending_patches().len(), m.membranes.len());
    assert_eq!(a.report["panel_bending"]["patch_count"], m.membranes.len());
    assert!(
        a.report["panel_bending"]["active_scalar_hinges"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert_ne!(
        a.report["material"]["record_sha256"],
        b.report["material"]["record_sha256"]
    );
    let repeat = run(&p, opts);
    assert_eq!(repeat.scene.solver.x, a.scene.solver.x);
    assert_eq!(repeat.report, a.report);
}

#[test]
fn only_selected_source_panels_get_patches_and_scalar_seams_are_retained() {
    let mut raw = band().raw().clone();
    raw["fabric"]["material"] = panel_sheet(2e-4, 5e-5);
    raw["pieces"][1]["fabric"] = json!({"material":sheet(1.0)});
    let r = run(
        &Pattern::new(raw),
        DrapeOptions {
            spacing: 8.0,
            ..options()
        },
    );
    let m = &r.scene.solver.model;
    assert!(m.is_heterogeneous()); // only bending differs
    assert!(m.panel_bending_patches().len() < m.membranes.len());
    assert!(!m.panel_bending_patches().is_empty());
    for &i in m.panel_bending_triangles() {
        assert_eq!(r.scene.triangle_owner[i], 0);
    }
    assert_eq!(r.report["material"]["panel_bending_piece_count"], 1);
    let weld = &r.scene.weld;
    let ownership: Vec<_> = (0..m.mass.len())
        .map(|v| {
            r.scene
                .mesh
                .owner
                .iter()
                .zip(weld)
                .filter_map(|(&o, &w)| (w == v).then_some(o))
                .collect::<std::collections::BTreeSet<_>>()
        })
        .collect();
    // Curvature support on the selected front may include seam vertices, but no interior back vertex.
    for p in m.panel_bending_patches() {
        for &v in &p.v {
            assert!(ownership[v].contains(&0));
        }
    }
}

#[test]
fn tensor_default_and_same_explicit_piece_selection_are_identical_with_self_contact() {
    let mut raw = band().raw().clone();
    raw["fabric"]["material"] = panel_sheet(2e-4, 5e-5);
    let opts = DrapeOptions {
        spacing: 8.0,
        self_contact: Some(DrapeOptions::default().contact),
        ..options()
    };
    let a = run(&Pattern::new(raw.clone()), opts);
    raw["pieces"][1]["fabric"] = raw["fabric"].clone();
    let b = run(&Pattern::new(raw), opts);
    assert_eq!(a.scene.solver.x, b.scene.solver.x);
    assert_eq!(a.scene.solver.v, b.scene.solver.v);
    assert_eq!(a.fold.frames, b.fold.frames);
    assert_eq!(
        a.scene.solver.model.panel_bending_triangles(),
        b.scene.solver.model.panel_bending_triangles()
    );
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
    let mut expected_reactions = contact.reaction_map(&mixed.scene.solver.x).unwrap();
    expected_reactions["material_report_sha256"] = json!(ftx_core::receipt::sha256_hex(
        &serde_json::to_vec(&mixed.report["material"]).unwrap()
    ));
    assert_eq!(
        mixed.report["contact_diagnostics"]["final_reactions"],
        json!({"status":"available","result":expected_reactions})
    );
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
    for (name, value) in [("thickness", -1.2), ("friction_skin", -0.5)] {
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

fn interface_pattern(raw: Value, mu: f64) -> Pattern {
    let p = Pattern::new(raw);
    let r = resolve(&p, &options()).unwrap().report;
    let mut raw = p.raw().clone();
    raw["fabric"]["interfaces"] = json!([{
        "materials":[r["pieces"][0]["record_sha256"], r["pieces"][1]["record_sha256"]],
        "friction":{"value":mu, "unit":"1", "source":"pair", "evidence":"estimate",
            "method":"Synthetic cross-sheet coefficient", "bounds":[mu,mu],
            "conditions":{"orientation":"symmetric scalar, both sides"}},
        "sources":{"pair":{"reference":"test:cross-sheet", "accessed_on":"2026-10-13",
            "note":"Synthetic interface, not a textile measurement"}},
        "allow_estimates":true
    }]);
    Pattern::new(raw)
}

fn interface_substitution() -> (Pattern, Changes) {
    let source = interface_pattern(assigned(2.0).raw().clone(), 0.12);
    let mut replacement = assigned(2.0).raw().clone();
    replacement["fabric"]["material"] = sheet(3.0);
    let replacement = interface_pattern(replacement, 0.18);
    let changes = serde_json::from_value(json!({
        "schema":"ftx-material-bindings/1", "default":sheet(3.0),
        "interfaces":replacement.raw()["fabric"]["interfaces"]
    }))
    .unwrap();
    (source, changes)
}

fn interface_scenario() -> ftx_drape::study::Scenario {
    serde_json::from_value(json!({
        "schema":"ftx-drape-scenario/1", "fit_spec":{},
        "options":DrapeOptions { self_contact:Some(options().contact), ..options() },
        "max_mesh_points":4096, "max_vertex_frames":1000000
    }))
    .unwrap()
}

#[test]
fn atomic_material_and_pair_replacement_reaches_sewn_drape_and_scenario() {
    let (source, changes) = interface_substitution();
    let original = source.raw().clone();
    let receipt = changes.apply(&source).unwrap();
    let selected = Pattern::from_json(receipt["pattern_json"].as_str().unwrap()).unwrap();
    assert_eq!(selected.raw(), &receipt["pattern"]);
    assert_eq!(receipt["geometry_changed"], false);
    assert_eq!(selected.raw()["seams"], original["seams"]);
    for (before, after) in original["pieces"]
        .as_array()
        .unwrap()
        .iter()
        .zip(selected.raw()["pieces"].as_array().unwrap())
    {
        for (key, value) in before.as_object().unwrap() {
            if key != "fabric" {
                assert_eq!(after[key], *value, "piece geometry/metadata {key}");
            }
        }
    }

    let scenario = interface_scenario();
    let before = run(&source, scenario.options);
    let after = run(&selected, scenario.options);
    assert_eq!(before.scene.mesh.verts2d, after.scene.mesh.verts2d);
    assert_eq!(before.scene.mesh.faces, after.scene.mesh.faces);
    assert_eq!(before.scene.weld, after.scene.weld);
    assert_eq!(before.scene.start.relaxed, after.scene.start.relaxed);
    assert!(after.scene.weld.len() > after.scene.solver.model.mass.len());
    let ratio = after.scene.solver.model.mass.iter().sum::<f64>()
        / before.scene.solver.model.mass.iter().sum::<f64>();
    assert!((ratio - 5.0 / 3.0).abs() < 1e-12);
    assert_ne!(before.scene.solver.x, after.scene.solver.x);
    let material = &after.report["material"];
    assert_ne!(
        before.report["material"]["pieces"][0]["record_sha256"],
        material["pieces"][0]["record_sha256"]
    );
    assert_eq!(
        before.report["material"]["pieces"][1]["record_sha256"],
        material["pieces"][1]["record_sha256"]
    );
    let order = material["contact_material_order"].as_array().unwrap();
    let ids: Vec<_> = material["pieces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| order.iter().position(|h| *h == p["record_sha256"]).unwrap())
        .collect();
    assert_eq!(material["self_friction_matrix"][ids[0]][ids[1]], 0.18);
    assert_eq!(material["self_friction_matrix"][ids[1]][ids[0]], 0.18);
    for field in ["friction", "sources", "allow_estimates"] {
        assert_eq!(
            material["interfaces"][0][field],
            receipt["pattern"]["fabric"]["interfaces"][0][field]
        );
    }
    assert!(
        after
            .scene
            .solver
            .contact
            .as_ref()
            .unwrap()
            .has_surface_materials()
    );

    let form = body();
    let body = DrapeBody {
        frames: &Form,
        collision: (&form.vertices, &form.triangles),
        display: (&form.vertices, &form.triangles),
        arm_deg: 35.,
    };
    let result = ftx_drape::study::Session::new(&scenario, &body)
        .unwrap()
        .evaluate(&selected, |_| {})
        .unwrap();
    assert_eq!(result["drape"], after.report);
    assert_eq!(result["positions_m"], json!(after.scene.solver.x));
    assert_eq!(changes.apply(&source).unwrap(), receipt);
    assert_eq!(source.raw(), &original);
}

#[test]
fn material_substitution_never_reuses_stale_or_unadmitted_pair_friction() {
    let (source, changes) = interface_substitution();
    let original = source.raw().clone();
    let scenario = interface_scenario();
    let form = body();
    let body = DrapeBody {
        frames: &Form,
        collision: (&form.vertices, &form.triangles),
        display: (&form.vertices, &form.triangles),
        arm_deg: 35.,
    };
    for (mode, expected) in [
        (0, "unassigned or stale"),
        (1, "pairwise friction"),
        (2, "own explicit consent"),
    ] {
        let mut candidate = changes.clone();
        match mode {
            0 => candidate.interfaces = None,
            1 => candidate.interfaces = Some(vec![]),
            _ => candidate.interfaces.as_mut().unwrap()[0]["allow_estimates"] = json!(false),
        }
        // All three operations are useful for non-contact consumers and remain bindable.
        let bound = candidate.apply(&source).unwrap();
        if mode == 0 {
            assert_eq!(
                bound["pattern"]["fabric"]["interfaces"],
                original["fabric"]["interfaces"]
            );
        }
        let mut started = false;
        let refusal = ftx_drape::study::Session::new(&scenario, &body)
            .unwrap()
            .evaluate(&Pattern::new(bound["pattern"].clone()), |_| started = true)
            .unwrap_err();
        assert_eq!(refusal.code, ftx_core::RefusalCode::InvalidDocument);
        assert!(refusal.message.contains(expected), "{}", refusal.message);
        assert!(
            !started,
            "invalid contact evidence must refuse before solver work"
        );
    }
    let mut stale = changes;
    stale.interfaces.as_mut().unwrap()[0]["materials"][0] = json!("0".repeat(64));
    assert!(stale.apply(&source).is_err());
    assert_eq!(source.raw(), &original);
}

#[test]
fn piece_surface_field_reaches_contact_and_survives_checked_body_motion() {
    let mut raw = assigned(2.0).raw().clone();
    let props = &mut raw["pieces"][1]["fabric"]["material"]["record"]["properties"];
    props["thickness"]["value"] = json!(1.2);
    props["friction_skin"]["value"] = json!(0.7);
    props["friction_self"]["value"] = json!(0.9);
    let p = interface_pattern(raw, 0.12);
    let opts = DrapeOptions {
        self_contact: Some(options().contact),
        ..options()
    };
    let mut r = run(&p, opts);
    let mat = &r.report["material"];
    let order = mat["contact_material_order"].as_array().unwrap();
    let ids: Vec<_> = mat["pieces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| order.iter().position(|h| *h == p["record_sha256"]).unwrap())
        .collect();
    let f = &mat["self_friction_matrix"];
    assert_eq!(f[ids[0]][ids[0]], 0.3);
    assert_eq!(f[ids[1]][ids[1]], 0.9);
    assert_eq!(f[ids[0]][ids[1]], 0.12);
    assert_eq!(f[ids[1]][ids[0]], 0.12);
    let c = r.scene.solver.contact.as_ref().unwrap();
    assert!(c.has_surface_materials());
    assert_eq!(c.params.thickness, 0.0006);
    assert_eq!(c.params.mu, 0.7);
    assert_eq!(c.self_contact_params().unwrap().thickness, 0.0012);
    assert_eq!(r.report["self_contact"]["friction_mu"], 0.9); // explicitly an envelope, not mu_AB
    assert!(
        r.report["self_contact"]["coefficient_basis"]
            .as_str()
            .unwrap()
            .contains("upper envelopes")
    );
    let target: Vec<_> = c
        .body
        .vertices
        .iter()
        .map(|p| *p + Vec3::new(0., 0., 0.00001))
        .collect();
    let mass = r.scene.solver.model.mass.clone();
    let laws: Vec<_> = (0..r.scene.triangle_owner.len())
        .map(|i| r.scene.solver.model.material(i))
        .collect();
    let before = format!("{:?}", r.scene.solver.contact);
    let original_x = r.scene.solver.x.clone();
    let err = ftx_drape::motion::advance_body(
        &mut r.scene.solver,
        &target,
        0.001,
        ftx_drape::motion::MotionOptions {
            max_sample_vertices: 1,
            ..Default::default()
        },
    )
    .unwrap_err();
    assert_eq!(err.code, ftx_core::RefusalCode::BudgetExceeded);
    assert_eq!(format!("{:?}", r.scene.solver.contact), before);
    assert_eq!(r.scene.solver.x, original_x);
    let motion =
        ftx_drape::motion::advance_body(&mut r.scene.solver, &target, 0.001, Default::default())
            .unwrap();
    assert!(!motion.samples.is_empty());
    let c = r.scene.solver.contact.as_ref().unwrap();
    assert!(c.has_surface_materials());
    assert_eq!(c.body.vertices, target);
    c.audit_self_contact(&r.scene.solver.x).unwrap();
    assert_eq!(r.scene.solver.model.mass, mass);
    for (i, law) in laws.into_iter().enumerate() {
        assert_eq!(r.scene.solver.model.material(i), law);
    }
}

#[test]
fn interface_source_locks_coverage_and_consent_cannot_be_silently_repaired() {
    let p = interface_pattern(assigned(2.0).raw().clone(), 0.12);
    let opts = DrapeOptions {
        self_contact: Some(options().contact),
        ..options()
    };
    let original = resolve(&p, &opts).unwrap().report;
    let mut reversed = p.raw().clone();
    reversed["fabric"]["interfaces"][0]["materials"]
        .as_array_mut()
        .unwrap()
        .reverse();
    assert_eq!(
        resolve(&Pattern::new(reversed), &opts).unwrap().report,
        original
    );
    for i in 0..12 {
        let mut raw = p.raw().clone();
        let pair = &mut raw["fabric"]["interfaces"][0];
        match i {
            0 => pair["friction"]["unit"] = json!("N/m"),
            1 => pair["friction"]["value"] = json!(-0.1),
            2 => pair["friction"]["source"] = json!("absent"),
            3 => pair["sources"]["pair"]["reference"] = json!(""),
            4 => pair["allow_estimates"] = json!(false),
            5 => pair["allow_estimates"] = json!("true"),
            6 => pair["materials"][0] = json!("test-sheet"), // display ID is not a snapshot lock
            7 => pair["friction"]["bounds"] = json!([0.3, 0.4]),
            8 => pair["friction"]["method"] = json!(" "),
            9 => pair["materials"][0] = pair["materials"][1].clone(),
            10 => pair["invented_field"] = json!(1),
            _ => pair["friction"]["conditions"] = json!({"": "unspecified"}),
        }
        for opts in [&opts, &options()] {
            assert!(
                resolve(&Pattern::new(raw.clone()), opts).is_err(),
                "case {i}"
            );
        }
    }
    let mut stale = p.raw().clone();
    stale["pieces"][1]["fabric"]["material"]["record"]["properties"]["warp_stiffness"]["value"] =
        json!(800.);
    assert!(resolve(&Pattern::new(stale), &opts).is_err());
    let mut duplicate = p.raw().clone();
    let second = duplicate["fabric"]["interfaces"][0].clone();
    duplicate["fabric"]["interfaces"]
        .as_array_mut()
        .unwrap()
        .push(second);
    assert!(resolve(&Pattern::new(duplicate), &opts).is_err());
    let mut third = p.raw().clone();
    let mut piece = third["pieces"][1].clone();
    piece["id"] = json!("lining");
    piece["fabric"]["material"] = sheet(3.0);
    third["pieces"].as_array_mut().unwrap().push(piece);
    assert!(
        resolve(&Pattern::new(third), &opts).is_err(),
        "all unlike pairs need evidence"
    );
}

#[test]
fn explicit_cross_friction_changes_slip_on_the_assembled_two_sheet_model() {
    fn sliding(mu: f64) -> ftx_drape::Solver {
        let mut raw = assigned(2.0).raw().clone();
        raw["seams"] = json!([]);
        for (i, piece) in raw["pieces"].as_array_mut().unwrap().iter_mut().enumerate() {
            piece["vertices"] = json!({"a":[0,0],"b":[20,0],"c":[20,20],"d":[0,20]});
            piece["placement"]["theta_deg"] = json!(0);
            piece["placement"]["x0"] = json!(10);
            piece["placement"]["radius_mm"] = json!(if i == 0 { 40. } else { 41.3 });
        }
        let p = interface_pattern(raw, mu);
        let r = run(
            &p,
            DrapeOptions {
                self_contact: Some(options().contact),
                spacing: 10.,
                ..options()
            },
        );
        let fixed: Vec<_> = r
            .scene
            .mesh
            .owner
            .iter()
            .map(|&o| [o == 0, true, true])
            .collect();
        // Start both comparisons inside the activation band. The preceding free drape can
        // already separate unloaded layers, leaving no contact traction to test.
        let mut x = r.scene.solver.x;
        for (v, &w) in r.scene.weld.iter().enumerate() {
            x[w] = r.scene.start.relaxed[v];
        }
        let mut solver = ftx_drape::Solver::new(r.scene.solver.model, x, &fixed, Vec3::ZERO);
        solver.contact = r.scene.solver.contact;
        solver = solver.with_self_contact(options().contact).unwrap();
        solver.tol = 1e-8;
        for (i, v) in solver.v.iter_mut().enumerate() {
            if r.scene.mesh.owner[i] == 1 {
                v.x = 0.01;
            }
        }
        solver
    }
    let mut frictionless = sliding(0.0);
    let mut frictional = sliding(0.8);
    assert_eq!(frictionless.x, frictional.x);
    assert_eq!(frictionless.model.mass, frictional.model.mass);
    assert!(
        frictionless
            .contact
            .as_ref()
            .unwrap()
            .min_self_gap(&frictionless.x, 0.001)
            .unwrap()
            < 0.001
    );
    let x0: f64 = frictionless.x.iter().map(|v| v.x).sum();
    let y0: f64 = frictional.x.iter().map(|v| v.x).sum();
    for _ in 0..5 {
        assert!(frictionless.step(0.001).unwrap().converged);
        assert!(frictional.step(0.001).unwrap().converged);
        frictional
            .contact
            .as_ref()
            .unwrap()
            .audit_self_contact(&frictional.x)
            .unwrap();
    }
    let no_friction = frictionless.x.iter().map(|v| v.x).sum::<f64>() - x0;
    let friction = frictional.x.iter().map(|v| v.x).sum::<f64>() - y0;
    assert!(
        no_friction > 0.0 && friction.abs() < no_friction * 0.8,
        "{no_friction} vs {friction}"
    );
}

#[path = "piece_materials/friction_acquisition.rs"]
mod friction_acquisition;

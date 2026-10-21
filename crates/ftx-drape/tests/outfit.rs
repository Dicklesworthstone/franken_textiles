//! Joint cloth dynamics and source isolation. All specimens and bodies here are synthetic.
use ftx_core::{Vec3, det};
use ftx_drape::outfit::{Garment, Input, SCHEMA, prepare};
use ftx_drape::run::{DrapeBody, DrapeOptions};
use ftx_drape::scene::PartFrames;
use ftx_drape::{Body, PartFrame};
use ftx_pattern::textile_import::resolve_fabric;
use serde_json::{Value, json};

fn sheet() -> Value {
    let raw: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-friction-sliding.json"
    ))
    .unwrap();
    raw["target"]["materials"][0].clone()
}
fn panel(radius: f64) -> Value {
    json!({"schema":"fabric-pattern/1","units":"mm","style":"SYNTHETIC",
        "fabric":{"material":sheet()},"seams":[],"pieces":[{
            "id":"panel","material":"shell label","cut":{"qty":1},
            "vertices":{"a":[0,0],"b":[20,0],"c":[20,20],"d":[0,20]},
            "edges":[{"id":"bottom","from":"a","to":"b"},{"id":"right","from":"b","to":"c"},
                {"id":"top","from":"c","to":"d"},{"id":"left","from":"d","to":"a"}],
            "grain":[[0,0],[0,20]],"placement":{"part":"torso","radius_mm":radius,"x0":10}}]})
}
fn input() -> Input {
    Input {
        schema: SCHEMA.into(),
        garments: vec![
            Garment {
                id: "inner".into(),
                pattern: panel(40.0),
            },
            Garment {
                id: "outer".into(),
                pattern: panel(41.3),
            },
        ],
        interfaces: vec![],
    }
}
fn options() -> DrapeOptions {
    DrapeOptions {
        spacing: 10.0,
        sew_steps: 3,
        drape_steps: 3,
        step: 0.001,
        self_contact: Some(DrapeOptions::default().contact),
        ..DrapeOptions::default()
    }
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
fn skin() -> Body {
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
fn body(b: &Body) -> DrapeBody<'_> {
    DrapeBody {
        frames: &Form,
        collision: (&b.vertices, &b.triangles),
        display: (&b.vertices, &b.triangles),
        arm_deg: 35.,
    }
}

#[test]
fn joint_run_preserves_source_geometry_materials_mass_and_independent_ownership() {
    let input = input();
    let before = serde_json::to_vec(&input).unwrap();
    let composed = prepare(&input, &options()).unwrap();
    let p = composed.pattern().raw();
    assert_eq!(p["pieces"][0]["id"], "inner::panel");
    assert_eq!(p["pieces"][1]["id"], "outer::panel");
    for (i, g) in input.garments.iter().enumerate() {
        for field in ["vertices", "edges", "grain", "placement", "cut"] {
            assert_eq!(g.pattern["pieces"][0][field], p["pieces"][i][field]);
        }
        assert!(p["pieces"][i]["fabric"]["material"]["expected_sha256"].is_string());
    }
    let skin = skin();
    let run = composed
        .drape(&json!({}), &body(&skin), &options(), |_| {})
        .unwrap();
    assert_eq!(run.scene.mesh.instances, ["inner::panel", "outer::panel"]);
    assert_eq!(run.report["outfit"]["fit_claim"], false);
    assert!(
        run.report["outfit"]["triangle_garment"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == 1)
    );
    let masses = run.report["outfit"]["garment_mass_kg"].as_array().unwrap();
    assert!((masses[0].as_f64().unwrap() - 0.00008).abs() < 1e-12);
    assert!((masses[1].as_f64().unwrap() - 0.00008).abs() < 1e-12);
    assert!((run.report["mass_kg"].as_f64().unwrap() - 0.00016).abs() < 1e-12);
    run.scene
        .solver
        .contact
        .as_ref()
        .unwrap()
        .audit_self_contact(&run.scene.solver.x)
        .unwrap();
    assert_eq!(before, serde_json::to_vec(&input).unwrap());
    let repeat = composed
        .drape(&json!({}), &body(&skin), &options(), |_| {})
        .unwrap();
    assert_eq!(run.scene.solver.x, repeat.scene.solver.x);
    assert_eq!(run.report, repeat.report);
}

#[test]
fn duplicate_source_names_mirror_and_fold_references_stay_with_their_own_garment() {
    let mut input = input();
    for g in &mut input.garments {
        g.pattern["pieces"][0]["cut"]["qty"] = json!(2);
        let mut folded = g.pattern["pieces"][0].clone();
        folded["id"] = json!("fold");
        folded["cut"] = json!({"qty":1,"on_fold":"left"});
        g.pattern["pieces"].as_array_mut().unwrap().push(folded);
        g.pattern["seams"] = json!([
            {"id":"join","a":"panel.R/top","b":"panel.L/bottom","reverse":false,"ratio":1},
            {"id":"join-fold","a":"fold/bottom.R+bottom.L","b":"panel.R/right","sim":false}
        ]);
    }
    // Use a single unfolded half-edge so that its reference is a valid consecutive path.
    for g in &mut input.garments {
        g.pattern["seams"][1]["a"] = json!("fold/top.R");
    }
    let composed = prepare(&input, &options()).unwrap();
    let p = composed.pattern();
    assert_eq!(p.raw()["seams"][0]["a"], "inner::panel.R/top");
    assert_eq!(p.raw()["seams"][2]["a"], "outer::panel.R/top");
    assert_eq!(p.raw()["seams"][0]["reverse"], false);
    assert_eq!(p.raw()["seams"][1]["a"], "inner::fold/top.R");
    for s in p.seams() {
        let (a, _) = p.side(s.side_ref("a").unwrap()).unwrap();
        let (b, _) = p.side(s.side_ref("b").unwrap()).unwrap();
        assert_eq!(a.piece.split("::").next(), b.piece.split("::").next());
    }
}

fn pair(input: &Input, mu: f64) -> Value {
    let hashes: Vec<_> = input
        .garments
        .iter()
        .map(|g| {
            resolve_fabric(&g.pattern["fabric"])
                .unwrap()
                .unwrap()
                .sha256()
                .to_owned()
        })
        .collect();
    json!({"materials":hashes,"friction":{"value":mu,"unit":"1","source":"test","evidence":"estimate","method":"Synthetic pair, not measured"},
        "sources":{"test":{"reference":"test:outfit-pair","accessed_on":"2026-10-20","note":"Synthetic friction only"}},"allow_estimates":true})
}

#[test]
fn unlike_garments_need_exact_pair_evidence_and_never_inherit_a_global_coefficient() {
    let mut input = input();
    input.garments[1].pattern["fabric"]["material"]["record"]["properties"]["areal_density"]["value"] =
        json!(0.4);
    assert!(
        prepare(&input, &options())
            .err()
            .unwrap()
            .message
            .contains("pairwise")
    );
    input.interfaces.push(pair(&input, 0.12));
    let p = prepare(&input, &options()).unwrap();
    let resolved = ftx_drape::material::resolve(p.pattern(), &options()).unwrap();
    assert_eq!(resolved.report["self_friction_matrix"][0][1], 0.12);
    assert_eq!(resolved.report["self_friction_matrix"][1][0], 0.12);
    input.garments[1].pattern["fabric"]["material"]["record"]["properties"]["areal_density"]["value"] =
        json!(0.5);
    assert!(prepare(&input, &options()).is_err());
}

#[test]
fn joint_contact_changes_inter_garment_slip_without_changing_geometry_or_mass() {
    let skin = skin();
    let solve = |mu| {
        let mut input = input();
        // Distinct source records allow changing only the cross-interface coefficient.
        input.garments[1].pattern["fabric"]["material"]["id"] = json!("second");
        input.garments[1].pattern["fabric"]["material"]["record"]["id"] = json!("second");
        input.interfaces.push(pair(&input, mu));
        let prepared = prepare(&input, &options()).unwrap();
        let r = prepared
            .drape(&json!({}), &body(&skin), &options(), |_| {})
            .unwrap();
        let fixed: Vec<_> = r
            .scene
            .mesh
            .owner
            .iter()
            .map(|&o| [o == 0, true, true])
            .collect();
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
    };
    let mut a = solve(0.0);
    let mut b = solve(0.8);
    assert_eq!(a.x, b.x);
    assert_eq!(a.v, b.v);
    assert_eq!(a.model.mass, b.model.mass);
    assert_eq!(a.model.membranes, b.model.membranes);
    let start: f64 = a.x.iter().map(|v| v.x).sum();
    for _ in 0..5 {
        assert!(a.step(0.001).unwrap().converged);
        assert!(b.step(0.001).unwrap().converged);
        b.contact
            .as_ref()
            .unwrap()
            .audit_self_contact(&b.x)
            .unwrap();
    }
    let free = a.x.iter().map(|v| v.x).sum::<f64>() - start;
    let drag = b.x.iter().map(|v| v.x).sum::<f64>() - start;
    assert!(free > 0.0 && drag.abs() < free * 0.8, "{free} vs {drag}");
}

#[test]
fn missing_physics_cannot_turn_into_a_successful_frozen_overlay() {
    let mut options = options();
    options.self_contact = None;
    assert!(
        prepare(&input(), &options)
            .err()
            .unwrap()
            .message
            .contains("self-contact")
    );
    for change in [
        (|i: &mut Input| i.garments[1].id = i.garments[0].id.clone()) as fn(&mut Input),
        |i| i.garments[0].pattern["pieces"][0]["placement"] = Value::Null,
        |i| i.garments[0].pattern["fabric"] = json!({"areal_g_m2":200}),
        |i| i.garments[0].pattern["meta"] = json!({"solved":{"elastic_cut_mm":100}}),
        |i| i.garments[0].pattern["attached_mass"] = json!([]),
        |i| {
            i.garments[0].pattern["seams"] =
                json!([{"id":"cross","a":"outer::panel/top","b":"panel/bottom"}])
        },
        |i| i.garments[0].pattern["pieces"][0]["sim"] = json!(false),
        |i| i.schema = "wrong".into(),
    ] {
        let mut i = input();
        change(&mut i);
        assert!(
            prepare(
                &i,
                &DrapeOptions {
                    self_contact: Some(options.contact),
                    ..options
                }
            )
            .is_err()
        );
    }
}

#[test]
fn coincident_garment_starts_refuse_instead_of_inventing_layer_offsets() {
    let mut input = input();
    input.garments[1].pattern = input.garments[0].pattern.clone();
    let prepared = prepare(&input, &options()).unwrap();
    let skin = skin();
    assert!(
        prepared
            .drape(&json!({}), &body(&skin), &options(), |_| {})
            .is_err()
    );
}

#[test]
fn two_sewn_bands_weld_independently_and_continue_on_the_same_moving_body() {
    let mut input = input();
    for (i, g) in input.garments.iter_mut().enumerate() {
        let radius = if i == 0 { 40.0 } else { 44.0 };
        let width = radius * std::f64::consts::PI;
        let mut front = g.pattern["pieces"][0].clone();
        front["id"] = json!("front");
        front["vertices"] = json!({"a":[0,0],"b":[width,0],"c":[width,20],"d":[0,20]});
        front["placement"] =
            json!({"part":"torso","radius_mm":radius,"x0":width/2.0,"theta_deg":0});
        let mut back = front.clone();
        back["id"] = json!("back");
        back["placement"]["theta_deg"] = json!(180);
        g.pattern["pieces"] = json!([front, back]);
        g.pattern["seams"] = json!([
            {"id":"right","a":"front/right","b":"back/left"},
            {"id":"left","a":"back/right","b":"front/left"}
        ]);
    }
    let opts = DrapeOptions {
        spacing: 24.0,
        ..options()
    };
    let prepared = prepare(&input, &opts).unwrap();
    let skin = skin();
    let mut joint = prepared
        .drape(&json!({}), &body(&skin), &opts, |_| {})
        .unwrap();
    assert!(joint.scene.weld.len() > joint.scene.solver.model.mass.len());
    assert_eq!(prepared.pattern().seams().len(), 4);
    let masses = joint.report["outfit"]["garment_mass_kg"]
        .as_array()
        .unwrap();
    for (mass, radius) in masses.iter().zip([40.0, 44.0]) {
        let expected = std::f64::consts::TAU * radius * 20.0 * 1e-6 * 0.2;
        assert!((mass.as_f64().unwrap() - expected).abs() < 1e-12);
    }
    let before_mass = joint.scene.solver.model.mass.clone();
    let target: Vec<_> = skin
        .vertices
        .iter()
        .map(|v| *v + Vec3::new(0.0, 0.0, 0.00001))
        .collect();
    let motion = ftx_drape::motion::advance_body(
        &mut joint.scene.solver,
        &target,
        0.001,
        Default::default(),
    )
    .unwrap();
    assert!(!motion.samples.is_empty());
    assert_eq!(before_mass, joint.scene.solver.model.mass);
    let contact = joint.scene.solver.contact.as_ref().unwrap();
    assert_eq!(contact.body.vertices, target);
    contact.audit_self_contact(&joint.scene.solver.x).unwrap();
}

#[test]
fn shared_interface_evidence_is_coalesced_but_conflicting_sources_are_not_overwritten() {
    let mut input = input();
    input.garments[1].pattern["fabric"]["material"]["record"]["name"] = json!("Distinct specimen");
    let interface = pair(&input, 0.2);
    input.garments[0].pattern["fabric"]["interfaces"] = json!([interface]);
    input.garments[1].pattern["fabric"]["interfaces"] = json!([interface]);
    input.interfaces.push(interface.clone());
    let prepared = prepare(&input, &options()).unwrap();
    assert_eq!(
        prepared.pattern().raw()["fabric"]["interfaces"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    input.interfaces[0]["friction"]["value"] = json!(0.7);
    assert!(
        prepare(&input, &options())
            .err()
            .unwrap()
            .message
            .contains("conflicting")
    );
    input.interfaces = vec![interface.clone(), interface];
    assert!(
        prepare(&input, &options())
            .err()
            .unwrap()
            .message
            .contains("duplicate")
    );
}

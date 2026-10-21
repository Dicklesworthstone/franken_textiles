//! Joint cloth dynamics and source isolation. All specimens and bodies here are synthetic.
use ftx_core::{Vec3, det};
use ftx_drape::outfit::{Garment, Input, SCHEMA, prepare};
use ftx_drape::run::{DrapeBody, DrapeOptions};
use ftx_drape::scene::PartFrames;
use ftx_drape::{Body, PartFrame};
use ftx_pattern::textile_import::resolve_fabric;
use serde_json::{Value, json};

#[path = "outfit/material_bands.rs"]
mod material_bands;

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

fn bands() -> Input {
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
    input
}

fn casing_outfit() -> Input {
    let mut input = input();
    for (i, garment) in input.garments.iter_mut().enumerate() {
        let radius = [40.0, 44.0][i];
        let width = std::f64::consts::TAU * radius;
        // The inner rest is shorter than the 30 mm body's circumference; the outer is slack.
        let fraction = [0.7, 1.1][i];
        let piece = &mut garment.pattern["pieces"][0];
        piece["id"] = json!("waistband");
        piece["role"] = json!("waistband");
        piece["vertices"] = json!({"a":[0,0],"b":[width,0],"c":[width,20],"d":[0,20]});
        piece["edges"] = json!([
            {"id":"bottom","from":"a","to":"b"}, {"id":"end_r","from":"b","to":"c"},
            {"id":"top","from":"c","to":"d"}, {"id":"end_l","from":"d","to":"a"}
        ]);
        piece["placement"]["x0"] = json!(width / 2.0);
        piece["placement"]["radius_mm"] = json!(radius);
        piece["sim_scale"] = json!([0.9, 1]);
        garment.pattern["seams"] = json!([
            {"id":"close","a":"waistband/end_r","b":"waistband/end_l","reverse":true}
        ]);
        garment.pattern["meta"] = json!({"style":{"elastic_overlap":10.0},
            "solved":{"elastic_cut_mm":width*fraction+10.0}});
        garment.pattern["fabric"]["material"]["record"]["properties"]["friction_skin"]["value"] =
            json!([0.12, 0.6][i]);
    }
    input.interfaces.push(pair(&input, 0.2));
    input
}

#[test]
fn independent_casings_drive_joint_physics_and_replay_without_merging_grip_or_loads() {
    use ftx_drape::scene::{elastic_casing, elastic_casings};
    let mut input = casing_outfit();
    input.garments[1].pattern["attached_mass"] = attachment(
        0.003,
        json!([{"instance":"waistband","vertex":"b","fraction":1.0}]),
    );
    let before = serde_json::to_vec(&input).unwrap();
    let opts = DrapeOptions {
        spacing: 24.0,
        ..options()
    };
    let prepared = prepare(&input, &opts).unwrap();
    let casings = elastic_casings(prepared.pattern(), opts.elastic_ea).unwrap();
    assert_eq!(casings.len(), 2);
    for (i, casing) in casings.iter().enumerate() {
        let source = ftx_pattern::Pattern::new(input.garments[i].pattern.clone());
        assert_eq!(
            casing.length.to_bits(),
            elastic_casing(&source, opts.elastic_ea)
                .unwrap()
                .length
                .to_bits()
        );
        assert_eq!(
            casing.load_instances,
            [format!("{}::waistband", input.garments[i].id)]
        );
        assert_eq!(
            prepared.pattern().raw()["pieces"][i]["sim_scale"],
            json!([0.9, 1])
        );
    }
    let skin = skin();
    let run = prepared
        .drape(&json!({}), &body(&skin), &opts, |_| {})
        .unwrap();
    assert_eq!(run.scene.elastic_rings.len(), 4);
    assert_eq!(run.scene.elastic_groups.len(), 2);
    assert!(run.report.get("elastic").is_none());
    let rows = run.report["elastics"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    for (i, group) in run.scene.elastic_groups.iter().enumerate() {
        assert_eq!(group.rings.len(), 2);
        for ring in &run.scene.elastic_rings[group.rings.clone()] {
            let length: f64 = run.scene.solver.model.elastics[ring.clone()]
                .iter()
                .map(|s| s.rest)
                .sum();
            assert!((length - casings[i].length).abs() < 1e-12);
        }
        let expected = std::f64::consts::TAU * [40.0, 44.0][i] * 20.0 * 1e-6 * 0.2
            + if i == 1 { 0.003 } else { 0.0 };
        let actual: f64 = group
            .loaded_vertices
            .iter()
            .map(|&v| run.scene.solver.model.mass[v])
            .sum();
        assert!((actual - expected).abs() < 1e-12);
        assert_eq!(rows[i]["friction_mu"], json!([0.12, 0.6][i]));
        assert_eq!(
            rows[i]["weight_n"],
            json!(ftx_core::py::round(expected * 9.81, 3))
        );
        assert_eq!(rows[i]["instance"], casings[i].instance);
        assert_eq!(rows[i]["ea_n"], opts.elastic_ea);
    }
    assert!(run.report["elastics"][0]["tension_n"].as_f64().unwrap() > 0.0);
    assert_eq!(run.report["elastics"][1]["tension_n"], 0.0);
    assert!(
        run.scene.elastic_groups[0]
            .loaded_vertices
            .iter()
            .all(|v| !run.scene.elastic_groups[1].loaded_vertices.contains(v))
    );
    let restored = ftx_pattern::Pattern::new(
        serde_json::from_slice(&serde_json::to_vec(prepared.pattern().raw()).unwrap()).unwrap(),
    );
    let replay = ftx_drape::run::drape(&restored, &json!({}), &body(&skin), &opts, |_| {}).unwrap();
    assert_eq!(run.scene.solver.x, replay.scene.solver.x);
    assert_eq!(
        run.scene.solver.model.elastics,
        replay.scene.solver.model.elastics
    );
    assert_eq!(run.report["elastics"], replay.report["elastics"]);
    assert_eq!(before, serde_json::to_vec(&input).unwrap());
    let disabled = DrapeOptions {
        elastic_ea: 0.0,
        ..opts
    };
    // The authored .9 scale no longer closes this synthetic fixed-radius band when no elastic
    // restores its cut geometry. Keep that refusal; do not untangle it or bypass contact checks.
    assert_eq!(
        prepared
            .drape(&json!({}), &body(&skin), &disabled, |_| {})
            .err()
            .unwrap()
            .code,
        ftx_core::RefusalCode::IntersectingStart
    );
    let mut cut_geometry = input.clone();
    for garment in &mut cut_geometry.garments {
        garment.pattern["pieces"][0]["sim_scale"] = json!([1, 1]);
    }
    let no_elastic = prepare(&cut_geometry, &disabled)
        .unwrap()
        .drape(&json!({}), &body(&skin), &disabled, |_| {})
        .unwrap();
    assert!(no_elastic.scene.solver.model.elastics.is_empty());
    assert!(no_elastic.scene.elastic_groups.is_empty());
    assert_ne!(run.scene.solver.x, no_elastic.scene.solver.x);
}

#[test]
fn legacy_single_casing_retains_its_physics_and_singular_report() {
    let input = casing_outfit();
    let pattern = ftx_pattern::Pattern::new(input.garments[0].pattern.clone());
    let opts = DrapeOptions {
        spacing: 24.0,
        ..options()
    };
    let skin = skin();
    let legacy = ftx_drape::run::drape(&pattern, &json!({}), &body(&skin), &opts, |_| {}).unwrap();
    assert!(legacy.report["elastic"]["tension_n"].as_f64().unwrap() > 0.0);
    assert!(legacy.report.get("elastics").is_none());
    let c = ftx_drape::scene::elastic_casing(&pattern, opts.elastic_ea).unwrap();
    let mut explicit = pattern.raw().clone();
    explicit["meta"]["solved"] = json!({"elastic_casings":[{
        "instance":c.instance,"relaxed_m":c.length,"load_instances":c.load_instances}]});
    let normalized = ftx_drape::run::drape(
        &ftx_pattern::Pattern::new(explicit),
        &json!({}),
        &body(&skin),
        &opts,
        |_| {},
    )
    .unwrap();
    assert_eq!(legacy.scene.solver.x, normalized.scene.solver.x);
    assert_eq!(
        legacy.scene.solver.model.elastics,
        normalized.scene.solver.model.elastics
    );
    for field in [
        "relaxed_mm",
        "length_mm",
        "rings_mm",
        "stretch_pct",
        "tension_n",
        "grip_n",
        "weight_n",
        "holds",
        "ea_n",
    ] {
        assert_eq!(
            legacy.report["elastic"][field], normalized.report["elastics"][0][field],
            "{field}"
        );
    }
    // The uniform legacy report did not expose this additional field; preserve its wire shape.
    assert!(legacy.report["elastic"].get("friction_mu").is_none());
    assert_eq!(normalized.report["elastics"][0]["friction_mu"], 0.12);
}

#[test]
fn casing_declarations_reject_ambiguous_unknown_duplicate_and_disabled_invalid_sources() {
    use ftx_drape::scene::elastic_casings;
    let prepared = prepare(&casing_outfit(), &options()).unwrap();
    for change in [
        (|v: &mut Value| v["meta"]["solved"]["elastic_cut_mm"] = json!(200)) as fn(&mut Value),
        |v| v["meta"]["solved"]["elastic_casings"][0]["relaxed_m"] = json!(-1),
        |v| v["meta"]["solved"]["elastic_casings"][0]["instance"] = json!("absent"),
        |v| {
            v["meta"]["solved"]["elastic_casings"][0]["load_instances"] =
                json!(["outer::waistband"])
        },
        |v| {
            v["meta"]["solved"]["elastic_casings"][1] =
                v["meta"]["solved"]["elastic_casings"][0].clone()
        },
        |v| v["pieces"][0]["sim"] = json!(false),
    ] {
        let mut raw = prepared.pattern().raw().clone();
        change(&mut raw);
        for ea in [0.0, 15.0] {
            assert!(elastic_casings(&ftx_pattern::Pattern::new(raw.clone()), ea).is_err());
        }
    }
}

#[test]
fn drafted_trousers_keep_their_original_casing_in_a_jacket_outfit() {
    let drafts: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/oracle/pattern/drafts.json"
    ))
    .unwrap();
    let mut input = input();
    for (garment, id) in input
        .garments
        .iter_mut()
        .zip(["zouave_bolero-920", "zouave_trousers-920"])
    {
        let case = drafts["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == id)
            .unwrap();
        garment.pattern = case["input"]["pattern"].clone();
        // An explicit synthetic material only for composition software, not these garments' textiles.
        garment.pattern["fabric"] = json!({"material":sheet()});
    }
    let prepared = prepare(&input, &options()).unwrap();
    let casings = ftx_drape::scene::elastic_casings(prepared.pattern(), 15.0).unwrap();
    assert_eq!(casings.len(), 1);
    assert!(casings[0].instance.starts_with("outer::"));
    assert!(
        casings[0]
            .load_instances
            .iter()
            .all(|i| i.starts_with("outer::"))
    );
    assert_eq!(
        prepared.receipt()["garments"][1]["meta"],
        input.garments[1].pattern["meta"]
    );
}

#[test]
fn two_sewn_bands_weld_independently_and_continue_on_the_same_moving_body() {
    let input = bands();
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

fn attachment(mass: f64, targets: Value) -> Value {
    json!({"schema":"ftx-attached-mass/1","groups":[{
        "extra":{"id":"ornament","mass":{"value":mass,"unit":"kg","source":"test",
            "evidence":"estimate","method":"Synthetic total accessory-group mass",
            "bounds":[mass*0.9,mass*1.1],"conditions":{"purpose":"software regression"}},
            "sources":{"test":{"reference":"test:outfit-mass","accessed_on":"2026-10-21",
                "note":"Synthetic, not a measured accessory or attachment-strength qualification"}},
            "allow_estimates":true},"targets":targets}]})
}

#[test]
fn accessory_evidence_and_mirrored_folded_targets_survive_namespacing_exactly() {
    let mut input = input();
    for (i, g) in input.garments.iter_mut().enumerate() {
        g.pattern["pieces"][0]["cut"]["qty"] = json!(2);
        let mut folded = g.pattern["pieces"][0].clone();
        folded["id"] = json!("fold");
        folded["cut"] = json!({"qty":1,"on_fold":"left"});
        g.pattern["pieces"].as_array_mut().unwrap().push(folded);
        g.pattern["attached_mass"] = attachment(
            0.003 * (i + 1) as f64,
            json!([{"instance":"panel.R","vertex":"b","fraction":0.25},
                {"instance":"panel.L","vertex":"b","fraction":0.25},
                {"instance":"fold","vertex":"b.L","fraction":0.5}]),
        );
    }
    let original = serde_json::to_vec(&input).unwrap();
    let prepared = prepare(&input, &options()).unwrap();
    for (i, g) in input.garments.iter().enumerate() {
        let raw = &g.pattern["attached_mass"];
        let mut expected = raw["groups"][0].clone();
        let id = format!("{}::ornament", g.id);
        expected["extra"]["id"] = json!(id);
        for target in expected["targets"].as_array_mut().unwrap() {
            target["instance"] = json!(format!(
                "{}::{}",
                g.id,
                target["instance"].as_str().unwrap()
            ));
        }
        assert_eq!(
            prepared.pattern().raw()["attached_mass"]["groups"][i],
            expected
        );
        let mapping = &prepared.receipt()["garments"][i]["attached_mass"];
        assert_eq!(mapping["groups"]["ornament"], id);
        assert_eq!(
            mapping["source_input_sha256"],
            ftx_core::receipt::sha256_hex(&serde_json::to_vec(raw).unwrap())
        );
    }
    assert_eq!(original, serde_json::to_vec(&input).unwrap());
}

#[test]
fn joint_sewn_accessories_change_inertia_once_and_survive_motion() {
    let mut input = bands();
    let opts = DrapeOptions {
        spacing: 24.0,
        ..options()
    };
    let skin = skin();
    let plain = prepare(&input, &opts)
        .unwrap()
        .drape(&json!({}), &body(&skin), &opts, |_| {})
        .unwrap();
    for (g, mass) in input.garments.iter_mut().zip([0.003, 0.002]) {
        g.pattern["attached_mass"] = attachment(
            mass,
            json!([{"instance":"front","vertex":"b","fraction":0.25},
                {"instance":"back","vertex":"a","fraction":0.75}]),
        );
    }
    let prepared = prepare(&input, &opts).unwrap();
    let mut loaded = prepared
        .drape(&json!({}), &body(&skin), &opts, |_| {})
        .unwrap();
    assert_eq!(plain.scene.mesh.faces, loaded.scene.mesh.faces);
    assert_eq!(plain.scene.mesh.verts2d, loaded.scene.mesh.verts2d);
    assert_eq!(plain.scene.weld, loaded.scene.weld);
    assert_eq!(plain.scene.start.relaxed, loaded.scene.start.relaxed);
    assert_eq!(
        plain.scene.solver.model.membranes,
        loaded.scene.solver.model.membranes
    );
    assert_eq!(plain.report["mass_kg"], loaded.report["mass_kg"]); // sheet-only legacy metric
    assert_ne!(plain.scene.solver.x, loaded.scene.solver.x);
    let report = loaded
        .scene
        .solver
        .model
        .attached_mass_report()
        .unwrap()
        .unwrap();
    assert!((report.attached_mass_kg - 0.005).abs() < 1e-15);
    assert!(report.mass_balance_residual_kg.abs() < 1e-15);
    let mut added = vec![0.0; loaded.scene.solver.model.mass.len()];
    for (i, (group, mass)) in report.groups.iter().zip([0.003, 0.002]).enumerate() {
        assert_eq!(group.id, format!("{}::ornament", input.garments[i].id));
        assert_eq!(
            group.targets[0].solver_vertex,
            group.targets[1].solver_vertex
        );
        assert_ne!(group.targets[0].mesh_vertex, group.targets[1].mesh_vertex);
        added[group.targets[0].solver_vertex] += mass; // ONE group, not mass per target
        assert_eq!(loaded.report["outfit"]["garment_attached_mass_kg"][i], mass);
        let inertial = loaded.report["outfit"]["garment_mass_kg"][i]
            .as_f64()
            .unwrap();
        let cloth = plain.report["outfit"]["garment_mass_kg"][i]
            .as_f64()
            .unwrap();
        assert!((inertial - cloth - mass).abs() < 1e-15);
    }
    assert_ne!(
        report.groups[0].targets[0].solver_vertex,
        report.groups[1].targets[0].solver_vertex
    );
    for ((&base, &with), &extra) in plain
        .scene
        .solver
        .model
        .mass
        .iter()
        .zip(&loaded.scene.solver.model.mass)
        .zip(&added)
    {
        assert!((with - base - extra).abs() < 1e-15);
    }
    let before = serde_json::to_value(report).unwrap();
    let mass = loaded.scene.solver.model.mass.clone();
    let target: Vec<_> = skin
        .vertices
        .iter()
        .map(|p| *p + Vec3::new(0., 0., 0.00001))
        .collect();
    ftx_drape::motion::advance_body(&mut loaded.scene.solver, &target, 0.001, Default::default())
        .unwrap();
    assert_eq!(mass, loaded.scene.solver.model.mass);
    assert_eq!(
        before,
        serde_json::to_value(
            loaded
                .scene
                .solver
                .model
                .attached_mass_report()
                .unwrap()
                .unwrap()
        )
        .unwrap()
    );
    loaded
        .scene
        .solver
        .contact
        .as_ref()
        .unwrap()
        .audit_self_contact(&loaded.scene.solver.x)
        .unwrap();
}

#[test]
fn invalid_or_inactive_accessories_refuse_before_simulation_instead_of_disappearing() {
    let mut good = input();
    good.garments[0].pattern["attached_mass"] = attachment(
        0.003,
        json!([{"instance":"panel","vertex":"b","fraction":1}]),
    );
    for change in [
        (|p: &mut Value| p["attached_mass"]["groups"][0]["extra"]["allow_estimates"] = json!(false))
            as fn(&mut Value),
        |p| p["attached_mass"]["groups"][0]["extra"]["mass"] = Value::Null,
        |p| p["attached_mass"]["groups"][0]["targets"][0]["fraction"] = json!(0.5),
        |p| p["attached_mass"]["groups"][0]["targets"][0]["instance"] = json!("outer::panel"),
        |p| p["attached_mass"]["groups"][0]["targets"][0]["vertex"] = json!("absent"),
        |p| p["attached_mass"]["groups"][0]["extra"]["mass"]["source"] = json!("absent"),
        |p| {
            let mut other = p["pieces"][0].clone();
            other["id"] = json!("active");
            p["pieces"].as_array_mut().unwrap().push(other);
            p["pieces"][0]["sim"] = json!(false);
        },
    ] {
        let mut bad = good.clone();
        change(&mut bad.garments[0].pattern);
        assert!(prepare(&bad, &options()).is_err());
    }
    assert!(prepare(&good, &options()).is_ok());
}

#[test]
fn attachment_limits_are_outfit_wide_and_empty_groups_leave_physics_unchanged() {
    let mut input = input();
    let baseline = prepare(&input, &options()).unwrap();
    input.garments[0].pattern["attached_mass"] =
        json!({"schema":"ftx-attached-mass/1","groups":[]});
    assert_eq!(
        baseline.pattern().raw(),
        prepare(&input, &options()).unwrap().pattern().raw()
    );
    for garment in &mut input.garments {
        let mut attached = attachment(
            0.003,
            json!([{"instance":"panel","vertex":"a","fraction":1}]),
        );
        let first = attached["groups"][0].clone();
        attached["groups"] = Value::Array(
            (0..513)
                .map(|i| {
                    let mut group = first.clone();
                    group["extra"]["id"] = json!(format!("weight-{i}"));
                    group
                })
                .collect(),
        );
        garment.pattern["attached_mass"] = attached;
    }
    assert!(
        prepare(&input, &options())
            .err()
            .unwrap()
            .message
            .contains("1024 groups")
    );
}

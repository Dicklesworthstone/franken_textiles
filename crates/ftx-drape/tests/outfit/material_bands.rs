//! Actual independently sewn casings; source-band substitution must not edit their cloth.
use super::*;
use ftx_drape::scene::elastic_casings;
use ftx_pattern::Pattern;

fn band(ea: f64) -> Value {
    let mut v: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/materials/synthetic-elastic-band.json"
    ))
    .unwrap();
    v["record"]["properties"]["axial_stiffness"]["value"] = json!(ea);
    v
}

#[test]
fn characterized_band_preserves_legacy_dynamics_and_cannot_be_overridden_by_positive_global_ea() {
    let raw = casing_outfit().garments[0].pattern.clone();
    let opts = DrapeOptions {
        spacing: 24.0,
        ..options()
    };
    let skin = skin();
    let plain = ftx_drape::run::drape(
        &Pattern::new(raw.clone()),
        &json!({}),
        &body(&skin),
        &opts,
        |_| {},
    )
    .unwrap();
    let mut selected = raw.clone();
    selected["meta"]["solved"]["elastic_material"] = band(opts.elastic_ea);
    let pattern = Pattern::new(selected);
    let characterized =
        ftx_drape::run::drape(&pattern, &json!({}), &body(&skin), &opts, |_| {}).unwrap();
    assert_eq!(plain.scene.solver.x, characterized.scene.solver.x);
    assert_eq!(plain.scene.solver.v, characterized.scene.solver.v);
    assert_eq!(plain.fold.frames, characterized.fold.frames);
    assert_eq!(
        plain.scene.solver.model.elastics,
        characterized.scene.solver.model.elastics
    );
    assert_eq!(raw["pieces"], pattern.raw()["pieces"]);
    assert_eq!(raw["fabric"], pattern.raw()["fabric"]);
    let again = ftx_drape::run::drape(
        &pattern,
        &json!({}),
        &body(&skin),
        &DrapeOptions {
            elastic_ea: 1.0,
            ..opts
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(again.scene.solver.x, characterized.scene.solver.x);
    assert_eq!(again.report["elastic"], characterized.report["elastic"]);
    assert!(characterized.report["elastic"]["band_material"]["snapshot"]["sha256"].is_string());
    assert!(plain.report["elastic"].get("band_material").is_none());
}

#[test]
fn band_only_substitution_changes_assembled_ring_forces_and_joint_motion_not_sheet_properties() {
    let mut a = casing_outfit();
    for (g, ea) in a.garments.iter_mut().zip([15.0, 23.0]) {
        g.pattern["meta"]["solved"]["elastic_material"] = band(ea);
    }
    let mut b = a.clone();
    b.garments[0].pattern["meta"]["solved"]["elastic_material"] = band(30.0);
    let opts = DrapeOptions {
        spacing: 24.0,
        ..options()
    };
    let (pa, pb) = (prepare(&a, &opts).unwrap(), prepare(&b, &opts).unwrap());
    assert_eq!(pa.pattern().raw()["pieces"], pb.pattern().raw()["pieces"]);
    assert_eq!(pa.pattern().raw()["seams"], pb.pattern().raw()["seams"]);
    assert_eq!(pa.pattern().raw()["fabric"], pb.pattern().raw()["fabric"]);
    let skin = skin();
    let ra = pa.drape(&json!({}), &body(&skin), &opts, |_| {}).unwrap();
    let mut rb = pb.drape(&json!({}), &body(&skin), &opts, |_| {}).unwrap();
    assert_eq!(ra.scene.weld, rb.scene.weld);
    assert_eq!(ra.scene.mesh.verts2d, rb.scene.mesh.verts2d);
    assert_eq!(ra.scene.solver.model.mass, rb.scene.solver.model.mass);
    assert_eq!(
        ra.scene.solver.model.membrane,
        rb.scene.solver.model.membrane
    );
    assert_eq!(
        ra.scene.solver.model.membranes,
        rb.scene.solver.model.membranes
    );
    assert_eq!(ra.report["material"], rb.report["material"]);
    assert_ne!(ra.scene.solver.x, rb.scene.solver.x);
    let ma = &ra.scene.solver.model;
    let mb = &rb.scene.solver.model;
    let mut positive_force = false;
    for (g, group) in ra.scene.elastic_groups.iter().enumerate() {
        let ratio = if g == 0 { 2.0 } else { 1.0 };
        for ring in &ra.scene.elastic_rings[group.rings.clone()] {
            for (ea, eb) in ma.elastics[ring.clone()]
                .iter()
                .zip(&mb.elastics[ring.clone()])
            {
                assert_eq!(ea.v, eb.v);
                assert_eq!(ea.rest, eb.rest);
                assert_eq!(eb.ea, ratio * ea.ea);
                // Same test geometry isolates constitutive force from equilibrium changes.
                let force = ea.tension(&ra.scene.solver.x);
                positive_force |= g == 0 && force > 0.0;
                assert!((eb.tension(&ra.scene.solver.x) - ratio * force).abs() < 1e-12);
            }
        }
        assert_eq!(rb.report["elastics"][g]["ea_n"], json!([30.0, 23.0][g]));
    }
    assert!(positive_force);
    let frozen = Pattern::new(
        serde_json::from_slice(&serde_json::to_vec(pb.pattern().raw()).unwrap()).unwrap(),
    );
    let replay = ftx_drape::run::drape(&frozen, &json!({}), &body(&skin), &opts, |_| {}).unwrap();
    assert_eq!(rb.scene.solver.x, replay.scene.solver.x);
    assert_eq!(rb.report["elastics"], replay.report["elastics"]);
    let laws = rb.scene.solver.model.elastics.clone();
    let moved: Vec<_> = skin
        .vertices
        .iter()
        .map(|p| *p + Vec3::new(0.0, 0.0, 0.00001))
        .collect();
    ftx_drape::motion::advance_body(&mut rb.scene.solver, &moved, 0.001, Default::default())
        .unwrap();
    assert_eq!(laws, rb.scene.solver.model.elastics);
    rb.scene
        .solver
        .contact
        .as_ref()
        .unwrap()
        .audit_self_contact(&rb.scene.solver.x)
        .unwrap();
}

#[test]
fn frozen_band_identity_and_independent_evidence_are_checked_even_when_disabled() {
    let mut input = casing_outfit();
    input.garments[0].pattern["meta"]["solved"]["elastic_material"] = band(15.0);
    let prepared = prepare(&input, &options()).unwrap();
    let raw = prepared.pattern().raw();
    assert!(
        raw["meta"]["solved"]["elastic_casings"][0]["elastic_material"]["expected_sha256"]
            .is_string()
    );
    let mut stale = raw.clone();
    stale["meta"]["solved"]["elastic_casings"][0]["elastic_material"]["record"]["properties"]["axial_stiffness"]
        ["value"] = json!(30.0);
    for ea in [0.0, 15.0] {
        assert!(elastic_casings(&Pattern::new(stale.clone()), ea).is_err());
        let mut denied = input.clone();
        denied.garments[0].pattern["meta"]["solved"]["elastic_material"]["allow_estimates"] =
            json!(false);
        assert!(
            prepare(
                &denied,
                &DrapeOptions {
                    elastic_ea: ea,
                    ..options()
                }
            )
            .is_err()
        );
        let mut partial = input.clone();
        partial.garments[0].pattern["meta"]["solved"]["elastic_material"]["record"]["properties"] =
            json!({});
        assert!(
            prepare(
                &partial,
                &DrapeOptions {
                    elastic_ea: ea,
                    ..options()
                }
            )
            .is_err()
        );
    }
    assert!(elastic_casings(prepared.pattern(), 0.0).unwrap().is_empty());
    let mut ambiguous = raw.clone();
    ambiguous["meta"]["solved"]["elastic_material"] = band(15.0);
    assert!(elastic_casings(&Pattern::new(ambiguous), 15.0).is_err());
}

#[test]
fn explicit_source_selection_does_not_become_an_unscoped_default() {
    let pattern = Pattern::new(casing_outfit().garments[0].pattern.clone());
    let c = elastic_casings(&pattern, 15.0).unwrap().remove(0);
    let mut raw = pattern.raw().clone();
    raw["meta"]["solved"] = json!({"elastic_casings":[{
        "instance":c.instance,"relaxed_m":c.length,"load_instances":c.load_instances,
        "elastic_material":band(7.0)}]});
    let selected = elastic_casings(&Pattern::new(raw.clone()), 60.0).unwrap();
    assert_eq!(selected[0].ea, 7.0);
    raw["meta"]["solved"]["elastic_casings"][0]["elastic_material"] = Value::Null;
    assert!(elastic_casings(&Pattern::new(raw), 60.0).is_err());
    let mut inactive = pattern.raw().clone();
    inactive["meta"]["solved"]["elastic_material"] = band(7.0);
    inactive["pieces"][0]["sim"] = json!(false);
    assert!(elastic_casings(&Pattern::new(inactive), 15.0).is_err());
}

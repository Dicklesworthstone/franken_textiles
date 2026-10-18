//! Synthetic loading answers and counterface/evidence regressions, not measured textile data.
use ftx_pattern::material_catalog::Selection;
use ftx_pattern::material_friction::{Binding, Input, Interface, Target, calibrate};
use ftx_pattern::materials::{Catalog, Evidence, Source};
use serde_json::json;

fn catalog() -> Catalog {
    serde_json::from_value(json!({"schema":"ftx-textile-catalog/1","materials":[]})).unwrap()
}
fn material(id: &str) -> Selection {
    serde_json::from_value(json!({"id":id,"record":{
        "id":id,"name":"Synthetic friction coupon","form":"woven","sources":{
            "test":{"reference":"test:synthetic-sheet","accessed_on":"2026-10-18","note":"Synthetic, not measured textile data"}},
        "properties":{
            "thickness":{"value":0.001,"unit":"m","source":"test","evidence":"estimate","method":"Synthetic test"},
            "friction_skin":{"value":0.2,"unit":"1","source":"test","evidence":"estimate","method":"Synthetic test"},
            "friction_self":{"value":0.3,"unit":"1","source":"test","evidence":"estimate","method":"Synthetic test"}}},
        "allow_estimates":true})).unwrap()
}
fn input() -> Input {
    serde_json::from_value(json!({
        "schema":"ftx-friction-calibration/1","target":{"kind":"pair","materials":[material("a"),material("b")]},
        "method":"Synthetic corrected steady pull; not an actual sled test",
        "conditions":{"surface_state":"synthetic dry scenario"},"fitted_on":"2026-10-18",
        "sources":{"acq":{"reference":"test:force-oracle","accessed_on":"2026-10-18","note":"All observations synthetic"}},
        "experiments":[
            {"id":"fit","source":"acq","evidence":"estimate","partition":"fit","samples":[
                {"normal_n":2.0,"pull_n":[0.8,0.0],"velocity_m_s":[0.01,0.0],"scale_n":0.1},
                {"normal_n":5.0,"pull_n":[-1.2,1.6],"velocity_m_s":[-0.03,0.04],"scale_n":0.1}]},
            {"id":"check","source":"acq","evidence":"estimate","partition":"check","samples":[
                {"normal_n":3.0,"pull_n":[0.0,-1.2],"velocity_m_s":[0.0,-0.02],"scale_n":0.1}]}],
        "allow_estimated_observations":true,"replace_existing":false,"max_rms_scaled":1e-10,"max_peak_scaled":1e-10
    })).unwrap()
}
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-12, "{a} != {b}");
}

#[test]
fn exact_vector_fit_produces_the_existing_source_locked_interface() {
    let db = catalog();
    let r = calibrate(&input(), &db).unwrap();
    near(r.coefficient.value, 0.4);
    assert_eq!(r.coefficient.evidence, Evidence::Estimate);
    assert_eq!(r.fit.components, 4);
    assert_eq!(r.check.as_ref().unwrap().components, 2);
    assert_eq!(r.fit_normal_n_range, [2.0, 5.0]);
    near(r.fit_speed_m_s_range[0], 0.01);
    near(r.fit_speed_m_s_range[1], 0.05);
    let Binding::Interface {
        interface,
        materials,
    } = r.binding(&db, true).unwrap()
    else {
        panic!()
    };
    interface.validate().unwrap();
    assert_eq!(
        serde_json::to_value(&interface).unwrap(),
        interface.report()
    );
    Interface::parse(&interface.report())
        .unwrap()
        .validate()
        .unwrap();
    assert!(
        interface.sources[&interface.friction.source]
            .reference
            .ends_with(&r.input_sha256)
    );
    for s in &materials {
        let m = db.resolve(s).unwrap();
        assert!(interface.materials.iter().any(|h| h == m.sha256()));
        // Pair acquisition never rewrites a same-cloth or body coefficient.
        near(m.material().properties["friction_self"].value, 0.3);
        near(m.material().properties["friction_skin"].value, 0.2);
    }
}

#[test]
fn consuming_an_estimate_is_separate_from_observation_and_sheet_permission() {
    let db = catalog();
    let r = calibrate(&input(), &db).unwrap();
    let Binding::Interface {
        interface,
        materials,
    } = r.binding(&db, false).unwrap()
    else {
        panic!()
    };
    assert!(materials.iter().all(|s| s.allow_estimates));
    assert!(
        interface
            .validate()
            .unwrap_err()
            .message
            .contains("own explicit consent")
    );
    let mut i = input();
    i.allow_estimated_observations = false;
    assert!(calibrate(&i, &db).is_err());
}

#[test]
fn held_out_forces_do_not_leak_and_no_check_is_explicit() {
    let db = catalog();
    let mut i = input();
    i.experiments[1].samples[0].pull_n = [7.0, 8.0];
    assert!(calibrate(&i, &db).unwrap_err().message.contains("held-out"));
    i.max_rms_scaled = 1e5;
    i.max_peak_scaled = 1e5;
    near(calibrate(&i, &db).unwrap().coefficient.value, 0.4);
    i.experiments.pop();
    assert!(calibrate(&i, &db).unwrap().check.is_none());
}

#[test]
fn frame_rotation_and_force_scaling_do_not_change_the_coefficient() {
    let db = catalog();
    let mut i = input();
    for e in &mut i.experiments {
        for s in &mut e.samples {
            s.pull_n = [-s.pull_n[1] * 1000.0, s.pull_n[0] * 1000.0];
            s.velocity_m_s = [-s.velocity_m_s[1], s.velocity_m_s[0]];
            s.normal_n *= 1000.0;
            s.scale_n *= 1000.0;
        }
    }
    near(calibrate(&i, &db).unwrap().coefficient.value, 0.4);
}

#[test]
fn transverse_adhesive_and_wrong_sign_forces_cannot_masquerade_as_friction() {
    let db = catalog();
    let mut i = input();
    i.experiments[0].samples[0].pull_n[1] = 1.0;
    assert!(calibrate(&i, &db).is_err());
    i = input();
    for e in &mut i.experiments {
        for s in &mut e.samples {
            s.pull_n = s.pull_n.map(|v| -v);
        }
    }
    assert!(calibrate(&i, &db).is_err());
    i = input();
    i.experiments[0].samples[0].pull_n[0] += 0.5;
    assert!(calibrate(&i, &db).is_err());
}

#[test]
fn zero_and_greater_than_one_friction_are_not_clamped() {
    for mu in [0.0, 1.7] {
        let mut i = input();
        for e in &mut i.experiments {
            for s in &mut e.samples {
                s.pull_n = s.pull_n.map(|v| v * mu / 0.4);
            }
        }
        near(calibrate(&i, &catalog()).unwrap().coefficient.value, mu);
    }
}

#[test]
fn self_and_body_bindings_only_change_the_explicit_slot_with_replacement_permission() {
    let db = catalog();
    for body in [false, true] {
        let mut i = input();
        i.target = if body {
            Target::Body {
                material: Box::new(material("a")),
                counterface: Source {
                    reference: "test:body-surrogate".into(),
                    accessed_on: "2026-10-18".into(),
                    note: "Synthetic, not human skin".into(),
                },
            }
        } else {
            Target::SelfContact {
                material: Box::new(material("a")),
            }
        };
        assert!(calibrate(&i, &db).unwrap().binding(&db, true).is_err());
        i.replace_existing = true;
        let r = calibrate(&i, &db).unwrap();
        let Binding::Material { selection } = r.binding(&db, true).unwrap() else {
            panic!()
        };
        let m = db.resolve(&selection).unwrap();
        near(m.material().properties["thickness"].value, 0.001);
        near(
            m.material().properties["friction_skin"].value,
            if body { 0.4 } else { 0.2 },
        );
        near(
            m.material().properties["friction_self"].value,
            if body { 0.3 } else { 0.4 },
        );
        assert_ne!(
            selection.expected_sha256,
            r.resolved_materials[0].expected_sha256
        );
        assert!(r.input.replace_existing);
    }
}

#[test]
fn malformed_nonfinite_stationary_and_unloaded_acquisitions_refuse() {
    let db = catalog();
    for change in [
        (|i: &mut Input| i.experiments[0].samples[0].normal_n = 0.0) as fn(&mut Input),
        |i| i.experiments[0].samples[0].normal_n = f64::NAN,
        |i| i.experiments[0].samples[0].velocity_m_s = [0.0, 0.0],
        |i| i.experiments[0].samples[0].scale_n = 0.0,
        |i| i.experiments[0].samples[0].pull_n = [f64::INFINITY, 0.0],
        |i| i.experiments[0].source = "missing".into(),
        |i| i.experiments[1].id = i.experiments[0].id.clone(),
        |i| i.experiments[0].partition = ftx_pattern::material_calibration::Partition::Check,
        |i| i.max_peak_scaled = f64::INFINITY,
        |i| i.experiments[0].samples = vec![i.experiments[0].samples[0].clone(); 4097],
    ] {
        let mut i = input();
        change(&mut i);
        assert!(calibrate(&i, &db).is_err());
    }
}

#[test]
fn deterministic_receipts_and_target_locks_preserve_scope() {
    let i = input();
    let db = catalog();
    let a = calibrate(&i, &db).unwrap();
    let b = calibrate(&i, &db).unwrap();
    assert_eq!(
        serde_json::to_string(&a).unwrap(),
        serde_json::to_string(&b).unwrap()
    );
    let mut changed = i.clone();
    let Target::Pair { materials } = &mut changed.target else {
        panic!()
    };
    materials[0] = a.resolved_materials[0].clone();
    materials[0]
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("thickness")
        .unwrap()
        .value = 0.002;
    assert!(calibrate(&changed, &db).is_err());
    changed = i;
    let Target::Pair { materials } = &mut changed.target else {
        panic!()
    };
    materials[1] = materials[0].clone();
    assert!(calibrate(&changed, &db).is_err());
}

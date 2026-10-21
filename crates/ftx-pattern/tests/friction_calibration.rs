//! Synthetic loading answers and counterface/evidence regressions, not measured textile data.
use ftx_pattern::material_catalog::Selection;
use ftx_pattern::material_friction::{Binding, Input, Interface, Target, calibrate};
use ftx_pattern::materials::{Catalog, Evidence, Source};
use ftx_pattern::{Pattern, material_binding::Changes};
use serde_json::{Value, json};

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
    interface.validate_observation().unwrap();
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
    i.experiments[1].samples[0].pull_n = [Some(7.0), Some(8.0)];
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
            s.pull_n = [
                s.pull_n[1].map(|v| -v * 1000.0),
                s.pull_n[0].map(|v| v * 1000.0),
            ];
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
    i.experiments[0].samples[0].pull_n[1] = Some(1.0);
    assert!(calibrate(&i, &db).is_err());
    i = input();
    for e in &mut i.experiments {
        for s in &mut e.samples {
            s.pull_n = s.pull_n.map(|v| v.map(|x| -x));
        }
    }
    assert!(calibrate(&i, &db).is_err());
    i = input();
    i.experiments[0].samples[0].pull_n[0] = Some(1.3);
    assert!(calibrate(&i, &db).is_err());
}

#[test]
fn zero_and_greater_than_one_friction_are_not_clamped() {
    for mu in [0.0, 1.7] {
        let mut i = input();
        for e in &mut i.experiments {
            for s in &mut e.samples {
                s.pull_n = s.pull_n.map(|v| v.map(|x| x * mu / 0.4));
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
        |i| i.experiments[0].samples[0].pull_n = [Some(f64::INFINITY), Some(0.0)],
        |i| i.experiments[0].samples[0].pull_n = [None, None],
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

#[test]
fn one_axis_instrument_keeps_unmeasured_force_components_unknown() {
    let mut i = input();
    for e in &mut i.experiments {
        for s in &mut e.samples {
            s.pull_n[1] = None;
        }
    }
    let r = calibrate(&i, &catalog()).unwrap();
    near(r.coefficient.value, 0.4);
    assert_eq!(r.fit.components, 2);
    assert_eq!(r.check.unwrap().components, 1);
    assert!(r.predictions.iter().all(|p| p.residual_n[1].is_none()));
    for e in &mut i.experiments {
        for s in &mut e.samples {
            s.velocity_m_s = [0.0, 0.01];
            s.pull_n = [Some(0.0), None];
        }
    }
    // Measuring only perpendicular zero force cannot identify the missing sliding force.
    assert!(
        calibrate(&i, &catalog())
            .unwrap_err()
            .message
            .contains("slip direction")
    );
}

fn pair(a: &str, b: &str) -> Value {
    let db = catalog();
    json!({"materials":[db.resolve(&material(a)).unwrap().sha256(),
        db.resolve(&material(b)).unwrap().sha256()],
        "friction":{"value":0.4,"unit":"1","evidence":"estimate","source":"test",
            "method":"Synthetic interface binding test"},
        "sources":{"test":{"reference":"test:binding-pair","accessed_on":"2026-10-19",
            "note":"Synthetic pair evidence, not measured textile data"}}})
}

fn paired_pattern() -> Pattern {
    let mut raw: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-sewing-pattern.json"
    ))
    .unwrap();
    raw["fabric"]["material"] = json!(material("a"));
    raw["fabric"]["interfaces"] = json!([pair("a", "b")]);
    raw["pieces"][1]["fabric"] = json!({"material":material("b"),"role":"lining"});
    Pattern::new(raw)
}

#[test]
fn explicit_binding_retains_authored_pair_numbers_order_and_unadmitted_estimates() {
    let p = paired_pattern();
    let original = p.raw().clone();
    let mut authored = pair("b", "a");
    authored["friction"]["value"] = json!(0);
    authored["friction"]["bounds"] = serde_json::from_str("[-0.0, 0]").unwrap();
    let changes: Changes = serde_json::from_value(json!({"schema":"ftx-material-bindings/1",
        "interfaces":[authored]}))
    .unwrap();
    let bound = changes.apply(&p).unwrap();
    let retained = &bound["pattern"]["fabric"]["interfaces"][0];
    assert_eq!(
        serde_json::to_string(retained).unwrap(),
        serde_json::to_string(&authored).unwrap()
    );
    assert!(retained["friction"]["value"].is_u64());
    assert!(retained.get("allow_estimates").is_none());
    assert!(retained["friction"].get("conditions").is_none());
    let parsed = Interface::parse(retained).unwrap();
    parsed.validate_observation().unwrap();
    assert!(!parsed.allow_estimates);
    assert!(parsed.validate().is_err());
    assert_eq!(bound["pattern"]["seams"], original["seams"]);
    assert_eq!(bound["geometry_changed"], false);
    assert_eq!(p.raw(), &original);
    let parsed_pattern: Value =
        serde_json::from_str(bound["pattern_json"].as_str().unwrap()).unwrap();
    assert_eq!(parsed_pattern, bound["pattern"]);
}

#[test]
fn omitted_or_null_interfaces_preserve_original_evidence_and_empty_explicit_list_clears() {
    let p = paired_pattern();
    let omitted: Changes =
        serde_json::from_value(json!({"schema":"ftx-material-bindings/1"})).unwrap();
    let null: Changes =
        serde_json::from_value(json!({"schema":"ftx-material-bindings/1","interfaces":null}))
            .unwrap();
    for changes in [&omitted, &null] {
        assert!(
            serde_json::to_value(changes)
                .unwrap()
                .get("interfaces")
                .is_none()
        );
        assert_eq!(
            changes.apply(&p).unwrap()["pattern"]["fabric"]["interfaces"],
            p.raw()["fabric"]["interfaces"]
        );
    }
    assert_eq!(omitted.apply(&p).unwrap(), null.apply(&p).unwrap());
    let cleared: Changes =
        serde_json::from_value(json!({"schema":"ftx-material-bindings/1","interfaces":[]}))
            .unwrap();
    assert_eq!(
        cleared.apply(&p).unwrap()["pattern"]["fabric"]["interfaces"],
        json!([])
    );
    // Existing unsupported evidence remains a downstream consumer's refusal when not replaced.
    let mut original = p.raw().clone();
    original["fabric"]["interfaces"] = json!({"legacy_invalid_interface":"retained"});
    let p = Pattern::new(original.clone());
    assert_eq!(
        omitted.apply(&p).unwrap()["pattern"]["fabric"]["interfaces"],
        original["fabric"]["interfaces"]
    );
    assert_eq!(
        cleared.apply(&p).unwrap()["pattern"]["fabric"]["interfaces"],
        json!([])
    );
    assert_eq!(p.raw(), &original);
}

#[test]
fn explicit_interfaces_bind_to_final_assignments_without_rewriting_counterface_hashes() {
    let p = paired_pattern();
    let original = p.raw().clone();
    let mut changes: Changes = serde_json::from_value(json!({"schema":"ftx-material-bindings/1",
        "default":material("c"),"interfaces":[pair("a","b")]}))
    .unwrap();
    assert!(
        changes
            .apply(&p)
            .unwrap_err()
            .message
            .contains("unassigned or stale")
    );
    changes.interfaces = Some(vec![pair("c", "b")]);
    let replaced = changes.apply(&p).unwrap();
    assert_eq!(
        replaced["pattern"]["fabric"]["interfaces"],
        json!([pair("c", "b")])
    );
    assert_eq!(replaced["pieces"][0]["material_id"], "c");
    assert_eq!(replaced["pieces"][1]["material_id"], "b");
    assert_eq!(replaced["pattern"]["pieces"][1]["fabric"]["role"], "lining");
    changes.interfaces = None;
    let retained = changes.apply(&p).unwrap();
    assert_eq!(
        retained["pattern"]["fabric"]["interfaces"],
        original["fabric"]["interfaces"]
    );
    assert_eq!(retained["pattern"]["pieces"], replaced["pattern"]["pieces"]);
    assert_eq!(retained["pattern"]["seams"], replaced["pattern"]["seams"]);
    assert_eq!(p.raw(), &original);
}

#[test]
fn replacement_refuses_duplicate_asymmetric_stale_and_malformed_pair_evidence_atomically() {
    let p = paired_pattern();
    let original = p.raw().clone();
    for mutate in [
        (|v: &mut Value| v["materials"][0] = json!("0".repeat(64))) as fn(&mut Value),
        |v| v["materials"][0] = v["materials"][1].clone(),
        |v| v["friction"]["unit"] = json!("N/m"),
        |v| v["friction"]["value"] = json!(-0.1),
        |v| v["friction"]["source"] = json!("missing"),
        |v| v["friction"]["bounds"] = json!([0.5, 0.9]),
        |v| v["friction"]["conditions"] = json!({"":"undeclared"}),
        |v| v["sources"]["test"]["reference"] = json!(""),
        |v| v["allow_estimates"] = json!("true"),
        |v| v["reverse_friction"] = json!(0.7),
    ] {
        let mut invalid = pair("a", "b");
        mutate(&mut invalid);
        let changes: Changes = serde_json::from_value(json!({"schema":"ftx-material-bindings/1",
            "interfaces":[invalid]}))
        .unwrap();
        assert!(changes.apply(&p).is_err());
        assert_eq!(p.raw(), &original);
    }
    let mut reverse = pair("b", "a");
    reverse["friction"]["value"] = json!(0.8);
    let mut changes: Changes = serde_json::from_value(json!({"schema":"ftx-material-bindings/1",
        "interfaces":[pair("a","b"),reverse]}))
    .unwrap();
    assert!(changes.apply(&p).unwrap_err().message.contains("duplicate"));
    changes.interfaces = Some(vec![pair("a", "b"); 2017]);
    assert!(changes.apply(&p).unwrap_err().message.contains("budget"));
    assert!(
        serde_json::from_value::<Changes>(json!({"schema":"ftx-material-bindings/1",
        "interfaces":{"unexpected":"object"}}))
        .is_err()
    );
}

use super::*;

fn request() -> ftx_drape::study::contact_loads::Request {
    serde_json::from_value(json!({"schema":"ftx-contact-loads/1", "instances":["band"],
        "max_vertex_samples":100_000,"max_pair_evaluations":1_000_000}))
    .unwrap()
}

#[test]
fn contact_loads_replay_the_real_material_bound_solver_on_static_and_moving_bodies() {
    let form = form();
    let body = body(&form);
    let p = pattern();
    let mut s = motion_scenario();
    // Explicit wide numerical barrier makes the sparse synthetic band exercise nonzero forces.
    s.options.contact.dhat = 0.02;
    s.contact_loads = Some(request());
    let mut session = Session::new(&s, &body).unwrap();
    session.set_motion_path(Ok(path(&s, &body))).unwrap();
    let result = session.evaluate(&p, |_| {}).unwrap();
    assert_eq!(
        result["contact_loads"]["status"], "available",
        "{}",
        result["contact_loads"]
    );
    assert_eq!(
        result["motion"]["status"], "available",
        "{}",
        result["motion"]
    );
    let outcome = &result["motion"]["result"]["contact_loads"];
    assert_eq!(outcome["status"], "available", "{outcome}");
    let history = &outcome["result"];
    let mut contact = drape(&p, &s.fit_spec, &body, &s.options, |_| {})
        .unwrap()
        .scene
        .solver
        .contact
        .unwrap();
    let samples: Vec<_> = std::iter::once(&result["motion"]["result"]["initial_sample"])
        .chain(result["motion"]["result"]["samples"].as_array().unwrap())
        .collect();
    let frames = history["frames"].as_array().unwrap();
    assert_eq!(frames.len(), samples.len());
    assert_eq!(frames[0], result["contact_loads"]["result"]["frames"][0]);
    let selected: Vec<usize> =
        serde_json::from_value(history["source"]["selected_vertices"].clone()).unwrap();
    assert!(selected.windows(2).all(|p| p[0] < p[1]));
    let mut peak = 0_f64;
    for (frame, sample) in frames.iter().zip(samples) {
        let positions: Vec<Vec3> = serde_json::from_value(sample["positions_m"].clone()).unwrap();
        contact.body = Body::new(
            serde_json::from_value(sample["body_positions_m"].clone()).unwrap(),
            form.triangles.clone(),
            contact.params.dhat,
        )
        .unwrap();
        let map = contact.reaction_map(&positions).unwrap();
        let forces: Vec<Vec3> =
            serde_json::from_value(map["body_force_on_cloth_n"].clone()).unwrap();
        let chosen: Vec<_> = selected.iter().map(|&v| forces[v]).collect();
        assert_eq!(frame["body_force_on_selected_vertices_n"], json!(chosen));
        assert_eq!(frame["body_state_sha256"], map["body_state_sha256"]);
        assert_eq!(frame["active_policy_sha256"], map["active_policy_sha256"]);
        assert_eq!(frame["time_s"], sample["time_s"]);
        let sum = chosen.iter().copied().fold(Vec3::ZERO, |a, b| a + b);
        assert_eq!(frame["body"]["resultant_n"], json!(sum));
        let maximum = chosen.iter().map(|f| f.norm()).fold(0., f64::max);
        assert_eq!(frame["body"]["max_vertex_force_n"], maximum);
        peak = peak.max(maximum);
    }
    assert!(peak > 0.);
    assert_eq!(history["max_body_vertex_force_n"], peak);
    assert_eq!(
        history["source"],
        result["contact_loads"]["result"]["source"]
    );
    assert_eq!(
        history["duration_s"],
        result["motion"]["result"]["duration_s"]
    );
    assert_eq!(history["pressure_claim"], false);

    let replacement = changed(&p, "thickness", 0.001);
    assert_eq!(replacement.raw()["pieces"], p.raw()["pieces"]);
    let swapped = session.evaluate(&replacement, |_| {}).unwrap();
    assert_eq!(swapped["contact_loads"]["status"], "available");
    assert_ne!(
        swapped["contact_loads"]["result"]["source_sha256"],
        result["contact_loads"]["result"]["source_sha256"]
    );
    assert_ne!(
        swapped["contact_loads"]["result"]["max_body_vertex_force_n"],
        result["contact_loads"]["result"]["max_body_vertex_force_n"]
    );
}

#[test]
fn contact_load_refusals_leave_mechanics_heat_and_complete_motion_unchanged() {
    let form = form();
    let body = body(&form);
    let p = pattern();
    let mut s = motion_scenario();
    let mut original = Session::new(&s, &body).unwrap();
    original.set_motion_path(Ok(path(&s, &body))).unwrap();
    let original = original.evaluate(&p, |_| {}).unwrap();
    let count = original["positions_m"].as_array().unwrap().len() + form.vertices.len();
    for policy in [
        json!({"schema":"ftx-contact-loads/1","instances":["absent"],"max_vertex_samples":100000,"max_pair_evaluations":1000000}),
        json!({"schema":"ftx-contact-loads/1","max_vertex_samples":1,"max_pair_evaluations":1000000}),
        json!({"schema":"ftx-contact-loads/1","max_vertex_samples":count,"max_pair_evaluations":1000000}),
    ] {
        s.contact_loads = Some(serde_json::from_value(policy).unwrap());
        let mut session = Session::new(&s, &body).unwrap();
        session.set_motion_path(Ok(path(&s, &body))).unwrap();
        let result = session.evaluate(&p, |_| {}).unwrap();
        assert_eq!(result["drape"], original["drape"]);
        assert_eq!(result["surface"], original["surface"]);
        assert_eq!(result["motion"]["status"], "available");
        assert_eq!(
            result["motion"]["result"]["final_positions_sha256"],
            original["motion"]["result"]["final_positions_sha256"]
        );
        assert_eq!(
            result["motion"]["result"]["contact_loads"]["status"],
            "refused"
        );
        assert_eq!(
            result["contact_loads"]["status"],
            if s.contact_loads.as_ref().unwrap().max_vertex_samples == count {
                "available"
            } else {
                "refused"
            }
        );
    }
}

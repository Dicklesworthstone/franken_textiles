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
    let mut peak_sum = 0_f64;
    let mut peak_sum_sample = 0;
    for (sample_index, (frame, sample)) in frames.iter().zip(samples).enumerate() {
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
        let magnitude_sum: f64 = chosen.iter().map(|f| f.norm()).sum();
        assert_eq!(frame["body"]["sum_vertex_force_n"], magnitude_sum);
        if magnitude_sum > peak_sum {
            peak_sum = magnitude_sum;
            peak_sum_sample = sample_index;
        }
        let maximum = chosen.iter().map(|f| f.norm()).fold(0., f64::max);
        assert_eq!(frame["body"]["max_vertex_force_n"], maximum);
        peak = peak.max(maximum);
    }
    assert!(peak > 0.);
    assert_eq!(history["max_body_vertex_force_n"], peak);
    assert!(peak_sum >= peak);
    assert_eq!(history["peak_body_sum_vertex_force_n"], peak_sum);
    assert_eq!(
        history["peak_body_sum_vertex_force_sample"],
        peak_sum_sample
    );
    assert_eq!(
        result["contact_loads"]["result"]["peak_body_sum_vertex_force_n"],
        frames[0]["body"]["sum_vertex_force_n"]
    );
    assert_eq!(
        result["contact_loads"]["result"]["peak_body_sum_vertex_force_sample"],
        0
    );
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
    assert_ne!(
        swapped["contact_loads"]["result"]["peak_body_sum_vertex_force_n"],
        result["contact_loads"]["result"]["peak_body_sum_vertex_force_n"]
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
        json!({"schema":"ftx-contact-loads/2","regions":[{"id":"absent","instance":"absent",
            "selector":{"kind":"whole_triangle_centroid","polygon_mm":[[0.,0.],[300.,0.],[300.,30.],[0.,30.]]}}],
            "max_vertex_samples":100000,"max_pair_evaluations":1000000}),
        json!({"schema":"ftx-contact-loads/2","regions":[
            {"id":"first","instance":"band","selector":{"kind":"whole_triangle_centroid",
                "polygon_mm":[[0.,0.],[300.,0.],[300.,30.],[0.,30.]]}},
            {"id":"second","instance":"band","selector":{"kind":"whole_triangle_centroid",
                "polygon_mm":[[0.,0.],[300.,0.],[300.,30.],[0.,30.]]}}],
            "max_vertex_samples":100000,"max_pair_evaluations":1000000}),
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

fn regional_request() -> ftx_drape::study::contact_loads::Request {
    let split = 40. * std::f64::consts::TAU * 0.413;
    serde_json::from_value(json!({"schema":"ftx-contact-loads/2",
        "regions":[
            {"id":"left","instance":"band","selector":{"kind":"whole_triangle_centroid",
                "polygon_mm":[[-1.,-1.],[split,-1.],[split,21.],[-1.,21.]]}},
            {"id":"right","instance":"band","selector":{"kind":"whole_triangle_centroid",
                "polygon_mm":[[split,-1.],[300.,-1.],[300.,21.],[split,21.]]}}
        ],"max_vertex_samples":100_000,"max_pair_evaluations":1_000_000}))
    .unwrap()
}

#[test]
fn regional_contact_loads_select_intact_flat_panels_and_replay_the_same_sewn_motion() {
    let form = form();
    let body = body(&form);
    let p = pattern();
    let mut s = motion_scenario();
    s.options.contact.dhat = 0.02;
    s.contact_loads = Some(request());
    let mut whole = Session::new(&s, &body).unwrap();
    whole.set_motion_path(Ok(path(&s, &body))).unwrap();
    let whole = whole.evaluate(&p, |_| {}).unwrap();

    s.contact_loads = Some(regional_request());
    let mut session = Session::new(&s, &body).unwrap();
    session.set_motion_path(Ok(path(&s, &body))).unwrap();
    let result = session.evaluate(&p, |_| {}).unwrap();
    assert_eq!(result["contact_loads"]["status"], "available", "{result}");
    assert_eq!(
        result["motion"]["result"]["contact_loads"]["status"], "available",
        "{}",
        result["motion"]
    );
    assert_eq!(result["drape"], whole["drape"]);
    assert_eq!(result["positions_m"], whole["positions_m"]);
    assert_eq!(
        result["motion"]["result"]["samples"],
        whole["motion"]["result"]["samples"]
    );
    let source = &result["contact_loads"]["result"]["source"];
    let original = &whole["contact_loads"]["result"];
    assert_eq!(
        source["selected_vertices"],
        original["source"]["selected_vertices"]
    );
    let flat: Vec<[ftx_core::Vec2; 3]> =
        serde_json::from_value(result["contact_load_ownership"]["rest_triangles_mm"].clone())
            .unwrap();
    let faces: Vec<[usize; 3]> = serde_json::from_value(result["triangles"].clone()).unwrap();
    let split = 40. * std::f64::consts::TAU * 0.413;
    for (name, left) in [("left", true), ("right", false)] {
        let selected: Vec<usize> = flat
            .iter()
            .enumerate()
            .filter_map(|(i, triangle)| {
                let x = triangle.iter().map(|p| p.x).sum::<f64>() / 3.;
                assert_ne!(x, split);
                ((x < split) == left).then_some(i)
            })
            .collect();
        let vertices: std::collections::BTreeSet<_> =
            selected.iter().flat_map(|&i| faces[i]).collect();
        assert!(!selected.is_empty() && selected.len() < flat.len());
        assert_eq!(source["regions"][name]["source_triangles"], json!(selected));
        assert_eq!(
            source["regions"][name]["selected_vertices"],
            json!(vertices.into_iter().collect::<Vec<_>>())
        );
    }
    let report = &result["motion"]["result"]["contact_loads"]["result"];
    let baseline = &whole["motion"]["result"]["contact_loads"]["result"];
    assert_eq!(report["frames"], baseline["frames"]);
    for key in [
        "peak_body_resultant_n",
        "peak_body_resultant_sample",
        "peak_body_sum_vertex_force_n",
        "peak_body_sum_vertex_force_sample",
        "max_body_vertex_force_n",
        "max_body_vertex_force_sample",
        "vertex_samples",
        "pair_evaluations",
    ] {
        assert_eq!(report[key], baseline[key], "{key}");
    }
    assert_eq!(report["schema"], "ftx-contact-loads-report/2");
    assert_eq!(report["source"], *source);
    assert_eq!(report["pressure_claim"], false);

    // Keeping only one region changes the observed scope, never the garment topology or forces.
    s.contact_loads
        .as_mut()
        .unwrap()
        .regions
        .as_mut()
        .unwrap()
        .pop();
    let mut partial = Session::new(&s, &body).unwrap();
    partial.set_motion_path(Ok(path(&s, &body))).unwrap();
    let partial = partial.evaluate(&p, |_| {}).unwrap();
    assert_eq!(partial["drape"], whole["drape"]);
    assert_eq!(partial["triangles"], whole["triangles"]);
    let partial = &partial["motion"]["result"]["contact_loads"]["result"];
    let selected: Vec<usize> =
        serde_json::from_value(partial["source"]["selected_vertices"].clone()).unwrap();
    let all_selected: Vec<usize> =
        serde_json::from_value(source["selected_vertices"].clone()).unwrap();
    assert!(selected.len() < all_selected.len());
    for (frame, all) in partial["frames"]
        .as_array()
        .unwrap()
        .iter()
        .zip(baseline["frames"].as_array().unwrap())
    {
        let expected: Vec<_> = selected
            .iter()
            .map(|v| {
                all["body_force_on_selected_vertices_n"][all_selected.binary_search(v).unwrap()]
                    .clone()
            })
            .collect();
        assert_eq!(frame["body_force_on_selected_vertices_n"], json!(expected));
    }
    for key in [
        "peak_body_resultant_n",
        "peak_body_resultant_sample",
        "peak_body_sum_vertex_force_n",
        "peak_body_sum_vertex_force_sample",
        "max_body_vertex_force_n",
        "max_body_vertex_force_sample",
        "max_force_vertex",
    ] {
        assert_eq!(partial[key], partial["regions"]["left"][key], "{key}");
    }
}

#[test]
fn contact_loads_follow_adaptive_barrier_kappa_during_motion() {
    let form = form();
    let body = body(&form);
    let p = pattern();
    let mut s = motion_scenario();
    // Wide barrier and aggressive adaptive growth force kappa to escalate during motion so
    // stale t=0 kappa evaluation diverges visibly from the actual solver equilibrium forces.
    s.options.contact.dhat = 0.02;
    s.options.adaptive_barrier = Some(ftx_drape::AdaptiveBarrierPolicy {
        target_gap_fraction: 0.9,
        min_gap_fraction: 0.85,
        growth_factor: 4.0,
        membrane_floor_scale: 1e-6,
        kappa_min: 1e-3,
        kappa_max: 1e6,
    });
    s.contact_loads = Some(request());
    // Push the cylinder slightly along +x to tighten the contact gap and force kappa growth.
    let endpoints: Vec<Vec<Vec3>> = [0.002, 0.004]
        .into_iter()
        .map(|dx| {
            body.collision
                .0
                .iter()
                .map(|v| *v + Vec3::new(dx, 0.0, 0.0))
                .collect()
        })
        .collect();
    let motion_path = ftx_drape::study::motion::Path::new(
        s.motion.as_ref().unwrap(),
        body.collision,
        endpoints.clone(),
    )
    .unwrap();
    let mut session = Session::new(&s, &body).unwrap();
    session.set_motion_path(Ok(motion_path)).unwrap();
    let result = session.evaluate(&p, |_| {}).unwrap();
    assert_eq!(
        result["motion"]["status"], "available",
        "{}",
        result["motion"]
    );
    let outcome = &result["motion"]["result"]["contact_loads"];
    assert_eq!(outcome["status"], "available", "{outcome}");
    let history = &outcome["result"];
    let frames = history["frames"].as_array().unwrap();
    assert!(frames.len() >= 2, "expected initial + motion frames");

    // Replay the exact initial drape + motion solver independently to observe per-substep kappa.
    let mut run = drape(&p, &s.fit_spec, &body, &s.options, |_| {}).unwrap();
    let initial_contact = run.scene.solver.contact.as_ref().unwrap().clone();
    let initial_kappa = initial_contact.params.kappa;
    let req = s.motion.as_ref().unwrap();
    let motion_opts = ftx_drape::motion::MotionOptions {
        max_step_s: req.max_step_s,
        max_trials: req.max_trials,
        max_pairs: req.max_pairs,
        max_sample_vertices: req.max_sample_vertices,
    };
    let mut all_samples = Vec::new();
    let mut prev_t = 0.0;
    for (idx, frame) in req.keyframes.iter().enumerate() {
        let rep = ftx_drape::motion::advance_body(
            &mut run.scene.solver,
            &endpoints[idx],
            frame.time_s - prev_t,
            motion_opts,
        )
        .unwrap();
        all_samples.extend(rep.samples);
        prev_t = frame.time_s;
    }
    assert_eq!(frames.len(), all_samples.len() + 1);
    let selected: Vec<usize> =
        serde_json::from_value(history["source"]["selected_vertices"].clone()).unwrap();
    let mut escalated = false;
    for (frame, sample) in frames.iter().skip(1).zip(&all_samples) {
        let bk = sample
            .body_kappa_n_m
            .expect("adaptive barrier motion sample must record body_kappa_n_m");
        assert!(sample.self_kappa_n_m.is_none());
        if bk > initial_kappa {
            escalated = true;
        }
        let mut expected_contact = initial_contact.clone();
        expected_contact.params.kappa = bk;
        expected_contact.body = Body::new(
            sample.body.clone(),
            form.triangles.clone(),
            expected_contact.params.dhat,
        )
        .unwrap();
        let expected_map = expected_contact.reaction_map(&sample.cloth).unwrap();
        let expected_all: Vec<Vec3> =
            serde_json::from_value(expected_map["body_force_on_cloth_n"].clone()).unwrap();
        let expected_sel: Vec<_> = selected.iter().map(|&v| expected_all[v]).collect();
        assert_eq!(
            frame["active_policy_sha256"],
            expected_map["active_policy_sha256"]
        );
        assert_eq!(
            frame["body_force_on_selected_vertices_n"],
            json!(expected_sel)
        );
    }
    assert!(
        escalated,
        "test must exercise real adaptive-barrier kappa escalation during motion (initial {initial_kappa})"
    );
    // The initial frame's policy hash must differ from the escalated frame's policy hash.
    assert_ne!(
        frames[0]["active_policy_sha256"],
        frames.last().unwrap()["active_policy_sha256"]
    );
}

use super::*;

fn request(max: usize) -> ftx_drape::study::strain::Request {
    serde_json::from_value(json!({"schema":"ftx-material-strain/1",
        "max_triangle_samples":max}))
    .unwrap()
}

fn extrema(elements: &[ftx_drape::elements::MembraneElement], positions: &[Vec3]) -> [[f64; 2]; 3] {
    let mut ranges = [[f64::INFINITY, f64::NEG_INFINITY]; 3];
    for element in elements {
        for (i, value) in element.strain(positions).into_iter().enumerate() {
            ranges[i][0] = ranges[i][0].min(value);
            ranges[i][1] = ranges[i][1].max(value);
        }
    }
    ranges
}

#[test]
fn material_strain_uses_actual_grain_frames_and_replays_static_source_identity() {
    let form = form();
    let body = body(&form);
    let p = pattern();
    let mut s = scenario();
    s.material_strain = Some(request(100_000));
    let mut session = Session::new(&s, &body).unwrap();
    let result = session.evaluate(&p, |_| {}).unwrap();
    let strain = &result["material_strain"];
    assert_eq!(strain["status"], "available", "{strain}");
    let report = &strain["result"];
    let direct = drape(&p, &s.fit_spec, &body, &s.options, |_| {}).unwrap();
    assert_eq!(result["drape"], direct.report);
    assert_eq!(
        result["material_strain_ownership"],
        json!({
        "instances":direct.scene.mesh.instances,
        "triangle_instance":direct.scene.triangle_owner})
    );
    assert_eq!(
        report["source_sha256"],
        ftx_core::receipt::sha256_hex(&serde_json::to_vec(&report["source"]).unwrap())
    );
    assert_eq!(
        report["green_strain_range"],
        json!(extrema(
            &direct.scene.solver.model.membranes,
            &direct.scene.solver.x
        ))
    );
    assert_eq!(
        report["initial_positions_sha256"],
        result["positions_sha256"]
    );
    assert_eq!(report["scope"], "initial_state");
    assert_eq!(report["unit"], "1");
    assert_eq!(report["source"]["instances"], json!(["band"]));
    assert_eq!(
        report["source"]["triangles"].as_array().unwrap().len(),
        direct.scene.solver.model.membranes.len()
    );
    for (i, (source, element)) in report["source"]["triangles"]
        .as_array()
        .unwrap()
        .iter()
        .zip(&direct.scene.solver.model.membranes)
        .enumerate()
    {
        assert_eq!(source["source_triangle"], i);
        assert_eq!(source["vertices"], json!(element.v));
        assert_eq!(source["material_gradients_m_inv"], json!(element.g));
        assert_eq!(source["fitted_strain_range"], Value::Null);
        assert_eq!(source["nonlinear_strain_range"], Value::Null);
    }
    assert_eq!(session.evaluate(&p, |_| {}).unwrap(), result);
    let mut raw = p.raw().clone();
    raw["fabric"]["material"]["record"]["sources"]["test"]["note"] =
        json!("A separately retained synthetic source revision");
    let revised = session.evaluate(&Pattern::new(raw), |_| {}).unwrap();
    assert_eq!(revised["material_strain"]["status"], "available");
    assert_eq!(revised["positions_sha256"], result["positions_sha256"]);
    assert_ne!(
        revised["material_strain"]["result"]["source_sha256"],
        report["source_sha256"]
    );
}

#[test]
fn material_strain_retains_every_accepted_motion_sample_and_budget_refusals_are_independent() {
    let form = form();
    let body = body(&form);
    let p = pattern();
    let mut s = motion_scenario();
    s.material_strain = Some(request(100_000));
    let mut session = Session::new(&s, &body).unwrap();
    session.set_motion_path(Ok(path(&s, &body))).unwrap();
    let result = session.evaluate(&p, |_| {}).unwrap();
    assert_eq!(
        result["motion"]["status"], "available",
        "{}",
        result["motion"]
    );
    let motion = &result["motion"]["result"];
    let outcome = &motion["material_strain"];
    assert_eq!(outcome["status"], "available", "{outcome}");
    let report = &outcome["result"];
    let direct = drape(&p, &s.fit_spec, &body, &s.options, |_| {}).unwrap();
    let samples: Vec<_> = std::iter::once(&motion["initial_sample"])
        .chain(motion["samples"].as_array().unwrap())
        .collect();
    let frames = report["frames"].as_array().unwrap();
    assert_eq!(frames.len(), samples.len());
    let mut all = [[f64::INFINITY, f64::NEG_INFINITY]; 3];
    for (i, (frame, sample)) in frames.iter().zip(samples).enumerate() {
        let x: Vec<Vec3> = serde_json::from_value(sample["positions_m"].clone()).unwrap();
        let ranges = extrema(&direct.scene.solver.model.membranes, &x);
        assert_eq!(frame["sample_index"], i);
        assert_eq!(frame["time_s"], sample["time_s"]);
        assert_eq!(frame["green_strain_range"], json!(ranges));
        for j in 0..3 {
            all[j][0] = all[j][0].min(ranges[j][0]);
            all[j][1] = all[j][1].max(ranges[j][1]);
        }
    }
    assert_eq!(report["green_strain_range"], json!(all));
    assert_eq!(
        report["source"],
        result["material_strain"]["result"]["source"]
    );
    assert_eq!(
        report["triangle_samples"],
        frames.len() * direct.scene.solver.model.membranes.len()
    );
    assert_eq!(report["duration_s"], motion["duration_s"]);
    assert_eq!(
        report["final_positions_sha256"],
        motion["final_positions_sha256"]
    );

    s.material_strain.as_mut().unwrap().max_triangle_samples =
        direct.scene.solver.model.membranes.len();
    let mut limited = Session::new(&s, &body).unwrap();
    limited.set_motion_path(Ok(path(&s, &body))).unwrap();
    let limited = limited.evaluate(&p, |_| {}).unwrap();
    assert_eq!(limited["material_strain"]["status"], "available");
    assert_eq!(limited["motion"]["status"], "available");
    assert_eq!(
        limited["motion"]["result"]["material_strain"]["status"],
        "refused"
    );
    assert_eq!(
        limited["motion"]["result"]["final_positions_sha256"],
        motion["final_positions_sha256"]
    );
    assert_eq!(limited["surface"], result["surface"]);
}

#[test]
fn material_strain_missing_scope_and_budget_do_not_erase_drape_or_heat() {
    let form = form();
    let body = body(&form);
    let p = pattern();
    let mut s = scenario();
    let original = Session::new(&s, &body)
        .unwrap()
        .evaluate(&p, |_| {})
        .unwrap();
    assert!(original.get("material_strain_ownership").is_none());
    for r in [
        json!({"schema":"ftx-material-strain/1","instances":["absent"],"max_triangle_samples":1000}),
        json!({"schema":"ftx-material-strain/1","instances":["band","band"],"max_triangle_samples":1000}),
        json!({"schema":"ftx-material-strain/1","max_triangle_samples":1}),
    ] {
        s.material_strain = Some(serde_json::from_value(r).unwrap());
        let result = Session::new(&s, &body)
            .unwrap()
            .evaluate(&p, |_| {})
            .unwrap();
        assert_eq!(result["material_strain"]["status"], "refused");
        assert_eq!(result["drape"], original["drape"]);
        assert_eq!(result["surface"], original["surface"]);
    }
}

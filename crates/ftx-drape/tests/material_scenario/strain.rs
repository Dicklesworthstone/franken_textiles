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

fn regional_request(max: usize) -> ftx_drape::study::strain::Request {
    let width = 40. * std::f64::consts::TAU;
    serde_json::from_value(json!({"schema":"ftx-material-strain/2",
    "max_triangle_samples":max,"regions":[
        {"id":"left","instance":"band","selector":{"kind":"whole_triangle_centroid",
            "polygon_mm":[[-1.,-1.],[width*0.25,-1.],[width*0.25,21.],[-1.,21.]]}},
        {"id":"right","instance":"band","selector":{"kind":"whole_triangle_centroid",
            "polygon_mm":[[width*0.75,-1.],[width+1.,-1.],[width+1.,21.],[width*0.75,21.]]}}
    ]}))
    .unwrap()
}

#[test]
fn regional_material_strain_preserves_whole_panel_and_samples_only_declared_flat_regions() {
    let form = form();
    let body = body(&form);
    let p = pattern();
    let mut s = motion_scenario();
    s.material_strain = Some(regional_request(100_000));
    let mut session = Session::new(&s, &body).unwrap();
    session.set_motion_path(Ok(path(&s, &body))).unwrap();
    let result = session.evaluate(&p, |_| {}).unwrap();
    let direct = drape(&p, &s.fit_spec, &body, &s.options, |_| {}).unwrap();
    let initial = &result["material_strain"]["result"];
    assert_eq!(result["material_strain"]["status"], "available", "{result}");
    assert_eq!(
        result["drape"], direct.report,
        "regional inspection does not split or alter the panel"
    );
    let flat: Vec<[ftx_core::Vec2; 3]> = direct
        .scene
        .mesh
        .faces
        .iter()
        .filter_map(|face| {
            let welded = face.map(|v| direct.scene.weld[v]);
            if welded[0] != welded[1] && welded[1] != welded[2] && welded[0] != welded[2] {
                Some(face.map(|v| direct.scene.mesh.verts2d[v]))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        result["material_strain_ownership"]["rest_triangles_mm"],
        json!(flat)
    );
    let width = 40. * std::f64::consts::TAU;
    let left: Vec<_> = flat
        .iter()
        .enumerate()
        .filter_map(|(i, triangle)| {
            (triangle.iter().map(|p| p.x).sum::<f64>() <= 3. * width * 0.25).then_some(i)
        })
        .collect();
    let right: Vec<_> = flat
        .iter()
        .enumerate()
        .filter_map(|(i, triangle)| {
            (triangle.iter().map(|p| p.x).sum::<f64>() >= 3. * width * 0.75).then_some(i)
        })
        .collect();
    assert!(!left.is_empty() && !right.is_empty());
    assert!(
        left.len() + right.len() < flat.len(),
        "middle triangles remain outside the scope"
    );
    assert_eq!(
        initial["source"]["regions"]["left"]["source_triangles"],
        json!(left)
    );
    assert_eq!(
        initial["source"]["regions"]["right"]["source_triangles"],
        json!(right)
    );
    let selected: Vec<_> = (0..flat.len())
        .filter(|i| left.contains(i) || right.contains(i))
        .collect();
    let motion = &result["motion"]["result"];
    assert_eq!(
        result["motion"]["status"], "available",
        "{}",
        result["motion"]
    );
    let report = &motion["material_strain"]["result"];
    assert_eq!(
        motion["material_strain"]["status"], "available",
        "{}",
        motion["material_strain"]
    );
    assert_eq!(initial["schema"], "ftx-material-strain-report/2");
    assert_eq!(report["source"], initial["source"]);
    assert_eq!(
        report["source_sha256"],
        ftx_core::receipt::sha256_hex(&serde_json::to_vec(&report["source"]).unwrap())
    );
    let samples: Vec<_> = std::iter::once(&motion["initial_sample"])
        .chain(motion["samples"].as_array().unwrap())
        .collect();
    for (id, indices) in [("left", &left), ("right", &right)] {
        let elements: Vec<_> = indices
            .iter()
            .map(|&i| direct.scene.solver.model.membranes[i])
            .collect();
        let area: f64 = elements.iter().map(|element| element.area).sum();
        assert_eq!(report["source"]["regions"][id]["reference_area_m2"], area);
        let mut ranges = [[f64::INFINITY, f64::NEG_INFINITY]; 3];
        for sample in &samples {
            let positions: Vec<Vec3> =
                serde_json::from_value(sample["positions_m"].clone()).unwrap();
            let at = extrema(&elements, &positions);
            for axis in 0..3 {
                ranges[axis][0] = ranges[axis][0].min(at[axis][0]);
                ranges[axis][1] = ranges[axis][1].max(at[axis][1]);
            }
        }
        assert_eq!(report["regions"][id]["green_strain_range"], json!(ranges));
        for key in ["minimum_witness", "maximum_witness"] {
            for witness in report["regions"][id][key].as_array().unwrap() {
                assert!(indices.contains(&(witness["source_triangle"].as_u64().unwrap() as usize)));
            }
        }
    }
    let selected_elements: Vec<_> = selected
        .iter()
        .map(|&i| direct.scene.solver.model.membranes[i])
        .collect();
    for (frame, sample) in report["frames"].as_array().unwrap().iter().zip(&samples) {
        let positions: Vec<Vec3> = serde_json::from_value(sample["positions_m"].clone()).unwrap();
        assert_eq!(
            frame["green_strain_range"],
            json!(extrema(&selected_elements, &positions))
        );
    }
    assert_eq!(report["triangle_samples"], selected.len() * samples.len());
    assert_eq!(
        report["instances"]["band"]["green_strain_range"],
        report["green_strain_range"]
    );
    for triangle in report["source"]["triangles"].as_array().unwrap() {
        let index = triangle["source_triangle"].as_u64().unwrap() as usize;
        assert_eq!(
            triangle["region"],
            if left.contains(&index) {
                "left"
            } else {
                "right"
            }
        );
    }

    let mut raw = p.raw().clone();
    raw["fabric"]["material"]["record"]["sources"]["test"]["note"] =
        json!("Independent synthetic source revision");
    let revised = session.evaluate(&Pattern::new(raw), |_| {}).unwrap();
    assert_eq!(revised["material_strain"]["status"], "available");
    assert_eq!(
        revised["material_strain_ownership"],
        result["material_strain_ownership"]
    );
    assert_eq!(
        revised["material_strain"]["result"]["regions"],
        initial["regions"]
    );
    assert_ne!(
        revised["material_strain"]["result"]["source_sha256"],
        initial["source_sha256"]
    );
}

#[test]
fn regional_strain_refusals_preserve_independent_consumers_and_partial_motion_is_unavailable() {
    let form = form();
    let body = body(&form);
    let p = pattern();
    let mut s = motion_scenario();
    s.material_strain = Some(regional_request(100_000));
    let mut session = Session::new(&s, &body).unwrap();
    session.set_motion_path(Ok(path(&s, &body))).unwrap();
    let complete = session.evaluate(&p, |_| {}).unwrap();
    let selected = complete["material_strain"]["result"]["triangle_samples"]
        .as_u64()
        .unwrap() as usize;
    assert!(selected > 0);
    for variant in 0..5 {
        let mut request = regional_request(100_000);
        let regions = request.regions.as_mut().unwrap();
        match variant {
            0 => regions[0].instance = "absent".into(),
            1 => regions[1].selector = regions[0].selector.clone(),
            2 => {
                for vertex in &mut regions[0].selector.polygon_mm {
                    vertex.x += 1000.;
                }
            }
            3 => request.max_triangle_samples = selected - 1,
            _ => request.max_triangle_samples = selected,
        }
        s.material_strain = Some(request);
        let mut session = Session::new(&s, &body).unwrap();
        session.set_motion_path(Ok(path(&s, &body))).unwrap();
        let result = session.evaluate(&p, |_| {}).unwrap();
        assert_eq!(
            result["material_strain"]["status"],
            if variant == 4 { "available" } else { "refused" }
        );
        assert_eq!(result["motion"]["status"], "available");
        assert_eq!(
            result["motion"]["result"]["material_strain"]["status"],
            "refused"
        );
        assert!(result["motion"]["result"]["material_strain"]["result"].is_null());
        assert_eq!(result["drape"], complete["drape"]);
        assert_eq!(result["surface"], complete["surface"]);
        assert_eq!(
            result["motion"]["result"]["final_positions_sha256"],
            complete["motion"]["result"]["final_positions_sha256"]
        );
    }
}

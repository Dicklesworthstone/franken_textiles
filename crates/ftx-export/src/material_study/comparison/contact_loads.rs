//! Reconcile trusted production force receipts with the bound material, mesh and accepted path.
//! Arithmetic replay checks forces and extrema; it is not an independent contact solve.

use super::*;
use ftx_core::Vec3;

fn fail() -> Refusal {
    invalid(
        "contact-load comparison requires source-bound instance ownership, complete accepted geometry/force samples and consistent extrema; discrete forces do not establish pressure or comfort",
    )
}
fn array<'a>(value: &'a Value, key: &str) -> Result<&'a [Value], Refusal> {
    vapor_array(value, key).map_err(|_| fail())
}
fn index(value: &Value) -> Result<usize, Refusal> {
    vapor_index(value).map_err(|_| fail())
}
fn digest(value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn source(
    alternative: &Alternative,
    policy: &Value,
    simulation: &Value,
    report: &Value,
) -> Result<(usize, usize, Vec<usize>), Refusal> {
    let source = &report["source"];
    let ownership = &simulation["contact_load_ownership"];
    let material = &simulation["drape"]["material"];
    let names = array(ownership, "instances")?;
    let owners = array(ownership, "triangle_instance")?;
    let faces = array(simulation, "triangles")?;
    let vertices = index(&source["vertex_count"])?;
    let body_vertices = index(&source["body_vertex_count"])?;
    if !(3..=196_608).contains(&vertices)
        || !(3..=196_608).contains(&body_vertices)
        || vertices != array(simulation, "positions_m")?.len()
        || faces.is_empty()
        || faces.len() > 65_536
        || owners.len() != faces.len()
        || names.is_empty()
        || names.len() > 196_608
        || source["all_instances"] != ownership["instances"]
        || source["triangle_instance"] != ownership["triangle_instance"]
        || source["material_report_sha256"] != hash(material)?
        || report["source_sha256"] != hash(source)?
    {
        return Err(fail());
    }
    let body_faces = array(source, "body_triangles")?;
    if body_faces.is_empty() || body_faces.len() > 65_536 {
        return Err(fail());
    }
    for face in body_faces {
        let face: [usize; 3] = serde_json::from_value(face.clone()).map_err(|_| fail())?;
        if face.iter().any(|&v| v >= body_vertices) {
            return Err(fail());
        }
    }
    let bound = alternative.binding.value().ok_or_else(fail)?;
    let pattern = ftx_pattern::Pattern::new(bound["pattern"].clone());
    let changes = alternative.frozen_changes.as_ref().ok_or_else(fail)?;
    let mut all_names = BTreeSet::new();
    for name in names {
        let id = name.as_str().ok_or_else(fail)?;
        if !all_names.insert(id) || pattern.instance(id).is_none() {
            return Err(fail());
        }
    }
    let mut retained = BTreeSet::new();
    for owner in owners {
        retained.insert(
            names
                .get(index(owner)?)
                .and_then(Value::as_str)
                .ok_or_else(fail)?,
        );
    }
    let selected = if let Some(ids) = policy.get("instances") {
        let mut selected = BTreeSet::new();
        for name in ids.as_array().ok_or_else(fail)? {
            let name = name.as_str().ok_or_else(fail)?;
            if !retained.contains(name) || !selected.insert(name) {
                return Err(fail());
            }
        }
        selected
    } else {
        retained
    };
    if selected.is_empty() || selected.len() > 4096 {
        return Err(fail());
    }
    let actual = source["instances"].as_object().ok_or_else(fail)?;
    if actual.keys().map(String::as_str).collect::<BTreeSet<_>>() != selected {
        return Err(fail());
    }
    let catalog = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: vec![],
    };
    for name in &selected {
        let piece = &pattern.instance(name).ok_or_else(fail)?.piece;
        let selection = changes
            .pieces
            .get(piece)
            .or(changes.default.as_ref())
            .ok_or_else(fail)?;
        let resolved = catalog.resolve(selection)?;
        if actual[*name] != resolved.sha256() {
            return Err(fail());
        }
        let receipt = match material["basis"].as_str() {
            Some("characterized_record") => material,
            Some("per_piece_characterization") => {
                let pos = names.iter().position(|id| id == name).ok_or_else(fail)?;
                if material["instances"][pos]["instance"] != *name
                    || material["instances"][pos]["record_sha256"] != resolved.sha256()
                    || material["triangle_instance"] != ownership["triangle_instance"]
                {
                    return Err(fail());
                }
                &material["records"][resolved.sha256()]
            }
            _ => return Err(fail()),
        };
        if receipt["record_sha256"] != resolved.sha256()
            || receipt["record"] != serde_json::to_value(&resolved).map_err(|_| fail())?
        {
            return Err(fail());
        }
    }
    let mut selected_vertices = BTreeSet::new();
    for (face, owner) in faces.iter().zip(owners) {
        let name = names[index(owner)?].as_str().ok_or_else(fail)?;
        let face: [usize; 3] = serde_json::from_value(face.clone()).map_err(|_| fail())?;
        if face.iter().any(|&v| v >= vertices) {
            return Err(fail());
        }
        if selected.contains(name) {
            selected_vertices.extend(face);
        }
    }
    let selected_vertices: Vec<_> = selected_vertices.into_iter().collect();
    if selected_vertices.is_empty() || source["selected_vertices"] != json!(selected_vertices) {
        return Err(fail());
    }
    Ok((vertices, body_vertices, selected_vertices))
}

/// Drape and complete motion are admitted by the caller before this optional force consumer.
pub(super) fn admit<'a>(
    alternative: &Alternative,
    descriptor: &Value,
    simulation: &'a Value,
    motion: Option<&'a Value>,
) -> Result<&'a Value, Refusal> {
    let policy = &descriptor["scenario"]["contact_loads"];
    let object = policy.as_object().ok_or_else(fail)?;
    let max_vertices = index(&policy["max_vertex_samples"])?;
    let max_pairs = index(&policy["max_pair_evaluations"])?;
    let outcome = &motion.unwrap_or(simulation)["contact_loads"];
    let report = &outcome["result"];
    if policy["schema"] != "ftx-contact-loads/1"
        || !(1..=64_000_000).contains(&max_vertices)
        || !(1..=64_000_000).contains(&max_pairs)
        || object.keys().any(|k| {
            ![
                "schema",
                "instances",
                "max_vertex_samples",
                "max_pair_evaluations",
            ]
            .contains(&k.as_str())
        })
        || outcome["status"] != "available"
        || report["schema"] != "ftx-contact-loads-report/1"
        || report["request"] != *policy
        || report["unit"] != "N"
        || report["scope"]
            != if motion.is_some() {
                "accepted_motion_samples"
            } else {
                "initial_state"
            }
        || [
            "physical_validation_claim",
            "pressure_claim",
            "comfort_claim",
            "continuous_time_bound",
        ]
        .iter()
        .any(|k| report[*k] != false)
    {
        return Err(fail());
    }
    let initial = motion
        .map(|_| admit(alternative, descriptor, simulation, None))
        .transpose()?;
    let (vertices, body_vertices, selected) = source(alternative, policy, simulation, report)?;
    let frames = array(report, "frames")?;
    let samples = motion.map_or(Ok(&[][..]), |m| array(m, "samples"))?;
    let map_outcome = &simulation["drape"]["contact_diagnostics"]["final_reactions"];
    let map = &map_outcome["result"];
    if frames.len() != samples.len() + 1
        || frames.len() > 513
        || frames.len().saturating_mul(vertices + body_vertices) > max_vertices
        || report["vertex_samples"] != frames.len() * (vertices + body_vertices)
        || map_outcome["status"] != "available"
        || map["schema"] != "ftx-contact-reactions/1"
        || map["positions_m"] != simulation["positions_m"]
        || map["triangles"] != simulation["triangles"]
        || map["material_report_sha256"] != report["source"]["material_report_sha256"]
        || descriptor["contact_body"]["vertex_count"] != body_vertices
        || descriptor["contact_body"]["triangles_sha256"]
            != hash(&report["source"]["body_triangles"])?
        || descriptor["contact_body"]["collision_sha256"] != map["body_state_sha256"]
        || initial.is_some_and(|initial| {
            report["source"] != initial["source"] || frames[0] != initial["frames"][0]
        })
    {
        return Err(fail());
    }
    if let Some(motion) = motion
        && (samples.is_empty()
            || motion["triangles"] != simulation["triangles"]
            || motion["body_triangles"] != report["source"]["body_triangles"])
    {
        return Err(fail());
    }
    let all_initial: Vec<Vec3> =
        serde_json::from_value(map["body_force_on_cloth_n"].clone()).map_err(|_| fail())?;
    if all_initial.len() != vertices {
        return Err(fail());
    }
    let mut pairs = 0usize;
    let mut peak_resultant = 0.;
    let mut peak_vertex = 0.;
    let mut resultant_sample = 0;
    let mut vertex_sample = 0;
    let mut previous = 0.;
    for (i, frame) in frames.iter().enumerate() {
        let positions = if i == 0 {
            &simulation["positions_m"]
        } else {
            &samples[i - 1]["positions_m"]
        };
        let time = if i == 0 {
            0.
        } else {
            finite(&samples[i - 1]["time_s"]).ok_or_else(fail)?
        };
        let forces: Vec<Vec3> =
            serde_json::from_value(frame["body_force_on_selected_vertices_n"].clone())
                .map_err(|_| fail())?;
        if forces.len() != selected.len()
            || frame["sample_index"] != i
            || frame["time_s"].as_f64() != Some(time)
            || positions.as_array().map(Vec::len) != Some(vertices)
            || frame["positions_sha256"] != hash(positions)?
            || !digest(&frame["active_policy_sha256"])
            || (i > 0 && (time <= previous || samples[i - 1]["solve"]["converged"] != true))
        {
            return Err(fail());
        }
        if i == 0
            && (frame["body_state_sha256"] != map["body_state_sha256"]
                || frame["active_policy_sha256"] != map["active_policy_sha256"]
                || forces
                    .iter()
                    .zip(&selected)
                    .any(|(force, &v)| *force != all_initial[v]))
        {
            return Err(fail());
        }
        if let Some(motion) = motion {
            let body = if i == 0 {
                &motion["initial_sample"]["body_positions_m"]
            } else {
                &samples[i - 1]["body_positions_m"]
            };
            if body.as_array().map(Vec::len) != Some(body_vertices)
                || frame["body_state_sha256"]
                    != hash(&json!([body, report["source"]["body_triangles"]]))?
            {
                return Err(fail());
            }
        }
        let mut resultant = Vec3::ZERO;
        let mut maximum = 0.;
        let mut witness = selected[0];
        for (&vertex, &force) in selected.iter().zip(&forces) {
            let norm = force.norm();
            if !force.is_finite() || !norm.is_finite() {
                return Err(fail());
            }
            resultant += force;
            if norm > maximum {
                maximum = norm;
                witness = vertex;
            }
        }
        let norm = resultant.norm();
        if !norm.is_finite()
            || frame["body"]
                != json!({"resultant_n":resultant,
            "resultant_magnitude_n":norm,"max_vertex_force_n":maximum,"max_force_vertex":witness})
        {
            return Err(fail());
        }
        if norm > peak_resultant {
            peak_resultant = norm;
            resultant_sample = i;
        }
        if maximum > peak_vertex {
            peak_vertex = maximum;
            vertex_sample = i;
        }
        pairs = pairs
            .checked_add(index(&frame["candidate_pairs"])?)
            .ok_or_else(fail)?;
        if pairs > max_pairs {
            return Err(fail());
        }
        previous = time;
    }
    if report["peak_body_resultant_n"].as_f64() != Some(peak_resultant)
        || report["max_body_vertex_force_n"].as_f64() != Some(peak_vertex)
        || report["peak_body_resultant_sample"] != resultant_sample
        || report["max_body_vertex_force_sample"] != vertex_sample
        || report["pair_evaluations"] != pairs
        || report["duration_s"].as_f64() != Some(previous)
        || report["initial_positions_sha256"] != frames[0]["positions_sha256"]
        || report["final_positions_sha256"] != frames.last().ok_or_else(fail)?["positions_sha256"]
        || motion.is_some_and(|m| {
            report["duration_s"] != m["duration_s"]
                || report["final_positions_sha256"] != m["final_positions_sha256"]
        })
    {
        return Err(fail());
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Receipt/arithmetic fixture only: actual contact derivatives and sewn motion are tested in drape.
    fn case() -> (Alternative, Value, Value, Value) {
        let pattern = ftx_pattern::Pattern::from_json(include_str!(
            "../../../../../tests/fixtures/materials/synthetic-sewing-pattern.json"
        ))
        .unwrap();
        let request: Request = serde_json::from_value(json!({"schema":"ftx-material-study/1",
            "cutting":{"copies":1,"gap_mm":1.,"edge_margin_mm":0.,"end_margin_mm":0.,"seam_allowance_mm":0.},
            "variants":[{"key":"source","changes":{"schema":"ftx-material-bindings/1",
                "default":pattern.raw()["fabric"]["material"]}}]}))
        .unwrap();
        let mut manufacture = request.evaluate(&pattern).unwrap();
        let alternative = manufacture.alternatives.remove(0);
        let selected = alternative
            .frozen_changes
            .as_ref()
            .unwrap()
            .default
            .as_ref()
            .unwrap();
        let record = Catalog::builtin().unwrap().resolve(selected).unwrap();
        let material =
            json!({"basis":"characterized_record","record":record,"record_sha256":record.sha256()});
        let policy = json!({"schema":"ftx-contact-loads/1","instances":["front"],
            "max_vertex_samples":100,"max_pair_evaluations":100});
        let positions = json!([[0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [1., 1., 0.]]);
        let faces = json!([[0, 1, 2], [0, 2, 3]]);
        let body = json!([[0., 0., -1.], [1., 0., -1.], [0., 1., -1.]]);
        let moved = json!([[0., 0., -0.9], [1., 0., -0.9], [0., 1., -0.9]]);
        let body_faces = json!([[0, 1, 2]]);
        let ownership = json!({"instances":["front","back"],"triangle_instance":[0,1]});
        let source = json!({"vertex_count":4,"body_vertex_count":3,"body_triangles":body_faces,
            "all_instances":ownership["instances"],"triangle_instance":ownership["triangle_instance"],
            "instances":{"front":record.sha256()},"selected_vertices":[0,1,2],
            "material_report_sha256":hash(&material).unwrap()});
        let policy_hash = "a".repeat(64);
        let first = json!({"sample_index":0,"time_s":0.,"positions_sha256":hash(&positions).unwrap(),
            "body_state_sha256":hash(&json!([body,body_faces])).unwrap(),"active_policy_sha256":policy_hash,
            "candidate_pairs":3,"body_force_on_selected_vertices_n":[[3.,4.,0.],[-3.,-4.,0.],[0.,0.,2.]],
            "body":{"resultant_n":[0.,0.,2.],"resultant_magnitude_n":2.,"max_vertex_force_n":5.,"max_force_vertex":0}});
        let second = json!({"sample_index":1,"time_s":1.,"positions_sha256":hash(&positions).unwrap(),
            "body_state_sha256":hash(&json!([moved,body_faces])).unwrap(),"active_policy_sha256":policy_hash,
            "candidate_pairs":3,"body_force_on_selected_vertices_n":[[0.,0.,6.],[0.,0.,0.],[0.,0.,0.]],
            "body":{"resultant_n":[0.,0.,6.],"resultant_magnitude_n":6.,"max_vertex_force_n":6.,"max_force_vertex":0}});
        let make_report = |motion: bool| {
            json!({"schema":"ftx-contact-loads-report/1","request":policy,
            "source":source,"source_sha256":hash(&source).unwrap(),"unit":"N",
            "scope":if motion{"accepted_motion_samples"}else{"initial_state"},
            "frames":if motion{json!([first,second])}else{json!([first])},
            "initial_positions_sha256":hash(&positions).unwrap(),"final_positions_sha256":hash(&positions).unwrap(),
            "duration_s":if motion{1.}else{0.},"vertex_samples":if motion{14}else{7},
            "pair_evaluations":if motion{6}else{3},"peak_body_resultant_n":if motion{6.}else{2.},
            "max_body_vertex_force_n":if motion{6.}else{5.},"peak_body_resultant_sample":usize::from(motion),
            "max_body_vertex_force_sample":usize::from(motion),"physical_validation_claim":false,
            "pressure_claim":false,"comfort_claim":false,"continuous_time_bound":false})
        };
        let simulation = json!({"positions_m":positions,"triangles":faces,"contact_load_ownership":ownership,
            "drape":{"material":material,"contact_diagnostics":{"final_reactions":{"status":"available","result":{
                "schema":"ftx-contact-reactions/1","positions_m":positions,"triangles":faces,
                "material_report_sha256":hash(&material).unwrap(),"body_state_sha256":first["body_state_sha256"],
                "active_policy_sha256":policy_hash,"body_force_on_cloth_n":[[3.,4.,0.],[-3.,-4.,0.],[0.,0.,2.],[0.,0.,99.]]}}}},
            "contact_loads":{"status":"available","result":make_report(false)}});
        let motion = json!({"triangles":faces,"body_triangles":body_faces,"duration_s":1.,
            "final_positions_sha256":hash(&positions).unwrap(),"initial_sample":{"body_positions_m":body},
            "samples":[{"positions_m":positions,"body_positions_m":moved,"time_s":1.,"solve":{"converged":true}}],
            "contact_loads":{"status":"available","result":make_report(true)}});
        (
            alternative,
            json!({"scenario":{"contact_loads":policy},"contact_body":{
                "collision_sha256":hash(&json!([body,body_faces])).unwrap(),"vertex_count":3,
                "triangles_sha256":hash(&body_faces).unwrap()}}),
            simulation,
            motion,
        )
    }

    #[test]
    fn contact_receipts_admit_scoped_welded_forces_and_complete_moving_body_identity() {
        let (alternative, descriptor, simulation, motion) = case();
        let initial = admit(&alternative, &descriptor, &simulation, None).unwrap();
        assert_eq!(initial["peak_body_resultant_n"], 2.);
        assert_eq!(initial["max_body_vertex_force_n"], 5.);
        let history = admit(&alternative, &descriptor, &simulation, Some(&motion)).unwrap();
        assert_eq!(history["peak_body_resultant_n"], 6.);
        assert_eq!(history["max_body_vertex_force_sample"], 1);
    }

    #[test]
    fn stale_scope_force_extrema_and_incomplete_motion_cannot_be_ranked() {
        let (alternative, descriptor, simulation, motion) = case();
        for (pointer, value) in [
            (
                "/contact_loads/result/source/selected_vertices",
                json!([0, 0, 1, 2]),
            ),
            (
                "/contact_loads/result/source/material_report_sha256",
                json!("stale"),
            ),
            ("/contact_loads/result/max_body_vertex_force_n", json!(0.)),
            (
                "/contact_loads/result/frames/0/body/resultant_magnitude_n",
                json!(0.),
            ),
            (
                "/contact_loads/result/frames/0/body_force_on_selected_vertices_n/0",
                json!([0., 0., 0.]),
            ),
        ] {
            let mut changed = simulation.clone();
            *changed.pointer_mut(pointer).unwrap() = value;
            assert!(
                admit(&alternative, &descriptor, &changed, None).is_err(),
                "{pointer}"
            );
        }
        for (pointer, value) in [
            ("/contact_loads/result/frames", json!([])),
            (
                "/contact_loads/result/frames/1/body_state_sha256",
                json!("stale"),
            ),
            (
                "/contact_loads/result/frames/1/body/max_force_vertex",
                json!(2),
            ),
            (
                "/contact_loads/result/frames/1/candidate_pairs",
                json!(1000),
            ),
            ("/contact_loads/result/duration_s", json!(0.)),
            ("/samples/0/solve/converged", json!(false)),
        ] {
            let mut changed = motion.clone();
            *changed.pointer_mut(pointer).unwrap() = value;
            assert!(
                admit(&alternative, &descriptor, &simulation, Some(&changed)).is_err(),
                "{pointer}"
            );
        }
    }
}

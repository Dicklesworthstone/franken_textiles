//! Reconcile trusted production force receipts with the bound material, mesh and accepted path.
//! Arithmetic replay checks forces and extrema; it is not an independent contact solve.

use super::*;
use ftx_core::{Vec2, Vec3};
use ftx_pattern::material_compression::regions;

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

struct Scope {
    vertices: usize,
    body_vertices: usize,
    selected: Vec<usize>,
    regions: BTreeMap<String, Vec<usize>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Region {
    id: String,
    instance: String,
    selector: regions::Selector,
}

struct RegionalScope {
    selected_triangles: BTreeSet<usize>,
    vertices: BTreeMap<String, Vec<usize>>,
}

// Reconstruct every selector on the complete retained flat domain. This checks partition
// consistency independently of the producer's chosen faces; the force solve remains in drape.
fn regional_scope(
    policy: &Value,
    source: &Value,
    ownership: &Value,
    instances: &[Value],
    owners: &[Value],
    faces: &[Value],
) -> Result<RegionalScope, Refusal> {
    let requested = array(policy, "regions")?;
    let flat = array(ownership, "rest_triangles_mm")?;
    let published = source["regions"].as_object().ok_or_else(fail)?;
    if !(1..=256).contains(&requested.len())
        || published.len() != requested.len()
        || flat.len() != owners.len()
        || source["rest_triangles_sha256"] != hash(&ownership["rest_triangles_mm"])?
    {
        return Err(fail());
    }
    let flat = flat
        .iter()
        .map(|triangle| <[Vec2; 3]>::deserialize(triangle).map_err(|_| fail()))
        .collect::<Result<Vec<_>, _>>()?;
    let mut areas = Vec::with_capacity(flat.len());
    let mut by_instance = BTreeMap::<&str, Vec<usize>>::new();
    for (i, (triangle, owner)) in flat.iter().zip(owners).enumerate() {
        regions::triangle_centroid_mm(*triangle)?;
        let [a, b, c] = *triangle;
        let area = 0.5 * (b - a).cross(c - a).abs() * 1e-6;
        if !area.is_finite() || area <= 0. {
            return Err(fail());
        }
        areas.push(area);
        let instance = instances
            .get(index(owner)?)
            .and_then(Value::as_str)
            .ok_or_else(fail)?;
        by_instance.entry(instance).or_default().push(i);
    }
    let requested = requested
        .iter()
        .map(|value| Region::deserialize(value).map_err(|_| fail()))
        .collect::<Result<Vec<_>, _>>()?;
    let mut ids = BTreeSet::new();
    let mut edge_tests = 0usize;
    let mut selectors = Vec::with_capacity(requested.len());
    for region in &requested {
        if region.id.trim().is_empty()
            || region.id.len() > 512
            || !ids.insert(region.id.as_str())
            || region.instance.trim().is_empty()
            || region.instance.len() > 4096
        {
            return Err(fail());
        }
        let owned = by_instance.get(region.instance.as_str()).ok_or_else(fail)?;
        edge_tests = owned
            .len()
            .checked_mul(region.selector.polygon_mm.len())
            .and_then(|count| edge_tests.checked_add(count))
            .filter(|&count| count <= regions::MAX_REGION_EDGE_TESTS)
            .ok_or_else(fail)?;
        selectors.push((
            region,
            regions::PreparedSelector::new(&region.selector)?,
            owned,
        ));
    }
    let mut result = RegionalScope {
        selected_triangles: BTreeSet::new(),
        vertices: BTreeMap::new(),
    };
    for (region, selector, owned) in selectors {
        let mut triangles = Vec::new();
        let mut vertices = BTreeSet::new();
        let mut area = 0.;
        for &i in owned {
            if selector.contains_triangle_centroid(flat[i])? {
                if !result.selected_triangles.insert(i) {
                    return Err(fail());
                }
                triangles.push(i);
                let face: [usize; 3] =
                    serde_json::from_value(faces[i].clone()).map_err(|_| fail())?;
                vertices.extend(face);
                area += areas[i];
            }
        }
        let vertices: Vec<_> = vertices.into_iter().collect();
        if triangles.is_empty()
            || !area.is_finite()
            || area <= 0.
            || published.get(&region.id)
                != Some(&json!({"instance":region.instance,"selector":region.selector,
                    "source_triangles":triangles,"selected_vertices":vertices,
                    "reference_area_m2":area}))
        {
            return Err(fail());
        }
        result.vertices.insert(region.id.clone(), vertices);
    }
    Ok(result)
}

fn source(
    alternative: &Alternative,
    policy: &Value,
    simulation: &Value,
    report: &Value,
) -> Result<Scope, Refusal> {
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
    let regional = if policy["schema"] == "ftx-contact-loads/2" {
        Some(regional_scope(policy, source, ownership, names, owners, faces)?)
    } else {
        if source.get("regions").is_some()
            || source.get("rest_triangles_sha256").is_some()
            || ownership.get("rest_triangles_mm").is_some()
        {
            return Err(fail());
        }
        None
    };
    let selected = if let Some(regional) = &regional {
        regional
            .selected_triangles
            .iter()
            .map(|&i| names[index(&owners[i])?].as_str().ok_or_else(fail))
            .collect::<Result<BTreeSet<_>, _>>()?
    } else if let Some(ids) = policy.get("instances") {
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
    for (i, (face, owner)) in faces.iter().zip(owners).enumerate() {
        let name = names[index(owner)?].as_str().ok_or_else(fail)?;
        let face: [usize; 3] = serde_json::from_value(face.clone()).map_err(|_| fail())?;
        if face.iter().any(|&v| v >= vertices) {
            return Err(fail());
        }
        if regional
            .as_ref()
            .map_or_else(|| selected.contains(name), |scope| scope.selected_triangles.contains(&i))
        {
            selected_vertices.extend(face);
        }
    }
    let selected_vertices: Vec<_> = selected_vertices.into_iter().collect();
    if selected_vertices.is_empty() || source["selected_vertices"] != json!(selected_vertices) {
        return Err(fail());
    }
    Ok(Scope {
        vertices,
        body_vertices,
        selected: selected_vertices,
        regions: regional.map_or_else(BTreeMap::new, |scope| scope.vertices),
    })
}

#[derive(Serialize)]
struct RegionSummary {
    peak_body_resultant_n: f64,
    peak_body_resultant_sample: usize,
    peak_body_sum_vertex_force_n: f64,
    peak_body_sum_vertex_force_sample: usize,
    max_body_vertex_force_n: f64,
    max_body_vertex_force_sample: usize,
    max_force_vertex: usize,
}

impl RegionSummary {
    fn initial(first_vertex: usize) -> Self {
        Self {
            peak_body_resultant_n: 0.,
            peak_body_resultant_sample: 0,
            peak_body_sum_vertex_force_n: 0.,
            peak_body_sum_vertex_force_sample: 0,
            max_body_vertex_force_n: 0.,
            max_body_vertex_force_sample: 0,
            max_force_vertex: first_vertex,
        }
    }

    fn observe(
        &mut self,
        vertices: &[usize],
        indices: &[usize],
        forces: &[Vec3],
        sample: usize,
    ) -> Result<(), Refusal> {
        let mut resultant = Vec3::ZERO;
        let mut sum = 0.;
        let mut maximum = 0.;
        let mut witness = vertices[0];
        for (&vertex, &index) in vertices.iter().zip(indices) {
            let force = forces[index];
            let magnitude = force.norm();
            resultant += force;
            sum += magnitude;
            if magnitude > maximum {
                maximum = magnitude;
                witness = vertex;
            }
        }
        let norm = resultant.norm();
        if !norm.is_finite() || !sum.is_finite() {
            return Err(fail());
        }
        if norm > self.peak_body_resultant_n {
            self.peak_body_resultant_n = norm;
            self.peak_body_resultant_sample = sample;
        }
        if sum > self.peak_body_sum_vertex_force_n {
            self.peak_body_sum_vertex_force_n = sum;
            self.peak_body_sum_vertex_force_sample = sample;
        }
        if maximum > self.max_body_vertex_force_n {
            self.max_body_vertex_force_n = maximum;
            self.max_body_vertex_force_sample = sample;
            self.max_force_vertex = witness;
        }
        Ok(())
    }
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
    let regional = policy["schema"] == "ftx-contact-loads/2";
    if (!regional && policy["schema"] != "ftx-contact-loads/1")
        || (regional && (object.contains_key("instances") || !object.contains_key("regions")))
        || (!regional && object.contains_key("regions"))
        || !(1..=64_000_000).contains(&max_vertices)
        || !(1..=64_000_000).contains(&max_pairs)
        || object.keys().any(|k| {
            ![
                "schema",
                "instances",
                "regions",
                "max_vertex_samples",
                "max_pair_evaluations",
            ]
            .contains(&k.as_str())
        })
        || outcome["status"] != "available"
        || report["schema"]
            != if regional {
                "ftx-contact-loads-report/2"
            } else {
                "ftx-contact-loads-report/1"
            }
        || (!regional
            && (report.get("regions").is_some() || report.get("max_force_vertex").is_some()))
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
    let Scope {
        vertices,
        body_vertices,
        selected,
        regions,
    } = source(alternative, policy, simulation, report)?;
    let mut region_summaries = BTreeMap::new();
    let mut region_indices = BTreeMap::new();
    for (name, vertices) in &regions {
        let indices = vertices
            .iter()
            .map(|vertex| selected.binary_search(vertex).map_err(|_| fail()))
            .collect::<Result<Vec<_>, _>>()?;
        region_summaries.insert(name.clone(), RegionSummary::initial(vertices[0]));
        region_indices.insert(name.clone(), indices);
    }
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
    // Older frozen receipts remain usable for their original metrics. New aggregate readings
    // require the complete frame field and peak witness together; partial upgrades refuse.
    let has_sum = report.get("peak_body_sum_vertex_force_n").is_some()
        || report.get("peak_body_sum_vertex_force_sample").is_some()
        || frames
            .iter()
            .any(|frame| frame["body"].get("sum_vertex_force_n").is_some());
    if regional && !has_sum {
        return Err(fail());
    }
    let mut peak_sum = 0.;
    let mut peak_vertex = 0.;
    let mut resultant_sample = 0;
    let mut sum_sample = 0;
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
        let mut sum = 0.;
        let mut maximum = 0.;
        let mut witness = selected[0];
        for (&vertex, &force) in selected.iter().zip(&forces) {
            let norm = force.norm();
            if !force.is_finite() || !norm.is_finite() {
                return Err(fail());
            }
            resultant += force;
            sum += norm;
            if norm > maximum {
                maximum = norm;
                witness = vertex;
            }
        }
        let norm = resultant.norm();
        let mut expected = json!({"resultant_n":resultant,
            "resultant_magnitude_n":norm,"max_vertex_force_n":maximum,"max_force_vertex":witness});
        if has_sum {
            if !sum.is_finite() {
                return Err(fail());
            }
            expected["sum_vertex_force_n"] = json!(sum);
        }
        if !norm.is_finite() || frame["body"] != expected {
            return Err(fail());
        }
        for (name, vertices) in &regions {
            region_summaries
                .get_mut(name)
                .expect("admitted region")
                .observe(vertices, &region_indices[name], &forces, i)?;
        }
        if norm > peak_resultant {
            peak_resultant = norm;
            resultant_sample = i;
        }
        if sum > peak_sum {
            peak_sum = sum;
            sum_sample = i;
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
        || (has_sum
            && (report["peak_body_sum_vertex_force_n"].as_f64() != Some(peak_sum)
                || report["peak_body_sum_vertex_force_sample"] != sum_sample))
        || report["max_body_vertex_force_n"].as_f64() != Some(peak_vertex)
        || report["peak_body_resultant_sample"] != resultant_sample
        || report["max_body_vertex_force_sample"] != vertex_sample
        || (regional
            && (report["regions"] != json!(region_summaries)
                || report["max_force_vertex"] != frames[vertex_sample]["body"]["max_force_vertex"]))
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
        assert!(history.get("peak_body_sum_vertex_force_n").is_none());
    }

    fn summed_case() -> (Alternative, Value, Value, Value) {
        let (alternative, descriptor, mut simulation, mut motion) = case();
        // Independent arithmetic: two opposed 5 N forces plus 2 N, followed by a single 6 N.
        // The maximum aggregate is at the initial state; the resultant and vertex peaks are later.
        for report in [
            &mut simulation["contact_loads"]["result"],
            &mut motion["contact_loads"]["result"],
        ] {
            report["frames"][0]["body"]["sum_vertex_force_n"] = json!(12.);
            report["peak_body_sum_vertex_force_n"] = json!(12.);
            report["peak_body_sum_vertex_force_sample"] = json!(0);
        }
        motion["contact_loads"]["result"]["frames"][1]["body"]["sum_vertex_force_n"] = json!(6.);
        (alternative, descriptor, simulation, motion)
    }

    #[test]
    fn summed_loads_keep_opposing_forces_and_the_initial_peak_in_complete_motion() {
        let (alternative, descriptor, simulation, motion) = summed_case();
        let initial = admit(&alternative, &descriptor, &simulation, None).unwrap();
        assert_eq!(initial["peak_body_sum_vertex_force_n"], 12.);
        assert_eq!(initial["peak_body_resultant_n"], 2.);
        let history = admit(&alternative, &descriptor, &simulation, Some(&motion)).unwrap();
        assert_eq!(history["peak_body_sum_vertex_force_n"], 12.);
        assert_eq!(history["peak_body_sum_vertex_force_sample"], 0);
        assert_eq!(history["peak_body_resultant_n"], 6.);
        assert_eq!(history["peak_body_resultant_sample"], 1);
    }

    #[test]
    fn aggregate_readings_require_every_force_sample_and_the_exact_peak_witness() {
        let (alternative, descriptor, simulation, motion) = summed_case();
        for (pointer, value) in [
            (
                "/contact_loads/result/peak_body_sum_vertex_force_n",
                json!(6.),
            ),
            (
                "/contact_loads/result/peak_body_sum_vertex_force_sample",
                json!(1),
            ),
            (
                "/contact_loads/result/frames/1/body/sum_vertex_force_n",
                json!(5.),
            ),
            (
                "/contact_loads/result/frames/1/body/sum_vertex_force_n",
                Value::Null,
            ),
        ] {
            let mut changed = motion.clone();
            *changed.pointer_mut(pointer).unwrap() = value;
            assert!(
                admit(&alternative, &descriptor, &simulation, Some(&changed)).is_err(),
                "{pointer}"
            );
        }
        for key in [
            "peak_body_sum_vertex_force_n",
            "peak_body_sum_vertex_force_sample",
        ] {
            let mut changed = simulation.clone();
            changed["contact_loads"]["result"]
                .as_object_mut()
                .unwrap()
                .remove(key);
            assert!(
                admit(&alternative, &descriptor, &changed, None).is_err(),
                "{key}"
            );
        }
        let mut changed = simulation.clone();
        changed["contact_loads"]["result"]["frames"][0]["body"]
            .as_object_mut()
            .unwrap()
            .remove("sum_vertex_force_n");
        assert!(admit(&alternative, &descriptor, &changed, None).is_err());
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

    fn regional_case() -> (Alternative, Value, Value, Value) {
        let (alternative, mut descriptor, mut simulation, mut motion) = summed_case();
        let left = json!({"kind":"whole_triangle_centroid",
            "polygon_mm":[[-1.,-1.],[2.,-1.],[2.,4.],[-1.,4.]]});
        let right = json!({"kind":"whole_triangle_centroid",
            "polygon_mm":[[3.,-1.],[7.,-1.],[7.,4.],[3.,4.]]});
        let policy = json!({"schema":"ftx-contact-loads/2",
            "regions":[{"id":"left","instance":"front","selector":left},
                {"id":"right","instance":"front","selector":right}],
            "max_vertex_samples":100,"max_pair_evaluations":100});
        let flat = json!([[[0.,0.],[3.,0.],[0.,3.]],[[3.,0.],[6.,0.],[3.,3.]]]);
        descriptor["scenario"]["contact_loads"] = policy.clone();
        simulation["contact_load_ownership"]["triangle_instance"] = json!([0, 0]);
        simulation["contact_load_ownership"]["rest_triangles_mm"] = flat.clone();
        simulation["drape"]["contact_diagnostics"]["final_reactions"]["result"]
            ["body_force_on_cloth_n"][3] = json!([0., 0., 0.]);
        for (moving, report) in [
            (false, &mut simulation["contact_loads"]["result"]),
            (true, &mut motion["contact_loads"]["result"]),
        ] {
            report["schema"] = json!("ftx-contact-loads-report/2");
            report["request"] = policy.clone();
            report["source"]["triangle_instance"] = json!([0, 0]);
            report["source"]["selected_vertices"] = json!([0, 1, 2, 3]);
            report["source"]["rest_triangles_sha256"] = json!(hash(&flat).unwrap());
            report["source"]["regions"] = json!({
                "left":{"instance":"front","selector":left,"source_triangles":[0],
                    "selected_vertices":[0,1,2],"reference_area_m2":4.5e-6},
                "right":{"instance":"front","selector":right,"source_triangles":[1],
                    "selected_vertices":[0,2,3],"reference_area_m2":4.5e-6}});
            report["source_sha256"] = json!(hash(&report["source"]).unwrap());
            for frame in report["frames"].as_array_mut().unwrap() {
                frame["body_force_on_selected_vertices_n"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!([0., 0., 0.]));
            }
            report["max_force_vertex"] = json!(0);
            report["regions"] = json!({
                "left":{"peak_body_resultant_n":if moving{6.}else{2.},
                    "peak_body_resultant_sample":usize::from(moving),
                    "peak_body_sum_vertex_force_n":12.,"peak_body_sum_vertex_force_sample":0,
                    "max_body_vertex_force_n":if moving{6.}else{5.},
                    "max_body_vertex_force_sample":usize::from(moving),"max_force_vertex":0},
                "right":{"peak_body_resultant_n":if moving{6.}else{Vec3::new(3.,4.,2.).norm()},
                    "peak_body_resultant_sample":usize::from(moving),
                    "peak_body_sum_vertex_force_n":7.,"peak_body_sum_vertex_force_sample":0,
                    "max_body_vertex_force_n":if moving{6.}else{5.},
                    "max_body_vertex_force_sample":usize::from(moving),"max_force_vertex":0}});
        }
        (alternative, descriptor, simulation, motion)
    }


    #[test]
    fn regional_constraints_use_all_six_contact_metrics_without_adding_shared_forces() {
        let (alternative, mut descriptor, mut simulation, mut motion) = regional_case();
        descriptor["scenario_sha256"] = json!("a".repeat(64));
        descriptor["body_geometry_sha256"] = json!("b".repeat(64));
        descriptor["motion_path"] = json!({"sha256":"c".repeat(64)});
        descriptor["scenario"]["motion"] = json!({"schema":"ftx-drape-motion/1","max_trials":1,
            "keyframes":[{"time_s":1.,"arm_abduction_deg":0.}]});
        simulation["schema"] = json!("ftx-drape-scenario-result/1");
        simulation["pattern_sha256"] = alternative.binding.value().unwrap()["pattern_sha256"].clone();
        simulation["scenario_sha256"] = descriptor["scenario_sha256"].clone();
        simulation["body_geometry_sha256"] = descriptor["body_geometry_sha256"].clone();
        simulation["positions_sha256"] = json!(hash(&simulation["positions_m"]).unwrap());
        simulation["drape"]["settled"] = json!(true);
        simulation["drape"]["gating"] = json!([]);
        simulation["metrics"] = json!({"simulation_gating_clear":true});
        motion["schema"] = json!("ftx-drape-motion-result/1");
        motion["path_sha256"] = descriptor["motion_path"]["sha256"].clone();
        motion["initial_positions_sha256"] = simulation["positions_sha256"].clone();
        motion["initial_sample"]["time_s"] = json!(0.);
        motion["initial_sample"]["positions_m"] = simulation["positions_m"].clone();
        motion["continuous_contact_checked"] = json!(true);
        motion["initial_drape_settled"] = json!(true);
        motion["initial_drape_gating"] = json!([]);
        motion["trials"] = json!(1);
        motion["keyframes"] = json!([{"sample_index":0,"time_s":1.,"arm_abduction_deg":0.,
            "positions_sha256":hash(&motion["samples"][0]["positions_m"]).unwrap(),
            "body_collision_sha256":hash(&(&motion["samples"][0]["body_positions_m"],
                &motion["body_triangles"])).unwrap()}]);
        simulation["motion"] = json!({"status":"available","result":motion});
        let values = [
            (Metric::BodyContactResultantN, [2., Vec3::new(3., 4., 2.).norm()]),
            (Metric::BodyContactSumVertexForceN, [12., 7.]),
            (Metric::MaxBodyContactVertexForceN, [5., 5.]),
            (Metric::MotionPeakBodyContactResultantN, [6., 6.]),
            (Metric::MotionPeakBodyContactSumVertexForceN, [12., 7.]),
            (Metric::MotionMaxBodyContactVertexForceN, [6., 6.]),
        ];
        let mut policy: Policy = serde_json::from_value(json!({"basis":"nominal",
            "objectives":[{"metric":"body_contact_sum_vertex_force_n","direction":"minimize"}],
            "constraints":values.iter().flat_map(|&(metric, values)| {
                ["left", "right"].into_iter().zip(values).map(move |(region, value)|
                    json!({"metric":metric,"region":region,"minimum":value,"maximum":value}))
            }).collect::<Vec<_>>()})).unwrap();
        let request: Request = serde_json::from_value(json!({"schema":"ftx-material-study/1"})).unwrap();
        policy.validate(&request, true).unwrap();
        let admitted = policy.candidate(&alternative, None, &descriptor, &simulation);
        assert!(admitted.eligible, "{}", json!(admitted));
        assert_eq!(admitted.readings[&Metric::BodyContactSumVertexForceN].value, 12.);
        for (metric, expected) in values {
            for (region, value) in ["left", "right"].into_iter().zip(expected) {
                let reading = &admitted.regional_readings[region][&metric];
                assert_eq!(reading.value, value);
                assert_eq!(reading.unit, "N");
                assert_eq!(reading.consumer, metric.consumer());
            }
        }
        // The right region shares welded vertices with the left, but keeps its own 7 N sum.
        policy.constraints[3].minimum = None;
        policy.constraints[3].maximum = Some(6.);
        let refused = policy.candidate(&alternative, None, &descriptor, &simulation);
        assert!(!refused.eligible);
        let violations = refused.constraint_violations.as_ref().unwrap();
        let violation = violations.iter().find(|v| v.maximum_excess == Some(1.)).unwrap();
        assert_eq!(violation.metric, Metric::BodyContactSumVertexForceN);
        assert_eq!(violation.region.as_deref(), Some("right"));

        policy.constraints[3].maximum = Some(7.);
        policy.constraints[3].region = Some("unknown".into());
        let unknown = policy.candidate(&alternative, None, &descriptor, &simulation);
        assert!(!unknown.eligible);
        assert!(!unknown.regional_readings.contains_key("unknown"));
        assert!(unknown.constraint_violations.is_none());
        policy.constraints[3].region = Some("right".into());
        simulation["motion"]["result"]["contact_loads"]["status"] = json!("refused");
        let partial = policy.candidate(&alternative, None, &descriptor, &simulation);
        assert!(!partial.eligible);
        assert_eq!(partial.regional_readings["right"][&Metric::BodyContactSumVertexForceN].value, 7.);
        assert!(!partial.regional_readings["right"].contains_key(&Metric::MotionPeakBodyContactSumVertexForceN));
        assert!(partial.constraint_violations.is_none());
    }

    #[test]
    fn regional_loads_admit_intact_panel_regions_and_nonadditive_welded_force_summaries() {
        let (alternative, descriptor, simulation, motion) = regional_case();
        let initial = admit(&alternative, &descriptor, &simulation, None).unwrap();
        assert_eq!(initial["peak_body_sum_vertex_force_n"], 12.);
        assert_eq!(initial["regions"]["left"]["peak_body_sum_vertex_force_n"], 12.);
        assert_eq!(initial["regions"]["right"]["peak_body_sum_vertex_force_n"], 7.);
        let history = admit(&alternative, &descriptor, &simulation, Some(&motion)).unwrap();
        assert_eq!(history["regions"]["right"]["peak_body_resultant_n"], 6.);
        assert_eq!(history["regions"]["right"]["peak_body_resultant_sample"], 1);
        assert_eq!(history["regions"]["right"]["peak_body_sum_vertex_force_sample"], 0);
        assert_eq!(history["regions"]["right"]["max_force_vertex"], 0);

        let mut partial_descriptor = descriptor;
        partial_descriptor["scenario"]["contact_loads"]["regions"]
            .as_array_mut()
            .unwrap()
            .pop();
        let mut partial = simulation;
        let report = &mut partial["contact_loads"]["result"];
        report["request"] = partial_descriptor["scenario"]["contact_loads"].clone();
        report["source"]["regions"].as_object_mut().unwrap().remove("right");
        report["source"]["selected_vertices"] = json!([0, 1, 2]);
        report["source_sha256"] = json!(hash(&report["source"]).unwrap());
        report["regions"].as_object_mut().unwrap().remove("right");
        report["frames"][0]["body_force_on_selected_vertices_n"]
            .as_array_mut()
            .unwrap()
            .pop();
        assert!(admit(&alternative, &partial_descriptor, &partial, None).is_ok());
    }

    #[test]
    fn regional_loads_reconstruct_membership_even_after_source_receipts_are_rehashed() {
        let (alternative, descriptor, simulation, _) = regional_case();
        for (pointer, value) in [
            ("/contact_loads/result/source/regions/left/source_triangles", json!([1])),
            ("/contact_loads/result/source/regions/left/selected_vertices", json!([0, 2, 3])),
            ("/contact_loads/result/source/regions/left/reference_area_m2", json!(1.)),
            ("/contact_loads/result/source/regions/left/instance", json!("back")),
            ("/contact_loads/result/source/selected_vertices", json!([0, 1, 2])),
            ("/contact_load_ownership/rest_triangles_mm/0", json!([[30.,0.],[33.,0.],[30.,3.]])),
            ("/contact_load_ownership/rest_triangles_mm/1", json!([[0.,0.],[1.,0.],[2.,0.]])),
            ("/contact_load_ownership/triangle_instance/1", json!(1)),
        ] {
            let mut changed = simulation.clone();
            *changed.pointer_mut(pointer).unwrap() = value;
            changed["contact_loads"]["result"]["source"]["rest_triangles_sha256"] =
                json!(hash(&changed["contact_load_ownership"]["rest_triangles_mm"]).unwrap());
            changed["contact_loads"]["result"]["source_sha256"] =
                json!(hash(&changed["contact_loads"]["result"]["source"]).unwrap());
            assert!(admit(&alternative, &descriptor, &changed, None).is_err(), "{pointer}");
        }
    }

    #[test]
    fn regional_loads_refuse_inconsistent_extrema_witnesses_and_cross_schema_scopes() {
        let (alternative, descriptor, simulation, motion) = regional_case();
        for (pointer, value) in [
            ("/contact_loads/result/regions/right/peak_body_sum_vertex_force_n", json!(12.)),
            ("/contact_loads/result/regions/right/peak_body_sum_vertex_force_sample", json!(1)),
            ("/contact_loads/result/regions/right/peak_body_resultant_sample", json!(0)),
            ("/contact_loads/result/regions/right/max_force_vertex", json!(3)),
            ("/contact_loads/result/max_force_vertex", json!(3)),
            ("/contact_loads/result/regions/left", Value::Null),
        ] {
            let mut changed = motion.clone();
            *changed.pointer_mut(pointer).unwrap() = value;
            assert!(
                admit(&alternative, &descriptor, &simulation, Some(&changed)).is_err(),
                "{pointer}"
            );
        }
        for (key, value) in [
            ("instances", Value::Null),
            ("instances", json!(["front"])),
            ("regions", json!([])),
            ("regions", Value::Null),
            ("schema", json!("ftx-contact-loads/1")),
        ] {
            let mut changed_descriptor = descriptor.clone();
            changed_descriptor["scenario"]["contact_loads"][key] = value;
            let mut changed = simulation.clone();
            changed["contact_loads"]["result"]["request"] =
                changed_descriptor["scenario"]["contact_loads"].clone();
            assert!(
                admit(&alternative, &changed_descriptor, &changed, None).is_err(),
                "{key}"
            );
        }
        let mut overlap_descriptor = descriptor;
        let selector =
            overlap_descriptor["scenario"]["contact_loads"]["regions"][0]["selector"].clone();
        overlap_descriptor["scenario"]["contact_loads"]["regions"][1]["selector"] = selector;
        let mut overlap = simulation;
        overlap["contact_loads"]["result"]["request"] =
            overlap_descriptor["scenario"]["contact_loads"].clone();
        assert!(admit(&alternative, &overlap_descriptor, &overlap, None).is_err());
    }
}

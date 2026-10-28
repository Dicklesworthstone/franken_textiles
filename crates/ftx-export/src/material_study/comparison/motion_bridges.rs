//! Admission of the production zero-storage bridge ledger. This does not run another projection
//! or heat solver: source/domain identity, actual accepted edge geometry, exactly-once partitions
//! and signed boundary exchange are reconciled before any objective may consume the total.
use super::*;
use ftx_core::Vec3;

fn fail() -> Refusal {
    invalid(
        "moving thermal bridges need complete source/time/geometry-matched zero-storage exchange and baseline/total boundary accounting",
    )
}
fn array<'a>(value: &'a Value, key: &str) -> Result<&'a [Value], Refusal> {
    value[key].as_array().map(Vec::as_slice).ok_or_else(fail)
}
fn number(value: &Value, key: &str) -> Result<f64, Refusal> {
    finite(&value[key]).ok_or_else(fail)
}
fn index(value: &Value) -> Result<usize, Refusal> {
    value
        .as_u64()
        .and_then(|i| usize::try_from(i).ok())
        .ok_or_else(fail)
}
fn hashed(value: &Value) -> Result<(), Refusal> {
    let mut input = value.clone();
    input.as_object_mut().ok_or_else(fail)?.remove("sha256");
    if value["sha256"] != hash(&input)? {
        return Err(fail());
    }
    Ok(())
}
fn range(value: &Value) -> Result<[f64; 2], Refusal> {
    let raw = value.as_array().filter(|v| v.len() == 2).ok_or_else(fail)?;
    let lo = finite(&raw[0]).ok_or_else(fail)?;
    let hi = finite(&raw[1]).ok_or_else(fail)?;
    if lo > hi {
        return Err(fail());
    }
    Ok([lo, hi])
}
fn within(domain: &Value, value: f64) -> Result<bool, Refusal> {
    let [lo, hi] = range(domain)?;
    Ok(value >= lo && value <= hi)
}
fn near(a: f64, b: f64) -> bool {
    a.is_finite() && b.is_finite() && (a - b).abs() <= 4096. * f64::EPSILON * a.abs().max(b.abs())
}
fn product(a: f64, b: f64) -> Result<f64, Refusal> {
    let value = a * b;
    if !value.is_finite() || (a != 0. && b != 0. && value == 0.) {
        return Err(fail());
    }
    Ok(value)
}
fn add(a: f64, b: f64) -> Result<f64, Refusal> {
    let value = a + b;
    if !value.is_finite()
        || (a != 0.
            && b != 0.
            && a.signum() == b.signum()
            && (value.abs() <= a.abs() || value.abs() <= b.abs()))
    {
        return Err(fail());
    }
    Ok(value)
}
fn same_number(value: &Value, key: &str, expected: f64) -> Result<(), Refusal> {
    if !near(number(value, key)?, expected) {
        return Err(fail());
    }
    Ok(())
}
fn vertex_edge(value: &Value) -> Result<[usize; 2], Refusal> {
    let edge = value.as_array().filter(|v| v.len() == 2).ok_or_else(fail)?;
    let a = index(&edge[0])?;
    let b = index(&edge[1])?;
    if a >= b {
        return Err(fail());
    }
    Ok([a, b])
}
fn area(vertices: &Value, positions: &[Vec3]) -> Result<f64, Refusal> {
    let vertices = vertices
        .as_array()
        .filter(|v| v.len() == 3)
        .ok_or_else(fail)?;
    let points = vertices
        .iter()
        .map(|v| positions.get(index(v)?).copied().ok_or_else(fail))
        .collect::<Result<Vec<_>, _>>()?;
    let area = (points[1] - points[0]).cross(points[2] - points[0]).norm() * 0.5;
    if !area.is_finite() || area <= 0. {
        return Err(fail());
    }
    Ok(area)
}

fn accounting(row: &Value, bridge_energy: f64) -> Result<(), Refusal> {
    let bridge = &row["thermal_bridges"];
    let inside = number(bridge, "baseline_inside_energy_j")?;
    let outside = number(bridge, "baseline_outside_energy_j")?;
    same_number(bridge, "bridge_energy_j", bridge_energy)?;
    let total_inside = add(inside, bridge_energy)?;
    let total_outside = add(outside, bridge_energy)?;
    same_number(bridge, "total_inside_energy_j", total_inside)?;
    same_number(bridge, "total_outside_energy_j", total_outside)?;
    same_number(row, "inside_energy_j", total_inside)?;
    same_number(row, "outside_energy_j", total_outside)?;
    if bridge["bridge_stored_change_j"] != 0.
        || bridge["excess_balance_residual_j"] != 0.
        || !heat_near(number(row, "net_boundary_energy_j")?, inside - outside)
    {
        return Err(fail());
    }
    Ok(())
}

fn reference_matches(
    q: &Value,
    source: &Value,
    film: &Value,
    domain: &Value,
    materials: &Value,
    length: f64,
) -> Result<bool, Refusal> {
    let region = &domain["region"];
    if q["surface_model_sha256"] != source["model"]["sha256"]
        || q["material_sha256"] != *materials
        || q["interface_resistance_m2_k_w"] != region["interface_resistance_m2_k_w"]
        || q["inside_resistance_m2_k_w"] != film["inside_resistance_m2_k_w"]
        || q["outside_resistance_m2_k_w"] != film["outside_resistance_m2_k_w"]
        || !within(&q["length_range_m"], length)?
    {
        return Ok(false);
    }
    let temperatures = range(&domain["temperature_range_c"])?;
    for t in [
        temperatures[0],
        temperatures[1],
        number(film, "inside_c")?,
        number(film, "outside_c")?,
    ] {
        if !within(&q["temperature_range_c"], t)? {
            return Ok(false);
        }
    }
    let ranges = array(q, "layer_area_ratio_ranges")?;
    let ratios = array(domain, "layer_area_ratios")?;
    if ranges.len() != ratios.len() {
        return Ok(false);
    }
    for (domain, ratio) in ranges.iter().zip(ratios) {
        if !within(domain, finite(ratio).ok_or_else(fail)?)? {
            return Ok(false);
        }
    }
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
fn temporal_matches(
    q: &Value,
    path: &str,
    reference: &str,
    times: [f64; 2],
    temperatures: [f64; 2],
    inside: f64,
    outside: f64,
) -> Result<bool, Refusal> {
    if q["path_id"] != path
        || q["steady_qualification_sha256"] != reference
        || !within(&q["time_range_s"], times[0])?
        || !within(&q["time_range_s"], times[1])?
        || !within(&q["sample_interval_range_s"], times[1] - times[0])?
    {
        return Ok(false);
    }
    for t in [temperatures[0], temperatures[1], inside, outside] {
        if !within(&q["temperature_range_c"], t)? {
            return Ok(false);
        }
    }
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
fn interval(
    input: &Value,
    sample: &Value,
    thermal_row: &Value,
    source: &Value,
    motion: &Value,
    request: &Value,
    previous: f64,
) -> Result<f64, Refusal> {
    let result = &thermal_row["thermal_bridges"]["interval"];
    let end = number(sample, "time_s")?;
    let seconds = end - previous;
    let film = &request["boundaries"][index(&thermal_row["interval_index"])?];
    let inside = number(film, "inside_c")?;
    let outside = number(film, "outside_c")?;
    if seconds <= 0.
        || result["schema"] != "ftx-motion-thermal-bridge-sample/1"
        || result["model"] != input["model"]
        || result["request_sha256"] != hash(input)?
        || result["surface_model_sha256"] != source["model"]["sha256"]
        || result["time_range_s"] != json!([previous, end])
        || result["sample_interval_s"] != seconds
        || result["frame_sha256"] != thermal_row["frame_sha256"]
        || result["positions_sha256"] != hash(&sample["positions_m"])?
        || result["body_positions_sha256"] != hash(&sample["body_positions_m"])?
        || result["storage_modeled"] != false
        || result["bridge_stored_change_j"] != 0.
        || result["excess_balance_residual_j"] != 0.
    {
        return Err(fail());
    }
    let cloth: Vec<Vec3> =
        serde_json::from_value(sample["positions_m"].clone()).map_err(|_| fail())?;
    let body: Vec<Vec3> =
        serde_json::from_value(sample["body_positions_m"].clone()).map_err(|_| fail())?;
    let selected = array(request, "body_triangle_ids")?;
    let patches = array(&source["model"], "patches")?;
    let columns = array(&source["model"], "columns")?;
    let instances = array(source, "instances")?;
    let owners = array(source, "triangle_instance")?;
    let mut source_edges = BTreeMap::<[usize; 2], BTreeSet<usize>>::new();
    for (i, patch) in patches.iter().enumerate() {
        let vertices = array(patch, "vertices")?;
        if vertices.len() != 3 {
            return Err(fail());
        }
        let vertices = [
            index(&vertices[0])?,
            index(&vertices[1])?,
            index(&vertices[2])?,
        ];
        let owner = index(owners.get(i).ok_or_else(fail)?)?;
        for mut edge in [
            [vertices[0], vertices[1]],
            [vertices[1], vertices[2]],
            [vertices[2], vertices[0]],
        ] {
            edge.sort_unstable();
            source_edges.entry(edge).or_default().insert(owner);
        }
    }
    let mut domains = BTreeMap::new();
    for domain in array(result, "region_domains")? {
        let region = &domain["region"];
        let id = region["id"].as_str().ok_or_else(fail)?;
        let body_id = index(&region["body_triangle"])?;
        let body_triangle =
            &motion["body_triangles"][index(selected.get(body_id).ok_or_else(fail)?)?];
        let body_fraction = number(region, "body_fraction")?;
        if body_fraction <= 0.
            || body_fraction > 1.
            || range(&domain["temperature_range_c"])?[0] < -273.15
            || domains.insert(id, domain).is_some()
        {
            return Err(fail());
        }
        let body_area = area(body_triangle, &body)? * body_fraction;
        let layers = array(region, "layers")?;
        let ratios = array(domain, "layer_area_ratios")?;
        if layers.is_empty() || layers.len() != ratios.len() {
            return Err(fail());
        }
        for (layer, ratio) in layers.iter().zip(ratios) {
            let patch = patches
                .get(index(&layer["cloth_triangle"])?)
                .ok_or_else(fail)?;
            let fraction = number(layer, "fraction")?;
            if fraction <= 0.
                || fraction > 1.
                || !near(
                    finite(ratio).ok_or_else(fail)?,
                    area(&patch["vertices"], &cloth)? * fraction / body_area,
                )
            {
                return Err(fail());
            }
        }
    }
    let requested_paths = array(&input["reference"], "paths")?;
    let reported_paths = array(result, "paths")?;
    if requested_paths.len() != reported_paths.len() || requested_paths.is_empty() {
        return Err(fail());
    }
    let temporal = array(input, "qualifications")?;
    let mut seen_paths = BTreeSet::new();
    let mut seen_edges = BTreeSet::new();
    let mut by_body = vec![0.; selected.len()];
    for path in reported_paths {
        let id = path["id"].as_str().ok_or_else(fail)?;
        let matching: Vec<_> = requested_paths.iter().filter(|p| p["id"] == id).collect();
        if matching.len() != 1 || !seen_paths.insert(id) {
            return Err(fail());
        }
        let requested = matching[0];
        if path["instance"] != requested["instance"] {
            return Err(fail());
        }
        let owner = instances
            .iter()
            .position(|v| *v == requested["instance"])
            .ok_or_else(fail)?;
        let vertices = array(requested, "vertices")?;
        let mut lengths = BTreeMap::new();
        let mut length = 0.;
        for pair in vertices.windows(2) {
            let a = index(&pair[0])?;
            let b = index(&pair[1])?;
            let edge = [a.min(b), a.max(b)];
            if a == b
                || !seen_edges.insert(edge)
                || source_edges
                    .get(&edge)
                    .is_none_or(|owners| !owners.contains(&owner))
            {
                return Err(fail());
            }
            let current =
                (*cloth.get(a).ok_or_else(fail)? - *cloth.get(b).ok_or_else(fail)?).norm();
            if !current.is_finite() || current <= 0. {
                return Err(fail());
            }
            length = add(length, current)?;
            lengths.insert(edge, (current, Vec::<[f64; 2]>::new()));
        }
        same_number(path, "length_m", length)?;
        let mut power = 0.;
        for part in array(path, "contributions")? {
            let edge = vertex_edge(&part["edge"])?;
            let (edge_length, cuts) = lengths.get_mut(&edge).ok_or_else(fail)?;
            let cut = range(&part["edge_fraction"])?;
            if cut[0] < 0. || cut[1] > 1. || cut[0] >= cut[1] {
                return Err(fail());
            }
            cuts.push(cut);
            let triangle = index(&part["source_triangle"])?;
            let patch = patches.get(triangle).ok_or_else(fail)?;
            if edge.iter().any(|&v| {
                !patch["vertices"]
                    .as_array()
                    .is_some_and(|a| a.contains(&json!(v)))
            }) {
                return Err(fail());
            }
            let domain = *domains
                .get(part["region"].as_str().ok_or_else(fail)?)
                .ok_or_else(fail)?;
            let region = &domain["region"];
            let body_id = index(&part["body_triangle"])?;
            if region["body_triangle"] != part["body_triangle"]
                || body_id >= by_body.len()
                || !array(region, "layers")?
                    .iter()
                    .any(|layer| layer["cloth_triangle"] == part["source_triangle"])
            {
                return Err(fail());
            }
            let observations: Vec<_> = array(requested, "qualifications")?
                .iter()
                .filter(|q| q["id"] == part["qualification_id"])
                .collect();
            if observations.len() != 1 {
                return Err(fail());
            }
            let q = observations[0];
            let q_hash = hash(q)?;
            let material_ids: Vec<_> = array(region, "layers")?
                .iter()
                .map(|p| {
                    let patch = patches.get(index(&p["cloth_triangle"])?).ok_or_else(fail)?;
                    let column = columns.get(index(&patch["column"])?).ok_or_else(fail)?;
                    let layers = array(column, "layers")?;
                    if layers.len() != 1 {
                        return Err(fail());
                    }
                    Ok(layers[0]["expected_sha256"].clone())
                })
                .collect::<Result<_, Refusal>>()?;
            let materials = json!(material_ids);
            let qualified: Vec<_> = array(requested, "qualifications")?
                .iter()
                .map(|candidate| {
                    reference_matches(candidate, source, film, domain, &materials, length)
                        .map(|matches| (candidate, matches))
                })
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .filter(|(_, matches)| *matches)
                .map(|(q, _)| q)
                .collect();
            if qualified.len() != 1
                || qualified[0] != q
                || part["qualification_sha256"] != q_hash
                || (q["excess_linear_conductance"]["evidence"] == "estimate"
                    && q["allow_estimates"] != true)
            {
                return Err(fail());
            }
            let temperature = range(&domain["temperature_range_c"])?;
            let matches: Vec<_> = temporal
                .iter()
                .map(|q| {
                    temporal_matches(
                        q,
                        id,
                        &q_hash,
                        [previous, end],
                        temperature,
                        inside,
                        outside,
                    )
                    .map(|matches| (q, matches))
                })
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .filter(|(_, matches)| *matches)
                .map(|(q, _)| q)
                .collect();
            if matches.len() != 1 || part["temporal_qualification_sha256"] != hash(matches[0])? {
                return Err(fail());
            }
            let t = matches[0];
            if t["zero_storage"] != true
                || (t["evidence"] == "estimate" && t["allow_estimates"] != true)
                || !within(&t["time_range_s"], previous)?
                || !within(&t["time_range_s"], end)?
                || !within(&t["sample_interval_range_s"], seconds)?
            {
                return Err(fail());
            }
            for temperature in [temperature[0], temperature[1], inside, outside] {
                if !within(&t["temperature_range_c"], temperature)? {
                    return Err(fail());
                }
            }
            let coefficient = number(&q["excess_linear_conductance"], "value")?;
            if coefficient < 0. || q["excess_linear_conductance"]["unit"] != "W/(m*K)" {
                return Err(fail());
            }
            let length = product(*edge_length, cut[1] - cut[0])?;
            let conductance = product(coefficient, length)?;
            let contribution = product(conductance, inside - outside)?;
            same_number(part, "length_m", length)?;
            same_number(part, "conductance_w_k", conductance)?;
            same_number(part, "inside_c", inside)?;
            same_number(part, "outside_c", outside)?;
            same_number(part, "power_w", contribution)?;
            power = add(power, contribution)?;
            by_body[body_id] = add(by_body[body_id], contribution)?;
        }
        for (_, mut cuts) in lengths.into_values() {
            cuts.sort_by(|a, b| a[0].total_cmp(&b[0]));
            if cuts.is_empty()
                || cuts[0][0] != 0.
                || cuts.last().ok_or_else(fail)?[1] != 1.
                || cuts.windows(2).any(|p| p[0][1] != p[1][0])
            {
                return Err(fail());
            }
        }
        same_number(path, "power_w", power)?;
    }
    let power = by_body.iter().try_fold(0., |sum, &q| add(sum, q))?;
    let reported = array(result, "body_bridge_power_w")?;
    if reported.len() != by_body.len()
        || reported
            .iter()
            .zip(&by_body)
            .any(|(v, &q)| finite(v).is_none_or(|v| !near(v, q)))
    {
        return Err(fail());
    }
    let energy = product(power, seconds)?;
    for field in [
        "bridge_energy_j",
        "inside_excess_energy_j",
        "outside_excess_energy_j",
    ] {
        same_number(result, field, energy)?;
    }
    same_number(result, "bridge_power_w", power)?;
    Ok(energy)
}

pub(super) fn admit(
    simulation: &Value,
    motion: &Value,
    heat: &Value,
    request: &Value,
    coverage: bool,
) -> Result<bool, Refusal> {
    let input = &request["thermal_bridges"];
    let result = &heat["thermal_bridges"];
    let trace = array(heat, "trace")?;
    if input.is_null() {
        if !result.is_null()
            || !heat["final_field"]["thermal_bridges"].is_null()
            || trace.iter().any(|r| !r["thermal_bridges"].is_null())
        {
            return Err(fail());
        }
        return Ok(false);
    }
    let source = &simulation["drape"]["steady_thermal_surface"];
    hashed(source)?;
    hashed(&source["model"])?;
    if !coverage
        || input["schema"] != "ftx-motion-thermal-bridges/1"
        || input["model"] != "qualified_zero_storage_centerline_exchange/1"
        || result["schema"] != "ftx-motion-thermal-bridge-result/1"
        || result["model"] != input["model"]
        || result["surface_model_sha256"] != source["model"]["sha256"]
        || source["available"] != true
        || source["material_report_sha256"] != hash(&simulation["drape"]["material"])?
        || result["storage_modeled"] != false
        || result["temperature_feedback"] != false
    {
        return Err(fail());
    }
    let samples = array(motion, "samples")?;
    if samples.len() != trace.len() {
        return Err(fail());
    }
    let mut previous = 0.;
    let mut energy = 0.;
    let mut work = 0_u64;
    for (sample, row) in samples.iter().zip(trace) {
        energy = add(
            energy,
            interval(input, sample, row, source, motion, request, previous)?,
        )?;
        accounting(row, energy)?;
        work = work
            .checked_add(
                row["thermal_bridges"]["interval"]["geometry_operations"]
                    .as_u64()
                    .filter(|n| *n > 0)
                    .ok_or_else(fail)?,
            )
            .ok_or_else(fail)?;
        previous = number(sample, "time_s")?;
    }
    accounting(heat, energy)?;
    hashed(&heat["checkpoint"])?;
    let last = &trace.last().ok_or_else(fail)?["thermal_bridges"]["interval"];
    if heat["final_field"]["thermal_bridges"] != *last
        || last["projection_input_sha256"]
            != heat["checkpoint"]["frame"]["projection"]["input_sha256"]
        || heat["geometry_operations"].as_u64().is_none_or(|n| {
            n < work
                || request["max_geometry_operations"]
                    .as_u64()
                    .is_none_or(|max| n > max)
        })
    {
        return Err(fail());
    }
    for side in ["inside", "outside"] {
        let baseline = number(
            &heat["final_field"],
            &format!("baseline_body_{side}_heat_w"),
        )?;
        same_number(
            &heat["final_field"],
            &format!("body_{side}_heat_w"),
            add(baseline, number(last, "bridge_power_w")?)?,
        )?;
    }
    same_number(
        &heat["final_field"],
        "bridge_power_w",
        number(last, "bridge_power_w")?,
    )?;
    let fields = array(&heat["final_field"], "body")?;
    let powers = array(last, "body_bridge_power_w")?;
    if fields.len() != powers.len() {
        return Err(fail());
    }
    for (field, power) in fields.iter().zip(powers) {
        let power = finite(power).ok_or_else(fail)?;
        same_number(field, "bridge_power_w", power)?;
        for side in ["inside", "outside"] {
            let baseline = number(field, &format!("baseline_{side}_heat_w"))?;
            same_number(field, &format!("{side}_heat_w"), add(baseline, power)?)?;
        }
    }
    let domains = array(last, "region_domains")?;
    let regions = array(
        &heat["checkpoint"]["frame"]["projection"]["layout"],
        "regions",
    )?;
    let temperatures = array(&heat["checkpoint"], "temperature_c")?;
    if regions.len() != temperatures.len() || domains.is_empty() {
        return Err(fail());
    }
    let mut region_indices = BTreeMap::new();
    for (index, region) in regions.iter().enumerate() {
        if region_indices
            .insert(region["id"].as_str().ok_or_else(fail)?, index)
            .is_some()
        {
            return Err(fail());
        }
    }
    for domain in domains {
        let index = *region_indices
            .get(domain["region"]["id"].as_str().ok_or_else(fail)?)
            .ok_or_else(fail)?;
        if domain["region"] != regions[index] {
            return Err(fail());
        }
        for t in temperatures[index].as_array().ok_or_else(fail)? {
            if !within(&domain["temperature_range_c"], finite(t).ok_or_else(fail)?)? {
                return Err(fail());
            }
        }
    }
    Ok(true)
}

pub(super) fn protocol(heat: &Value) -> Result<Value, Refusal> {
    let mut sources = BTreeMap::<String, BTreeSet<(String, String)>>::new();
    for row in array(heat, "trace")? {
        for path in array(&row["thermal_bridges"]["interval"], "paths")? {
            let key = path["id"].as_str().ok_or_else(fail)?.to_owned();
            for part in array(path, "contributions")? {
                sources.entry(key.clone()).or_default().insert((
                    part["qualification_sha256"]
                        .as_str()
                        .ok_or_else(fail)?
                        .to_owned(),
                    part["temporal_qualification_sha256"]
                        .as_str()
                        .ok_or_else(fail)?
                        .to_owned(),
                ));
            }
        }
    }
    Ok(
        json!({"model":"qualified_zero_storage_centerline_exchange/1","consumed_qualifications":sources}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_bridge_additions_preserve_a_small_baseline_storage_balance() {
        let extra = 1e16;
        let inside = 4.5 + extra;
        let outside = 3.5 + extra;
        // The correctly rounded total sides coincide even though baseline storage gains 1 J.
        assert_eq!(inside, outside);
        let row = json!({"inside_energy_j":inside,"outside_energy_j":outside,"net_boundary_energy_j":1.,
            "thermal_bridges":{"baseline_inside_energy_j":4.5,"baseline_outside_energy_j":3.5,
            "bridge_energy_j":extra,"total_inside_energy_j":inside,"total_outside_energy_j":outside,
            "bridge_stored_change_j":0.,"excess_balance_residual_j":0.}});
        accounting(&row, extra).unwrap();
        let mut false_storage = row;
        false_storage["net_boundary_energy_j"] = json!(0.);
        assert!(accounting(&false_storage, extra).is_err());
        assert!(product(f64::MIN_POSITIVE, 1e-20).is_err());
        assert!(add(extra, 1.).is_err());
    }
}

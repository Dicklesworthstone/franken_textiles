//! Conditional steady heat ranges on the admitted production geometry. The material authority
//! supplies all resistance/flux/endpoint arithmetic; this adapter reconciles its source and area
//! accounting with the retained consumer, without rerunning drape or implementing another solver.
use super::{
    Alternative, Catalog, Consumer, Refusal, Selection, Value, admit_vapor_assignments, finite,
    hash, invalid, json, same_identity, vapor_triangle_area,
};
use ftx_pattern::material_thermal::{ConductionLaw, ReportedConductionBounds};
use std::collections::BTreeSet;

fn fail() -> Refusal {
    invalid(
        "steady heat bounds require the exact selected sources, requested boundaries and retained geometry with complete canonical component and aggregate endpoints",
    )
}

fn array<'a>(value: &'a Value, key: &str) -> Result<&'a [Value], Refusal> {
    value[key].as_array().map(Vec::as_slice).ok_or_else(fail)
}

fn index(value: &Value) -> Result<usize, Refusal> {
    value
        .as_u64()
        .and_then(|i| usize::try_from(i).ok())
        .ok_or_else(fail)
}

fn number(value: &Value) -> Result<f64, Refusal> {
    finite(value).ok_or_else(fail)
}

fn positive(value: &Value) -> Result<f64, Refusal> {
    finite(value).filter(|v| *v > 0.).ok_or_else(fail)
}

fn fraction(value: &Value) -> Result<f64, Refusal> {
    positive(value).and_then(|v| if v <= 1. { Ok(v) } else { Err(fail()) })
}

fn identity(value: &Value) -> Result<(), Refusal> {
    let mut content = value.clone();
    content.as_object_mut().ok_or_else(fail)?.remove("sha256");
    if !same_identity(&value["sha256"], &json!(hash(&content)?)) {
        return Err(fail());
    }
    Ok(())
}

struct Source<'a> {
    frame: &'a Value,
    model: &'a Value,
    patches: &'a [Value],
    owners: &'a [Value],
    instances: &'a [Value],
    laws: Vec<ConductionLaw>,
    areas: Vec<f64>,
}

fn source<'a>(alternative: &Alternative, simulation: &'a Value) -> Result<Source<'a>, Refusal> {
    let frame = &simulation["drape"]["steady_thermal_surface"];
    let model = &frame["model"];
    if frame["schema"] != "ftx-drape-steady-thermal-surface/1"
        || frame["available"] != true
        || model["schema"] != "ftx-steady-thermal-surface/1"
        || frame["positions_m"] != simulation["positions_m"]
        || frame["material_report_sha256"] != json!(hash(&simulation["drape"]["material"])?)
        || !same_identity(
            &simulation["positions_sha256"],
            &json!(hash(&simulation["positions_m"])?),
        )
    {
        return Err(fail());
    }
    identity(frame)?;
    identity(model)?;
    let positions = array(simulation, "positions_m")?;
    let triangles = array(simulation, "triangles")?;
    let patches = array(model, "patches")?;
    let owners = array(frame, "triangle_instance")?;
    let instances = array(frame, "instances")?;
    let columns = array(model, "columns")?;
    let mut ids = BTreeSet::new();
    if patches.is_empty()
        || patches.len() > 65_536
        || patches.len() != triangles.len()
        || owners.len() != patches.len()
        || columns.is_empty()
        || columns.len() > patches.len()
        || positions.is_empty()
        || positions.len() > 196_608
        || index(&model["vertex_count"])? != positions.len()
        || instances.is_empty()
        || instances.len() > 65_536
        || instances
            .iter()
            .any(|v| v.as_str().is_none_or(|id| id.is_empty() || !ids.insert(id)))
    {
        return Err(fail());
    }
    let catalog = Catalog {
        schema: ftx_pattern::materials::CATALOG_SCHEMA.into(),
        materials: vec![],
    };
    let mut selections = Vec::new();
    let mut laws = Vec::new();
    for column in columns {
        let layers = array(column, "layers")?;
        if column["schema"] != "ftx-steady-conduction-model/1"
            || layers.len() != 1
            || !array(column, "interface_resistance_m2_k_w")?.is_empty()
        {
            return Err(fail());
        }
        let selection: Selection = serde_json::from_value(layers[0].clone()).map_err(|_| fail())?;
        if selection.record.is_none() || selection.expected_sha256.is_none() {
            return Err(fail());
        }
        let material = catalog.resolve(&selection)?;
        if json!(material.frozen_selection()) != layers[0] {
            return Err(fail());
        }
        laws.push(ConductionLaw::from_material(
            material.material(),
            material.allows_estimates(),
        )?);
        selections.push(layers[0].clone());
    }
    admit_vapor_assignments(alternative, instances, owners, patches, &selections).map_err(
        |_| {
            invalid("steady heat source differs from this alternative's final material assignments")
        },
    )?;
    let mut used = BTreeSet::new();
    let mut faces = BTreeSet::new();
    let mut areas = Vec::new();
    for (patch, triangle) in patches.iter().zip(triangles) {
        let column = index(&patch["column"])?;
        let mut vertices: [usize; 3] =
            serde_json::from_value(triangle.clone()).map_err(|_| fail())?;
        vertices.sort_unstable();
        if patch["vertices"] != *triangle || column >= laws.len() || !faces.insert(vertices) {
            return Err(fail());
        }
        positive(&patch["reference_area_m2"])?;
        used.insert(column);
        areas.push(vapor_triangle_area(&simulation["positions_m"], triangle)?);
    }
    if used.len() != laws.len() {
        return Err(fail());
    }
    Ok(Source {
        frame,
        model,
        patches,
        owners,
        instances,
        laws,
        areas,
    })
}

fn boundary(conditions: &Value, count: usize, i: usize) -> Result<&Value, Refusal> {
    let values = array(conditions, "boundaries")?;
    if values.len() == 1 {
        Ok(&values[0])
    } else if values.len() == count {
        values.get(i).ok_or_else(fail)
    } else {
        Err(fail())
    }
}

fn network(
    laws: &[ConductionLaw],
    factors: &[f64],
    interfaces: &Value,
    boundary: &Value,
    state: &Value,
) -> Result<ReportedConductionBounds, Refusal> {
    let interfaces: Vec<f64> = serde_json::from_value(interfaces.clone()).map_err(|_| fail())?;
    let bounds = ConductionLaw::reported_stack_bounds(
        laws,
        factors,
        &interfaces,
        [
            number(&boundary["inside_c"])?,
            number(&boundary["outside_c"])?,
        ],
        [
            number(&boundary["inside_resistance_m2_k_w"])?,
            number(&boundary["outside_resistance_m2_k_w"])?,
        ],
    )?;
    if state["reported_bounds"] != json!(bounds) || !array(state, "bounds_unavailable")?.is_empty()
    {
        return Err(fail());
    }
    let q = number(&state["flux_w_m2"])?;
    if q < bounds.flux_w_m2[0] || q > bounds.flux_w_m2[1] {
        return Err(fail());
    }
    Ok(bounds)
}

fn match_power(value: &Value, expected: [f64; 2]) -> Result<(), Refusal> {
    let nominal = number(&value["power_w"])?;
    if value["reported_bounds"] != json!({"power_w":expected})
        || !array(value, "bounds_unavailable")?.is_empty()
        || nominal < expected[0]
        || nominal > expected[1]
    {
        return Err(fail());
    }
    Ok(())
}

pub(super) fn bounds(
    alternative: &Alternative,
    descriptor: &Value,
    simulation: &Value,
    consumer: Consumer,
) -> Result<[f64; 2], Refusal> {
    let name = match consumer {
        Consumer::Surface => "surface",
        Consumer::Coverage => "coverage",
        _ => return Err(fail()),
    };
    let policy = &descriptor["scenario"][name];
    let result = &simulation[name]["result"];
    let conditions = if consumer == Consumer::Surface {
        policy
    } else {
        &policy["conditions"]
    };
    if !policy.is_object() || simulation[name]["status"] != "available" {
        return Err(fail());
    }
    if !conditions["exterior_exchange"].is_null() || !result["exterior_exchange"].is_null() {
        return Err(invalid(
            "steady heat source ranges do not cover coupled exterior exchange; nominal heat remains available",
        ));
    }
    if consumer == Consumer::Coverage
        && (!policy["thermal_bridges"].is_null()
            || !result["thermal_bridges"].is_null()
            || !result["request"]["thermal_bridges"].is_null())
    {
        return Err(invalid(
            "steady heat source ranges do not cover thermal-bridge qualification domains; nominal heat remains available",
        ));
    }
    let source = source(alternative, simulation)?;
    if result["request"]["report"]
        != json!({"material":simulation["drape"]["material"], "steady_thermal_surface":source.frame})
        || !same_identity(
            &result["drape_steady_frame_sha256"],
            &source.frame["sha256"],
        )
        || !same_identity(&result["request_sha256"], &json!(hash(&result["request"])?))
    {
        return Err(fail());
    }
    match consumer {
        Consumer::Surface => surface(policy, result, &source),
        Consumer::Coverage => coverage(
            policy,
            result,
            &source,
            &descriptor["coverage_body_mesh_sha256"],
        ),
        _ => Err(fail()),
    }
}

fn surface(policy: &Value, result: &Value, source: &Source<'_>) -> Result<[f64; 2], Refusal> {
    let mut expected = policy.clone();
    expected["model_sha256"] = source.model["sha256"].clone();
    if policy["schema"] != "ftx-steady-surface-conditions/1"
        || result["schema"] != "ftx-steady-surface-result/1"
        || result["request"]["conditions"] != expected
        || !same_identity(&result["model_sha256"], &source.model["sha256"])
        || (!policy["model_sha256"].is_null() && policy["model_sha256"] != source.model["sha256"])
        || (array(policy, "boundaries")?.len() != 1 && policy["model_sha256"].is_null())
        || result["positions_m"] != source.frame["positions_m"]
        || result["patch_topology"] != source.model["patches"]
        || result["triangle_instance"] != source.frame["triangle_instance"]
        || result["instances"] != source.frame["instances"]
    {
        return Err(fail());
    }
    let steady = &result["steady"];
    let columns = array(steady, "columns")?;
    let patches = array(steady, "patches")?;
    if columns.len() != source.laws.len() || patches.len() != source.patches.len() {
        return Err(fail());
    }
    let mut column_bounds = Vec::new();
    for (i, law) in source.laws.iter().enumerate() {
        column_bounds.push(network(
            std::slice::from_ref(law),
            &[1.],
            &json!([]),
            boundary(policy, columns.len(), i)?,
            &columns[i],
        )?);
    }
    let mut total = [0.; 2];
    let mut nominal = 0.;
    let mut area = 0.;
    for ((patch, reading), &current_area) in source.patches.iter().zip(patches).zip(&source.areas) {
        let column = index(&patch["column"])?;
        let range = column_bounds[column].power_bounds(current_area)?;
        let power = number(&columns[column]["flux_w_m2"])? * current_area;
        if reading["column"] != patch["column"]
            || number(&reading["area_m2"])? != current_area
            || number(&reading["power_w"])? != power
        {
            return Err(fail());
        }
        match_power(reading, range)?;
        ReportedConductionBounds::add_power_bounds(&mut total, range)?;
        nominal += power;
        area += current_area;
    }
    if number(&steady["power_w"])? != nominal || number(&steady["area_m2"])? != area {
        return Err(fail());
    }
    match_power(steady, total)?;
    Ok(total)
}

fn coverage(
    policy: &Value,
    result: &Value,
    source: &Source<'_>,
    body_identity: &Value,
) -> Result<[f64; 2], Refusal> {
    let request = &result["request"];
    let map = &request["map"];
    let assembly = &result["assembly"];
    let layout = &assembly["layout"];
    let mut conditions = policy["conditions"].clone();
    conditions["body_mesh_sha256"] = body_identity.clone();
    if result["schema"] != "ftx-steady-coverage-result/1"
        || request["schema"] != "ftx-steady-coverage-request/1"
        || assembly["schema"] != "ftx-steady-thermal-coverage/1"
        || assembly["source"] != *source.model
        || layout["schema"] != "ftx-thermal-coverage-layout/1"
        || result["projection"]["layout"] != *layout
        || map["instance_layers"] != policy["instance_layers"]
        || map["settings"] != policy["settings"]
        || request["conditions"] != conditions
        || !conditions["bare_boundaries"].is_null()
        || !same_identity(&result["body_mesh_sha256"], body_identity)
        || !same_identity(
            body_identity,
            &json!(hash(
                &json!({"positions_m":map["body_positions_m"],"triangles":map["body_triangles"]})
            )?),
        )
        || (!policy["conditions"]["body_mesh_sha256"].is_null()
            && policy["conditions"]["body_mesh_sha256"] != *body_identity)
        || (array(&conditions, "boundaries")?.len() != 1
            && policy["conditions"]["body_mesh_sha256"].is_null())
        || layout["body_triangles"] != map["body_triangles"]
        || index(&layout["body_vertex_count"])? != array(map, "body_positions_m")?.len()
    {
        return Err(fail());
    }
    identity(assembly)?;
    let body_triangles = array(map, "body_triangles")?;
    let mappings = map["instance_layers"].as_object().ok_or_else(fail)?;
    if body_triangles.is_empty()
        || body_triangles.len() > 65_536
        || array(policy, "body_triangle_ids")?.len() != body_triangles.len()
        || mappings.len() != source.instances.len()
    {
        return Err(fail());
    }
    let cloth_layers = source
        .owners
        .iter()
        .map(|owner| {
            let id = source
                .instances
                .get(index(owner)?)
                .and_then(Value::as_str)
                .ok_or_else(fail)?;
            mappings
                .get(id)
                .filter(|value| value.as_str().is_some_and(|name| !name.trim().is_empty()))
                .cloned()
                .ok_or_else(fail)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let regions = array(layout, "regions")?;
    if result["projection"]["gap_interfaces"]
        .as_array()
        .is_some_and(|gaps| !gaps.is_empty())
    {
        return Err(invalid(
            "steady heat source ranges do not cover source-dependent stagnant-gap interfaces; nominal heat remains available",
        ));
    }
    let steady = &result["steady"];
    let readings = array(steady, "regions")?;
    let fields = array(steady, "body")?;
    if regions.len() > 65_536
        || readings.len() != regions.len()
        || fields.len() != body_triangles.len()
    {
        return Err(fail());
    }
    let areas = body_triangles
        .iter()
        .map(|face| vapor_triangle_area(&map["body_positions_m"], face))
        .collect::<Result<Vec<_>, _>>()?;
    let mut covered = vec![0.; areas.len()];
    let mut allocated = vec![0.; source.patches.len()];
    let mut ids = BTreeSet::new();
    for region in regions {
        let body = index(&region["body_triangle"])?;
        let amount = fraction(&region["body_fraction"])?;
        let value = covered.get_mut(body).ok_or_else(fail)?;
        *value += amount;
        if *value > 1.
            || region["id"]
                .as_str()
                .is_none_or(|id| id.is_empty() || !ids.insert(id))
        {
            return Err(fail());
        }
        let layers = array(region, "layers")?;
        if layers.is_empty() || layers.len() > 64 {
            return Err(fail());
        }
        let mut layer_names = BTreeSet::new();
        for portion in layers {
            let cloth = index(&portion["cloth_triangle"])?;
            let amount = fraction(&portion["fraction"])?;
            let value = allocated.get_mut(cloth).ok_or_else(fail)?;
            *value += amount;
            if *value > 1.
                || portion["layer"] != cloth_layers[cloth]
                || !layer_names.insert(portion["layer"].as_str().ok_or_else(fail)?)
            {
                return Err(fail());
            }
        }
        let resistances = array(region, "interface_resistance_m2_k_w")?;
        if resistances.len() != layers.len() - 1 {
            return Err(fail());
        }
        for (pair, resistance) in layers.windows(2).zip(resistances) {
            let mut interfaces =
                array(&map["settings"], "interfaces")?
                    .iter()
                    .filter(|interface| {
                        interface["inner"] == pair[0]["layer"]
                            && interface["outer"] == pair[1]["layer"]
                    });
            let interface = interfaces.next().ok_or_else(fail)?;
            if !interface["stagnant_gap"].is_null() {
                return Err(invalid(
                    "steady heat source ranges do not cover source-dependent stagnant-gap interfaces; nominal heat remains available",
                ));
            }
            if interfaces.next().is_some() || interface["resistance_m2_k_w"] != *resistance {
                return Err(fail());
            }
        }
    }
    let mut powers = Vec::new();
    let mut ranges = Vec::new();
    let mut bare_total = 0.;
    for (i, ((&area, &covered), field)) in areas.iter().zip(&covered).zip(fields).enumerate() {
        let b = boundary(&conditions, areas.len(), i)?;
        let temperatures = [number(&b["inside_c"])?, number(&b["outside_c"])?];
        let films = [
            number(&b["inside_resistance_m2_k_w"])?,
            number(&b["outside_resistance_m2_k_w"])?,
        ];
        if temperatures.iter().any(|value| *value < -273.15)
            || films.iter().any(|value| *value < 0.)
        {
            return Err(fail());
        }
        let bare = if covered < 1. {
            let resistance = films[0] + films[1];
            if !resistance.is_finite() || resistance <= 0. {
                return Err(fail());
            }
            let delta = temperatures[0] - temperatures[1];
            let flux = delta / resistance;
            if !flux.is_finite() || (delta != 0. && flux == 0.) {
                return Err(fail());
            }
            ReportedConductionBounds {
                layer_resistance_m2_k_w: [0.; 2],
                total_resistance_m2_k_w: [resistance; 2],
                flux_w_m2: [flux; 2],
            }
            .power_bounds(area * (1. - covered))?[0]
        } else {
            0.
        };
        if !bare.is_finite()
            || number(&field["area_m2"])? != area
            || number(&field["covered_area_m2"])? != area * covered
            || number(&field["bare_power_w"])? != bare
            || !field["bridge_power_w"].is_null()
            || !field["baseline_power_w"].is_null()
        {
            return Err(fail());
        }
        powers.push(bare);
        ranges.push([bare; 2]);
        bare_total += bare;
    }
    for (region, reading) in regions.iter().zip(readings) {
        let body = index(&region["body_triangle"])?;
        let area = areas[body] * fraction(&region["body_fraction"])?;
        let mut laws = Vec::new();
        let mut layer_areas = Vec::new();
        for portion in array(region, "layers")? {
            let cloth = index(&portion["cloth_triangle"])?;
            laws.push(source.laws[index(&source.patches[cloth]["column"])?].clone());
            layer_areas.push(source.areas[cloth] * fraction(&portion["fraction"])?);
        }
        let factors: Vec<_> = layer_areas
            .iter()
            .map(|layer_area| area / layer_area)
            .collect();
        let conduction = network(
            &laws,
            &factors,
            &region["interface_resistance_m2_k_w"],
            boundary(&conditions, areas.len(), body)?,
            &reading["conduction"],
        )?;
        let range = conduction.power_bounds(area)?;
        let power = area * number(&reading["conduction"]["flux_w_m2"])?;
        if reading["id"] != region["id"]
            || reading["body_triangle"] != region["body_triangle"]
            || number(&reading["body_area_m2"])? != area
            || reading["layer_area_m2"] != json!(layer_areas)
            || number(&reading["power_w"])? != power
        {
            return Err(fail());
        }
        match_power(reading, range)?;
        powers[body] += power;
        ReportedConductionBounds::add_power_bounds(&mut ranges[body], range)?;
    }
    let mut total = [0.; 2];
    for ((field, &power), range) in fields.iter().zip(&powers).zip(ranges) {
        if number(&field["power_w"])? != power {
            return Err(fail());
        }
        match_power(field, range)?;
        ReportedConductionBounds::add_power_bounds(&mut total, range)?;
    }
    let unassigned = source
        .patches
        .iter()
        .zip(allocated)
        .map(|(patch, fraction)| Ok(number(&patch["reference_area_m2"])? * (1. - fraction)))
        .collect::<Result<Vec<_>, Refusal>>()?
        .iter()
        .sum::<f64>();
    if number(&steady["power_w"])? != powers.iter().sum::<f64>()
        || number(&steady["body_area_m2"])? != areas.iter().sum::<f64>()
        || number(&steady["covered_area_m2"])?
            != areas
                .iter()
                .zip(covered)
                .map(|(area, fraction)| area * fraction)
                .sum::<f64>()
        || number(&steady["bare_power_w"])? != bare_total
        || number(&steady["unassigned_cloth_reference_area_m2"])? != unassigned
        || !steady["bridge_power_w"].is_null()
        || !steady["baseline_power_w"].is_null()
    {
        return Err(fail());
    }
    match_power(steady, total)?;
    Ok(total)
}

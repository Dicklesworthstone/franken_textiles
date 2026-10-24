//! Decision admission for the existing textile-water producer; no second transport/heat solve.
use super::*;
use ftx_pattern::{material_vapor, materials::Evidence};

fn fail() -> Refusal {
    invalid(
        "textile liquid and vapor enthalpy ledger needs complete source-matched inventory, temperature and energy accounting",
    )
}

fn array<'a>(v: &'a Value, key: &str) -> Result<&'a [Value], Refusal> {
    v[key].as_array().map(Vec::as_slice).ok_or_else(fail)
}

fn number(v: &Value, key: &str) -> Result<f64, Refusal> {
    finite(&v[key]).ok_or_else(fail)
}

fn nonnegative(v: &Value, key: &str) -> Result<f64, Refusal> {
    finite(&v[key]).filter(|v| *v >= 0.).ok_or_else(fail)
}

// Water inventories can be much smaller than a kilogram. An absolute unit-sized tolerance would
// turn missing or invented liquid into an admissible objective. Zero inventory stays exactly zero.
fn near(a: f64, b: f64, scale: f64) -> bool {
    a.is_finite()
        && b.is_finite()
        && scale.is_finite()
        && (a - b).abs() <= 2048. * f64::EPSILON * scale.abs().max(a.abs()).max(b.abs())
}

fn inside(value: f64, lo: f64, hi: f64, scale: f64) -> bool {
    (value >= lo || near(value, lo, scale)) && (value <= hi || near(value, hi, scale))
}

fn hashed(value: &Value) -> Result<(), Refusal> {
    let mut input = value.clone();
    input.as_object_mut().ok_or_else(fail)?.remove("sha256");
    if value["sha256"].as_str() != Some(hash(&input)?.as_str()) {
        return Err(fail());
    }
    Ok(())
}

/// Return the additional liquid/vapor energy on each retained sample and the final receipt.
/// The caller reconciles this with the existing independently admitted mechanical heat sources.
pub(super) fn admit(
    alternative: &Alternative,
    simulation: &Value,
    motion: &Value,
    heat: &Value,
    heat_request: &Value,
    coverage: bool,
    mechanical_heat: &[f64],
) -> Result<Option<Vec<f64>>, Refusal> {
    let policy = &heat_request["surface_water"];
    let water = &heat["surface_water"];
    let trace = array(heat, "trace")?;
    if policy.is_null() {
        return if water.is_null() && trace.iter().all(|row| row["surface_water"].is_null()) {
            Ok(None)
        } else {
            Err(fail())
        };
    }
    // Supply version 2 is admitted separately; an unsupported energy owner must never be ignored.
    if coverage
        || policy["schema"] != "ftx-textile-surface-water/1"
        || policy["model"] != "inner_cell_equilibrium_free_film_frozen_properties/1"
        || policy["replenishment_kg_m2_s"].as_f64() != Some(0.)
        || !policy["supply"].is_null()
        || water["request"] != *policy
        || heat["initial_temperature_c"] != heat_request["initial_temperature_c"]
    {
        return Err(fail());
    }
    let text = |v: &Value| {
        v.as_str()
            .is_some_and(|s| !s.trim().is_empty() && s.len() <= 4096)
    };
    let evidence: Evidence =
        serde_json::from_value(policy["evidence"].clone()).map_err(|_| fail())?;
    if !text(&policy["source"]["reference"])
        || !text(&policy["source"]["note"])
        || !policy["source"]["accessed_on"]
            .as_str()
            .is_some_and(|s| s.len() == 10)
        || !text(&policy["method"])
        || !text(&policy["qualification"])
        || (evidence == Evidence::Estimate && policy["allow_estimates"] != true)
    {
        return Err(fail());
    }
    let cp = number(policy, "liquid_specific_heat_j_kg_k")?;
    let latent = number(policy, "latent_heat_j_kg")?;
    let maximum = number(policy, "maximum_liquid_loading_kg_m2")?;
    let reference = number(policy, "enthalpy_reference_c")?;
    let initial_temperature = number(heat_request, "initial_temperature_c")?;
    if cp <= 0. || latent <= 0. || maximum <= 0. || reference < -273.15 {
        return Err(fail());
    }
    let table = array(policy, "wet_vapor_pressure_pa")?;
    if !(2..=128).contains(&table.len()) {
        return Err(fail());
    }
    let mut previous: Option<(f64, f64)> = None;
    for knot in table {
        let pair = knot.as_array().filter(|v| v.len() == 2).ok_or_else(fail)?;
        let t = finite(&pair[0])
            .filter(|t| *t >= -273.15)
            .ok_or_else(fail)?;
        let p = finite(&pair[1]).filter(|p| *p >= 0.).ok_or_else(fail)?;
        if previous
            .is_some_and(|(old_t, old_p)| t <= old_t || p < old_p || !(t - old_t).is_finite())
        {
            return Err(fail());
        }
        previous = Some((t, p));
    }
    let domain = [
        table[0][0].as_f64().ok_or_else(fail)?,
        table[table.len() - 1][0].as_f64().ok_or_else(fail)?,
    ];
    if initial_temperature < domain[0] || initial_temperature > domain[1] {
        return Err(fail());
    }
    let pressure = |temperature: f64| -> Result<f64, Refusal> {
        if !temperature.is_finite() || temperature < domain[0] || temperature > domain[1] {
            return Err(fail());
        }
        for pair in table.windows(2) {
            let t0 = pair[0][0].as_f64().ok_or_else(fail)?;
            let t1 = pair[1][0].as_f64().ok_or_else(fail)?;
            if temperature <= t1 {
                let p0 = pair[0][1].as_f64().ok_or_else(fail)?;
                let p1 = pair[1][1].as_f64().ok_or_else(fail)?;
                let value = p0 + (temperature - t0) / (t1 - t0) * (p1 - p0);
                return value.is_finite().then_some(value).ok_or_else(fail);
            }
        }
        Err(fail())
    };
    let ambient = array(policy, "outside_vapor_pressure_pa")?;
    if ambient.len() != array(motion, "keyframes")?.len()
        || ambient.iter().any(|p| finite(p).is_none_or(|p| p < 0.))
    {
        return Err(fail());
    }

    // Reference area and ownership come from the initial drape's canonical retained frame, not
    // current stretched triangle areas or identifiers copied only into the liquid receipt.
    let drape = &simulation["drape"];
    let frame = &drape["thermal_surface"];
    let model = &heat["model"];
    let patches = array(model, "patches")?;
    let columns = array(model, "columns")?;
    let owners = array(frame, "triangle_instance")?;
    let instances = array(frame, "instances")?;
    let triangles = array(motion, "triangles")?;
    let liquid_patches = array(water, "patches")?;
    let temperatures = array(&heat["final"], "temperature_c")?;
    let bindings = policy["bindings"].as_object().ok_or_else(fail)?;
    if frame["available"] != true
        || frame["schema"] != "ftx-drape-thermal-surface/1"
        || model["schema"] != "ftx-thermal-surface/1"
        || frame["material_report_sha256"] != hash(&drape["material"])?
        || frame["positions_m"] != simulation["positions_m"]
        || frame["model"]["patches"] != model["patches"]
        || frame["model"]["vertex_count"] != model["vertex_count"]
        || simulation["triangles"] != motion["triangles"]
        || patches.is_empty()
        || patches.len() > 65_536
        || columns.is_empty()
        || columns.len() > patches.len()
        || owners.len() != patches.len()
        || triangles.len() != patches.len()
        || liquid_patches.len() != patches.len()
        || temperatures.len() != patches.len()
        || instances.is_empty()
        || instances.len() != bindings.len()
        || array(&frame["model"], "columns")?.len() != columns.len()
    {
        return Err(fail());
    }
    hashed(frame)?;
    hashed(model)?;
    let catalog = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: vec![],
    };
    let networks = array(water, "unit_pressure_networks")?;
    let cells = vapor_index(&heat_request["cells_per_layer"])?;
    if cells == 0 || cells > 4096 || networks.len() != columns.len() {
        return Err(fail());
    }
    let mut selections = Vec::with_capacity(columns.len());
    let mut capacities = Vec::with_capacity(columns.len());
    for (i, column) in columns.iter().enumerate() {
        let layers = array(column, "layers")?;
        if column["schema"] != "ftx-dry-thermal-model/1"
            || layers.len() != 1
            || column["layers"] != frame["model"]["columns"][i]["layers"]
            || column["cells_per_layer"] != heat_request["cells_per_layer"]
            || !array(column, "interface_resistance_m2_k_w")?.is_empty()
        {
            return Err(fail());
        }
        let retained = &layers[0];
        let selection = Selection {
            id: retained["authored"]["id"].as_str().ok_or_else(fail)?.into(),
            record: Some(serde_json::from_value(retained["authored"].clone()).map_err(|_| fail())?),
            allow_estimates: retained["allow_estimates"].as_bool().ok_or_else(fail)?,
            expected_sha256: Some(retained["sha256"].as_str().ok_or_else(fail)?.into()),
            ..Selection::default()
        };
        let material = catalog.resolve(&selection)?;
        if serde_json::to_value(&material).map_err(|_| fail())? != *retained
            || material.material().thermal_response.is_some()
        {
            return Err(fail());
        }
        let c =
            material
                .material()
                .require("areal_density", "kg/m2", material.allows_estimates())?
                * material.material().require(
                    "specific_heat",
                    "J/(kg*K)",
                    material.allows_estimates(),
                )?
                / cells as f64;
        let r = material.thermal_resistance()? / cells as f64;
        if !c.is_finite()
            || c <= 0.
            || !r.is_finite()
            || r <= 0.
            || column["cell_capacity_j_m2_k"] != json!(vec![c; cells])
            || column["cell_resistance_m2_k_w"] != json!(vec![r; cells])
        {
            return Err(fail());
        }
        let network = material_vapor::Request {
            schema: material_vapor::SCHEMA.into(),
            layers: vec![material_vapor::Layer {
                key: material.material().id.clone(),
                selection: material.frozen_selection(),
                area_m2: None,
            }],
            area_m2: 1.,
            inside_vapor_pressure_pa: 1.,
            outside_vapor_pressure_pa: 0.,
            inside_film_resistance_m2_pa_w: number(policy, "inside_film_resistance_m2_pa_w")?,
            outside_film_resistance_m2_pa_w: number(policy, "outside_film_resistance_m2_pa_w")?,
            latent_heat_j_kg: latent,
            required_conditions: BTreeMap::new(),
            required_method: None,
            allow_mixed_protocols: false,
            interface_resistance_m2_pa_w: vec![],
        }
        .evaluate(&catalog)?;
        if networks[i] != serde_json::to_value(network).map_err(|_| fail())? {
            return Err(fail());
        }
        selections.push(serde_json::to_value(material.frozen_selection()).map_err(|_| fail())?);
        capacities.push(c);
    }
    admit_vapor_assignments(alternative, instances, owners, patches, &selections)?;
    admit_vapor_material(
        &json!({"schema":"ftx-vapor-coverage-source/1", "instances":instances,
        "triangle_instance":owners,"patches":patches,"materials":selections}),
        &drape["material"],
    )?;

    let mut totals = [0.; 3];
    let mut dryouts = 0_u64;
    let mut used = BTreeSet::new();
    let mut liquid_storage = 0.;
    let mut dry_storage = 0.;
    let mut dry_capacity = 0.;
    let mut final_range = [f64::INFINITY, f64::NEG_INFINITY];
    for (i, patch) in patches.iter().enumerate() {
        let instance = instances
            .get(vapor_index(&owners[i])?)
            .and_then(Value::as_str)
            .ok_or_else(fail)?;
        let binding = bindings.get(instance).ok_or_else(fail)?;
        let column = vapor_index(&patch["column"])?;
        let selection = selections.get(column).ok_or_else(fail)?;
        let area = number(patch, "reference_area_m2")?;
        let loading = nonnegative(binding, "initial_liquid_kg_m2")?;
        let row = &liquid_patches[i];
        let initial = nonnegative(row, "initial_liquid_kg")?;
        let remaining = nonnegative(row, "remaining_liquid_kg")?;
        let evaporated = nonnegative(row, "evaporated_liquid_kg")?;
        if area <= 0.
            || loading > maximum
            || (loading > 0. && initial <= 0.)
            || binding["material_sha256"] != selection["expected_sha256"]
            || patch["vertices"] != triangles[i]
            || row["source_triangle"].as_u64() != Some(i as u64)
            || row["instance"] != instance
            || initial != loading * area
            || remaining > initial
            || !near(remaining + evaporated, initial, initial)
        {
            return Err(fail());
        }
        used.insert(instance);
        dryouts += u64::from(initial > 0. && remaining == 0.);
        for (total, value) in totals.iter_mut().zip([initial, remaining, evaporated]) {
            *total += value;
        }
        let ts = temperatures[i]
            .as_array()
            .filter(|v| v.len() == cells)
            .ok_or_else(fail)?;
        let inner = finite(&ts[0]).ok_or_else(fail)?;
        if (initial > 0.
            && pressure(initial_temperature)?
                < finite(ambient.first().ok_or_else(fail)?).ok_or_else(fail)?)
            || (remaining > 0.
                && pressure(inner)? < finite(ambient.last().ok_or_else(fail)?).ok_or_else(fail)?)
        {
            return Err(fail());
        }
        liquid_storage +=
            cp * (remaining * (inner - reference) - initial * (initial_temperature - reference));
        for t in ts {
            let t = finite(t)
                .filter(|t| *t >= domain[0] && *t <= domain[1])
                .ok_or_else(fail)?;
            final_range[0] = final_range[0].min(t);
            final_range[1] = final_range[1].max(t);
            dry_storage += area * capacities[column] * (t - initial_temperature);
            dry_capacity += area * capacities[column];
        }
    }
    if used.len() != bindings.len()
        || totals.iter().any(|v| !v.is_finite())
        || !heat_near(dry_storage, number(heat, "stored_change_j")?)
        || !near(
            liquid_storage,
            number(water, "liquid_stored_change_j")?,
            cp * totals[0]
                * (domain[0] - reference)
                    .abs()
                    .max((domain[1] - reference).abs()),
        )
        || water["dryout_events"].as_u64() != Some(dryouts)
        || trace.last().ok_or_else(fail)?["temperature_range_c"] != json!(final_range)
    {
        return Err(fail());
    }
    for (field, expected) in [
        "initial_liquid_kg",
        "remaining_liquid_kg",
        "evaporated_liquid_kg",
    ]
    .into_iter()
    .zip(totals)
    {
        if !near(number(water, field)?, expected, totals[0]) {
            return Err(fail());
        }
    }

    let mut previous = [totals[0], 0.];
    let mut previous_dryouts = 0;
    let mut steps = 0_u64;
    let mut corrections = Vec::with_capacity(trace.len() + 1);
    for (index, row) in trace.iter().chain(std::iter::once(heat)).enumerate() {
        let w = &row["surface_water"];
        let initial = nonnegative(w, "initial_liquid_kg")?;
        let remaining = nonnegative(w, "remaining_liquid_kg")?;
        let evaporated = nonnegative(w, "evaporated_liquid_kg")?;
        let latent_j = nonnegative(w, "latent_energy_j")?;
        let stored_j = number(w, "liquid_stored_change_j")?;
        let exported_j = number(w, "exported_vapor_sensible_energy_j")?;
        let events = w["dryout_events"].as_u64().ok_or_else(fail)?;
        let enthalpy_scale = cp
            * initial
            * (domain[0] - reference)
                .abs()
                .max((domain[1] - reference).abs());
        let range = if std::ptr::eq(row, heat) {
            final_range
        } else {
            steps = steps
                .checked_add(row["thermal_substeps"].as_u64().ok_or_else(fail)?)
                .ok_or_else(fail)?;
            let t = array(row, "temperature_range_c")?;
            [
                finite(&t[0]).ok_or_else(fail)?,
                finite(&t[1]).ok_or_else(fail)?,
            ]
        };
        // Bound the sum of the producer's per-substep 1e-8 energy tolerances, including
        // temperature excursions that cancel in a cumulative receipt. No substep is solved here.
        let energy_scale = steps as f64
            * (1. + (dry_capacity + cp * initial) * (domain[1] - domain[0]))
            + 2. * cp
                * evaporated
                * (domain[0] - reference)
                    .abs()
                    .max((domain[1] - reference).abs())
            + latent_j
            + mechanical_heat.get(index).copied().ok_or_else(fail)?;
        let ambient_index = if std::ptr::eq(row, heat) {
            ambient.len() - 1
        } else {
            vapor_index(&row["interval_index"])?
        };
        let mass_error = remaining + evaporated - initial;
        if w["schema"] != "ftx-textile-surface-water-result/1"
            || !w["supplied_liquid_kg"].is_null()
            || !w["supplied_liquid_sensible_energy_j"].is_null()
            || initial != totals[0]
            || remaining > previous[0]
            || evaporated < previous[1]
            || events < previous_dryouts
            || events > dryouts
            || !near(remaining + evaporated, initial, initial)
            || !near(number(w, "mass_balance_error_kg")?, mass_error, initial)
            || !near(latent_j, evaporated * latent, latent_j)
            || range[0] < domain[0]
            || range[1] > domain[1]
            || (remaining > 0.
                && pressure(range[1])?
                    < finite(ambient.get(ambient_index).ok_or_else(fail)?).ok_or_else(fail)?)
            || !inside(
                exported_j,
                evaporated * cp * (domain[0] - reference),
                evaporated * cp * (domain[1] - reference),
                enthalpy_scale,
            )
            || !inside(
                stored_j,
                cp * (remaining * (range[0] - reference)
                    - initial * (initial_temperature - reference)),
                cp * (remaining * (range[1] - reference)
                    - initial * (initial_temperature - reference)),
                enthalpy_scale,
            )
            || !inside(
                number(row, "stored_change_j")?,
                dry_capacity * (range[0] - initial_temperature),
                dry_capacity * (range[1] - initial_temperature),
                dry_capacity * (domain[1] - domain[0]),
            )
            || w["balance_error_j"] != row["balance_error_j"]
            || !energy_scale.is_finite()
            || number(row, "balance_error_j")?.abs() > 1e-8 * energy_scale
            || (std::ptr::eq(row, heat)
                && nonnegative(heat, "absolute_step_balance_error_j")? > 1e-8 * energy_scale)
        {
            return Err(fail());
        }
        previous = [remaining, evaporated];
        previous_dryouts = events;
        let correction = stored_j + exported_j + latent_j;
        if !correction.is_finite() {
            return Err(fail());
        }
        corrections.push(correction);
    }
    for field in [
        "initial_liquid_kg",
        "remaining_liquid_kg",
        "evaporated_liquid_kg",
        "latent_energy_j",
        "exported_vapor_sensible_energy_j",
        "liquid_stored_change_j",
        "mass_balance_error_kg",
        "dryout_events",
        "balance_error_j",
    ] {
        if water[field] != trace.last().ok_or_else(fail)?["surface_water"][field] {
            return Err(fail());
        }
    }
    Ok(Some(corrections))
}

#[cfg(test)]
mod tests;

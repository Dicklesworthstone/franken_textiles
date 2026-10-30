//! Decision admission for the existing textile-water producer; no second transport/heat solve.
use super::*;
use ftx_pattern::{material_thermal::ThermalLaw, material_vapor, materials::Evidence};

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

fn inside(value: f64, lo: f64, hi: f64, scale: f64, steps: u64) -> bool {
    (value >= lo || accumulated_near(value, lo, scale, steps))
        && (value <= hi || accumulated_near(value, hi, scale, steps))
}

// Positive source heat capacity makes enthalpy monotone. A global temperature range bounds each
// column only inside that column's own characterized domain; no endpoint is extrapolated.
fn dry_heat_bounds(
    laws: &[ThermalLaw],
    areas: &[f64],
    initial: f64,
    range: [f64; 2],
) -> Result<[f64; 2], Refusal> {
    if range.iter().any(|t| !t.is_finite()) || range[0] > range[1] {
        return Err(fail());
    }
    let mut bounds = [0.0; 2];
    for (law, &area) in laws.iter().zip(areas) {
        let [lo, hi] = law
            .domain()
            .map_or(range, |[lo, hi]| [range[0].max(lo), range[1].min(hi)]);
        if lo > hi {
            return Err(fail());
        }
        bounds[0] += area * law.sensible_change(initial, lo)?;
        bounds[1] += area * law.sensible_change(initial, hi)?;
    }
    if bounds.iter().any(|v| !v.is_finite()) {
        return Err(fail());
    }
    Ok(bounds)
}

fn hashed(value: &Value) -> Result<(), Refusal> {
    let mut input = value.clone();
    input.as_object_mut().ok_or_else(fail)?.remove("sha256");
    if value["sha256"].as_str() != Some(hash(&input)?.as_str()) {
        return Err(fail());
    }
    Ok(())
}

fn source(value: &Value) -> Result<(), Refusal> {
    let text = |v: &Value| {
        v.as_str()
            .is_some_and(|s| !s.trim().is_empty() && s.len() <= 4096)
    };
    let evidence: Evidence =
        serde_json::from_value(value["evidence"].clone()).map_err(|_| fail())?;
    if !text(&value["source"]["reference"])
        || !text(&value["source"]["note"])
        || !value["source"]["accessed_on"]
            .as_str()
            .is_some_and(|s| s.len() == 10)
        || !text(&value["method"])
        || (evidence == Evidence::Estimate && value["allow_estimates"] != true)
    {
        return Err(fail());
    }
    Ok(())
}

// This checks the coupled producer's retained accounting, not its equations of motion. The
// thermal inventory remains authoritative; bulk transfer energy is never added to latent heat.
fn admit_mechanics(
    simulation: &Value,
    motion: &Value,
    heat: &Value,
    request: &Value,
    coverage: bool,
) -> Result<(), Refusal> {
    let qualification = &request["mechanical_coupling"];
    let trace = array(heat, "trace")?;
    let initial = &motion["initial_sample"]["liquid_mechanics"];
    let final_report = &motion["liquid_mechanics"];
    if qualification.is_null() {
        return if heat["mechanical_coupling"].is_null()
            && trace.iter().all(|row| row["mechanical_coupling"].is_null())
            && (coverage || (initial.is_null() && final_report.is_null()))
        {
            Ok(())
        } else {
            Err(fail())
        };
    }
    let water = &request["surface_water"];
    let bindings = qualification["bindings"].as_object().ok_or_else(fail)?;
    let water_bindings = water["bindings"].as_object().ok_or_else(fail)?;
    let prescribed_velocity = qualification["schema"] == "ftx-coupled-liquid-mechanics/2";
    let exchange =
        prescribed_velocity || qualification["schema"] == "ftx-coupled-liquid-mechanics/1";
    let compatible_policy = if exchange {
        matches!(
            water["schema"].as_str(),
            Some(
                "ftx-textile-surface-water/1"
                    | "ftx-textile-surface-water/2"
                    | "ftx-textile-surface-water/3"
            )
        ) && qualification["model"]
            == if prescribed_velocity {
                "prescribed_incoming_velocity_mass_exchange_frozen_mechanics/1"
            } else {
                "co_moving_mass_exchange_frozen_mechanics/1"
            }
    } else {
        water["schema"] == "ftx-textile-surface-water/1"
            && qualification["schema"] == "ftx-evaporating-liquid-mechanics/1"
            && qualification["model"] == "co_moving_evaporation_frozen_mechanics/1"
    };
    source(qualification)?;
    if coverage
        || !compatible_policy
        || qualification["material_report_sha256"] != hash(&simulation["drape"]["material"])?
        || bindings.is_empty()
        || bindings.len() > 65_536
        || bindings.keys().ne(water_bindings.keys())
        || !qualification["qualification"]
            .as_str()
            .is_some_and(|s| !s.trim().is_empty() && s.len() <= 4096)
        || heat["mechanical_coupling"] != *final_report
    {
        return Err(fail());
    }
    let pressure = array(water, "wet_vapor_pressure_pa")?;
    let low = finite(&pressure.first().ok_or_else(fail)?[0]).ok_or_else(fail)?;
    let high = finite(&pressure.last().ok_or_else(fail)?[0]).ok_or_else(fail)?;
    for (instance, binding) in bindings {
        let range = array(binding, "temperature_range_c")?;
        let maximum = number(binding, "maximum_liquid_loading_kg_m2")?;
        if prescribed_velocity {
            let velocity = array(binding, "incoming_velocity_world_m_s")?;
            if velocity.len() != 3 || velocity.iter().any(|v| finite(v).is_none()) {
                return Err(fail());
            }
        } else if binding.get("incoming_velocity_world_m_s").is_some() {
            return Err(fail());
        }
        if range.len() != 2
            || binding["material_sha256"] != water_bindings[instance]["material_sha256"]
            || maximum <= 0.
            || maximum < number(water, "maximum_liquid_loading_kg_m2")?
            || finite(&range[0]).is_none_or(|t| t < -273.15 || t > low)
            || finite(&range[1]).is_none_or(|t| t < high)
        {
            return Err(fail());
        }
    }
    let summary = |report: &Value| -> Result<Value, Refusal> {
        if report["schema"]
            != if prescribed_velocity {
                "ftx-coupled-liquid-mechanics-result/2"
            } else if exchange {
                "ftx-coupled-liquid-mechanics-result/1"
            } else {
                "ftx-evaporating-liquid-mechanics-result/1"
            }
            || report["qualification"] != *qualification
            || !report["scope"]
                .as_str()
                .is_some_and(|s| !s.trim().is_empty())
        {
            return Err(fail());
        }
        let mut value = report.as_object().ok_or_else(fail)?.clone();
        for key in ["schema", "qualification", "scope"] {
            value.remove(key);
        }
        for (full, short) in [
            ("initial_liquid_mass_kg", "initial_liquid_kg"),
            ("remaining_liquid_mass_kg", "remaining_liquid_kg"),
            ("evaporated_liquid_mass_kg", "evaporated_liquid_kg"),
        ] {
            let mass = value.remove(full).ok_or_else(fail)?;
            value.insert(short.into(), mass);
        }
        Ok(Value::Object(value))
    };
    let initial = summary(initial)?;
    let final_summary = summary(final_report)?;
    if trace.last().ok_or_else(fail)?["mechanical_coupling"] != final_summary {
        return Err(fail());
    }
    let drape = &simulation["drape"];
    let substrate = if drape["attached_mass"].is_null() {
        nonnegative(drape, "mass_kg")?
    } else {
        nonnegative(&drape["attached_mass"], "total_inertial_mass_kg")?
    };
    if substrate <= 0. {
        return Err(fail());
    }
    let mut previous_energy = 0.;
    let mut previous_incoming = 0.;
    let mut previous_outgoing = 0.;
    let mut previous_incoming_energy = 0.;
    let mut previous_mixing_dissipation = 0.;
    let mut previous_incoming_potential = 0.;
    let mut previous_incoming_momentum = [0.; 3];
    for (index, row) in std::iter::once(&initial)
        .chain(trace.iter().map(|row| &row["mechanical_coupling"]))
        .enumerate()
    {
        let water = if index == 0 {
            &heat["surface_water"]
        } else {
            &trace[index - 1]["surface_water"]
        };
        let initial_mass = nonnegative(water, "initial_liquid_kg")?;
        let remaining = nonnegative(row, "remaining_liquid_kg")?;
        let evaporated = nonnegative(row, "evaporated_liquid_kg")?;
        let total = nonnegative(row, "total_inertial_mass_kg")?;
        let energy = nonnegative(row, "exported_kinetic_energy_j")?;
        let incoming = if exchange {
            let flow = &row["exchange"];
            let incoming = nonnegative(flow, "incoming_mass_kg")?;
            let outgoing = nonnegative(flow, "outgoing_mass_kg")?;
            let incoming_energy = nonnegative(flow, "incoming_kinetic_energy_j")?;
            if prescribed_velocity {
                // Inelastic capture dissipates bulk kinetic energy. It is not an extra thermal
                // source: the thermal producer retains its independent sensible/latent ledger.
                let mixing = nonnegative(flow, "mixing_dissipation_j")?;
                if mixing < previous_mixing_dissipation
                    || (index == 0 && mixing != 0.)
                    || (incoming == previous_incoming && mixing != previous_mixing_dissipation)
                {
                    return Err(fail());
                }
                previous_mixing_dissipation = mixing;
            } else if flow.get("mixing_dissipation_j").is_some() {
                return Err(fail());
            }
            let thermal_incoming = if index == 0 {
                0.
            } else {
                let supply = if request["surface_water"]["schema"] == "ftx-textile-surface-water/1"
                {
                    0.
                } else {
                    nonnegative(water, "supplied_liquid_kg")?
                };
                let condensation =
                    if request["surface_water"]["schema"] == "ftx-textile-surface-water/3" {
                        nonnegative(water, "condensed_liquid_kg")?
                    } else {
                        0.
                    };
                supply + condensation
            };
            if !near(incoming, thermal_incoming, thermal_incoming)
                || !near(outgoing, evaporated, evaporated)
                || incoming < previous_incoming
                || outgoing < previous_outgoing
                || incoming_energy < previous_incoming_energy
                || (index == 0 && (incoming != 0. || outgoing != 0. || incoming_energy != 0.))
            {
                return Err(fail());
            }
            for key in ["initial_kinetic_energy_j", "initial_gravitational_energy_j"] {
                let value = number(flow, key)?;
                if flow[key] != initial["exchange"][key]
                    || (key == "initial_kinetic_energy_j" && value < 0.)
                    || (initial_mass == 0. && value != 0.)
                {
                    return Err(fail());
                }
            }
            let incoming_potential = number(flow, "incoming_gravitational_energy_j")?;
            if incoming == previous_incoming
                && (incoming_energy != previous_incoming_energy
                    || incoming_potential != previous_incoming_potential)
            {
                return Err(fail());
            }
            for key in ["initial_momentum_kg_m_s", "incoming_momentum_kg_m_s"] {
                let vector = array(flow, key)?;
                if vector.len() != 3
                    || vector.iter().any(|v| {
                        finite(v).is_none_or(|v| {
                            (key == "initial_momentum_kg_m_s" && initial_mass == 0.
                                || key == "incoming_momentum_kg_m_s" && index == 0)
                                && v != 0.
                        })
                    })
                    || (key == "initial_momentum_kg_m_s" && flow[key] != initial["exchange"][key])
                {
                    return Err(fail());
                }
                if key == "incoming_momentum_kg_m_s" {
                    for (component, previous) in vector.iter().zip(&mut previous_incoming_momentum)
                    {
                        let current = finite(component).ok_or_else(fail)?;
                        if incoming == previous_incoming && current != *previous {
                            return Err(fail());
                        }
                        *previous = current;
                    }
                }
            }
            previous_incoming = incoming;
            previous_outgoing = outgoing;
            previous_incoming_energy = incoming_energy;
            previous_incoming_potential = incoming_potential;
            incoming
        } else {
            if !row["exchange"].is_null() {
                return Err(fail());
            }
            0.
        };
        let available = initial_mass + incoming;
        if number(row, "substrate_and_attached_mass_kg")? != substrate
            || !near(
                number(row, "initial_liquid_kg")?,
                initial_mass,
                initial_mass,
            )
            || !near(remaining + evaporated, available, available)
            || !near(total, substrate + remaining, substrate + available)
            || !near(
                number(row, "mass_balance_residual_kg")?,
                total - substrate - remaining,
                total,
            )
            || energy < previous_energy
            || (index == 0 && (remaining != initial_mass || evaporated != 0. || energy != 0.))
            || (index != 0
                && (!near(remaining, number(water, "remaining_liquid_kg")?, available)
                    || !near(
                        evaporated,
                        number(water, "evaporated_liquid_kg")?,
                        available,
                    )))
        {
            return Err(fail());
        }
        for key in [
            "exported_gravitational_energy_j",
            "transfer_kinetic_residual_j",
            "transfer_gravitational_residual_j",
        ] {
            let value = number(row, key)?;
            if index == 0 && value != 0. {
                return Err(fail());
            }
        }
        for key in [
            "exported_momentum_kg_m_s",
            "transfer_momentum_residual_kg_m_s",
        ] {
            let vector = array(row, key)?;
            if vector.len() != 3
                || vector
                    .iter()
                    .any(|v| finite(v).is_none_or(|v| index == 0 && v != 0.))
            {
                return Err(fail());
            }
        }
        previous_energy = energy;
    }
    Ok(())
}

// This integrates authored delivery, not evaporation or heat. Mechanical endpoints own each
// piecewise-constant interval; retained thermal subdivisions must not change supplied inventory.
struct Delivery {
    rates: Vec<[f64; 2]>,
    total: [f64; 2],
}

struct Supply {
    ends: Vec<f64>,
    deliveries: BTreeMap<String, Delivery>,
}

impl Supply {
    fn new(
        request: &Value,
        owners: &serde_json::Map<String, Value>,
        motion: &Value,
        cp: f64,
        reference: f64,
        domain: [f64; 2],
    ) -> Result<Self, Refusal> {
        source(request)?;
        let bindings = request["bindings"].as_object().ok_or_else(fail)?;
        if request["schema"] != "ftx-textile-liquid-supply/1" || bindings.keys().ne(owners.keys()) {
            return Err(fail());
        }
        let ends = array(motion, "keyframes")?
            .iter()
            .map(|frame| number(frame, "time_s"))
            .collect::<Result<Vec<_>, _>>()?;
        if ends.is_empty()
            || ends[0] <= 0.
            || ends.windows(2).any(|pair| pair[1] <= pair[0])
            || ends.last().copied() != Some(number(motion, "duration_s")?)
        {
            return Err(fail());
        }
        let mut deliveries = BTreeMap::new();
        for (instance, binding) in bindings {
            let mass_rates = array(binding, "rate_kg_m2_s")?;
            let temperatures = array(binding, "temperature_c")?;
            if mass_rates.len() != ends.len() || temperatures.len() != ends.len() {
                return Err(fail());
            }
            let mut total = [0.; 2];
            let mut rates = Vec::with_capacity(ends.len());
            let mut previous = 0.;
            for ((rate, t), end) in mass_rates.iter().zip(temperatures).zip(&ends) {
                let rate = finite(rate).filter(|v| *v >= 0.).ok_or_else(fail)?;
                let t = finite(t)
                    .filter(|v| *v >= domain[0] && *v <= domain[1])
                    .ok_or_else(fail)?;
                let pair = [rate, rate * cp * (t - reference)];
                let old_mass = total[0];
                for (sum, rate) in total.iter_mut().zip(pair) {
                    *sum += rate * (end - previous);
                }
                if pair.iter().chain(&total).any(|v| !v.is_finite())
                    || (rate > 0. && total[0] <= old_mass)
                {
                    return Err(fail());
                }
                rates.push(pair);
                previous = *end;
            }
            deliveries.insert(instance.clone(), Delivery { rates, total });
        }
        Ok(Self { ends, deliveries })
    }

    fn rates(&self, areas: &BTreeMap<&str, f64>) -> Result<Vec<[f64; 2]>, Refusal> {
        let mut rates = vec![[0.; 2]; self.ends.len()];
        for (instance, area) in areas {
            let delivery = self.deliveries.get(*instance).ok_or_else(fail)?;
            for (sum, per_area) in rates.iter_mut().zip(&delivery.rates) {
                for (total, rate) in sum.iter_mut().zip(per_area) {
                    *total += area * rate;
                    if !total.is_finite() {
                        return Err(fail());
                    }
                }
            }
        }
        Ok(rates)
    }
}

// Producer substeps accumulate ordinary floating-point increments. Permit their bounded summation
// error against independently reconstructed inventories/enthalpy, without a unit-sized tolerance.
fn accumulated_near(actual: f64, expected: f64, scale: f64, steps: u64) -> bool {
    actual.is_finite()
        && expected.is_finite()
        && scale.is_finite()
        && (actual - expected).abs()
            <= (2048. + 16. * steps as f64)
                * f64::EPSILON
                * scale.abs().max(actual.abs()).max(expected.abs())
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
    admit_mechanics(simulation, motion, heat, heat_request, coverage)?;
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
    let condensing = policy["schema"] == "ftx-textile-surface-water/3";
    let supplied = condensing || policy["schema"] == "ftx-textile-surface-water/2";
    let version_valid = if supplied {
        policy["replenishment_kg_m2_s"].is_null() && !policy["supply"].is_null()
    } else {
        policy["schema"] == "ftx-textile-surface-water/1"
            && policy["replenishment_kg_m2_s"].as_f64() == Some(0.)
            && policy["supply"].is_null()
    };
    if coverage
        || !version_valid
        || condensing == policy["condensation"].is_null()
        || policy["model"] != "inner_cell_equilibrium_free_film_frozen_properties/1"
        || water["request"] != *policy
        || heat["initial_temperature_c"] != heat_request["initial_temperature_c"]
    {
        return Err(fail());
    }
    source(policy)?;
    if !policy["qualification"]
        .as_str()
        .is_some_and(|s| !s.trim().is_empty() && s.len() <= 4096)
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
    let vapor_temperatures = if condensing {
        let input = &policy["condensation"];
        source(input)?;
        if input["schema"] != "ftx-textile-condensation/1"
            || !input["qualification"]
                .as_str()
                .is_some_and(|s| !s.trim().is_empty() && s.len() <= 4096)
        {
            return Err(fail());
        }
        let values = array(input, "vapor_temperature_c")?
            .iter()
            .map(|v| {
                finite(v)
                    .filter(|t| *t >= domain[0] && *t <= domain[1])
                    .ok_or_else(fail)
            })
            .collect::<Result<Vec<_>, _>>()?;
        if values.len() != ambient.len() {
            return Err(fail());
        }
        Some(values)
    } else {
        None
    };

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
    let supply = supplied
        .then(|| Supply::new(&policy["supply"], bindings, motion, cp, reference, domain))
        .transpose()?;
    let total_steps = heat["thermal_steps"].as_u64().ok_or_else(fail)?;
    let enthalpy_scale = |mass: f64| {
        cp * mass
            * (domain[0] - reference)
                .abs()
                .max((domain[1] - reference).abs())
    };
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
    let mut laws = Vec::with_capacity(columns.len());
    let mut dry_areas = vec![0.0; columns.len()];
    for (i, column) in columns.iter().enumerate() {
        let layers = array(column, "layers")?;
        if layers.len() != 1
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
        if serde_json::to_value(&material).map_err(|_| fail())? != *retained {
            return Err(fail());
        }
        let law = ThermalLaw::from_material(material.material(), material.allows_estimates())?;
        law.check_temperature(initial_temperature)?;
        if material.material().thermal_response.is_some() {
            if column["schema"] != "ftx-dry-thermal-model/2"
                || column["temperature_domain_c"]
                    != json!(law.domain().unwrap_or([-273.15, f64::MAX]))
                || column.get("cell_capacity_j_m2_k").is_some()
                || column.get("cell_resistance_m2_k_w").is_some()
            {
                return Err(fail());
            }
        } else {
            let c = law.capacity_at(initial_temperature)? / cells as f64;
            let r = material.thermal_resistance()? / cells as f64;
            if column["schema"] != "ftx-dry-thermal-model/1"
                || !c.is_finite()
                || c <= 0.
                || !r.is_finite()
                || r <= 0.
                || column["cell_capacity_j_m2_k"] != json!(vec![c; cells])
                || column["cell_resistance_m2_k_w"] != json!(vec![r; cells])
            {
                return Err(fail());
            }
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
        laws.push(law);
    }
    admit_vapor_assignments(alternative, instances, owners, patches, &selections)?;
    admit_vapor_material(
        &json!({"schema":"ftx-vapor-coverage-source/1", "instances":instances,
        "triangle_instance":owners,"patches":patches,"materials":selections}),
        &drape["material"],
    )?;

    let mut totals = [0.; 7];
    let mut dryouts = 0_u64;
    let mut used = BTreeSet::new();
    let mut areas = BTreeMap::new();
    let mut liquid_storage = 0.;
    let mut dry_storage = 0.;
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
        let (condensed, incoming_j) = if condensing {
            let mass = nonnegative(row, "condensed_liquid_kg")?;
            let energy = number(row, "incoming_vapor_sensible_energy_j")?;
            let temperatures = vapor_temperatures.as_ref().ok_or_else(fail)?;
            let low = temperatures.iter().copied().fold(f64::INFINITY, f64::min);
            let high = temperatures
                .iter()
                .copied()
                .fold(f64::NEG_INFINITY, f64::max);
            if !inside(
                energy,
                mass * cp * (low - reference),
                mass * cp * (high - reference),
                enthalpy_scale(mass),
                total_steps,
            ) {
                return Err(fail());
            }
            (mass, energy)
        } else {
            if !row["condensed_liquid_kg"].is_null()
                || !row["incoming_vapor_sensible_energy_j"].is_null()
            {
                return Err(fail());
            }
            (0., 0.)
        };
        let delivery = supply.as_ref().and_then(|s| s.deliveries.get(instance));
        let (supplied_mass, supplied_j) = if let Some(delivery) = delivery {
            let mass = nonnegative(row, "supplied_liquid_kg")?;
            let energy = number(row, "supplied_liquid_sensible_energy_j")?;
            if (delivery.total[0] > 0. && (mass <= 0. || area * delivery.total[0] <= 0.))
                || !accumulated_near(mass, area * delivery.total[0], mass, total_steps)
                || !accumulated_near(
                    energy,
                    area * delivery.total[1],
                    enthalpy_scale(mass),
                    total_steps,
                )
            {
                return Err(fail());
            }
            (mass, energy)
        } else {
            if !row["supplied_liquid_kg"].is_null()
                || !row["supplied_liquid_sensible_energy_j"].is_null()
            {
                return Err(fail());
            }
            (0., 0.)
        };
        if area <= 0.
            || loading > maximum
            || (loading > 0. && initial <= 0.)
            || binding["material_sha256"] != selection["expected_sha256"]
            || patch["vertices"] != triangles[i]
            || row["source_triangle"].as_u64() != Some(i as u64)
            || row["instance"] != instance
            || initial != loading * area
            || (supplied && !(maximum * area).is_finite())
            || remaining > maximum * area
            || (!supplied && remaining > initial)
            || !near(
                remaining + evaporated,
                initial + supplied_mass + condensed,
                initial + supplied_mass + condensed,
            )
        {
            return Err(fail());
        }
        used.insert(instance);
        *areas.entry(instance).or_insert(0.) += area;
        dry_areas[column] += area;
        dryouts += u64::from(initial > 0. && remaining == 0.);
        for (total, value) in totals.iter_mut().zip([
            initial,
            remaining,
            evaporated,
            supplied_mass,
            supplied_j,
            condensed,
            incoming_j,
        ]) {
            *total += value;
        }
        let ts = temperatures[i]
            .as_array()
            .filter(|v| v.len() == cells)
            .ok_or_else(fail)?;
        let inner = finite(&ts[0]).ok_or_else(fail)?;
        if !condensing
            && (((initial > 0. || delivery.is_some_and(|d| d.rates[0][0] > 0.))
                && pressure(initial_temperature)?
                    < finite(ambient.first().ok_or_else(fail)?).ok_or_else(fail)?)
                || ((remaining > 0.
                    || delivery.is_some_and(|d| d.rates.last().is_some_and(|r| r[0] > 0.)))
                    && pressure(inner)?
                        < finite(ambient.last().ok_or_else(fail)?).ok_or_else(fail)?))
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
            dry_storage +=
                area * laws[column].sensible_change(initial_temperature, t)? / cells as f64;
        }
    }
    if used.len() != bindings.len()
        || totals.iter().any(|v| !v.is_finite())
        || !heat_near(dry_storage, number(heat, "stored_change_j")?)
        || !accumulated_near(
            liquid_storage,
            number(water, "liquid_stored_change_j")?,
            enthalpy_scale(totals[0] + totals[3] + totals[5]),
            total_steps,
        )
        || (!supplied && water["dryout_events"].as_u64() != Some(dryouts))
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
        if !near(
            number(water, field)?,
            expected,
            totals[0] + totals[3] + totals[5],
        ) {
            return Err(fail());
        }
    }
    if supplied
        && (!near(number(water, "supplied_liquid_kg")?, totals[3], totals[3])
            || !near(
                number(water, "supplied_liquid_sensible_energy_j")?,
                totals[4],
                enthalpy_scale(totals[3]),
            ))
    {
        return Err(fail());
    }
    if condensing
        && (!near(number(water, "condensed_liquid_kg")?, totals[5], totals[5])
            || !near(
                number(water, "incoming_vapor_sensible_energy_j")?,
                totals[6],
                enthalpy_scale(totals[5]),
            ))
    {
        return Err(fail());
    }
    let event_bound = if supplied {
        total_steps
            .checked_mul(patches.len() as u64)
            .ok_or_else(fail)?
    } else {
        dryouts
    };
    let supply_rates = supply.as_ref().map(|s| s.rates(&areas)).transpose()?;
    let maximum_total_liquid: f64 = areas.values().map(|area| area * maximum).sum();
    let mut supply_before_interval = vec![[0.; 2]];
    if let (Some(s), Some(rates)) = (&supply, &supply_rates) {
        let mut previous = 0.;
        for (end, rate) in s.ends.iter().zip(rates) {
            let before = supply_before_interval.last().ok_or_else(fail)?;
            supply_before_interval.push([
                before[0] + rate[0] * (end - previous),
                before[1] + rate[1] * (end - previous),
            ]);
            previous = *end;
        }
    }

    let dry_domain_bounds = dry_heat_bounds(&laws, &dry_areas, initial_temperature, domain)?;
    let dry_enthalpy_span = dry_domain_bounds[1] - dry_domain_bounds[0];
    let mut previous = [totals[0], 0., 0., 0.];
    let mut previous_incoming_j = 0.;
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
        let (condensed, condensation_j, incoming_j) = if condensing {
            (
                nonnegative(w, "condensed_liquid_kg")?,
                nonnegative(w, "condensation_latent_energy_j")?,
                number(w, "incoming_vapor_sensible_energy_j")?,
            )
        } else {
            if !w["condensed_liquid_kg"].is_null()
                || !w["condensation_latent_energy_j"].is_null()
                || !w["incoming_vapor_sensible_energy_j"].is_null()
            {
                return Err(fail());
            }
            (0., 0., 0.)
        };
        let events = w["dryout_events"].as_u64().ok_or_else(fail)?;
        let (supplied_mass, supplied_j) = if supplied {
            (
                nonnegative(w, "supplied_liquid_kg")?,
                number(w, "supplied_liquid_sensible_energy_j")?,
            )
        } else {
            if !w["supplied_liquid_kg"].is_null()
                || !w["supplied_liquid_sensible_energy_j"].is_null()
            {
                return Err(fail());
            }
            (0., 0.)
        };
        let available_mass = initial + supplied_mass + condensed;
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
        let dry_bounds = dry_heat_bounds(&laws, &dry_areas, initial_temperature, range)?;
        let energy_scale = steps as f64
            * (1. + dry_enthalpy_span + cp * available_mass * (domain[1] - domain[0]))
            + 2. * enthalpy_scale(evaporated + supplied_mass + condensed)
            + cp * (supplied_mass + condensed) * (domain[1] - domain[0])
            + latent_j
            + condensation_j
            + mechanical_heat.get(index).copied().ok_or_else(fail)?;
        let ambient_index = if std::ptr::eq(row, heat) {
            ambient.len() - 1
        } else {
            vapor_index(&row["interval_index"])?
        };
        let active_supply = if let (Some(s), Some(rates)) = (&supply, &supply_rates) {
            let rate = rates.get(ambient_index).ok_or_else(fail)?;
            let before = supply_before_interval.get(ambient_index).ok_or_else(fail)?;
            let start = if ambient_index == 0 {
                0.
            } else {
                s.ends[ambient_index - 1]
            };
            let time = if std::ptr::eq(row, heat) {
                number(heat, "duration_s")?
            } else {
                number(row, "time_s")?
            };
            if time < start
                || time > *s.ends.get(ambient_index).ok_or_else(fail)?
                || !accumulated_near(
                    supplied_mass,
                    before[0] + rate[0] * (time - start),
                    supplied_mass,
                    steps,
                )
                || !accumulated_near(
                    supplied_j,
                    before[1] + rate[1] * (time - start),
                    enthalpy_scale(supplied_mass),
                    steps,
                )
            {
                return Err(fail());
            }
            rate[0] > 0.
        } else {
            false
        };
        if let Some(temperatures) = &vapor_temperatures {
            let temperature = *temperatures.get(ambient_index).ok_or_else(fail)?;
            // Incoming vapor has an explicit, constant effective enthalpy within this physical
            // interval. Check its mass-weighted integral without rerunning transport or heat.
            if !accumulated_near(
                incoming_j,
                previous_incoming_j + (condensed - previous[3]) * cp * (temperature - reference),
                enthalpy_scale(condensed),
                steps,
            ) {
                return Err(fail());
            }
        }
        let mass_error = remaining + evaporated - initial - supplied_mass - condensed;
        let expected_schema = if condensing {
            "ftx-textile-surface-water-result/3"
        } else if supplied {
            "ftx-textile-surface-water-result/2"
        } else {
            "ftx-textile-surface-water-result/1"
        };
        if w["schema"] != expected_schema
            || initial != totals[0]
            || (!supplied && remaining > previous[0])
            || (supplied
                && remaining > previous[0] + supplied_mass - previous[2] + condensed - previous[3]
                && !near(
                    remaining,
                    previous[0] + supplied_mass - previous[2] + condensed - previous[3],
                    available_mass,
                ))
            || evaporated < previous[1]
            || supplied_mass < previous[2]
            || condensed < previous[3]
            || events < previous_dryouts
            || events > event_bound
            || (supplied
                && (events > steps.checked_mul(patches.len() as u64).ok_or_else(fail)?
                    || !maximum_total_liquid.is_finite()
                    || (remaining > maximum_total_liquid
                        && !near(remaining, maximum_total_liquid, maximum_total_liquid))))
            || !near(remaining + evaporated, available_mass, available_mass)
            || !near(
                number(w, "mass_balance_error_kg")?,
                mass_error,
                available_mass,
            )
            || !accumulated_near(latent_j, evaporated * latent, latent_j, steps)
            || !accumulated_near(condensation_j, condensed * latent, condensation_j, steps)
            || range[0] < domain[0]
            || range[1] > domain[1]
            || (!condensing
                && (remaining > 0. || active_supply)
                && pressure(range[1])?
                    < finite(ambient.get(ambient_index).ok_or_else(fail)?).ok_or_else(fail)?)
            || !inside(
                exported_j,
                evaporated * cp * (domain[0] - reference),
                evaporated * cp * (domain[1] - reference),
                enthalpy_scale(available_mass),
                steps,
            )
            || !inside(
                stored_j,
                cp * (remaining * (range[0] - reference)
                    - initial * (initial_temperature - reference)),
                cp * (remaining * (range[1] - reference)
                    - initial * (initial_temperature - reference)),
                enthalpy_scale(available_mass),
                steps,
            )
            || !inside(
                number(row, "stored_change_j")?,
                dry_bounds[0],
                dry_bounds[1],
                dry_enthalpy_span,
                steps,
            )
            || w["balance_error_j"] != row["balance_error_j"]
            || !energy_scale.is_finite()
            || number(row, "balance_error_j")?.abs() > 1e-8 * energy_scale
            || (std::ptr::eq(row, heat)
                && nonnegative(heat, "absolute_step_balance_error_j")? > 1e-8 * energy_scale)
        {
            return Err(fail());
        }
        previous = [remaining, evaporated, supplied_mass, condensed];
        previous_incoming_j = incoming_j;
        previous_dryouts = events;
        let correction =
            stored_j + exported_j + latent_j - supplied_j - condensation_j - incoming_j;
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
        "supplied_liquid_kg",
        "supplied_liquid_sensible_energy_j",
        "condensed_liquid_kg",
        "condensation_latent_energy_j",
        "incoming_vapor_sensible_energy_j",
    ] {
        if water[field] != trace.last().ok_or_else(fail)?["surface_water"][field] {
            return Err(fail());
        }
    }
    Ok(Some(corrections))
}

#[cfg(test)]
mod tests;

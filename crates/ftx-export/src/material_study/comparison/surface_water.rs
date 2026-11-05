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

fn velocity(v: &Value) -> bool {
    v.as_array()
        .is_some_and(|v| v.len() == 3 && v.iter().all(|component| finite(component).is_some()))
}

fn velocity_pair(v: &Value, phases: [&str; 2]) -> bool {
    v.as_object().is_some_and(|v| {
        v.len() == 2
            && phases
                .iter()
                .all(|phase| v.get(*phase).is_some_and(velocity))
    })
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

struct DryInitialColumn {
    area_m2: f64,
    anchor_c: f64,
    offset_j: f64,
}

// Positive source heat capacity makes enthalpy monotone. A global temperature range bounds each
// column only inside that column's own characterized domain; no endpoint is extrapolated. Its
// initial enthalpy is aggregated once relative to a valid applied anchor, so different owners of
// one material keep their own starts without revisiting every patch at every retained sample.
fn dry_heat_bounds(
    laws: &[ThermalLaw],
    initial: &[DryInitialColumn],
    range: [f64; 2],
) -> Result<[f64; 2], Refusal> {
    if range.iter().any(|t| !t.is_finite()) || range[0] > range[1] {
        return Err(fail());
    }
    let mut bounds = [0.0; 2];
    for (law, initial) in laws.iter().zip(initial) {
        let [lo, hi] = law
            .domain()
            .map_or(range, |[lo, hi]| [range[0].max(lo), range[1].min(hi)]);
        if lo > hi {
            return Err(fail());
        }
        bounds[0] +=
            initial.area_m2 * law.sensible_change(initial.anchor_c, lo)? - initial.offset_j;
        bounds[1] +=
            initial.area_m2 * law.sensible_change(initial.anchor_c, hi)? - initial.offset_j;
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

// The producer selects a complete original qualification, never a rewritten material hash.
// Independently check every allowed difference before treating its collection as one case.
fn alternative_case_inputs(policy: &Value) -> Result<Value, Refusal> {
    let mut value = policy.clone();
    for name in [
        None,
        Some("supply"),
        Some("condensation"),
        Some("drainage"),
        Some("vapor_reservoir"),
    ] {
        let item = match name {
            None => &mut value,
            Some(name) => match value.get_mut(name) {
                None | Some(Value::Null) => continue,
                Some(item) => item,
            },
        };
        source(item)?;
        // These fields are removed from the common-case comparison, so their complete native
        // metadata shape must be checked even on an unselected qualification. Source has three
        // required fields; a non-estimated record still cannot carry a non-boolean consent.
        if item["source"]
            .as_object()
            .is_none_or(|source| source.len() != 3)
            || item
                .get("allow_estimates")
                .is_some_and(|value| !value.is_boolean())
            || (name == Some("supply") && item.get("qualification").is_some())
        {
            return Err(fail());
        }
        if name != Some("supply")
            && !item["qualification"]
                .as_str()
                .is_some_and(|text| !text.trim().is_empty() && text.len() <= 4096)
        {
            return Err(fail());
        }
        let object = item.as_object_mut().ok_or_else(fail)?;
        for key in [
            "source",
            "evidence",
            "allow_estimates",
            "method",
            "qualification",
        ] {
            object.remove(key);
        }
    }
    let maximum = number(policy, "maximum_liquid_loading_kg_m2")?;
    if maximum <= 0. {
        return Err(fail());
    }
    value
        .as_object_mut()
        .ok_or_else(fail)?
        .remove("maximum_liquid_loading_kg_m2");
    let bindings = value["bindings"].as_object_mut().ok_or_else(fail)?;
    for (instance, binding) in bindings {
        let identity = binding["material_sha256"].as_str().ok_or_else(fail)?;
        let loading = nonnegative(binding, "initial_liquid_kg_m2")?;
        if instance.trim().is_empty()
            || instance.len() > 4096
            || identity.len() != 64
            || !identity
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            || loading > maximum
        {
            return Err(fail());
        }
        binding
            .as_object_mut()
            .ok_or_else(fail)?
            .remove("material_sha256");
    }
    Ok(value)
}

// A material may change its qualified mechanical domain, but cannot secretly change the imposed
// incoming/outgoing velocities or the coupling model inside one material comparison.
fn mechanical_case_inputs(qualification: &Value, water: &Value) -> Result<Value, Refusal> {
    source(qualification)?;
    let identity = qualification["material_report_sha256"]
        .as_str()
        .ok_or_else(fail)?;
    if identity.len() != 64
        || !identity
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        || qualification["source"]
            .as_object()
            .is_none_or(|source| source.len() != 3)
        || qualification
            .get("allow_estimates")
            .is_some_and(|value| !value.is_boolean())
        || !qualification["qualification"]
            .as_str()
            .is_some_and(|text| !text.trim().is_empty() && text.len() <= 4096)
    {
        return Err(fail());
    }
    let water_bindings = water["bindings"].as_object().ok_or_else(fail)?;
    let pressure = array(water, "wet_vapor_pressure_pa")?;
    let low = finite(&pressure.first().ok_or_else(fail)?[0]).ok_or_else(fail)?;
    let high = finite(&pressure.last().ok_or_else(fail)?[0]).ok_or_else(fail)?;
    let mut value = qualification.clone();
    let bindings = value["bindings"].as_object_mut().ok_or_else(fail)?;
    if bindings.is_empty() || bindings.keys().ne(water_bindings.keys()) {
        return Err(fail());
    }
    for (instance, binding) in bindings {
        let range = array(binding, "temperature_range_c")?;
        let maximum = number(binding, "maximum_liquid_loading_kg_m2")?;
        if binding["material_sha256"] != water_bindings[instance]["material_sha256"]
            || maximum <= 0.
            || maximum < number(water, "maximum_liquid_loading_kg_m2")?
            || range.len() != 2
            || finite(&range[0]).is_none_or(|t| t < -273.15 || t > low)
            || finite(&range[1]).is_none_or(|t| t < high)
        {
            return Err(fail());
        }
        let binding = binding.as_object_mut().ok_or_else(fail)?;
        for key in [
            "material_sha256",
            "maximum_liquid_loading_kg_m2",
            "temperature_range_c",
        ] {
            binding.remove(key);
        }
    }
    let object = value.as_object_mut().ok_or_else(fail)?;
    for key in [
        "source",
        "evidence",
        "allow_estimates",
        "method",
        "qualification",
        "material_report_sha256",
    ] {
        object.remove(key);
    }
    Ok(value)
}

fn selected_policy<'a>(
    request: &'a Value,
    heat: &Value,
    simulation: &Value,
) -> Result<(&'a Value, &'a Value), Refusal> {
    let coupled = !request["coupled_water_alternatives"].is_null();
    if (coupled && heat.get("surface_water_selection").is_some())
        || (!coupled && heat.get("coupled_water_selection").is_some())
    {
        return Err(fail());
    }
    let choices = if coupled {
        &request["coupled_water_alternatives"]
    } else {
        &request["surface_water_alternatives"]
    };
    if choices.is_null() {
        return if heat.get("surface_water_selection").is_none() {
            Ok((&request["surface_water"], &request["mechanical_coupling"]))
        } else {
            Err(fail())
        };
    }
    let choices = choices.as_array().ok_or_else(fail)?;
    if !(1..=64).contains(&choices.len())
        || !request["surface_water"].is_null()
        || !request["mechanical_coupling"].is_null()
        || (coupled && !request["surface_water_alternatives"].is_null())
    {
        return Err(fail());
    }
    let frame = &simulation["drape"]["thermal_surface"];
    let instances = array(frame, "instances")?;
    let owners = array(frame, "triangle_instance")?;
    let patches = array(&frame["model"], "patches")?;
    let columns = array(&frame["model"], "columns")?;
    if instances.is_empty()
        || instances.len() > 65_536
        || patches.is_empty()
        || patches.len() > 65_536
        || owners.len() != patches.len()
        || columns.is_empty()
        || columns.len() > patches.len()
    {
        return Err(fail());
    }
    let mut assigned = BTreeMap::new();
    for (patch, owner) in patches.iter().zip(owners) {
        let instance = instances
            .get(vapor_index(owner)?)
            .and_then(Value::as_str)
            .ok_or_else(fail)?;
        let layers = array(
            columns
                .get(vapor_index(&patch["column"])?)
                .ok_or_else(fail)?,
            "layers",
        )?;
        if layers.len() != 1 {
            return Err(fail());
        }
        let identity = layers[0]["sha256"].as_str().ok_or_else(fail)?;
        if assigned
            .insert(instance, identity)
            .is_some_and(|previous| previous != identity)
        {
            return Err(fail());
        }
    }
    if assigned.len() != instances.len() {
        return Err(fail());
    }
    let mut count = 0_usize;
    let mut common = None;
    let mut seen = BTreeSet::new();
    let mut selected = None;
    for (index, choice) in choices.iter().enumerate() {
        let policy = if coupled {
            if choice.as_object().is_none_or(|pair| pair.len() != 2) {
                return Err(fail());
            }
            &choice["surface_water"]
        } else {
            choice
        };
        let bindings = policy["bindings"].as_object().ok_or_else(fail)?;
        count = count.saturating_add(bindings.len());
        if bindings.is_empty() || count > 65_536 {
            return Err(fail());
        }
        let physical = if coupled {
            hash(&(
                alternative_case_inputs(policy)?,
                mechanical_case_inputs(&choice["mechanical_coupling"], policy)?,
            ))?
        } else {
            hash(&alternative_case_inputs(policy)?)?
        };
        if common
            .as_ref()
            .is_some_and(|expected| expected != &physical)
        {
            return Err(fail());
        }
        common = Some(physical);
        let identity = bindings
            .iter()
            .map(|(instance, binding)| {
                Ok((
                    instance.as_str(),
                    binding["material_sha256"].as_str().ok_or_else(fail)?,
                ))
            })
            .collect::<Result<BTreeMap<_, _>, Refusal>>()?;
        if !seen.insert(identity.clone()) {
            return Err(fail());
        }
        if identity == assigned && selected.replace(index).is_some() {
            return Err(fail());
        }
    }
    let index = selected.ok_or_else(fail)?;
    if coupled {
        let pair = &choices[index];
        if heat["coupled_water_selection"] != json!({"index":index,"pair_sha256":hash(pair)?})
            || pair["mechanical_coupling"]["material_report_sha256"]
                != hash(&simulation["drape"]["material"])?
        {
            return Err(fail());
        }
        Ok((&pair["surface_water"], &pair["mechanical_coupling"]))
    } else if heat["surface_water_selection"]
        != json!({"index":index,"request_sha256":hash(&choices[index])?})
    {
        Err(fail())
    } else {
        Ok((&choices[index], &request["mechanical_coupling"]))
    }
}

// This checks the coupled producer's retained accounting, not its equations of motion. The
// thermal inventory remains authoritative; bulk transfer energy is never added to latent heat.
// Retained only after the complete coupled mechanical history has passed admission.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct InertialMass {
    pub(super) final_kg: f64,
    pub(super) max_sample_kg: f64,
}

#[derive(Debug, PartialEq)]
pub(super) struct Admission {
    pub(super) energy_corrections_j: Vec<f64>,
    pub(super) inertial_mass: Option<InertialMass>,
}

fn admit_mechanics(
    simulation: &Value,
    motion: &Value,
    heat: &Value,
    request: &Value,
    coverage: bool,
    water_policy: &Value,
    qualification: &Value,
) -> Result<Option<InertialMass>, Refusal> {
    let trace = array(heat, "trace")?;
    let initial = &motion["initial_sample"]["liquid_mechanics"];
    let final_report = &motion["liquid_mechanics"];
    if qualification.is_null() {
        return if heat["mechanical_coupling"].is_null()
            && trace.iter().all(|row| row["mechanical_coupling"].is_null())
            && (coverage || (initial.is_null() && final_report.is_null()))
        {
            Ok(None)
        } else {
            Err(fail())
        };
    }
    let water = water_policy;
    let draining = water["schema"] == "ftx-textile-surface-water/4"
        || water["schema"] == "ftx-textile-surface-water/5" && !water["drainage"].is_null();
    let bindings = qualification["bindings"].as_object().ok_or_else(fail)?;
    let water_bindings = water["bindings"].as_object().ok_or_else(fail)?;
    let recoil = qualification["schema"] == "ftx-coupled-liquid-mechanics/6";
    let phase_schedule = recoil || qualification["schema"] == "ftx-coupled-liquid-mechanics/5";
    let phase_velocities = qualification["schema"] == "ftx-coupled-liquid-mechanics/4";
    let scheduled_velocity = qualification["schema"] == "ftx-coupled-liquid-mechanics/3";
    let prescribed_velocity = phase_schedule
        || phase_velocities
        || scheduled_velocity
        || qualification["schema"] == "ftx-coupled-liquid-mechanics/2";
    let exchange =
        prescribed_velocity || qualification["schema"] == "ftx-coupled-liquid-mechanics/1";
    let compatible_policy = if exchange {
        matches!(
            water["schema"].as_str(),
            Some(
                "ftx-textile-surface-water/1"
                    | "ftx-textile-surface-water/2"
                    | "ftx-textile-surface-water/3"
                    | "ftx-textile-surface-water/4"
                    | "ftx-textile-surface-water/5"
            )
        ) && qualification["model"]
            == if recoil {
                "scheduled_phase_specific_incoming_and_outgoing_velocity_mass_exchange_frozen_mechanics/1"
            } else if phase_schedule {
                "scheduled_phase_specific_incoming_velocity_mass_exchange_frozen_mechanics/1"
            } else if phase_velocities {
                "phase_specific_incoming_velocity_mass_exchange_frozen_mechanics/1"
            } else if scheduled_velocity {
                "scheduled_incoming_velocity_mass_exchange_frozen_mechanics/1"
            } else if prescribed_velocity {
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
    let intervals = if phase_schedule || scheduled_velocity {
        let count = array(motion, "keyframes")?.len();
        if !(1..=16).contains(&count) || array(request, "boundaries")?.len() != count {
            return Err(fail());
        }
        count
    } else {
        0
    };
    let incoming_field = if phase_schedule {
        Some("incoming_stream_velocities_world_m_s_by_interval")
    } else if phase_velocities {
        Some("incoming_stream_velocities_world_m_s")
    } else if scheduled_velocity {
        Some("incoming_velocity_world_m_s_by_interval")
    } else if prescribed_velocity {
        Some("incoming_velocity_world_m_s")
    } else {
        None
    };
    let outgoing_field = "outgoing_stream_velocities_relative_to_cloth_m_s_by_interval";
    let mut recoil_has_velocity = false;
    for (instance, binding) in bindings {
        let range = array(binding, "temperature_range_c")?;
        let maximum = number(binding, "maximum_liquid_loading_kg_m2")?;
        // The producer's versions have exclusive velocity fields. Explicit null is not absence.
        for field in [
            "incoming_velocity_world_m_s",
            "incoming_stream_velocities_world_m_s",
            "incoming_velocity_world_m_s_by_interval",
            "incoming_stream_velocities_world_m_s_by_interval",
        ] {
            if binding.get(field).is_some() != (incoming_field == Some(field)) {
                return Err(fail());
            }
        }
        if binding.get(outgoing_field).is_some() != recoil {
            return Err(fail());
        }
        if let Some(field) = incoming_field {
            let valid = if phase_schedule || scheduled_velocity {
                let values = array(binding, field)?;
                values.len() == intervals
                    && values.iter().all(|v| {
                        if phase_schedule {
                            velocity_pair(v, ["supply", "condensation"])
                        } else {
                            velocity(v)
                        }
                    })
            } else if phase_velocities {
                velocity_pair(&binding[field], ["supply", "condensation"])
            } else {
                velocity(&binding[field])
            };
            if !valid {
                return Err(fail());
            }
        }
        if recoil {
            let values = array(binding, outgoing_field)?;
            if values.len() != intervals {
                return Err(fail());
            }
            for pair in values {
                if !velocity_pair(pair, ["evaporation", "drainage"]) {
                    return Err(fail());
                }
                for phase in ["evaporation", "drainage"] {
                    recoil_has_velocity |=
                        array(pair, phase)?.iter().any(|v| finite(v) != Some(0.));
                }
            }
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
            != if recoil {
                "ftx-coupled-liquid-mechanics-result/6"
            } else if phase_schedule {
                "ftx-coupled-liquid-mechanics-result/5"
            } else if phase_velocities {
                "ftx-coupled-liquid-mechanics-result/4"
            } else if scheduled_velocity {
                "ftx-coupled-liquid-mechanics-result/3"
            } else if prescribed_velocity {
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
        if draining {
            let mass = value.remove("drained_liquid_mass_kg").ok_or_else(fail)?;
            value.insert("drained_liquid_kg".into(), mass);
        } else if value.contains_key("drained_liquid_mass_kg") {
            return Err(fail());
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
    let mut previous_separation_work = 0.;
    let mut previous_incoming_potential = 0.;
    let mut previous_incoming_momentum = [0.; 3];
    let mut previous_drained = 0.;
    let mut inertial_mass = InertialMass {
        final_kg: 0.,
        max_sample_kg: 0.,
    };
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
        let drained = if draining {
            nonnegative(row, "drained_liquid_kg")?
        } else {
            if row.get("drained_liquid_kg").is_some() {
                return Err(fail());
            }
            0.
        };
        let total = nonnegative(row, "total_inertial_mass_kg")?;
        inertial_mass.final_kg = total;
        inertial_mass.max_sample_kg = inertial_mass.max_sample_kg.max(total);
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
            if recoil {
                // Relative outgoing velocity requires mechanical boundary work, not thermal heat.
                let work = nonnegative(flow, "outgoing_separation_work_j")?;
                if work < previous_separation_work
                    || (index == 0 && work != 0.)
                    || (!recoil_has_velocity && work != 0.)
                    || (outgoing == previous_outgoing && work != previous_separation_work)
                {
                    return Err(fail());
                }
                previous_separation_work = work;
            } else if flow.get("outgoing_separation_work_j").is_some() {
                return Err(fail());
            }
            let thermal_incoming = if index == 0 {
                0.
            } else {
                let supply = if water_policy["schema"] == "ftx-textile-surface-water/1" {
                    0.
                } else {
                    nonnegative(water, "supplied_liquid_kg")?
                };
                let condensation = if !water_policy["condensation"].is_null() {
                    nonnegative(water, "condensed_liquid_kg")?
                } else {
                    0.
                };
                supply + condensation
            };
            if !near(incoming, thermal_incoming, thermal_incoming)
                || !near(outgoing, evaporated + drained, evaporated + drained)
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
            || !near(remaining + evaporated + drained, available, available)
            || !near(total, substrate + remaining, substrate + available)
            || !near(
                number(row, "mass_balance_residual_kg")?,
                total - substrate - remaining,
                total,
            )
            || energy < previous_energy
            || drained < previous_drained
            || (index == 0
                && (remaining != initial_mass || evaporated != 0. || drained != 0. || energy != 0.))
            || (index != 0
                && (!near(remaining, number(water, "remaining_liquid_kg")?, available)
                    || !near(
                        evaporated,
                        number(water, "evaporated_liquid_kg")?,
                        available,
                    )
                    || draining && !near(drained, number(water, "drained_liquid_kg")?, available)))
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
        previous_drained = drained;
    }
    Ok(Some(inertial_mass))
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

// The source prescribes a maximum drain demand. Available stock and competing evaporation can
// reduce actual export; this integrates the authored boundary, never a second transport solve.
struct Drainage {
    ends: Vec<f64>,
    rates: BTreeMap<String, Vec<f64>>,
}

impl Drainage {
    fn new(
        request: &Value,
        owners: &serde_json::Map<String, Value>,
        motion: &Value,
    ) -> Result<Self, Refusal> {
        source(request)?;
        let bindings = request["bindings"].as_object().ok_or_else(fail)?;
        if request["schema"] != "ftx-textile-liquid-drainage/1"
            || bindings.keys().ne(owners.keys())
            || !request["qualification"]
                .as_str()
                .is_some_and(|s| !s.trim().is_empty() && s.len() <= 4096)
        {
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
        let rates = bindings
            .iter()
            .map(|(instance, binding)| {
                let values = array(binding, "rate_kg_m2_s")?;
                if values.len() != ends.len() {
                    return Err(fail());
                }
                Ok((
                    instance.clone(),
                    values
                        .iter()
                        .map(|value| finite(value).filter(|v| *v >= 0.).ok_or_else(fail))
                        .collect::<Result<Vec<_>, _>>()?,
                ))
            })
            .collect::<Result<BTreeMap<_, _>, Refusal>>()?;
        Ok(Self { ends, rates })
    }

    fn integral(&self, instance: &str, from: f64, to: f64) -> Result<f64, Refusal> {
        if !from.is_finite()
            || !to.is_finite()
            || from < 0.
            || to < from
            || to > *self.ends.last().ok_or_else(fail)?
        {
            return Err(fail());
        }
        let rates = self.rates.get(instance).ok_or_else(fail)?;
        let mut start: f64 = 0.;
        let mut total = 0.;
        for (&end, &rate) in self.ends.iter().zip(rates) {
            let seconds = end.min(to) - start.max(from);
            if seconds > 0. {
                let increment = seconds * rate;
                let next = total + increment;
                if !next.is_finite() || (rate > 0. && (increment <= 0. || next <= total)) {
                    return Err(fail());
                }
                total = next;
            }
            start = end;
        }
        Ok(total)
    }

    fn demand(&self, areas: &BTreeMap<&str, f64>, from: f64, to: f64) -> Result<f64, Refusal> {
        let mut total = 0.;
        for (instance, area) in areas {
            let integral = self.integral(instance, from, to)?;
            let amount = area * integral;
            let next = total + amount;
            if !next.is_finite()
                || (integral > 0. && amount <= 0.)
                || (amount > 0. && next <= total)
            {
                return Err(fail());
            }
            total = next;
        }
        Ok(total)
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

// Independent accounting for the shared vapor owner. The pressure/transport root is never
// solved here: each retained liquid ledger must explain this inventory and its thermostat.
struct Reservoir {
    capacity: f64,
    initial_mass: f64,
    initial_pressure: f64,
    temperature: f64,
    domain: [f64; 2],
    mass_tolerance: f64,
    effective_enthalpy: f64,
    previous_steps: u64,
    previous_trials: u64,
    previous_maximum_residual: f64,
}

impl Reservoir {
    fn new(request: &Value, cp: f64, latent: f64, reference: f64) -> Result<Self, Refusal> {
        source(request)?;
        let capacity = number(request, "vapor_capacitance_kg_pa")?;
        let initial_mass = nonnegative(request, "initial_vapor_mass_kg")?;
        let temperature = number(request, "temperature_c")?;
        let bounds = array(request, "pressure_domain_pa")?;
        let maximum_iterations = request["max_iterations"].as_u64().ok_or_else(fail)?;
        let mass_tolerance = number(request, "mass_tolerance_kg")?;
        if request["schema"] != "ftx-textile-vapor-reservoir/1"
            || request["model"] != "isothermal_constant_vapor_capacitance/1"
            || capacity <= 0.
            || temperature <= -273.15
            || bounds.len() != 2
            || !(1..=128).contains(&maximum_iterations)
            || mass_tolerance <= 0.
            || request
                .get("allow_estimates")
                .is_some_and(|v| !v.is_boolean())
            || !request["qualification"]
                .as_str()
                .is_some_and(|s| !s.trim().is_empty() && s.len() <= 4096)
            || request.as_object().ok_or_else(fail)?.keys().any(|key| {
                !matches!(
                    key.as_str(),
                    "schema"
                        | "model"
                        | "vapor_capacitance_kg_pa"
                        | "initial_vapor_mass_kg"
                        | "temperature_c"
                        | "pressure_domain_pa"
                        | "max_iterations"
                        | "mass_tolerance_kg"
                        | "source"
                        | "evidence"
                        | "allow_estimates"
                        | "method"
                        | "qualification"
                )
            })
        {
            return Err(fail());
        }
        let domain = [
            finite(&bounds[0]).ok_or_else(fail)?,
            finite(&bounds[1]).ok_or_else(fail)?,
        ];
        let initial_pressure = initial_mass / capacity;
        let maximum_mass = domain[1] * capacity;
        let effective_enthalpy = latent + cp * (temperature - reference);
        if domain[0] < 0.
            || domain[0] >= domain[1]
            || !initial_pressure.is_finite()
            || initial_pressure < domain[0]
            || initial_pressure > domain[1]
            || (initial_mass > 0. && initial_pressure <= 0.)
            || !maximum_mass.is_finite()
            || maximum_mass <= 0.
            || !effective_enthalpy.is_finite()
        {
            return Err(fail());
        }
        Ok(Self {
            capacity,
            initial_mass,
            initial_pressure,
            temperature,
            domain,
            mass_tolerance,
            effective_enthalpy,
            previous_steps: 0,
            previous_trials: 0,
            previous_maximum_residual: 0.,
        })
    }

    fn admit(&mut self, water: &Value, time: f64, steps: u64) -> Result<(), Refusal> {
        let report = &water["vapor_reservoir"];
        let mass = nonnegative(report, "remaining_vapor_mass_kg")?;
        let pressure = nonnegative(report, "vapor_pressure_pa")?;
        let evaporated = nonnegative(water, "evaporated_liquid_kg")?;
        let condensed = nonnegative(water, "condensed_liquid_kg")?;
        let received = number(report, "net_vapor_received_kg")?;
        let residual = number(report, "mass_balance_error_kg")?;
        let mass_scale = mass + self.initial_mass + evaporated + condensed;
        let available = self.initial_mass + evaporated;
        let pressure_trials = report["pressure_trials"].as_u64().ok_or_else(fail)?;
        let maximum_residual = nonnegative(report, "maximum_step_mass_residual_kg")?;
        let new_steps = steps.checked_sub(self.previous_steps).ok_or_else(fail)?;
        let new_trials = pressure_trials
            .checked_sub(self.previous_trials)
            .ok_or_else(fail)?;
        // Physical old/new stock is at most initial vapor plus all evaporation so far. Trial
        // C*p may differ from that committed stock by the reported maximum root residual.
        // This bounds the maximum without pretending that trace samples expose every probe.
        let step_mass_scale = 2. * available + evaporated + condensed + maximum_residual;
        if report["schema"] != "ftx-textile-vapor-reservoir-result/1"
            || nonnegative(report, "initial_vapor_mass_kg")? != self.initial_mass
            || !near(
                nonnegative(report, "initial_vapor_pressure_pa")?,
                self.initial_pressure,
                self.initial_pressure,
            )
            || !near(pressure, mass / self.capacity, pressure)
            || pressure < self.domain[0]
            || pressure > self.domain[1]
            || (mass > 0. && pressure <= 0.)
            || !near(received, evaporated - condensed, mass_scale)
            || !near(residual, mass - self.initial_mass - received, mass_scale)
            || residual.abs() > self.mass_tolerance.min(4096. * f64::EPSILON * mass_scale)
            || !inside(mass, 0., available, mass_scale, steps)
            || !inside(condensed, 0., available, mass_scale, steps)
            || !accumulated_near(nonnegative(report, "elapsed_seconds")?, time, time, steps)
            || new_trials < new_steps
            || maximum_residual < self.previous_maximum_residual
            || maximum_residual > self.mass_tolerance
            || maximum_residual > 1024. * f64::EPSILON * step_mass_scale
            || !step_mass_scale.is_finite()
        {
            return Err(fail());
        }
        let change = number(report, "vapor_enthalpy_change_j")?;
        let received_j = number(report, "net_vapor_enthalpy_received_j")?;
        let removed_j = number(report, "isothermal_heat_removed_j")?;
        let error_j = number(report, "enthalpy_balance_error_j")?;
        let latent = nonnegative(water, "latent_energy_j")?;
        let exported = number(water, "exported_vapor_sensible_energy_j")?;
        let condensation = nonnegative(water, "condensation_latent_energy_j")?;
        let incoming = number(water, "incoming_vapor_sensible_energy_j")?;
        let exchanged_scale = latent + exported.abs() + condensation + incoming.abs();
        let expected_change = self.effective_enthalpy * (mass - self.initial_mass);
        let balance_scale = change.abs() + removed_j.abs() + received_j.abs();
        if !near(change, expected_change, expected_change)
            || !accumulated_near(
                received_j,
                latent + exported - condensation - incoming,
                exchanged_scale,
                steps,
            )
            || !near(removed_j, received_j - change, balance_scale)
            || !near(error_j, change + removed_j - received_j, balance_scale)
            || error_j.abs() > 1024. * f64::EPSILON * balance_scale
        {
            return Err(fail());
        }
        self.previous_steps = steps;
        self.previous_trials = pressure_trials;
        self.previous_maximum_residual = maximum_residual;
        Ok(())
    }
}

/// Return additional liquid/vapor energy and admitted coupled mass from the same history passes.
/// The caller reconciles energy with the existing independently admitted mechanical heat sources.
pub(super) fn admit(
    alternative: &Alternative,
    simulation: &Value,
    motion: &Value,
    heat: &Value,
    heat_request: &Value,
    coverage: bool,
    mechanical_heat: &[f64],
) -> Result<Option<Admission>, Refusal> {
    let (policy, qualification) = selected_policy(heat_request, heat, simulation)?;
    let inertial_mass = admit_mechanics(
        simulation,
        motion,
        heat,
        heat_request,
        coverage,
        policy,
        qualification,
    )?;
    let water = &heat["surface_water"];
    let trace = array(heat, "trace")?;
    if policy.is_null() {
        return if water.is_null() && trace.iter().all(|row| row["surface_water"].is_null()) {
            Ok(None)
        } else {
            Err(fail())
        };
    }
    let finite_reservoir = policy["schema"] == "ftx-textile-surface-water/5";
    let draining = policy["schema"] == "ftx-textile-surface-water/4"
        || finite_reservoir && !policy["drainage"].is_null();
    let condensing = policy["schema"] == "ftx-textile-surface-water/3"
        || finite_reservoir
        || draining && !policy["condensation"].is_null();
    let supplied = draining || condensing || policy["schema"] == "ftx-textile-surface-water/2";
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
        || draining == policy["drainage"].is_null()
        || (finite_reservoir && policy["vapor_reservoir"].is_null())
        || (!finite_reservoir && policy.get("vapor_reservoir").is_some())
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
    if cp <= 0. || latent <= 0. || maximum <= 0. || reference < -273.15 {
        return Err(fail());
    }
    let mut reservoir = finite_reservoir
        .then(|| Reservoir::new(&policy["vapor_reservoir"], cp, latent, reference))
        .transpose()?;
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
    let intervals = array(motion, "keyframes")?.len();
    let ambient = if finite_reservoir && policy.get("outside_vapor_pressure_pa").is_none() {
        &[][..]
    } else {
        array(policy, "outside_vapor_pressure_pa")?
    };
    if intervals == 0
        || (finite_reservoir && !ambient.is_empty())
        || (!finite_reservoir && ambient.len() != intervals)
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
        if values.len() != intervals
            || reservoir
                .as_ref()
                .is_some_and(|r| values.iter().any(|&t| t != r.temperature))
        {
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
    let drainage = draining
        .then(|| Drainage::new(&policy["drainage"], bindings, motion))
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
    let initial_temperatures = initial_heat_temperatures(heat_request, instances)?;
    let mut anchors = vec![None; columns.len()];
    for (patch, owner) in patches.iter().zip(owners) {
        let initial = *initial_temperatures
            .get(vapor_index(owner)?)
            .ok_or_else(fail)?;
        if initial < domain[0] || initial > domain[1] {
            return Err(fail());
        }
        anchors
            .get_mut(vapor_index(&patch["column"])?)
            .ok_or_else(fail)?
            .get_or_insert(initial);
    }
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
    let mut dry_initial = Vec::with_capacity(columns.len());
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
        let anchor_c = anchors[i].ok_or_else(fail)?;
        law.check_temperature(anchor_c)?;
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
            let c = law.capacity_at(anchor_c)? / cells as f64;
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
        dry_initial.push(DryInitialColumn {
            area_m2: 0.,
            anchor_c,
            offset_j: 0.,
        });
    }
    admit_vapor_assignments(alternative, instances, owners, patches, &selections)?;
    admit_vapor_material(
        &json!({"schema":"ftx-vapor-coverage-source/1", "instances":instances,
        "triangle_instance":owners,"patches":patches,"materials":selections}),
        &drape["material"],
    )?;

    let mut totals = [0.; 9];
    let mut dryouts = 0_u64;
    let mut used = BTreeSet::new();
    let mut areas = BTreeMap::new();
    let mut initial_liquid_sensible_j = 0.;
    let mut liquid_storage = 0.;
    let mut dry_storage = 0.;
    let mut final_range = [f64::INFINITY, f64::NEG_INFINITY];
    for (i, patch) in patches.iter().enumerate() {
        let owner = vapor_index(&owners[i])?;
        let initial_temperature = *initial_temperatures.get(owner).ok_or_else(fail)?;
        let instance = instances
            .get(owner)
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
        let (drained, drained_j) = if let Some(drainage) = &drainage {
            let mass = nonnegative(row, "drained_liquid_kg")?;
            let energy = number(row, "exported_liquid_sensible_energy_j")?;
            let demand = area * drainage.integral(instance, 0., number(motion, "duration_s")?)?;
            if !inside(mass, 0., demand, demand, total_steps)
                || !inside(
                    energy,
                    mass * cp * (domain[0] - reference),
                    mass * cp * (domain[1] - reference),
                    enthalpy_scale(mass),
                    total_steps,
                )
            {
                return Err(fail());
            }
            (mass, energy)
        } else {
            if row.get("drained_liquid_kg").is_some()
                || row.get("exported_liquid_sensible_energy_j").is_some()
            {
                return Err(fail());
            }
            (0., 0.)
        };
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
                remaining + evaporated + drained,
                initial + supplied_mass + condensed,
                initial + supplied_mass + condensed,
            )
        {
            return Err(fail());
        }
        used.insert(instance);
        *areas.entry(instance).or_insert(0.) += area;
        let dry_column = &mut dry_initial[column];
        dry_column.area_m2 += area;
        dry_column.offset_j +=
            area * laws[column].sensible_change(dry_column.anchor_c, initial_temperature)?;
        initial_liquid_sensible_j += initial * cp * (initial_temperature - reference);
        dryouts += u64::from(initial > 0. && remaining == 0.);
        for (total, value) in totals.iter_mut().zip([
            initial,
            remaining,
            evaporated,
            supplied_mass,
            supplied_j,
            condensed,
            incoming_j,
            drained,
            drained_j,
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
        || !initial_liquid_sensible_j.is_finite()
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
    if draining
        && (!near(number(water, "drained_liquid_kg")?, totals[7], totals[7])
            || !near(
                number(water, "exported_liquid_sensible_energy_j")?,
                totals[8],
                enthalpy_scale(totals[7]),
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

    let dry_domain_bounds = dry_heat_bounds(&laws, &dry_initial, domain)?;
    let dry_enthalpy_span = dry_domain_bounds[1] - dry_domain_bounds[0];
    let mut previous = [totals[0], 0., 0., 0.];
    let mut previous_incoming_j = 0.;
    let mut previous_drained = 0.;
    let mut previous_drained_j = 0.;
    let mut previous_time = 0.;
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
        let (drained, drained_j) = if draining {
            (
                nonnegative(w, "drained_liquid_kg")?,
                number(w, "exported_liquid_sensible_energy_j")?,
            )
        } else {
            if w.get("drained_liquid_kg").is_some()
                || w.get("exported_liquid_sensible_energy_j").is_some()
            {
                return Err(fail());
            }
            (0., 0.)
        };
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
        let dry_bounds = dry_heat_bounds(&laws, &dry_initial, range)?;
        let energy_scale = steps as f64
            * (1. + dry_enthalpy_span + cp * available_mass * (domain[1] - domain[0]))
            + 2. * enthalpy_scale(evaporated + drained + supplied_mass + condensed)
            + cp * (supplied_mass + condensed) * (domain[1] - domain[0])
            + latent_j
            + condensation_j
            + mechanical_heat.get(index).copied().ok_or_else(fail)?;
        let ambient_index = if std::ptr::eq(row, heat) {
            intervals - 1
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
        if let Some(drainage) = &drainage {
            let time = if std::ptr::eq(row, heat) {
                number(heat, "duration_s")?
            } else {
                number(row, "time_s")?
            };
            let demand = drainage.demand(&areas, previous_time, time)?;
            let increment = drained - previous_drained;
            if !inside(increment, 0., demand, drained, steps)
                || !inside(
                    drained_j - previous_drained_j,
                    increment * cp * (domain[0] - reference),
                    increment * cp * (domain[1] - reference),
                    enthalpy_scale(drained),
                    steps,
                )
            {
                return Err(fail());
            }
            previous_time = time;
        }
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
        let mass_error = remaining + evaporated + drained - initial - supplied_mass - condensed;
        let expected_schema = if finite_reservoir {
            "ftx-textile-surface-water-result/5"
        } else if draining {
            "ftx-textile-surface-water-result/4"
        } else if condensing {
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
            || drained < previous_drained
            || events < previous_dryouts
            || events > event_bound
            || (supplied
                && (events > steps.checked_mul(patches.len() as u64).ok_or_else(fail)?
                    || !maximum_total_liquid.is_finite()
                    || (remaining > maximum_total_liquid
                        && !near(remaining, maximum_total_liquid, maximum_total_liquid))))
            || !near(
                remaining + evaporated + drained,
                available_mass,
                available_mass,
            )
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
                cp * remaining * (range[0] - reference) - initial_liquid_sensible_j,
                cp * remaining * (range[1] - reference) - initial_liquid_sensible_j,
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
        if let Some(reservoir) = &mut reservoir {
            let time = if std::ptr::eq(row, heat) {
                number(heat, "duration_s")?
            } else {
                number(row, "time_s")?
            };
            reservoir.admit(w, time, steps)?;
        } else if w.get("vapor_reservoir").is_some() {
            return Err(fail());
        }
        previous = [remaining, evaporated, supplied_mass, condensed];
        previous_incoming_j = incoming_j;
        previous_drained = drained;
        previous_drained_j = drained_j;
        previous_dryouts = events;
        let correction =
            stored_j + exported_j + drained_j + latent_j - supplied_j - condensation_j - incoming_j;
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
        "drained_liquid_kg",
        "exported_liquid_sensible_energy_j",
        "vapor_reservoir",
    ] {
        if water[field] != trace.last().ok_or_else(fail)?["surface_water"][field] {
            return Err(fail());
        }
    }
    if let Some(reservoir) = &reservoir {
        // Retried histories retain failed pressure probes. They need not fit an upper bound
        // based on committed substeps; every attempt must fit the exact charged history budget.
        let cell_cost = if [
            "plastic_dissipation_heat",
            "relaxation_dissipation_heat",
            "bending_plastic_dissipation_heat",
        ]
        .iter()
        .any(|key| !heat_request[key].is_null())
            || columns
                .iter()
                .any(|column| column["schema"] == "ftx-dry-thermal-model/2")
        {
            129
        } else {
            1
        };
        let operations = total_steps
            .checked_mul(cell_cost)
            .and_then(|n| n.checked_add(reservoir.previous_trials))
            .and_then(|n| n.checked_mul(patches.len() as u64))
            .and_then(|n| n.checked_mul(cells as u64))
            .ok_or_else(fail)?;
        let maximum = heat_request["max_cell_steps"].as_u64().ok_or_else(fail)?;
        if heat["cell_steps"].as_u64() != Some(operations)
            || !(1..=10_000_000).contains(&maximum)
            || operations > maximum
        {
            return Err(fail());
        }
    }
    Ok(Some(Admission {
        energy_corrections_j: corrections,
        inertial_mass,
    }))
}

#[cfg(test)]
mod tests;

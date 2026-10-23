//! Admit finite-water readings only after the parent has verified the canonical material,
//! regional transport, complete body coverage and every retained mechanical frame.
//! This checks the existing ledger against its authored inputs; it does not supply new physics.

use ftx_core::Refusal;
use serde::Deserialize;
use serde_json::Value;

use super::{finite, invalid, vapor_array, vapor_index};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Inputs {
    schema: String,
    body_mesh_sha256: String,
    source: String,
    basis: String,
    initial_liquid_mass_kg: Vec<f64>,
    supply_rate_kg_s: Vec<f64>,
}

const FIELDS: [&str; 7] = [
    "starting_liquid_mass_kg",
    "supplied_mass_kg",
    "potential_evaporation_mass_kg",
    "evaporated_mass_kg",
    "unmet_demand_mass_kg",
    "remaining_liquid_mass_kg",
    "body_reservoir_latent_energy_j",
];

fn refusal() -> Refusal {
    invalid(
        "finite-water comparison needs the exact sourced body film, complete bounded per-triangle history, canonical covered demand and conserved step/cumulative mass and body-reservoir latent energy",
    )
}

fn number(value: &Value) -> Result<f64, Refusal> {
    finite(value).filter(|v| *v >= 0.).ok_or_else(refusal)
}

fn add(total: &mut f64, value: f64) -> Result<(), Refusal> {
    let next = *total + value;
    if !value.is_finite() || value < 0. || !next.is_finite() || (value > 0. && next <= *total) {
        return Err(refusal());
    }
    *total = next;
    Ok(())
}

fn product(value: f64, factor: f64) -> Result<f64, Refusal> {
    let result = value * factor;
    if !result.is_finite() || result < 0. || (value > 0. && result <= 0.) {
        return Err(refusal());
    }
    Ok(result)
}

fn verify_balance(recorded: &Value, expected: &[f64; 7]) -> Result<(), Refusal> {
    if FIELDS
        .iter()
        .zip(expected)
        .any(|(field, value)| recorded[*field].as_f64() != Some(*value))
    {
        return Err(refusal());
    }
    let [starting, supplied, demand, evaporated, unmet, remaining, _] = *expected;
    let mass_error = remaining - starting + evaporated - supplied;
    let demand_error = evaporated + unmet - demand;
    let scale = starting
        .max(supplied)
        .max(demand)
        .max(evaporated)
        .max(remaining);
    let tolerance = 256. * f64::EPSILON * scale;
    for (field, value) in [
        ("mass_balance_error_kg", mass_error),
        ("demand_balance_error_kg", demand_error),
    ] {
        if !value.is_finite() || value.abs() > tolerance || recorded[field].as_f64() != Some(value)
        {
            return Err(refusal());
        }
    }
    Ok(())
}

fn verify_receipt(recorded: &Value, expected: &[[f64; 7]], schema: &str) -> Result<(), Refusal> {
    let rows = vapor_array(recorded, "body")?;
    if recorded["schema"] != schema || rows.len() != expected.len() {
        return Err(refusal());
    }
    let mut totals = [0.; 7];
    for (index, (row, balance)) in rows.iter().zip(expected).enumerate() {
        if row["body_triangle"].as_u64() != Some(index as u64) {
            return Err(refusal());
        }
        verify_balance(row, balance)?;
        for (total, value) in totals.iter_mut().zip(balance) {
            add(total, *value)?;
        }
    }
    verify_balance(&recorded["totals"], &totals)
}

fn rates(frame: &Value, count: usize) -> Result<Vec<f64>, Refusal> {
    let mut values = vec![0.; count];
    for region in vapor_array(&frame["coverage"], "regions")? {
        let body = vapor_index(&region["body_triangle"])?;
        add(
            values.get_mut(body).ok_or_else(refusal)?,
            number(&region["vapor"]["potential_mass_flow_kg_s"])?,
        )?;
    }
    Ok(values)
}

/// `history` must already have passed `admit_motion_vapor_coverage`. Keep the authored declaration
/// separate: editing a retained request and recomputing its digest never grants different water.
pub(super) fn admit<'a>(history: &'a Value, authored: &Value) -> Result<&'a Value, Refusal> {
    let request = &history["request"];
    let policy = &request["policy"];
    let input: Inputs = serde_json::from_value(authored.clone()).map_err(|_| refusal())?;
    let report = &history["wet_film"];
    let initial = &request["initial"];
    let count = vapor_array(&initial["map"], "body_triangles")?.len();
    let rows = vapor_array(history, "samples")?;
    let text = |s: &str| (8..=4096).contains(&s.trim().len());
    if policy["wet_film"] != *authored
        || input.schema != "ftx-body-wet-film/1"
        || !(1..=4096).contains(&count)
        || input.initial_liquid_mass_kg.len() != count
        || input.supply_rate_kg_s.len() != count
        || input
            .initial_liquid_mass_kg
            .iter()
            .chain(&input.supply_rate_kg_s)
            .any(|v| !v.is_finite() || *v < 0.)
        || !text(&input.source)
        || !text(&input.basis)
        || report["body_mesh_sha256"].as_str() != Some(input.body_mesh_sha256.as_str())
        || history["initial"]["result"]["body_mesh_sha256"] != report["body_mesh_sha256"]
        || report["source"].as_str() != Some(input.source.as_str())
        || report["basis"].as_str() != Some(input.basis.as_str())
        || report["textile_temperature_coupled"] != false
        || report["uncertainty_propagated"] != false
        || history["latent_heat_coupled"] != false
    {
        return Err(refusal());
    }
    let retained_water = count.saturating_mul(16).saturating_mul(rows.len() + 1);
    let mut retained_vertices = vapor_array(&initial["source"], "positions_m")?
        .len()
        .saturating_add(vapor_array(&initial["map"], "body_positions_m")?.len());
    for sample in vapor_array(request, "samples")? {
        retained_vertices = retained_vertices
            .saturating_add(vapor_array(sample, "positions_m")?.len())
            .saturating_add(vapor_array(sample, "body_positions_m")?.len());
    }
    if retained_water.saturating_add(retained_vertices) > 2_000_000
        || history["retained_wet_film_scalar_equivalents"].as_u64() != Some(retained_water as u64)
        || history["retained_vertex_samples"].as_u64() != Some(retained_vertices as u64)
    {
        return Err(refusal());
    }
    let trapezoid = match policy["integration"].as_str() {
        Some("right_endpoint") => false,
        Some("composite_trapezoid") => true,
        _ => return Err(refusal()),
    };
    let boundaries = vapor_array(&initial["conditions"], "boundaries")?;
    if boundaries.len() != 1 && boundaries.len() != count {
        return Err(refusal());
    }
    let latent = (0..count)
        .map(|index| {
            let boundary = &boundaries[if boundaries.len() == 1 { 0 } else { index }];
            number(&boundary["latent_heat_j_kg"]).and_then(|value| {
                if value > 0. {
                    Ok(value)
                } else {
                    Err(refusal())
                }
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut cumulative: Vec<_> = input
        .initial_liquid_mass_kg
        .iter()
        .map(|&mass| [mass, 0., 0., 0., 0., mass, 0.])
        .collect();
    let mut previous = rates(&history["initial"]["result"], count)?;
    for row in rows {
        let current = rates(&row["result"], count)?;
        let dt = number(&row["dt_s"])?;
        if dt <= 0. {
            return Err(refusal());
        }
        let mut steps = Vec::with_capacity(count);
        for index in 0..count {
            let demand = if trapezoid {
                let mut demand = product(previous[index], 0.5 * dt)?;
                add(&mut demand, product(current[index], 0.5 * dt)?)?;
                demand
            } else {
                product(current[index], dt)?
            };
            let supplied = product(input.supply_rate_kg_s[index], dt)?;
            let starting = cumulative[index][5];
            let mut available = starting;
            add(&mut available, supplied)?;
            let evaporated = demand.min(available);
            let remaining = available - evaporated;
            let unmet = demand - evaporated;
            if (starting > 0. && supplied > 0. && available <= supplied)
                || (evaporated > 0. && remaining >= available)
                || (evaporated > 0. && unmet >= demand)
            {
                return Err(refusal());
            }
            let step = [
                starting,
                supplied,
                demand,
                evaporated,
                unmet,
                remaining,
                product(evaporated, latent[index])?,
            ];
            for field in [1, 2, 3, 4, 6] {
                add(&mut cumulative[index][field], step[field])?;
            }
            cumulative[index][5] = remaining;
            steps.push(step);
        }
        verify_receipt(&row["wet_film"], &steps, "ftx-body-wet-film-step/1")?;
        previous = current;
    }
    verify_receipt(report, &cumulative, "ftx-body-wet-film-report/1")?;
    Ok(report)
}

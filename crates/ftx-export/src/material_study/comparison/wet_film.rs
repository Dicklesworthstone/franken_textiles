//! Admit finite-water readings only after the parent has verified the canonical material,
//! regional transport, complete body coverage and every retained mechanical frame.
//! The same finite-liquid reducer used by motion reconstructs every triangle receipt; a plausible
//! total or self-authored digest cannot substitute for conserved state. No second water solver.

use std::collections::BTreeMap;

use ftx_core::Refusal;
use ftx_pattern::material_vapor::wet_film as canonical;
use serde_json::{Value, json};

use super::{Metric, finite, hash, invalid, vapor_array, vapor_index};

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

fn initial_state(input: &canonical::WetFilm, initial: &Value) -> Result<canonical::State, Refusal> {
    let count = vapor_array(&initial["map"], "body_triangles")?.len();
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
    let body_hash = hash(&json!({"positions_m":initial["map"]["body_positions_m"],
        "triangles":initial["map"]["body_triangles"]}))?;
    canonical::State::new(input, &body_hash, &latent)
}

/// `history` must already have passed `admit_motion_vapor_coverage`. Keep the authored declaration
/// separate: editing a retained request and recomputing its digest never grants different water.
pub(super) fn admit<'a>(history: &'a Value, authored: &Value) -> Result<&'a Value, Refusal> {
    let request = &history["request"];
    let policy = &request["policy"];
    let input: canonical::WetFilm =
        serde_json::from_value(authored.clone()).map_err(|_| refusal())?;
    let report = &history["wet_film"];
    let initial = &request["initial"];
    let count = vapor_array(&initial["map"], "body_triangles")?.len();
    let rows = vapor_array(history, "samples")?;
    input.validate(count)?;
    if policy["wet_film"] != *authored
        || !(1..=4096).contains(&count)
        || rows.is_empty()
        || rows.len() > 512
        || history["initial"]["result"]["body_mesh_sha256"] != report["body_mesh_sha256"]
        || [
            "storage_required",
            "dry_thermal_properties_required",
            "latent_heat_coupled",
            "physical_validation_claim",
            "comfort_claim",
        ]
        .iter()
        .any(|key| history[*key] != false)
    {
        return Err(refusal());
    }
    let retained_water =
        canonical::scalar_equivalents(count).saturating_mul(rows.len().saturating_add(1));
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
    let mut state = initial_state(&input, initial)?;
    let mut previous = if trapezoid {
        rates(&history["initial"]["result"], count)?
    } else {
        vec![0.; count]
    };
    for row in rows {
        let current = rates(&row["result"], count)?;
        let (next, receipt) = state.advance(
            &current,
            trapezoid.then_some(previous.as_slice()),
            number(&row["dt_s"])?,
        )?;
        if row["wet_film"] != receipt {
            return Err(refusal());
        }
        state = next;
        previous = current;
    }
    // Compare all physical state, source and interpretation fields. Human-readable notes do not
    // carry numerical authority and may be omitted by an otherwise complete retained receipt.
    let mut expected = state.report()?;
    let mut recorded = report.clone();
    expected
        .as_object_mut()
        .ok_or_else(refusal)?
        .remove("notes");
    recorded
        .as_object_mut()
        .ok_or_else(refusal)?
        .remove("notes");
    if recorded != expected {
        return Err(refusal());
    }
    Ok(report)
}

fn rate_bounds(frame: &Value, count: usize) -> Result<Option<[Vec<f64>; 2]>, Refusal> {
    let mut values = [vec![0.; count], vec![0.; count]];
    for region in vapor_array(&frame["coverage"], "regions")? {
        let body = vapor_index(&region["body_triangle"])?;
        let Some([_, flow]) = super::read_transfer_bounds(
            &region["vapor"]["reported_bounds"],
            ["potential_heat_flow_w", "potential_mass_flow_kg_s"],
        )?
        else {
            return Ok(None);
        };
        for (rates, endpoint) in values.iter_mut().zip(flow) {
            if add(rates.get_mut(body).ok_or_else(refusal)?, endpoint).is_err() {
                return Ok(None);
            }
        }
    }
    Ok(Some(values))
}

/// Derive final cumulative response ranges only after both canonical vapor and nominal water
/// admission. Initial liquid, supply, body geometry and latent heat remain fixed; source Ret
/// endpoints vary the complete regional rates. The parent separately admits fixed supplied mass.
pub(super) fn reported_bounds(
    history: &Value,
) -> Result<Option<BTreeMap<Metric, [f64; 2]>>, Refusal> {
    // Recheck the producer's regional, sample and aggregate Ret envelopes rather than trusting
    // a caller-carried total. Missing source endpoints are not nominal singleton intervals.
    if super::motion_vapor_bounds(history)?.is_none() {
        return Ok(None);
    }
    let request = &history["request"];
    let input: canonical::WetFilm =
        serde_json::from_value(request["policy"]["wet_film"].clone()).map_err(|_| refusal())?;
    let mut low = initial_state(&input, &request["initial"])?;
    let mut high = low.clone();
    let count = low.body_triangle_count();
    let trapezoid = match request["policy"]["integration"].as_str() {
        Some("right_endpoint") => false,
        Some("composite_trapezoid") => true,
        _ => return Err(refusal()),
    };
    let Some(mut previous) = (if trapezoid {
        rate_bounds(&history["initial"]["result"], count)?
    } else {
        Some([vec![0.; count], vec![0.; count]])
    }) else {
        return Ok(None);
    };
    for row in vapor_array(history, "samples")? {
        let Some(current) = rate_bounds(&row["result"], count)? else {
            return Ok(None);
        };
        let seconds = number(&row["dt_s"])?;
        let low_step = low.advance(
            &current[0],
            trapezoid.then_some(previous[0].as_slice()),
            seconds,
        );
        let high_step = high.advance(
            &current[1],
            trapezoid.then_some(previous[1].as_slice()),
            seconds,
        );
        let (Ok((next_low, _)), Ok((next_high, _))) = (low_step, high_step) else {
            return Ok(None);
        };
        low = next_low;
        high = next_high;
        previous = current;
    }
    let (Ok(low), Ok(high)) = (low.report(), high.report()) else {
        return Ok(None);
    };
    // Cumulative evaporation/unmet demand are monotone in each covered demand; remaining liquid
    // is antitone. Individual interval evaporation can reverse after earlier depletion, so these
    // final endpoints must never be presented as bounds on the intermediate water receipts.
    let mut ranges = BTreeMap::new();
    for (metric, field, reverse) in [
        (
            Metric::MotionWetFilmEvaporatedMassKg,
            "evaporated_mass_kg",
            false,
        ),
        (
            Metric::MotionWetFilmUnmetDemandMassKg,
            "unmet_demand_mass_kg",
            false,
        ),
        (
            Metric::MotionWetFilmBodyReservoirLatentEnergyJ,
            "body_reservoir_latent_energy_j",
            false,
        ),
        (
            Metric::MotionWetFilmRemainingLiquidMassKg,
            "remaining_liquid_mass_kg",
            true,
        ),
    ] {
        let low = number(&low["totals"][field])?;
        let high = number(&high["totals"][field])?;
        let range = if reverse { [high, low] } else { [low, high] };
        if range[0] > range[1] {
            return Ok(None);
        }
        ranges.insert(metric, range);
    }
    Ok(Some(ranges))
}

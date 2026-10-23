//! Admit finite-water readings only after the parent has verified the canonical material,
//! regional transport, complete body coverage and every retained mechanical frame.
//! The same finite-liquid reducer used by motion reconstructs every triangle receipt; a plausible
//! total or self-authored digest cannot substitute for conserved state. No second water solver.

use ftx_core::Refusal;
use ftx_pattern::material_vapor::wet_film as canonical;
use serde_json::{Value, json};

use super::{finite, hash, invalid, vapor_array, vapor_index};

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
    let mut state = canonical::State::new(&input, &body_hash, &latent)?;
    let mut previous = rates(&history["initial"]["result"], count)?;
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

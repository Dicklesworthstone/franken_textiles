//! Explicit layered coverage on a verified saved drape. No live material or inferred overlap lookup.
use ftx_core::{Refusal, Vec3, receipt::sha256_hex};
use ftx_drape::thermal::{
    Boundary,
    surface::{
        Surface,
        coverage::{Coverage, CoverageStep, Layout},
    },
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{invalid, read};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Mapping {
    schema: String,
    drape_thermal_frame_sha256: String,
    body_positions_m: Vec<Vec3>,
    layout: Layout,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Film {
    inside_c: f64,
    outside_c: f64,
    inside_resistance_m2_k_w: f64,
    outside_resistance_m2_k_w: f64,
}
impl Film {
    fn boundary(&self) -> Boundary {
        Boundary {
            inside_c: self.inside_c,
            outside_c: self.outside_c,
            inside_resistance: self.inside_resistance_m2_k_w,
            outside_resistance: self.outside_resistance_m2_k_w,
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Transient {
    initial_temperature_c: f64,
    step_seconds: f64,
    steps: usize,
    record_every: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Conditions {
    schema: String,
    /// One broadcasts; otherwise use body-triangle order, NOT source material-column order.
    boundaries: Vec<Film>,
    #[serde(default)]
    transient: Option<Transient>,
}

fn field(step: &CoverageStep) -> Value {
    json!({"regions":step.regions,"body":step.body,"temperature_c":step.state.temperature_c(),
        "body_area_m2":step.body_area_m2,"covered_area_m2":step.covered_area_m2,
        "bare_heat_w":step.bare_heat_w,"inside_heat_w":step.inside_heat_w,
        "outside_heat_w":step.outside_heat_w,"net_heat_w":step.net_heat_w,
        "stored_change_j":step.stored_change_j,"balance_error_j":step.balance_error_j,
        "unassigned_cloth_reference_area_m2":step.unassigned_cloth_reference_area_m2})
}

fn evaluate(report: &Value, mapping: &Value, conditions: &Value) -> Result<Value, Refusal> {
    let map: Mapping =
        serde_json::from_value(mapping.clone()).map_err(|e| invalid(e.to_string()))?;
    let input: Conditions =
        serde_json::from_value(conditions.clone()).map_err(|e| invalid(e.to_string()))?;
    if map.schema != "ftx-drape-thermal-coverage/1"
        || input.schema != "ftx-thermal-coverage-conditions/1"
    {
        return Err(invalid(
            "unknown thermal coverage mapping or conditions schema",
        ));
    }
    // Verify the material, topology AND final position receipt before interpreting any triangle ID.
    let (source, cloth) = Surface::from_drape_report(report)?;
    if report["thermal_surface"]["sha256"].as_str() != Some(&map.drape_thermal_frame_sha256) {
        return Err(invalid(
            "coverage mapping belongs to a different drape thermal frame",
        ));
    }
    let source_work = source.cells();
    let assembly = Coverage::new(source, map.layout)?;
    let boundaries = if input.boundaries.len() == 1 {
        vec![input.boundaries[0].boundary(); assembly.body_triangle_count()]
    } else if input.boundaries.len() == assembly.body_triangle_count() {
        input.boundaries.iter().map(Film::boundary).collect()
    } else {
        return Err(invalid(
            "provide one uniform boundary or one per coverage body triangle",
        ));
    };
    if let Some(t) = &input.transient {
        // Bare-only scenes still scan triangles. Do not let zero thermal cells bypass the work cap.
        let work = assembly
            .cells()
            .checked_add(source_work)
            .and_then(|n| n.checked_add(assembly.body_triangle_count()));
        if t.steps == 0
            || t.steps > 100_000
            || t.record_every == 0
            || !t.step_seconds.is_finite()
            || t.step_seconds <= 0.0
            || !(t.step_seconds * t.steps as f64).is_finite()
            || work
                .and_then(|n| n.checked_mul(t.steps))
                .is_none_or(|n| n > 10_000_000)
        {
            return Err(invalid(
                "invalid coverage schedule or more than ten million cell/geometry steps",
            ));
        }
    }
    let steady = assembly.steady(&cloth, &map.body_positions_m, &boundaries)?;
    let mut out = json!({"schema":"ftx-thermal-coverage-report/1",
        "drape_thermal_frame_sha256":map.drape_thermal_frame_sha256,
        "mapping_sha256":sha256_hex(&serde_json::to_vec(mapping).map_err(|e| invalid(e.to_string()))?),
        "model":assembly.snapshot(),"body_positions_m":map.body_positions_m,
        "cloth_positions_m":cloth,"conditions":conditions,
        "triangle_instance":report["thermal_surface"]["triangle_instance"],
        "instances":report["thermal_surface"]["instances"],"steady":field(&steady),
        "scope":"declared fractional body coverage with series layers and bare paths; fixed supplied poses",
        "mapping_is_geometrically_verified":false,"comfort_claim":false,"empirical_validation_claim":false,
        "not_simulated":["automatic overlap discovery","air-gap geometry","moisture","wind-dependent films",
            "radiative exchange","compressed loft","in-plane conduction","human thermoregulation"]});
    if let Some(t) = input.transient {
        let mut state = assembly.initial(t.initial_temperature_c)?;
        let mut trace = vec![json!({"time_seconds":0.0,"stored_energy_change_j":0.0})];
        let mut stored = 0.0;
        let mut incoming = 0.0;
        let mut outgoing = 0.0;
        let mut net = 0.0;
        let mut largest_error = 0.0_f64;
        for i in 1..=t.steps {
            let next = assembly.advance(
                &state,
                &cloth,
                &map.body_positions_m,
                t.step_seconds,
                &boundaries,
            )?;
            stored += next.stored_change_j;
            incoming += t.step_seconds * next.inside_heat_w;
            outgoing += t.step_seconds * next.outside_heat_w;
            // Sum net covered heat BEFORE adding identical bare contributions to the two totals.
            net += t.step_seconds * next.net_heat_w;
            largest_error = largest_error.max(next.balance_error_j.abs());
            if [stored, incoming, outgoing, net, largest_error, stored - net]
                .iter()
                .any(|v| !v.is_finite())
            {
                return Err(invalid("coverage accumulated energy overflow"));
            }
            if i % t.record_every == 0 || i == t.steps {
                trace.push(
                    json!({"time_seconds":i as f64 * t.step_seconds,"stored_energy_change_j":stored,
                    "inside_heat_w":next.inside_heat_w,"outside_heat_w":next.outside_heat_w,
                    "bare_heat_w":next.bare_heat_w,"net_heat_w":next.net_heat_w}),
                );
            }
            if i == t.steps {
                out["final"] = field(&next);
            }
            state = next.state;
        }
        out["transient"] = json!({"trace":trace,"incoming_energy_j":incoming,"outgoing_energy_j":outgoing,
            "net_energy_into_storage_j":net,"stored_energy_change_j":stored,"balance_error_j":stored-net,
            "largest_step_balance_error_j":largest_error,
            "accuracy_note":"Refine time, cells and the declared coverage partition; conservation is not measured garment accuracy."});
    }
    // Include conditions and actual geometries in the result identity, not just the material model.
    out["sha256"] = json!(sha256_hex(
        &serde_json::to_vec(&out).map_err(|e| invalid(e.to_string()))?
    ));
    Ok(out)
}

pub(super) fn run(report: &str, mapping: &str, conditions: &str) -> Result<Value, Refusal> {
    evaluate(&read(report)?, &read(mapping)?, &read(conditions)?)
}

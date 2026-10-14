//! Evaluate the frozen retained geometry/material exchange of an actual drape; no catalogue lookup.
use ftx_core::Refusal;
use ftx_drape::thermal::{
    Boundary,
    surface::{Surface, SurfaceStep},
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{invalid, read};

#[derive(Clone, Deserialize)]
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
    /// One boundary broadcasts uniformly; otherwise exactly one boundary in snapshot column order.
    boundaries: Vec<Film>,
    #[serde(default)]
    transient: Option<Transient>,
}

fn field(step: &SurfaceStep) -> Value {
    let patches: Vec<_> = step
        .patches
        .iter()
        .map(|p| {
            json!({"area_m2":p.area_m2,
        "inside_flux_w_m2":p.inside_flux_w_m2,"outside_flux_w_m2":p.outside_flux_w_m2,
        "stored_change_j":p.stored_change_j})
        })
        .collect();
    json!({"patches":patches,"temperature_c":step.state.temperature_c(),"area_m2":step.area_m2,
        "inside_heat_w":step.inside_heat_w,"outside_heat_w":step.outside_heat_w,
        "stored_change_j":step.stored_change_j,"balance_error_j":step.balance_error_j})
}

fn evaluate(report: &Value, conditions: &Value) -> Result<Value, Refusal> {
    let input: Conditions =
        serde_json::from_value(conditions.clone()).map_err(|e| invalid(e.to_string()))?;
    if input.schema != "ftx-thermal-surface-conditions/1" {
        return Err(invalid("unknown thermal surface conditions schema"));
    }
    let (surface, x) = Surface::from_drape_report(report)?;
    let boundaries = if input.boundaries.len() == 1 {
        vec![input.boundaries[0].boundary(); surface.column_count()]
    } else if input.boundaries.len() == surface.column_count() {
        input.boundaries.iter().map(Film::boundary).collect()
    } else {
        return Err(invalid(
            "provide one uniform boundary or one boundary per source-locked column",
        ));
    };
    let steady = surface.steady(&x, &boundaries)?;
    let mut out = json!({"schema":"ftx-thermal-surface-report/1",
        "drape_thermal_frame_sha256":report["thermal_surface"]["sha256"],
        "model_sha256":report["thermal_surface"]["model"]["sha256"],"conditions":conditions,
        "positions_m":x,"patch_topology":report["thermal_surface"]["model"]["patches"],
        "triangle_instance":report["thermal_surface"]["triangle_instance"],
        "instances":report["thermal_surface"]["instances"],"steady":field(&steady),
        "scope":"independent dry sheet exchange at fixed final drape geometry; NOT total wearer heat loss",
        "comfort_claim":false,"empirical_validation_claim":false,
        "geometry_policy":"fixed resistance per current area; heat capacity follows reference mass",
        "not_simulated":["overlapping sheet coupling","body coverage","air-gap geometry","moisture",
            "wind-dependent films","radiative exchange","compressed loft","mechanical work heating"]});
    if let Some(t) = input.transient {
        if t.steps == 0
            || t.steps > 100_000
            || t.record_every == 0
            || !t.step_seconds.is_finite()
            || t.step_seconds <= 0.
            || !(t.step_seconds * t.steps as f64).is_finite()
            || surface
                .cells()
                .checked_mul(t.steps)
                .is_none_or(|n| n > 10_000_000)
        {
            return Err(invalid(
                "invalid thermal schedule or more than ten million cell steps",
            ));
        }
        let mut old = surface.initial(t.initial_temperature_c)?;
        let mut trace = vec![json!({"time_seconds":0.,"stored_change_j":0.})];
        let mut stored = 0.;
        let mut incoming = 0.;
        let mut outgoing = 0.;
        let mut largest_error = 0.0_f64;
        for i in 1..=t.steps {
            let next = surface.advance(&old, &x, t.step_seconds, &boundaries)?;
            stored += next.stored_change_j;
            incoming += t.step_seconds * next.inside_heat_w;
            outgoing += t.step_seconds * next.outside_heat_w;
            largest_error = largest_error.max(next.balance_error_j.abs());
            if [stored, incoming, outgoing, largest_error]
                .iter()
                .any(|v| !v.is_finite())
            {
                return Err(invalid("thermal accumulated energy overflow"));
            }
            if i % t.record_every == 0 || i == t.steps {
                trace.push(
                    json!({"time_seconds":i as f64*t.step_seconds,"stored_change_j":stored,
                    "inside_heat_w":next.inside_heat_w,"outside_heat_w":next.outside_heat_w}),
                );
            }
            if i == t.steps {
                out["final"] = field(&next);
            }
            old = next.state;
        }
        let balance = stored - (incoming - outgoing);
        if !balance.is_finite() {
            return Err(invalid("thermal accumulated balance overflow"));
        }
        out["transient"] = json!({"trace":trace,"incoming_energy_j":incoming,"outgoing_energy_j":outgoing,
            "stored_energy_change_j":stored,"balance_error_j":balance,
            "largest_step_balance_error_j":largest_error,
            "accuracy_note":"Refine time and layer cells; implicit stability does not establish accuracy."});
    }
    Ok(out)
}

pub(super) fn run(report_path: &str, conditions_path: &str) -> Result<Value, Refusal> {
    evaluate(&read(report_path)?, &read(conditions_path)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ftx_core::receipt::sha256_hex;
    use ftx_drape::thermal::{DryThermalModel, surface::Patch};
    use ftx_pattern::{material_catalog::Selection, materials::Catalog};

    fn report() -> Value {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../../tests/fixtures/materials/synthetic-thermal.json"
        ))
        .unwrap();
        let catalog = Catalog::builtin().unwrap();
        let layers = fixture["layers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| {
                let selection: Selection = serde_json::from_value(v.clone()).unwrap();
                catalog.resolve(&selection).unwrap()
            })
            .collect();
        let column = DryThermalModel::new(
            layers,
            serde_json::from_value(fixture["interface_resistance_m2_k_w"].clone()).unwrap(),
            2,
        )
        .unwrap();
        let surface = Surface::new(
            vec![column],
            3,
            vec![Patch {
                vertices: [0, 1, 2],
                reference_area_m2: 0.5,
                column: 0,
            }],
        )
        .unwrap();
        let material = json!({"basis":"synthetic CLI test", "layers":fixture["layers"]});
        let mut frame = json!({"schema":"ftx-drape-thermal-surface/1","available":true,"model":surface.snapshot(),
            "material_report_sha256":sha256_hex(&serde_json::to_vec(&material).unwrap()),
            "positions_m":[[0.,0.,0.],[1.,0.,0.],[0.,1.,0.]],"triangle_instance":[0],"instances":["synthetic"]});
        frame["sha256"] = json!(sha256_hex(&serde_json::to_vec(&frame).unwrap()));
        json!({"thermal_surface":frame,"material":material})
    }
    fn conditions() -> Value {
        json!({"schema":"ftx-thermal-surface-conditions/1","boundaries":[{"inside_c":30.,"outside_c":0.,
            "inside_resistance_m2_k_w":0.1,"outside_resistance_m2_k_w":0.2}],
            "transient":{"initial_temperature_c":0.,"step_seconds":1.,"steps":30,"record_every":10}})
    }
    #[test]
    fn heat_map_and_energy_trace_use_frozen_geometry_and_sources() {
        let input = report();
        let out = evaluate(&input, &conditions()).unwrap();
        assert_eq!(out["positions_m"], input["thermal_surface"]["positions_m"]);
        assert_eq!(
            out["model_sha256"],
            input["thermal_surface"]["model"]["sha256"]
        );
        assert_eq!(out["comfort_claim"], false);
        assert_eq!(out["transient"]["trace"].as_array().unwrap().len(), 4);
        let error = out["transient"]["balance_error_j"].as_f64().unwrap();
        assert!(error.abs() < 1e-8, "{error}");
        assert_eq!(out["final"]["patches"].as_array().unwrap().len(), 1);
        assert!(out["final"]["inside_heat_w"].as_f64().unwrap() > 0.);
        let mut steady = conditions();
        steady.as_object_mut().unwrap().remove("transient");
        assert!(evaluate(&input, &steady).unwrap().get("final").is_none());
    }
    #[test]
    fn unsupported_conditions_and_incomplete_or_changed_drapes_refuse() {
        let report = report();
        for (key, value) in [
            ("steps", json!(0)),
            ("steps", json!(100_001)),
            ("record_every", json!(0)),
            ("step_seconds", json!(-1)),
        ] {
            let mut c = conditions();
            c["transient"][key] = value;
            assert!(evaluate(&report, &c).is_err());
        }
        let mut c = conditions();
        c["wind_speed"] = json!(3);
        assert!(evaluate(&report, &c).is_err());
        c = conditions();
        c["boundaries"] = json!([]);
        assert!(evaluate(&report, &c).is_err());
        assert!(
            evaluate(
                &json!({"thermal_surface":{"available":false}}),
                &conditions()
            )
            .is_err()
        );
        let mut changed = report;
        changed["thermal_surface"]["positions_m"][0][0] = json!(10);
        assert!(evaluate(&changed, &conditions()).is_err());
    }
}

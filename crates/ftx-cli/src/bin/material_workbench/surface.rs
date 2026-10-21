//! Evaluate the frozen retained geometry/material exchange of an actual drape; no catalogue lookup.
use ftx_core::Refusal;
use ftx_drape::thermal::{
    Boundary,
    steady::exterior::Exterior,
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
    /// Nonuniform column assignments must name the exact retained material/topology model.
    #[serde(default)]
    model_sha256: Option<String>,
    /// One qualified environment broadcasts; otherwise one in frozen column order.
    #[serde(default)]
    exterior_exchange: Option<Vec<Exterior>>,
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
    let mut result = json!({"patches":patches,"temperature_c":step.state.temperature_c(),"area_m2":step.area_m2,
        "inside_heat_w":step.inside_heat_w,"outside_heat_w":step.outside_heat_w,
        "stored_change_j":step.stored_change_j,"balance_error_j":step.balance_error_j});
    if let Some(e) = &step.exterior_exchange {
        result["exterior_exchange"] = json!(e);
    }
    result
}

fn evaluate(report: &Value, conditions: &Value) -> Result<Value, Refusal> {
    let input: Conditions =
        serde_json::from_value(conditions.clone()).map_err(|e| invalid(e.to_string()))?;
    if input.schema != "ftx-thermal-surface-conditions/1"
        || input.boundaries.is_empty()
        || input.boundaries.len() > 65_536
        || input
            .exterior_exchange
            .as_ref()
            .is_some_and(|e| e.is_empty() || e.len() > 65_536)
    {
        return Err(invalid("invalid thermal surface schema or boundary count"));
    }
    let (surface, x) = Surface::from_drape_report(report)?;
    let model_sha256 = report["thermal_surface"]["model"]["sha256"]
        .as_str()
        .ok_or_else(|| invalid("missing thermal surface model identity"))?;
    if input
        .model_sha256
        .as_deref()
        .is_some_and(|s| s != model_sha256)
        || ((input.boundaries.len() != 1
            || input
                .exterior_exchange
                .as_ref()
                .is_some_and(|e| e.len() != 1))
            && input.model_sha256.is_none())
    {
        return Err(invalid(
            "per-column boundaries require the exact model_sha256; use one uniform boundary for substitution",
        ));
    }
    let boundaries = if input.boundaries.len() == 1 {
        vec![input.boundaries[0].boundary(); surface.column_count()]
    } else if input.boundaries.len() == surface.column_count() {
        input.boundaries.iter().map(Film::boundary).collect()
    } else {
        return Err(invalid(
            "provide one uniform boundary or one boundary per source-locked column",
        ));
    };
    let exteriors = input
        .exterior_exchange
        .as_ref()
        .map(|e| {
            if e.len() == 1 {
                Ok(vec![e[0].clone(); surface.column_count()])
            } else if e.len() == surface.column_count() {
                Ok(e.clone())
            } else {
                Err(invalid(
                    "one exterior environment per source-locked column is required",
                ))
            }
        })
        .transpose()?;
    let steady = match &exteriors {
        Some(e) => surface.steady_exterior(&x, &boundaries, e)?,
        None => surface.steady(&x, &boundaries)?,
    };
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
    if let Some(e) = &exteriors {
        if e.iter().any(|e| e.radiation.is_some()) {
            out["not_simulated"]
                .as_array_mut()
                .expect("authored limit list")
                .retain(|v| v != "radiative exchange");
        }
        out["not_simulated"]
            .as_array_mut()
            .expect("authored limit list")
            .extend([
                json!("interlayer or finite-view-factor radiation"),
                json!("solar/spectral irradiation"),
            ]);
        out["exterior_model"] = json!(
            "prescribed convection; optional source-qualified opaque-gray large-isothermal-enclosure radiation; no wind correlation"
        );
    }
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
        let mut convective = 0.;
        let mut radiative = 0.;
        let mut largest_error = 0.0_f64;
        for i in 1..=t.steps {
            let next = match &exteriors {
                Some(e) => surface.advance_exterior(&old, &x, t.step_seconds, &boundaries, e)?,
                None => surface.advance(&old, &x, t.step_seconds, &boundaries)?,
            };
            stored += next.stored_change_j;
            incoming += t.step_seconds * next.inside_heat_w;
            outgoing += t.step_seconds * next.outside_heat_w;
            if let Some(e) = &next.exterior_exchange {
                convective += t.step_seconds * e.convective_heat_w;
                radiative += t.step_seconds * e.radiative_heat_w;
            }
            largest_error = largest_error.max(next.balance_error_j.abs());
            if [
                stored,
                incoming,
                outgoing,
                convective,
                radiative,
                largest_error,
            ]
            .iter()
            .any(|v| !v.is_finite())
            {
                return Err(invalid("thermal accumulated energy overflow"));
            }
            if i % t.record_every == 0 || i == t.steps {
                let mut sample = json!({"time_seconds":i as f64*t.step_seconds,"stored_change_j":stored,
                    "inside_heat_w":next.inside_heat_w,"outside_heat_w":next.outside_heat_w});
                if let Some(e) = &next.exterior_exchange {
                    sample["exterior_exchange"] = json!({"convective_heat_w":e.convective_heat_w,
                        "radiative_heat_w":e.radiative_heat_w,"balance_residual_w":e.balance_residual_w,
                        "convective_energy_j":convective,"radiative_energy_j":radiative});
                }
                trace.push(sample);
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
        if exteriors.is_some() {
            out["transient"]["exterior_exchange"] = json!({"convective_energy_j":convective,
                "radiative_energy_j":radiative});
        }
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
        report_from_fixture(fixture, false)
    }

    fn report_from_fixture(fixture: Value, two_columns: bool) -> Value {
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
        let mut columns = vec![column.clone()];
        let mut patches = vec![Patch {
            vertices: [0, 1, 2],
            reference_area_m2: 0.5,
            column: 0,
        }];
        let mut positions = vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]];
        let mut owners = vec![0];
        let mut instances = vec!["synthetic"];
        if two_columns {
            columns.push(column);
            patches.push(Patch {
                vertices: [1, 3, 2],
                reference_area_m2: 0.5,
                column: 1,
            });
            positions.push([1., 1., 0.]);
            owners.push(1);
            instances.push("synthetic-second");
        }
        let surface = Surface::new(columns, positions.len(), patches).unwrap();
        let material = json!({"basis":"synthetic CLI test", "layers":fixture["layers"]});
        let mut frame = json!({"schema":"ftx-drape-thermal-surface/1","available":true,"model":surface.snapshot(),
            "material_report_sha256":sha256_hex(&serde_json::to_vec(&material).unwrap()),
            "positions_m":positions,"triangle_instance":owners,"instances":instances});
        frame["sha256"] = json!(sha256_hex(&serde_json::to_vec(&frame).unwrap()));
        json!({"thermal_surface":frame,"material":material})
    }
    fn conditions() -> Value {
        json!({"schema":"ftx-thermal-surface-conditions/1","boundaries":[{"inside_c":30.,"outside_c":0.,
            "inside_resistance_m2_k_w":0.1,"outside_resistance_m2_k_w":0.2}],
            "transient":{"initial_temperature_c":0.,"step_seconds":1.,"steps":30,"record_every":10}})
    }

    fn exterior_fixture() -> Value {
        serde_json::from_str(include_str!(
            "../../../../../tests/fixtures/materials/synthetic-exterior-exchange.json"
        ))
        .unwrap()
    }

    fn exterior_report(emissivity: f64, two_columns: bool) -> Value {
        let mut fixture = exterior_fixture();
        fixture["layers"][0]["record"]["properties"]["emissivity"]["value"] = json!(emissivity);
        for (key, value, unit) in [
            ("areal_density", 0.2, "kg/m2"),
            ("specific_heat", 1000., "J/(kg*K)"),
        ] {
            fixture["layers"][0]["record"]["properties"][key] = json!({"value":value,
                "unit":unit,"evidence":"estimate","source":"synthetic","method":"Synthetic storage",
                "conditions":{"specimen":"synthetic-coupon","conditioning":"synthetic-dry"}});
        }
        report_from_fixture(fixture, two_columns)
    }

    fn exterior_conditions() -> Value {
        let mut c = conditions();
        c["boundaries"][0]["outside_c"] = json!(5.);
        c["boundaries"][0]["outside_resistance_m2_k_w"] = json!(0.);
        c["exterior_exchange"] = json!([exterior_fixture()["exterior_exchange"]]);
        c["transient"] = json!({"initial_temperature_c":10.,"step_seconds":0.5,
            "steps":4,"record_every":1});
        c
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
        assert!(out["steady"].get("exterior_exchange").is_none());
        assert!(out["transient"].get("exterior_exchange").is_none());
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

    #[test]
    fn exterior_surface_adapter_matches_core_and_follows_material_only_emissivity_changes() {
        let report = exterior_report(0.8, false);
        let c = exterior_conditions();
        let out = evaluate(&report, &c).unwrap();
        let input: Conditions = serde_json::from_value(c.clone()).unwrap();
        let (surface, positions) = Surface::from_drape_report(&report).unwrap();
        let boundaries = vec![input.boundaries[0].boundary()];
        let exteriors = input.exterior_exchange.unwrap();
        assert_eq!(
            out["steady"],
            field(
                &surface
                    .steady_exterior(&positions, &boundaries, &exteriors)
                    .unwrap()
            )
        );
        let mut old = surface.initial(10.).unwrap();
        let (mut convective, mut radiative) = (0., 0.);
        for i in 1..=4 {
            let next = surface
                .advance_exterior(&old, &positions, 0.5, &boundaries, &exteriors)
                .unwrap();
            let e = next.exterior_exchange.as_ref().unwrap();
            convective += 0.5 * e.convective_heat_w;
            radiative += 0.5 * e.radiative_heat_w;
            let sample = &out["transient"]["trace"][i]["exterior_exchange"];
            assert_eq!(sample["convective_heat_w"], json!(e.convective_heat_w));
            assert_eq!(sample["radiative_heat_w"], json!(e.radiative_heat_w));
            assert_eq!(sample["convective_energy_j"], json!(convective));
            assert_eq!(sample["radiative_energy_j"], json!(radiative));
            if i == 4 {
                assert_eq!(out["final"], field(&next));
            }
            old = next.state;
        }
        assert_eq!(out["conditions"], c);
        assert!(
            (out["transient"]["outgoing_energy_j"].as_f64().unwrap() - convective - radiative)
                .abs()
                < 1e-6
        );
        assert!(out["transient"]["balance_error_j"].as_f64().unwrap().abs() < 1e-6);
        let changed = evaluate(&exterior_report(0.05, false), &c).unwrap();
        assert_eq!(out["positions_m"], changed["positions_m"]);
        assert_eq!(out["patch_topology"], changed["patch_topology"]);
        assert_ne!(out["model_sha256"], changed["model_sha256"]);
        assert_ne!(
            out["final"]["temperature_c"],
            changed["final"]["temperature_c"]
        );
    }

    #[test]
    fn nonuniform_surface_conditions_need_matching_model_identity_and_exact_counts() {
        let report = exterior_report(0.8, true);
        let mut c = exterior_conditions();
        let mut second = c["exterior_exchange"][0].clone();
        second["convection_w_m2_k"] = json!(7.);
        c["exterior_exchange"] = json!([c["exterior_exchange"][0], second]);
        assert!(evaluate(&report, &c).is_err());
        c["model_sha256"] = report["thermal_surface"]["model"]["sha256"].clone();
        let out = evaluate(&report, &c).unwrap();
        let patches = out["steady"]["exterior_exchange"]["patches"]
            .as_array()
            .unwrap();
        assert_eq!(patches.len(), 2);
        assert_ne!(patches[0]["surface_c"], patches[1]["surface_c"]);
        for value in [json!([]), json!([second.clone(), second.clone(), second])] {
            let mut bad = c.clone();
            bad["exterior_exchange"] = value;
            assert!(evaluate(&report, &bad).is_err());
        }
        c["model_sha256"] = json!("0".repeat(64));
        assert!(evaluate(&report, &c).is_err());

        let mut films = conditions();
        films["boundaries"] = json!([films["boundaries"][0], films["boundaries"][0]]);
        assert!(evaluate(&report, &films).is_err());
        films["model_sha256"] = report["thermal_surface"]["model"]["sha256"].clone();
        assert!(evaluate(&report, &films).is_ok());
        films["boundaries"] = json!([]);
        assert!(evaluate(&report, &films).is_err());
    }
}

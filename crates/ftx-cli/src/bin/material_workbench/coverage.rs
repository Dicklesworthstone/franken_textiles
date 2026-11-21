//! Source-locked layered coverage; generated geometric maps are recomputed before evaluation.
use ftx_core::{Refusal, Vec3, receipt::sha256_hex};
use ftx_drape::thermal::{
    Boundary,
    microclimate::MicroclimateExterior,
    steady::exterior::Exterior,
    surface::{
        Surface,
        coverage::{Coverage, CoverageStep, ExteriorConditions, Layout, motion::Frame},
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{invalid, read, thermal_projection};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Mapping {
    schema: String,
    drape_thermal_frame_sha256: String,
    body_positions_m: Vec<Vec3>,
    layout: Layout,
    #[serde(default)]
    projection: Option<thermal_projection::Receipt>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Film {
    inside_c: f64,
    outside_c: f64,
    inside_resistance_m2_k_w: f64,
    outside_resistance_m2_k_w: f64,
}
impl Film {
    pub(super) fn boundary(&self) -> Boundary {
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
    exterior_exchange: Option<Vec<Exterior>>,
    /// Optional validity-gated moist-air Nusselt correlation convection (`Evidence::Estimate`;
    /// mutually exclusive with `exterior_exchange`). One entry broadcasts; otherwise body-triangle order.
    #[serde(default)]
    microclimate_exterior: Option<Vec<MicroclimateExterior>>,
    /// Explicit skin films when exterior exchange is enabled; one broadcasts in body order.
    #[serde(default)]
    bare_boundaries: Vec<Film>,
    #[serde(default)]
    transient: Option<Transient>,
}

pub(super) fn field(step: &CoverageStep) -> Value {
    let mut field = json!({"regions":step.regions,"body":step.body,"temperature_c":step.state.temperature_c(),
        "body_area_m2":step.body_area_m2,"covered_area_m2":step.covered_area_m2,
        "bare_heat_w":step.bare_heat_w,"inside_heat_w":step.inside_heat_w,
        "outside_heat_w":step.outside_heat_w,"net_heat_w":step.net_heat_w,
        "stored_change_j":step.stored_change_j,"balance_error_j":step.balance_error_j,
        "unassigned_cloth_reference_area_m2":step.unassigned_cloth_reference_area_m2});
    if let Some(exterior) = &step.exterior_exchange {
        field["exterior_exchange"] = json!(exterior);
    }
    field
}

/// Reuse saved-drape verification and the exact geometric-map replay for moving heat histories.
pub(super) fn motion_frame(report: &Value, mapping: &Value) -> Result<(Frame, usize), Refusal> {
    let map: Mapping =
        serde_json::from_value(mapping.clone()).map_err(|e| invalid(e.to_string()))?;
    if map.schema != "ftx-drape-thermal-coverage/1" {
        return Err(invalid("unknown thermal coverage mapping schema"));
    }
    let (source, cloth) = Surface::from_drape_report(report)?;
    if report["thermal_surface"]["sha256"].as_str() != Some(&map.drape_thermal_frame_sha256) {
        return Err(invalid(
            "coverage mapping belongs to a different drape thermal frame",
        ));
    }
    let work = source.cells();
    let receipt = map.projection.as_ref().ok_or_else(|| {
        invalid(
            "thermal motion requires geometric thermal-map output, not a manual coverage layout",
        )
    })?;
    let frame = thermal_projection::motion_frame(
        source,
        cloth,
        report,
        map.body_positions_m,
        &map.layout,
        receipt,
    )?;
    Ok((frame, work))
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
    let projection = map
        .projection
        .as_ref()
        .map(|receipt| {
            thermal_projection::verify(
                &source,
                &cloth,
                report,
                &map.body_positions_m,
                &map.layout,
                receipt,
            )
        })
        .transpose()?;
    let assembly = Coverage::new(source, map.layout)?;
    let count = assembly.body_triangle_count();
    let boundaries = if input.boundaries.len() == 1 {
        vec![input.boundaries[0].boundary(); count]
    } else if input.boundaries.len() == count {
        input.boundaries.iter().map(Film::boundary).collect()
    } else {
        return Err(invalid(
            "provide one uniform boundary or one per coverage body triangle",
        ));
    };
    if input.exterior_exchange.is_some() && input.microclimate_exterior.is_some() {
        return Err(invalid(
            "supply either exterior_exchange or microclimate_exterior, not both",
        ));
    }
    if input
        .microclimate_exterior
        .as_ref()
        .is_some_and(|v| v.is_empty() || v.len() > 65_536)
    {
        return Err(invalid(
            "provide one exterior environment or one per coverage body triangle",
        ));
    }
    let (environments, microclimate) = if let Some(values) = &input.exterior_exchange {
        let expanded = if values.len() == 1 {
            vec![values[0].clone(); count]
        } else if values.len() == count {
            values.clone()
        } else {
            return Err(invalid(
                "provide one exterior environment or one per coverage body triangle",
            ));
        };
        (Some(expanded), None)
    } else if let Some(mc) = &input.microclimate_exterior {
        if mc.len() != 1 && mc.len() != count {
            return Err(invalid(
                "provide one exterior environment or one per coverage body triangle",
            ));
        }
        let resolved = mc
            .iter()
            .map(MicroclimateExterior::resolve)
            .collect::<Result<Vec<_>, _>>()?;
        let expanded = if resolved.len() == 1 {
            vec![resolved[0].exterior.clone(); count]
        } else {
            resolved.iter().map(|r| r.exterior.clone()).collect()
        };
        let receipts: Vec<_> = resolved.iter().map(|r| r.receipt.clone()).collect();
        // Partial never rounds up: only omit "wind-dependent films" when every body triangle is forced.
        let forced = resolved.iter().all(|r| r.forced_convection);
        (Some(expanded), Some((json!(receipts), forced)))
    } else {
        (None, None)
    };
    if environments.is_none() && !input.bare_boundaries.is_empty() {
        return Err(invalid(
            "bare_boundaries requires exterior_exchange or microclimate_exterior",
        ));
    }
    let bare_boundaries = if input.bare_boundaries.is_empty() {
        Vec::new()
    } else if input.bare_boundaries.len() == 1 {
        vec![input.bare_boundaries[0].boundary(); count]
    } else if input.bare_boundaries.len() == count {
        input.bare_boundaries.iter().map(Film::boundary).collect()
    } else {
        return Err(invalid(
            "provide one bare-body film boundary or one per coverage body triangle",
        ));
    };
    if let Some(values) = &environments
        && boundaries
            .iter()
            .zip(values)
            .any(|(b, e)| b.outside_resistance != 0.0 || b.outside_c != e.air_c)
    {
        return Err(invalid(
            "covered exterior exchange needs zero outside film and outside_c equal to air_c",
        ));
    }
    let exterior = environments
        .as_deref()
        .map(|environments| ExteriorConditions {
            environments,
            bare_boundaries: &bare_boundaries,
        });
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
    let steady = match exterior {
        Some(e) => assembly.steady_exterior(&cloth, &map.body_positions_m, &boundaries, e)?,
        None => assembly.steady(&cloth, &map.body_positions_m, &boundaries)?,
    };
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
    if let Some(projection) = projection {
        out["projection"] = projection;
        out["mapping_is_geometrically_verified"] = json!(true);
        out["geometric_verification_scope"] = json!(
            "recomputed local-normal projection on supplied body triangles; not a global nonintersecting body, air-gap or empirical validity certificate"
        );
        out["scope"] = json!(
            "geometry-derived fractional body coverage with series layers and bare paths; fixed supplied poses"
        );
        out["not_simulated"] = json!([
            "global body validity",
            "surface air-gap transport",
            "moisture",
            "wind-dependent films",
            "radiative exchange",
            "compressed loft",
            "in-plane conduction",
            "human thermoregulation"
        ]);
    }
    if let Some(environments) = &environments {
        out["exterior_exchange"] = json!({
            "conditions":environments,"bare_boundaries":input.bare_boundaries,
            "area_basis":"current allocated outer cloth area for exterior flux; reference mass for storage",
            "scope": if microclimate.is_some() {
                "validity-gated moist-air Nusselt correlation convection (Evidence::Estimate) and optional qualified outer-material radiation; separate explicit bare-body films"
            } else {
                "prescribed convection and optional qualified outer-material radiation; separate explicit bare-body films"
            },
            "positive_heat_direction":"outward from textile",
            "inner_layer_radiation":false
        });
        // Partial never rounds up: only omit "radiative exchange" when every body triangle enables radiation.
        out["not_simulated"].as_array_mut().unwrap().retain(|v| {
            v != "radiative exchange"
                || environments.is_empty()
                || !environments.iter().all(|e| e.radiation.is_some())
        });
        if let Some((receipts, forced)) = microclimate {
            let limits = out["not_simulated"].as_array_mut().unwrap();
            if forced {
                limits.retain(|v| v != "wind-dependent films");
            }
            limits.extend([
                json!("empirical garment-film / wind-tunnel validation"),
                json!("interlayer or finite-view-factor radiation"),
                json!("solar/spectral irradiation"),
            ]);
            out["microclimate_convection"] = receipts;
            out["exterior_model"] = json!(
                "validity-gated moist-air Nusselt correlation convection (Evidence::Estimate; model-form discrepancy allowance retained); optional source-qualified opaque-gray large-isothermal-enclosure radiation; not a measured garment film"
            );
        }
    }
    if let Some(t) = input.transient {
        let mut state = assembly.initial(t.initial_temperature_c)?;
        let mut trace = vec![json!({"time_seconds":0.0,"stored_energy_change_j":0.0})];
        let mut stored = 0.0;
        let mut incoming = 0.0;
        let mut outgoing = 0.0;
        let mut net = 0.0;
        let mut convective = 0.0;
        let mut radiative = 0.0;
        let mut bare_energy = 0.0;
        let mut largest_error = 0.0_f64;
        for i in 1..=t.steps {
            let next = match exterior {
                Some(e) => assembly.advance_exterior(
                    &state,
                    &cloth,
                    &map.body_positions_m,
                    t.step_seconds,
                    &boundaries,
                    e,
                )?,
                None => assembly.advance(
                    &state,
                    &cloth,
                    &map.body_positions_m,
                    t.step_seconds,
                    &boundaries,
                )?,
            };
            stored += next.stored_change_j;
            incoming += t.step_seconds * next.inside_heat_w;
            outgoing += t.step_seconds * next.outside_heat_w;
            // Sum net covered heat BEFORE adding identical bare contributions to the two totals.
            net += t.step_seconds * next.net_heat_w;
            if let Some(e) = &next.exterior_exchange {
                convective += t.step_seconds * e.convective_heat_w;
                radiative += t.step_seconds * e.radiative_heat_w;
                bare_energy += t.step_seconds * next.bare_heat_w;
            }
            largest_error = largest_error.max(next.balance_error_j.abs());
            if [
                stored,
                incoming,
                outgoing,
                net,
                largest_error,
                stored - net,
                convective,
                radiative,
                bare_energy,
                (outgoing - bare_energy) - convective - radiative,
            ]
            .iter()
            .any(|v| !v.is_finite())
            {
                return Err(invalid("coverage accumulated energy overflow"));
            }
            if i % t.record_every == 0 || i == t.steps {
                let mut sample = json!({"time_seconds":i as f64 * t.step_seconds,"stored_energy_change_j":stored,
                    "inside_heat_w":next.inside_heat_w,"outside_heat_w":next.outside_heat_w,
                    "bare_heat_w":next.bare_heat_w,"net_heat_w":next.net_heat_w});
                if let Some(e) = &next.exterior_exchange {
                    sample["exterior_exchange"] = json!({
                        "convective_heat_w":e.convective_heat_w,"radiative_heat_w":e.radiative_heat_w,
                        "balance_residual_w":e.balance_residual_w,
                        "convective_energy_j":convective,"radiative_energy_j":radiative,
                        "bare_energy_j":bare_energy});
                }
                trace.push(sample);
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
        if exterior.is_some() {
            out["transient"]["exterior_exchange"] = json!({
                "convective_energy_j":convective,"radiative_energy_j":radiative,
                "bare_energy_j":bare_energy,
                "outside_energy_balance_error_j":(outgoing-bare_energy)-convective-radiative
            });
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use ftx_drape::thermal::{DryThermalModel, surface::Patch};
    use ftx_pattern::{material_catalog::Selection, materials::Catalog};

    fn two_triangle_report_and_mapping() -> (Value, Value) {
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
        let patches = vec![
            Patch {
                vertices: [0, 1, 2],
                reference_area_m2: 0.5,
                column: 0,
            },
            Patch {
                vertices: [1, 3, 2],
                reference_area_m2: 0.5,
                column: 0,
            },
        ];
        let positions = vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [1., 1., 0.]];
        let surface = Surface::new(vec![column], positions.len(), patches).unwrap();
        let material = json!({"basis": "synthetic CLI test", "layers": fixture["layers"]});
        let mut frame = json!({
            "schema": "ftx-drape-thermal-surface/1",
            "available": true,
            "model": surface.snapshot(),
            "material_report_sha256": sha256_hex(&serde_json::to_vec(&material).unwrap()),
            "positions_m": positions,
            "triangle_instance": [0, 0],
            "instances": ["synthetic"]
        });
        frame["sha256"] = json!(sha256_hex(&serde_json::to_vec(&frame).unwrap()));
        let report = json!({
            "thermal_surface": frame,
            "material": material
        });
        let mapping = json!({
            "schema": "ftx-drape-thermal-coverage/1",
            "drape_thermal_frame_sha256": report["thermal_surface"]["sha256"],
            "body_positions_m": [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [1., 1., 0.]],
            "layout": {
                "schema": "ftx-thermal-coverage-layout/1",
                "mapping_basis": "Synthetic two-triangle unit test allocation",
                "body_vertex_count": 4,
                "body_triangles": [[0, 1, 2], [1, 3, 2]],
                "cells_per_layer": 2,
                "regions": [
                    {
                        "id": "tri-0",
                        "body_triangle": 0,
                        "body_fraction": 0.5,
                        "layers": [{"layer": "shell", "cloth_triangle": 0, "fraction": 0.5}],
                        "interface_resistance_m2_k_w": []
                    },
                    {
                        "id": "tri-1",
                        "body_triangle": 1,
                        "body_fraction": 0.5,
                        "layers": [{"layer": "shell", "cloth_triangle": 1, "fraction": 0.5}],
                        "interface_resistance_m2_k_w": []
                    }
                ]
            }
        });
        (report, mapping)
    }

    #[test]
    fn microclimate_exterior_drives_steady_and_transient_coverage_and_keeps_wind_exclusion_on_mixed_triangles()
     {
        let (report, mapping) = two_triangle_report_and_mapping();
        let forced = json!({
            "air": {"temperature_c": 18.0, "pressure_pa": 101_325.0, "relative_humidity": 0.5},
            "correlation": "churchill_bernstein_cylinder",
            "characteristic_length_m": 0.25,
            "wind_speed_m_s": 2.0,
            "surface_temperature_range_c": [-50.0, 100.0],
            "allow_estimates": true
        });
        let natural = json!({
            "air": {"temperature_c": 18.0, "pressure_pa": 101_325.0, "relative_humidity": 0.5},
            "correlation": "churchill_chu_vertical_plate",
            "characteristic_length_m": 0.4,
            "delta_t_k": 12.0,
            "surface_temperature_range_c": [-50.0, 100.0],
            "allow_estimates": true
        });
        let bare = json!({
            "inside_c": 34.0,
            "outside_c": 18.0,
            "inside_resistance_m2_k_w": 0.08,
            "outside_resistance_m2_k_w": 0.12
        });

        // Uniform forced convection omits "wind-dependent films", keeps "radiative exchange",
        // works with bare_boundaries, and balances transient energy.
        let uniform = json!({
            "schema": "ftx-thermal-coverage-conditions/1",
            "boundaries": [{
                "inside_c": 34.0,
                "outside_c": 18.0,
                "inside_resistance_m2_k_w": 0.08,
                "outside_resistance_m2_k_w": 0.0
            }],
            "microclimate_exterior": [forced],
            "bare_boundaries": [bare],
            "transient": {"initial_temperature_c": 22.0, "step_seconds": 0.5, "steps": 4, "record_every": 2}
        });
        let out = evaluate(&report, &mapping, &uniform).unwrap();
        assert_eq!(out["microclimate_convection"].as_array().unwrap().len(), 1);
        assert_eq!(
            out["microclimate_convection"][0]["correlation"],
            "churchill_bernstein_cylinder"
        );
        assert_eq!(out["microclimate_convection"][0]["evidence"], "estimate");
        assert!(
            out["exterior_model"]
                .as_str()
                .unwrap()
                .contains("Nusselt correlation")
        );
        let limits = out["not_simulated"].as_array().unwrap();
        assert!(!limits.iter().any(|v| v == "wind-dependent films"));
        assert!(limits.iter().any(|v| v == "radiative exchange"));
        assert!(
            limits
                .iter()
                .any(|v| v == "empirical garment-film / wind-tunnel validation")
        );
        assert!(out["transient"]["balance_error_j"].as_f64().unwrap().abs() < 1e-6);
        assert!(
            out["transient"]["exterior_exchange"]["outside_energy_balance_error_j"]
                .as_f64()
                .unwrap()
                .abs()
                < 1e-6
        );

        // Mixed per-triangle forced + natural must retain "wind-dependent films".
        let mut mixed = uniform.clone();
        mixed["microclimate_exterior"] = json!([forced, natural]);
        let mixed_out = evaluate(&report, &mapping, &mixed).unwrap();
        assert_eq!(
            mixed_out["microclimate_convection"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert!(
            mixed_out["not_simulated"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v == "wind-dependent films")
        );

        // Fail-closed refusals: missing estimate consent, mutual exclusion, mismatched count,
        // non-matching outside film, and bare_boundaries without exterior mode.
        let mut denied = uniform.clone();
        denied["microclimate_exterior"][0]["allow_estimates"] = json!(false);
        assert!(evaluate(&report, &mapping, &denied).is_err());

        let mut both = uniform.clone();
        both["exterior_exchange"] = json!([{
            "air_c": 18.0, "convection_w_m2_k": 10.0, "surface_temperature_range_c": [-50.0, 100.0],
            "allow_estimates": true, "evidence": "estimate", "source": "synthetic",
            "method": "synthetic", "conditions": {}
        }]);
        assert!(evaluate(&report, &mapping, &both).is_err());

        let mut wrong_count = uniform.clone();
        wrong_count["microclimate_exterior"] = json!([forced, forced, forced]);
        assert!(evaluate(&report, &mapping, &wrong_count).is_err());

        let mut bad_film = uniform.clone();
        bad_film["boundaries"][0]["outside_resistance_m2_k_w"] = json!(0.1);
        assert!(evaluate(&report, &mapping, &bad_film).is_err());

        let mut bare_only = uniform;
        bare_only
            .as_object_mut()
            .unwrap()
            .remove("microclimate_exterior");
        assert!(evaluate(&report, &mapping, &bare_only).is_err());
    }
}

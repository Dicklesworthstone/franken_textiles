//! Admit prescribed pressure histories through the existing material conditioner. Normal work
//! remains a separate ledger; this checks source and state continuity without running heat again.
use super::*;
use ftx_core::Vec3;
use ftx_pattern::material_compression::{self, stack};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    material_sha256: String,
    preparation: material_compression::Input,
    direction: stack::Direction,
    #[serde(default)]
    pressure_pa: Option<Vec<f64>>,
    #[serde(default)]
    controls: Option<Vec<Control>>,
    #[serde(default)]
    branch_changes: Option<Vec<Option<stack::BranchChange>>>,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Control {
    Gap { gap_m: f64 },
    Pressure { pressure_pa: f64 },
}

fn fail() -> Refusal {
    invalid(
        "prescribed compression heat needs exact original material sources, ordered pressure transitions, unchanged reference storage and separate normal work",
    )
}

fn hashed(value: &Value) -> Result<(), Refusal> {
    let mut input = value.clone();
    input.as_object_mut().ok_or_else(fail)?.remove("sha256");
    if !same_identity(&value["sha256"], &json!(hash(&input)?)) {
        return Err(fail());
    }
    Ok(())
}

fn quantity(material: &ResolvedMaterial, name: &str, unit: &str) -> Result<f64, Refusal> {
    material
        .material()
        .require(name, unit, material.allows_estimates())
}

fn invariant_storage(preparation: &material_compression::Input) -> Result<(), Refusal> {
    if !preparation.allow_estimates
        || preparation.material.thermal_response.is_some()
        || ["areal_density", "specific_heat"].iter().any(|name| {
            !preparation.invariants.contains_key(*name)
                || preparation
                    .samples
                    .iter()
                    .any(|sample| sample.properties.contains_key(*name))
        })
    {
        return Err(fail());
    }
    Ok(())
}

fn resolved(catalog: &Catalog, value: &Value) -> Result<ResolvedMaterial, Refusal> {
    let material = catalog.resolve(&Selection {
        id: value["authored"]["id"].as_str().ok_or_else(fail)?.into(),
        record: Some(serde_json::from_value(value["authored"].clone()).map_err(|_| fail())?),
        allow_estimates: value["allow_estimates"].as_bool().ok_or_else(fail)?,
        expected_sha256: Some(value["sha256"].as_str().ok_or_else(fail)?.into()),
        ..Selection::default()
    })?;
    if serde_json::to_value(&material).map_err(|_| fail())? != *value {
        return Err(fail());
    }
    Ok(material)
}

// Rebuild only the constant-property column affected by the canonical pressure conditioner.
// Other columns retain their complete original receipts, including any temperature dependence.
fn conditioned_column(
    template: &Value,
    material: &ResolvedMaterial,
    cells: usize,
) -> Result<Value, Refusal> {
    if template["schema"] != "ftx-dry-thermal-model/1"
        || template["cells_per_layer"].as_u64() != Some(cells as u64)
        || !vapor_array(template, "interface_resistance_m2_k_w")?.is_empty()
        || material.material().thermal_response.is_some()
    {
        return Err(fail());
    }
    let resistance = material.thermal_resistance()? / cells as f64;
    let capacity = quantity(material, "areal_density", "kg/m2")?
        * quantity(material, "specific_heat", "J/(kg*K)")?
        / cells as f64;
    if !resistance.is_finite() || resistance <= 0. || !capacity.is_finite() || capacity <= 0. {
        return Err(fail());
    }
    let mut column = template.clone();
    column["layers"] = json!([material]);
    column["cell_resistance_m2_k_w"] = json!(vec![resistance; cells]);
    column["cell_capacity_j_m2_k"] = json!(vec![capacity; cells]);
    Ok(column)
}

fn surface_hash(model: &mut Value) -> Result<(), Refusal> {
    model.as_object_mut().ok_or_else(fail)?.remove("sha256");
    model["sha256"] = json!(hash(model)?);
    Ok(())
}

// Compression may change transport, but convection-only exterior exchange is still the entire
// outside boundary. Reconcile that separate receipt with the physical ledger at every sample.
fn admit_convection(
    heat: &Value,
    request: &Value,
    trace: &[Value],
    intervals: usize,
) -> Result<(), Refusal> {
    let Some(environments) = request
        .get("exterior_exchange")
        .filter(|value| !value.is_null())
    else {
        if !heat["exterior_exchange"].is_null()
            || !heat["final"]["exterior_exchange"].is_null()
            || trace.iter().any(|row| !row["exterior_exchange"].is_null())
        {
            return Err(fail());
        }
        return Ok(());
    };
    let environments = environments
        .as_array()
        .filter(|values| values.len() == intervals)
        .ok_or_else(fail)?;
    let boundaries = vapor_array(request, "boundaries")?;
    if boundaries.len() != intervals
        || environments.iter().zip(boundaries).any(|(environment, film)| {
            !environment.is_object()
                || !environment["radiation"].is_null()
                || film["outside_resistance_m2_k_w"] != 0.
                || film["outside_c"] != environment["air_c"]
        })
    {
        return Err(fail());
    }
    for row in trace.iter().chain(std::iter::once(heat)) {
        let exterior = &row["exterior_exchange"];
        if !heat_near(
            finite(&exterior["convective_energy_j"]).ok_or_else(fail)?,
            finite(&row["outside_energy_j"]).ok_or_else(fail)?,
        ) || finite(&exterior["radiative_energy_j"]) != Some(0.)
        {
            return Err(fail());
        }
    }
    let final_field = &heat["final"];
    for row in trace.iter().chain(std::iter::once(final_field)) {
        let exterior = &row["exterior_exchange"];
        if !heat_near(
            finite(&exterior["convective_heat_w"]).ok_or_else(fail)?,
            finite(&row["outside_heat_w"]).ok_or_else(fail)?,
        ) || finite(&exterior["radiative_heat_w"]) != Some(0.)
            || finite(&exterior["balance_residual_w"]).is_none()
        {
            return Err(fail());
        }
    }
    let last = trace.last().ok_or_else(fail)?;
    if heat["exterior_exchange"]["convective_energy_j"]
        != last["exterior_exchange"]["convective_energy_j"]
        || final_field["exterior_exchange"]["convective_heat_w"]
            != last["exterior_exchange"]["convective_heat_w"]
    {
        return Err(fail());
    }
    Ok(())
}

/// Return the admitted initial conditioned materials for instance/profile initialization.
pub(super) fn admit(
    alternative: &Alternative,
    simulation: &Value,
    motion: &Value,
    heat: &Value,
    request: &Value,
    coverage: bool,
) -> Result<Option<Vec<ResolvedMaterial>>, Refusal> {
    let receipt = &heat["prescribed_compression"];
    let trace = vapor_array(heat, "trace")?;
    let Some(policy) = request
        .get("prescribed_compression")
        .filter(|v| !v.is_null())
    else {
        return if receipt.is_null() && trace.iter().all(|r| r["prescribed_compression"].is_null()) {
            Ok(None)
        } else {
            Err(fail())
        };
    };
    let (cyclic, controlled) = match policy["schema"].as_str() {
        Some("ftx-motion-surface-compression/1") => (false, false),
        Some("ftx-motion-surface-compression/2") => (true, false),
        Some("ftx-motion-surface-compression/3") => (false, true),
        _ => return Err(fail()),
    };
    if coverage
        || !policy.as_object().is_some_and(|o| o.len() == 3)
        || !policy["thermal_evidence"]
            .as_str()
            .is_some_and(|s| !s.trim().is_empty() && s.len() <= 4096)
        || receipt["schema"]
            != if controlled {
                "ftx-motion-surface-compression-result/3"
            } else if cyclic {
                "ftx-motion-surface-compression-result/2"
            } else {
                "ftx-motion-surface-compression-result/1"
            }
        || receipt["normal_work_to_heat_j"] != 0.
        || request
            .get("exterior_exchange")
            .filter(|value| !value.is_null())
            .is_some_and(|value| {
                value.as_array().is_none_or(|environments| {
                    environments
                        .iter()
                        .any(|environment| !environment["radiation"].is_null())
                })
            })
        || [
            "plastic_dissipation_heat",
            "relaxation_dissipation_heat",
            "bending_plastic_dissipation_heat",
            "surface_water",
            "surface_water_alternatives",
            "coupled_water_alternatives",
            "mechanical_coupling",
        ]
        .iter()
        .any(|key| !request[key].is_null())
    {
        return Err(fail());
    }
    let bindings: BTreeMap<String, Binding> =
        serde_json::from_value(policy["bindings"].clone()).map_err(|_| fail())?;
    let source = &receipt["source_model"];
    let initial_model = &receipt["initial_model"];
    let report = &simulation["drape"]["material"];
    let instances = vapor_array(source, "instances")?;
    let owners = vapor_array(source, "triangle_instance")?;
    let columns = vapor_array(source, "columns")?;
    let column_instances = vapor_array(source, "column_instances")?;
    let patches = vapor_array(source, "patches")?;
    let triangles = vapor_array(simulation, "triangles")?;
    let positions = vapor_array(simulation, "positions_m")?;
    let samples = vapor_array(motion, "samples")?;
    let endpoints = vapor_array(motion, "keyframes")?;
    admit_convection(heat, request, trace, endpoints.len())?;
    // A present null field must not silently downgrade a requested branch or boundary policy.
    // Legacy schemas retain their original contracts and reject fields belonging only to /3.
    if policy["bindings"]
        .as_object()
        .ok_or_else(fail)?
        .values()
        .any(|binding| {
            let schedule = if controlled { "controls" } else { "pressure_pa" };
            let reserved = if controlled { "pressure_pa" } else { "controls" };
            binding.get(reserved).is_some()
                || !binding[schedule]
                    .as_array()
                    .is_some_and(|values| values.len() == endpoints.len())
                || binding.get("branch_changes").is_some_and(|changes| {
                    (!cyclic && !controlled)
                        || !changes
                            .as_array()
                            .is_some_and(|values| values.len() == endpoints.len())
                })
        })
        || (cyclic
            && !bindings.values().any(|binding| {
                binding
                    .branch_changes
                    .as_ref()
                    .is_some_and(|changes| changes.iter().any(Option::is_some))
            }))
    {
        return Err(fail());
    }
    let cells = vapor_index(&request["cells_per_layer"])?;
    let initial_columns = vapor_array(initial_model, "columns")?;
    let updates = vapor_array(receipt, "updates")?;
    if !(1..=256).contains(&bindings.len())
        || !(1..=16).contains(&endpoints.len())
        || source["schema"] != "ftx-motion-compression-source/1"
        || source["material_report_sha256"] != hash(report)?
        || receipt["material_report_sha256"] != source["material_report_sha256"]
        || source["cells_per_layer"] != request["cells_per_layer"]
        || initial_model["schema"] != "ftx-thermal-surface/1"
        || initial_model["patches"] != source["patches"]
        || initial_model["vertex_count"] != source["vertex_count"]
        || vapor_index(&source["vertex_count"])? != positions.len()
        || positions.is_empty()
        || positions.len() > 196_608
        || instances.is_empty()
        || instances.len() > 196_608
        || patches.is_empty()
        || patches.len() > 65_536
        || owners.len() != patches.len()
        || triangles.len() != patches.len()
        || columns.is_empty()
        || columns.len() > patches.len()
        || columns.len() != column_instances.len()
        || columns.len() != initial_columns.len()
        || !(1..=64).contains(&cells)
        || patches.len().saturating_mul(cells) > 1_000_000
        || updates.len() != endpoints.len()
        || trace.len() != samples.len()
        || motion["triangles"] != simulation["triangles"]
    {
        return Err(fail());
    }
    for value in [source, initial_model, &heat["model"]] {
        hashed(value)?;
    }
    let names: BTreeSet<_> = instances.iter().filter_map(Value::as_str).collect();
    if names.len() != instances.len() || names.iter().any(|s| s.is_empty()) {
        return Err(fail());
    }
    let owner_indices = owners
        .iter()
        .map(vapor_index)
        .collect::<Result<Vec<_>, _>>()?;
    let retained = owner_indices.iter().copied().collect::<BTreeSet<_>>();
    let expected_ids = retained
        .iter()
        .map(|&i| instances.get(i).ok_or_else(fail))
        .collect::<Result<Vec<_>, _>>()?;
    if expected_ids.iter().copied().ne(column_instances.iter())
        || bindings
            .keys()
            .any(|id| !column_instances.iter().any(|v| v.as_str() == Some(id)))
    {
        return Err(fail());
    }
    let catalog = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: vec![],
    };
    let mut owned_patches = vec![Vec::new(); columns.len()];
    for (index, patch) in patches.iter().enumerate() {
        owned_patches
            .get_mut(vapor_index(&patch["column"])?)
            .ok_or_else(fail)?
            .push(index);
    }
    let mut originals = Vec::new();
    let mut materials = Vec::new();
    let mut selections = Vec::new();
    let mut states = serde_json::Map::new();
    let mut active = BTreeMap::new();
    for (column, ((original, initial), id)) in columns
        .iter()
        .zip(initial_columns)
        .zip(column_instances)
        .enumerate()
    {
        let id = id.as_str().ok_or_else(fail)?;
        let material = resolved(&catalog, original)?;
        let retained_record = match report["basis"].as_str() {
            Some("characterized_record") if report["record_sha256"] == material.sha256() => {
                &report["record"]
            }
            Some("per_piece_characterization") => &report["records"][material.sha256()]["record"],
            _ => return Err(fail()),
        };
        if retained_record != original
            || initial["cells_per_layer"] != request["cells_per_layer"]
            || !vapor_array(initial, "interface_resistance_m2_k_w")?.is_empty()
            || vapor_array(initial, "layers")?.len() != 1
        {
            return Err(fail());
        }
        let mut selected = material.clone();
        if let Some(binding) = bindings.get(id) {
            if id.len() > 512
                || binding.material_sha256 != material.sha256()
                || hash(&ftx_pattern::material_catalog::normalize(
                    &binding.preparation.material,
                )?)? != hash(material.material())?
            {
                return Err(fail());
            }
            invariant_storage(&binding.preparation)?;
            selected = catalog
                .resolve(&material_compression::condition(&binding.preparation)?.selection)?;
            if conditioned_column(initial, &selected, cells)? != *initial
                || quantity(&selected, "areal_density", "kg/m2")?
                    != quantity(&material, "areal_density", "kg/m2")?
                || quantity(&selected, "specific_heat", "J/(kg*K)")?
                    != quantity(&material, "specific_heat", "J/(kg*K)")?
            {
                return Err(fail());
            }
            let area = owned_patches[column].iter().try_fold(0., |sum, &index| {
                finite(&patches[index]["reference_area_m2"])
                    .map(|area| sum + area)
                    .ok_or_else(fail)
            })?;
            let mass = area * quantity(&material, "areal_density", "kg/m2")?;
            if !area.is_finite() || area <= 0. || !mass.is_finite() || mass <= 0. {
                return Err(fail());
            }
            states.insert(id.into(), json!({"column":column,"material_sha256":material.sha256(),
                "conditioned_material_sha256":selected.sha256(),"pressure_pa":binding.preparation.pressure_pa,
                "thickness_m":quantity(&selected,"thickness","m")?,"reference_area_m2":area,
                "reference_mass_kg":mass,"cumulative_normal_work_j":0.}));
            active.insert(
                id.to_owned(),
                (binding.preparation.clone(), binding.direction),
            );
        } else if initial["layers"][0] != *original {
            return Err(fail());
        }
        selections.push(json!(material.frozen_selection()));
        originals.push(material);
        materials.push(selected);
    }
    admit_vapor_assignments(alternative, instances, owners, patches, &selections)?;
    let report_instances = if report["basis"] == "per_piece_characterization" {
        let rows = vapor_array(report, "instances")?;
        if rows.len() != instances.len()
            || report["triangle_instance"] != source["triangle_instance"]
            || rows
                .iter()
                .zip(instances)
                .any(|(r, id)| r["instance"] != *id)
        {
            return Err(fail());
        }
        Some(rows)
    } else {
        None
    };
    for ((patch, &owner), triangle) in patches.iter().zip(&owner_indices).zip(triangles) {
        let column = vapor_index(&patch["column"])?;
        let original = originals.get(column).ok_or_else(fail)?;
        if patch["vertices"] != *triangle
            || column_instances.get(column) != instances.get(owner)
            || !finite(&patch["reference_area_m2"]).is_some_and(|a| a > 0.)
            || report_instances.is_some_and(|r| r[owner]["record_sha256"] != original.sha256())
        {
            return Err(fail());
        }
    }
    if receipt["initial_instances"] != Value::Object(states.clone()) {
        return Err(fail());
    }
    let mut model = initial_model.clone();
    let mut total = 0.;
    let mut interval = 0;
    let mut previous_time = 0.;
    for (sample_index, (sample, row)) in samples.iter().zip(trace).enumerate() {
        if sample_index == 0
            || endpoints[interval]["sample_index"]
                .as_u64()
                .is_some_and(|i| i < sample_index as u64)
        {
            if sample_index != 0 {
                interval += 1;
            }
            let update = updates.get(interval).ok_or_else(fail)?;
            let positions: Vec<Vec3> =
                serde_json::from_value(sample["positions_m"].clone()).map_err(|_| fail())?;
            if positions.len() != vapor_index(&source["vertex_count"])?
                || positions.iter().any(|p| !p.is_finite())
                || update["interval_index"].as_u64() != Some(interval as u64)
                || update["sample_index"].as_u64() != Some(sample_index as u64)
                || update["time_s"] != previous_time
                || update["positions_sha256"] != hash(&sample["positions_m"])?
            {
                return Err(fail());
            }
            let mut expected = serde_json::Map::new();
            for (id, binding) in &bindings {
                let state = states.get_mut(id).ok_or_else(fail)?;
                let column = vapor_index(&state["column"])?;
                let previous_pressure = finite(&state["pressure_pa"]).ok_or_else(fail)?;
                let control = if controlled {
                    *binding
                        .controls
                        .as_ref()
                        .and_then(|controls| controls.get(interval))
                        .ok_or_else(fail)?
                } else {
                    Control::Pressure {
                        pressure_pa: *binding
                            .pressure_pa
                            .as_ref()
                            .and_then(|pressures| pressures.get(interval))
                            .ok_or_else(fail)?,
                    }
                };
                let (preparation, direction) = active.get_mut(id).ok_or_else(fail)?;
                let mut current_preparation = preparation.clone();
                current_preparation.pressure_pa = previous_pressure;
                let mut input = stack::Input {
                    schema: stack::SCHEMA.into(),
                    gap_m: finite(&state["thickness_m"]).ok_or_else(fail)?,
                    reference_area_m2: 1.,
                    direction: *direction,
                    layers: vec![stack::Layer {
                        id: id.clone(),
                        preparation: current_preparation,
                    }],
                    allow_estimates: preparation.allow_estimates,
                };
                let branch_change = binding
                    .branch_changes
                    .as_ref()
                    .and_then(|changes| changes[interval].as_ref())
                    .map(|change| {
                        if change.entry.layers.len() != 1 || change.entry.layers[0].id != *id {
                            return Err(fail());
                        }
                        invariant_storage(&change.entry.layers[0].preparation)?;
                        let current = stack::at_pressure(&input, previous_pressure)?;
                        let entry = stack::qualify_branch_change(&input, &current, change, true)?;
                        *preparation = change.entry.layers[0].preparation.clone();
                        *direction = change.entry.direction;
                        input = change.entry.clone();
                        Ok(json!({"change": change, "entry_compression": entry}))
                    })
                    .transpose()?;
                let compressed = match control {
                    Control::Pressure { pressure_pa } => stack::at_pressure(&input, pressure_pa)?,
                    Control::Gap { gap_m } => {
                        input.gap_m = gap_m;
                        stack::close(&input)?
                    }
                };
                let pressure = compressed.pressure_pa;
                let layer = &compressed.layers[0];
                let selected = catalog.resolve(&layer.conditioned.selection)?;
                let mut area = 0.;
                for &patch_index in &owned_patches[column] {
                    let patch = &patches[patch_index];
                    let triangle: [usize; 3] =
                        serde_json::from_value(patch["vertices"].clone()).map_err(|_| fail())?;
                    let [a, b, c] = triangle.map(|i| positions.get(i).copied());
                    let (a, b, c) = (
                        a.ok_or_else(fail)?,
                        b.ok_or_else(fail)?,
                        c.ok_or_else(fail)?,
                    );
                    let triangle_area = 0.5 * (b - a).cross(c - a).norm();
                    if !triangle_area.is_finite() || triangle_area <= 0. {
                        return Err(fail());
                    }
                    area += triangle_area;
                }
                let work = layer.external_work_j_m2 * area;
                total += work;
                let cumulative =
                    finite(&state["cumulative_normal_work_j"]).ok_or_else(fail)? + work;
                if !area.is_finite()
                    || !work.is_finite()
                    || !total.is_finite()
                    || !cumulative.is_finite()
                    || (layer.external_work_j_m2 != 0. && work == 0.)
                {
                    return Err(fail());
                }
                state["pressure_pa"] = json!(pressure);
                state["thickness_m"] = json!(layer.thickness_m);
                state["conditioned_material_sha256"] = json!(selected.sha256());
                state["cumulative_normal_work_j"] = json!(cumulative);
                let mut receipt = state.clone();
                receipt["previous_pressure_pa"] = json!(previous_pressure);
                receipt["work_area_m2"] = json!(area);
                receipt["normal_work_j"] = json!(work);
                receipt["compression_input_sha256"] = json!(compressed.input_sha256);
                if controlled {
                    receipt["control"] = json!(control);
                    receipt["closure_residual_m"] = json!(compressed.closure_residual_m);
                }
                if let Some(change) = branch_change {
                    receipt["branch_change"] = change;
                }
                expected.insert(id.clone(), receipt);
                let next = conditioned_column(&model["columns"][column], &selected, cells)?;
                if next["cell_capacity_j_m2_k"] != model["columns"][column]["cell_capacity_j_m2_k"]
                    || quantity(&selected, "areal_density", "kg/m2")?
                        != quantity(&materials[column], "areal_density", "kg/m2")?
                {
                    return Err(fail());
                }
                model["columns"][column] = next;
            }
            surface_hash(&mut model)?;
            if update["instances"] != Value::Object(expected)
                || update["model_sha256"] != model["sha256"]
            {
                return Err(fail());
            }
        }
        if row["prescribed_compression"]
            != json!({"model_sha256":model["sha256"],
            "instances":states,"cumulative_normal_work_j":total})
        {
            return Err(fail());
        }
        previous_time = finite(&sample["time_s"]).ok_or_else(fail)?;
    }
    if interval + 1 != updates.len()
        || model != heat["model"]
        || receipt["final_model_sha256"] != model["sha256"]
        || receipt["final_instances"] != Value::Object(states)
        || receipt["cumulative_normal_work_j"] != total
    {
        return Err(fail());
    }
    // Independently integrate retained final cells against their actual initial owner profiles.
    // Pressure changes transport only; no normal work is permitted to become thermal storage.
    let profiles = initial_heat_profiles(request, instances, cells)?;
    let final_profiles = vapor_array(&heat["final"], "temperature_c")?;
    if final_profiles.len() != patches.len() {
        return Err(fail());
    }
    let laws = materials
        .iter()
        .map(|m| {
            ftx_pattern::material_thermal::ThermalLaw::from_material(
                m.material(),
                m.allows_estimates(),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut storage = 0.;
    let mut final_range = [f64::INFINITY, f64::NEG_INFINITY];
    for ((patch, &owner), final_cells) in patches.iter().zip(&owner_indices).zip(final_profiles) {
        let column = vapor_index(&patch["column"])?;
        let area = finite(&patch["reference_area_m2"]).ok_or_else(fail)?;
        let initial = profiles
            .get(owner)
            .filter(|p| p.len() == cells)
            .ok_or_else(fail)?;
        let final_cells = final_cells
            .as_array()
            .filter(|v| v.len() == cells)
            .ok_or_else(fail)?;
        for (&from, to) in initial.iter().zip(final_cells) {
            let to = finite(to).ok_or_else(fail)?;
            storage += area * laws[column].sensible_change(from, to)? / cells as f64;
            final_range[0] = final_range[0].min(to);
            final_range[1] = final_range[1].max(to);
        }
    }
    if !heat_near(storage, finite(&heat["stored_change_j"]).ok_or_else(fail)?)
        || trace.last().ok_or_else(fail)?["temperature_range_c"] != json!(final_range)
    {
        return Err(fail());
    }
    Ok(Some(materials))
}

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
        || environments
            .iter()
            .zip(boundaries)
            .any(|(environment, film)| {
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

fn interval_options<'a>(
    request: &'a Value,
    field: &str,
    intervals: usize,
) -> Result<Vec<Option<&'a Value>>, Refusal> {
    match request.get(field).filter(|value| !value.is_null()) {
        None => Ok(vec![None; intervals]),
        Some(value) => value
            .as_array()
            .filter(|values| values.len() == intervals)
            .ok_or_else(fail)?
            .iter()
            .map(|value| {
                if value.is_null() {
                    Ok(None)
                } else if value.is_object() {
                    Ok(Some(value))
                } else {
                    Err(fail())
                }
            })
            .collect(),
    }
}

fn film(value: &Value) -> Result<[f64; 4], Refusal> {
    let values = [
        "inside_c",
        "outside_c",
        "inside_resistance_m2_k_w",
        "outside_resistance_m2_k_w",
    ]
    .map(|field| finite(&value[field]).ok_or_else(fail));
    let [inside, outside, inner, outer] = values;
    let values = [inside?, outside?, inner?, outer?];
    if values[0] < -273.15 || values[1] < -273.15 || values[2] < 0. || values[3] < 0. {
        return Err(fail());
    }
    Ok(values)
}

fn convection_boundary(environment: &Value, boundary: &Value) -> Result<(), Refusal> {
    let source: ftx_pattern::materials::Source =
        serde_json::from_value(environment["convection_source"].clone()).map_err(|_| fail())?;
    let evidence: ftx_pattern::materials::Evidence =
        serde_json::from_value(environment["convection_evidence"].clone()).map_err(|_| fail())?;
    let temperatures = vapor_array(environment, "surface_temperature_range_c")?;
    let air = finite(&environment["air_c"]).ok_or_else(fail)?;
    let h = finite(&environment["convection_w_m2_k"]).ok_or_else(fail)?;
    let boundary = film(boundary)?;
    if !environment["radiation"].is_null()
        || air < -273.15
        || h < 0.
        || boundary[1] != air
        || boundary[3] != 0.
        || temperatures.len() != 2
        || !finite(&temperatures[0]).is_some_and(|lo| {
            lo >= -273.15
                && finite(&temperatures[1]).is_some_and(|hi| hi >= lo && (hi - lo).is_finite())
        })
        || source.reference.trim().is_empty()
        || source.note.trim().is_empty()
        || source.accessed_on.len() != 10
        || !environment["convection_conditions"]
            .as_str()
            .is_some_and(|text| !text.trim().is_empty())
        || (evidence == ftx_pattern::materials::Evidence::Estimate
            && environment["allow_estimates"] != true)
    {
        return Err(fail());
    }
    Ok(())
}

const OUTSIDE_PATHS: [&str; 5] = [
    "covered_film",
    "exposed_film",
    "bare",
    "covered_convection",
    "exposed_convection",
];

fn outside_paths(value: &Value, unit: &str) -> Result<[f64; 5], Refusal> {
    if !value
        .as_object()
        .is_some_and(|fields| fields.len() == OUTSIDE_PATHS.len())
    {
        return Err(fail());
    }
    let mut values = [0.; 5];
    for (index, field) in OUTSIDE_PATHS.iter().enumerate() {
        values[index] = finite(&value[format!("{field}_{unit}")]).ok_or_else(fail)?;
    }
    if !values.iter().sum::<f64>().is_finite() {
        return Err(fail());
    }
    Ok(values)
}

fn convection_power(value: &Value, expected: f64) -> Result<(), Refusal> {
    if !heat_near(
        finite(&value["convective_heat_w"]).ok_or_else(fail)?,
        expected,
    ) || finite(&value["radiative_heat_w"]) != Some(0.)
        || finite(&value["balance_residual_w"]).is_none()
    {
        return Err(fail());
    }
    Ok(())
}

fn convection_energy(value: &Value, expected: f64) -> Result<(), Refusal> {
    if !heat_near(
        finite(&value["convective_energy_j"]).ok_or_else(fail)?,
        expected,
    ) || finite(&value["radiative_energy_j"]) != Some(0.)
    {
        return Err(fail());
    }
    Ok(())
}

// A compressed coverage domain can exchange through five distinct outside paths. Bare body
// and exposed-film heat are not textile convection, and an inactive interval does not erase
// earlier exterior energy. Reconcile the complete retained path rather than a final subtotal.
fn admit_coverage_convection(
    heat: &Value,
    request: &Value,
    trace: &[Value],
    intervals: usize,
) -> Result<(), Refusal> {
    let covered = interval_options(request, "exterior_exchange", intervals)?;
    let exposed = interval_options(request, "exposed_exterior_exchange", intervals)?;
    let bare = interval_options(request, "bare_boundaries", intervals)?;
    let exposed_films = interval_options(request, "exposed_boundaries", intervals)?;
    let boundaries = vapor_array(request, "boundaries")?;
    if boundaries.len() != intervals {
        return Err(fail());
    }
    let any_covered = covered.iter().any(Option::is_some);
    let any_exposed = exposed.iter().any(Option::is_some);
    if !any_covered && !any_exposed {
        if trace.iter().chain(std::iter::once(heat)).any(|row| {
            !row["outside_energy_paths"].is_null()
                || !row["outside_power_paths"].is_null()
                || !row["exterior_exchange"].is_null()
                || !row["exposed_exterior_exchange"].is_null()
        }) || [
            "outside_power_paths",
            "exterior_exchange",
            "exposed_exterior_exchange",
        ]
        .iter()
        .any(|field| !heat["final_field"][field].is_null())
        {
            return Err(fail());
        }
        return Ok(());
    }
    for i in 0..intervals {
        let boundary = film(&boundaries[i])?;
        if covered[i].is_some() != bare[i].is_some() {
            return Err(fail());
        }
        if let Some(environment) = covered[i] {
            convection_boundary(environment, &boundaries[i])?;
            let bare = film(bare[i].ok_or_else(fail)?)?;
            let resistance = bare[2] + bare[3];
            if bare[0] != boundary[0] || !resistance.is_finite() || resistance <= 0. {
                return Err(fail());
            }
        }
        if let Some(boundary) = exposed_films[i] {
            film(boundary)?;
        }
        if let Some(environment) = exposed[i] {
            convection_boundary(environment, exposed_films[i].ok_or_else(fail)?)?;
        }
    }
    let mut previous = [0.; 5];
    let mut previous_time = 0.;
    for row in trace {
        let interval = vapor_index(&row["interval_index"])?;
        if interval >= intervals {
            return Err(fail());
        }
        let energy = outside_paths(&row["outside_energy_paths"], "j")?;
        let power = outside_paths(&row["outside_power_paths"], "w")?;
        let time = finite(&row["time_s"]).ok_or_else(fail)?;
        if time <= previous_time
            || !heat_near(
                energy.iter().sum(),
                finite(&row["outside_energy_j"]).ok_or_else(fail)?,
            )
            || !heat_near(
                power.iter().sum(),
                finite(&row["outside_heat_w"]).ok_or_else(fail)?,
            )
            || !heat_near(energy[2], finite(&row["bare_energy_j"]).ok_or_else(fail)?)
        {
            return Err(fail());
        }
        let active = [
            covered[interval].is_none(),
            exposed[interval].is_none() && exposed_films[interval].is_some(),
            true,
            covered[interval].is_some(),
            exposed[interval].is_some(),
        ];
        for index in 0..5 {
            if (!active[index] && (power[index] != 0. || energy[index] != previous[index]))
                || (row["thermal_substeps"] == 1
                    && !heat_near(
                        energy[index],
                        previous[index] + (time - previous_time) * power[index],
                    ))
            {
                return Err(fail());
            }
        }
        for (field, path) in [("exterior_exchange", 3), ("exposed_exterior_exchange", 4)] {
            if active[path] {
                convection_energy(&row[field], energy[path])?;
                convection_power(&row[field], power[path])?;
            } else if !row[field].is_null() {
                return Err(fail());
            }
        }
        previous = energy;
        previous_time = time;
    }
    let last = trace.last().ok_or_else(fail)?;
    let final_field = &heat["final_field"];
    let energy = outside_paths(&heat["outside_energy_paths"], "j")?;
    let power = outside_paths(&final_field["outside_power_paths"], "w")?;
    if heat["outside_energy_paths"] != last["outside_energy_paths"]
        || final_field["outside_power_paths"] != last["outside_power_paths"]
        || !heat_near(
            energy.iter().sum(),
            finite(&heat["outside_energy_j"]).ok_or_else(fail)?,
        )
        || !heat_near(energy[2], finite(&heat["bare_energy_j"]).ok_or_else(fail)?)
        || !heat_near(
            power[2],
            finite(&final_field["bare_heat_w"]).ok_or_else(fail)?,
        )
        || !heat_near(
            power[0] + power[2] + power[3],
            finite(&final_field["body_outside_heat_w"]).ok_or_else(fail)?,
        )
        || !heat_near(
            power[1] + power[4],
            finite(&final_field["exposed_outside_heat_w"]).ok_or_else(fail)?,
        )
    {
        return Err(fail());
    }
    for (field, path, any, current) in [
        ("exterior_exchange", 3, any_covered, covered[intervals - 1]),
        (
            "exposed_exterior_exchange",
            4,
            any_exposed,
            exposed[intervals - 1],
        ),
    ] {
        if any {
            convection_energy(&heat[field], energy[path])?;
        } else if !heat[field].is_null() || energy[path] != 0. {
            return Err(fail());
        }
        if current.is_some() {
            convection_power(&final_field[field], power[path])?;
            for name in [
                "convective_heat_w",
                "radiative_heat_w",
                "balance_residual_w",
            ] {
                if final_field[field][name] != last[field][name] {
                    return Err(fail());
                }
            }
        } else if !final_field[field].is_null() {
            return Err(fail());
        }
    }
    Ok(())
}

// Projection hashes contain a typed request, whose field order is part of the production
// receipt. These private views reproduce that serialization only; geometry and gap integration
// remain with the coverage producer.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectionRequest {
    schema: String,
    cloth_layers: Vec<String>,
    settings: ProjectionSettings,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectionSettings {
    max_distance_m: f64,
    min_normal_cosine: f64,
    separation_tolerance_m: f64,
    cells_per_layer: usize,
    interfaces: Vec<ProjectionInterface>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectionInterface {
    inner: String,
    outer: String,
    resistance_m2_k_w: f64,
    basis: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stagnant_gap: Option<GapConduction>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GapConduction {
    conductivity_w_m_k: f64,
    source: ftx_pattern::materials::Source,
    conditions: String,
    gap_range_m: [f64; 2],
    relative_tolerance: f64,
    allow_conduction_only_approximation: bool,
}

// A final projection made before conditioning must not accompany the newly conditioned source,
// even when a caller rewrites the outer checkpoint digest. This verifies the existing source,
// geometry, request and derived-layout identities; it does not rerun projection or certify heat.
fn admit_coverage_frame(heat: &Value, model: &Value) -> Result<(), Refusal> {
    let frame = &heat["checkpoint"]["frame"];
    let projection = &frame["projection"];
    let layout = &projection["layout"];
    let request: ProjectionRequest =
        serde_json::from_value(frame["request"].clone()).map_err(|_| fail())?;
    let cloth: Vec<Vec3> =
        serde_json::from_value(frame["cloth_positions_m"].clone()).map_err(|_| fail())?;
    let body: Vec<Vec3> =
        serde_json::from_value(frame["body_positions_m"].clone()).map_err(|_| fail())?;
    let faces: Vec<[usize; 3]> =
        serde_json::from_value(layout["body_triangles"].clone()).map_err(|_| fail())?;
    let projection_hash = hash(&(model, &cloth, &body, &faces, &request))?;
    let mut mapping_basis = format!(
        "Local-normal geometric projection /1; input {projection_hash}; mid-surface separation is not an air-gap resistance"
    );
    if request
        .settings
        .interfaces
        .iter()
        .any(|i| i.stagnant_gap.is_some())
    {
        mapping_basis.push_str("; opted-in interlayer stagnant-gas conduction uses frozen textile face thickness and parallel gap integration; see full projection request and gap diagnostics");
    }
    let coverage_hash = hash(&json!({
        "schema":"ftx-thermal-coverage/1", "source":model, "layout":layout,
        "scope":"declared disjoint fractional coverage; independent series columns; no inferred geometric overlaps",
        "geometry_policy":"layer resistance uses current cloth area; enthalpy uses allocated reference mass; films/interfaces use covered body area",
        "bare_policy":"unallocated body fraction exchanges through declared films; no tissue storage",
        "unmapped_cloth_policy":"excluded and reported",
        "comfort_claim":false, "empirical_validation_claim":false
    }))?;
    let frame_hash = hash(&(
        "ftx-thermal-motion-frame/1",
        coverage_hash,
        &projection_hash,
    ))?;
    if frame["source"] != *model
        || projection["input_sha256"] != projection_hash
        || layout["mapping_basis"] != mapping_basis
        || frame["sha256"] != frame_hash
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
    if !policy.as_object().is_some_and(|o| o.len() == 3)
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
            "thermal_bridges",
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
        || (!coverage && !request["exposed_exterior_exchange"].is_null())
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
    if coverage {
        admit_coverage_convection(heat, request, trace, endpoints.len())?;
    } else {
        admit_convection(heat, request, trace, endpoints.len())?;
    }
    // A present null field must not silently downgrade a requested branch or boundary policy.
    // Legacy schemas retain their original contracts and reject fields belonging only to /3.
    if policy["bindings"]
        .as_object()
        .ok_or_else(fail)?
        .values()
        .any(|binding| {
            let schedule = if controlled {
                "controls"
            } else {
                "pressure_pa"
            };
            let reserved = if controlled {
                "pressure_pa"
            } else {
                "controls"
            };
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
    let requested_cells = if coverage {
        &request["settings"]["cells_per_layer"]
    } else {
        &request["cells_per_layer"]
    };
    let cells = vapor_index(requested_cells)?;
    let initial_columns = vapor_array(initial_model, "columns")?;
    let updates = vapor_array(receipt, "updates")?;
    if !(1..=256).contains(&bindings.len())
        || !(1..=16).contains(&endpoints.len())
        || source["schema"] != "ftx-motion-compression-source/1"
        || source["material_report_sha256"] != hash(report)?
        || receipt["material_report_sha256"] != source["material_report_sha256"]
        || &source["cells_per_layer"] != requested_cells
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
            || &initial["cells_per_layer"] != requested_cells
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
                    || quantity(&selected, "specific_heat", "J/(kg*K)")?
                        != quantity(&materials[column], "specific_heat", "J/(kg*K)")?
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
        || (coverage && model != heat["checkpoint"]["frame"]["source"])
        || receipt["final_model_sha256"] != model["sha256"]
        || receipt["final_instances"] != Value::Object(states)
        || receipt["cumulative_normal_work_j"] != total
    {
        return Err(fail());
    }
    // Independently integrate retained final cells against their actual initial owner profiles.
    // Pressure changes transport only; no normal work is permitted to become thermal storage.
    let profiles = initial_heat_profiles(request, instances, cells)?;
    let laws = materials
        .iter()
        .map(|m| {
            ftx_pattern::material_thermal::ThermalLaw::from_material(
                m.material(),
                m.allows_estimates(),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    if coverage {
        admit_coverage_frame(heat, &model)?;
        let storage = coverage_profile_storage(
            heat,
            motion,
            patches,
            (owners, instances),
            &profiles,
            &laws,
            cells,
        )?;
        if !heat_near(
            storage,
            finite(&heat["physical_stored_change_j"]).ok_or_else(fail)?,
        ) {
            return Err(fail());
        }
        return Ok(Some(materials));
    }
    let final_profiles = vapor_array(&heat["final"], "temperature_c")?;
    if final_profiles.len() != patches.len() {
        return Err(fail());
    }
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

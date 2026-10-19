//! Prepare a source-qualified, fixed-pressure material using the existing frozen-selection boundary.
//!
//! This is a dry, fixed-reference-area conditioning operation, not a contact-pressure solver,
//! loading/unloading history law, moisture model or rule for inferring properties from fiber identity.
//! Unqualified properties are explicitly discarded; no old transport or constitutive law survives
//! merely because it belonged to the starting record.

use std::collections::BTreeMap;

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use serde::{Deserialize, Serialize};

use crate::material_catalog::{Selection, normalize, property_unit};
use crate::materials::{CATALOG_SCHEMA, Catalog, Evidence, Form, Material, Quantity, Source};

/// Portable acquisition and conditioning request.
pub const SCHEMA: &str = "ftx-material-compression/1";
/// Marker for the operation that produced the ordinary material record.
pub const MODEL: &str = "pressure_conditioned_scalar/1";
/// Reserved material-structure key identifying a pressure-conditioned selection.
pub const MODEL_KEY: &str = "compression_model";
/// Reserved material-structure key holding the selected pressure in Pa as a decimal string.
pub const PRESSURE_KEY: &str = "compression_pressure_pa";

/// Observations at one prescribed normal pressure. Pressure is in Pa, never an unqualified force.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sample {
    /// Nonnegative normal pressure; contact area/force conversion must be established by the source.
    pub pressure_pa: f64,
    /// Same scalar keyset at every pressure. Thickness is mandatory; areal mass cannot vary here.
    /// Quantities retain authored units and reference sources in the input material.
    pub properties: BTreeMap<String, Quantity>,
}

/// Explicit source evidence permitting one original scalar observation to survive conditioning.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Invariant {
    /// Must cover the entire pressure table, not only its selected row or interpolation bracket.
    pub pressure_range_pa: [f64; 2],
    /// Source key in the original material, independently qualifying the invariance claim.
    pub source: String,
    /// Evidence for pressure independence; an estimate requires explicit consent.
    pub evidence: Evidence,
    /// How the property was established to remain applicable throughout the declared pressure range.
    pub method: String,
    /// Specimen, conditioning and loading_branch must match the request. Other original property
    /// conditions must also be preserved. This declaration uses pressure_range_pa, never a fixed
    /// pressure_pa condition; any original pressure_pa must lie inside that invariant interval.
    pub conditions: BTreeMap<String, String>,
}

/// Source-complete pressure conditioning; neither the original record nor its catalog is mutated.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    /// Must equal [`SCHEMA`].
    pub schema: String,
    /// Exact starting specimen/construction and all acquisition/invariance sources.
    pub material: Material,
    /// Specimen identity shared by all selected observations.
    pub specimen: String,
    /// Dry state, temperature, humidity and any other fixed acquisition conditions.
    pub conditioning: String,
    /// One explicitly characterized loading or unloading branch; no hysteresis is inferred.
    pub loading_branch: String,
    /// Acquisition, pressure calculation and reduction protocol; pressure is not inferred from force.
    pub method: String,
    /// Acquisition/analysis date copied into the conditioning receipt source.
    pub conditioned_on: String,
    /// Selected normal pressure in Pa, inside the characterized interval.
    pub pressure_pa: f64,
    /// Two to 128 rows in strictly increasing pressure order; loft must be nonincreasing.
    pub samples: Vec<Sample>,
    /// Original scalar keys explicitly retained. Unsampled, undeclared keys are reported as discarded.
    /// Existing response blocks and model selectors cannot be retained through this scalar operation.
    #[serde(default)]
    pub invariants: BTreeMap<String, Invariant>,
    /// Allow estimated observations/invariance and interior piecewise-linear interpolation.
    /// Interpolation is always an estimate, even between measured endpoints.
    #[serde(default)]
    pub allow_estimates: bool,
}

/// Reproducible preparation receipt and the ordinary selection consumed by existing physics modules.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    /// ftx-material-compression-report/1.
    pub schema: String,
    /// Complete source observations, including properties omitted from the conditioned record.
    pub input: Input,
    /// Identity of the complete typed request, not a physical validation certificate.
    pub input_sha256: String,
    /// Source-bearing conditioned material, retaining authored units at exact observed pressures.
    pub material: Material,
    /// Normal frozen selection with its existing normalized content lock.
    pub selection: Selection,
    /// Exact Rust serialization for downloading/replaying without a JavaScript numeric round trip.
    pub selection_json: String,
    /// Full characterized pressure interval, in Pa.
    pub pressure_range_pa: [f64; 2],
    /// Interpolation endpoints, or [p,p] for an exact observation.
    pub bracket_pa: [f64; 2],
    /// True only when scalar values were interpolated between observed pressures.
    pub interpolated: bool,
    /// Original scalar observations omitted because their pressure applicability was not supplied.
    pub discarded_properties: Vec<String>,
    /// Original response blocks and optical face assignments omitted by this scalar operation.
    pub discarded_responses: Vec<String>,
    /// Model selectors omitted so unsupported physical response models cannot remain active.
    pub discarded_structure: Vec<String>,
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

fn text_ok(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 4096
}

fn qualified(
    input: &Input,
    source: &str,
    evidence: Evidence,
    method: &str,
    conditions: &BTreeMap<String, String>,
) -> Result<(), Refusal> {
    if !input.material.sources.contains_key(source)
        || !text_ok(method)
        || conditions.len() > 64
        || conditions.iter().any(|(k, v)| !text_ok(k) || !text_ok(v))
        || (evidence == Evidence::Estimate && !input.allow_estimates)
    {
        return Err(invalid(
            "compression observation needs a resolved source, method, conditions and admitted evidence",
        ));
    }
    for (key, expected) in [
        ("specimen", &input.specimen),
        ("conditioning", &input.conditioning),
        ("loading_branch", &input.loading_branch),
    ] {
        if conditions.get(key) != Some(expected) {
            return Err(invalid(format!(
                "compression observation must match the declared {key}"
            )));
        }
    }
    Ok(())
}

fn scalar_record(material: &Material, properties: BTreeMap<String, Quantity>) -> Material {
    let mut result = material.clone();
    result.properties = properties;
    result.membrane_response = None;
    result.membrane_relaxation = None;
    result.thermal_response = None;
    // Pressure observations do not qualify the original face/finish assignment or its optics.
    result.optical_faces = None;
    result
}

fn positive_property(key: &str, q: &Quantity) -> Result<(), Refusal> {
    if matches!(
        key,
        "thickness"
            | "thermal_resistance"
            | "thermal_conductivity"
            | "specific_heat"
            | "areal_density"
            | "bulk_density"
    ) && (q.value <= 0.0 || q.bounds.is_some_and(|b| b[0] <= 0.0))
    {
        return Err(invalid(format!(
            "compression {key} and any supplied bounds must be positive"
        )));
    }
    Ok(())
}

fn pressure_conditions(q: &Quantity, pressure: f64) -> Result<BTreeMap<String, String>, Refusal> {
    let mut conditions = q.conditions.clone();
    if let Some(declared) = conditions.remove("pressure_pa")
        && declared.parse::<f64>().ok() != Some(pressure)
    {
        return Err(invalid(
            "quantity pressure_pa condition contradicts its pressure-table row",
        ));
    }
    Ok(conditions)
}

fn rows(input: &Input) -> Result<Vec<Material>, Refusal> {
    let first = &input.samples[0];
    if first.properties.len() > 128
        || !first.properties.contains_key("thickness")
        || first.properties.contains_key("areal_density")
        || first
            .properties
            .keys()
            .any(|key| property_unit(key).is_none())
    {
        return Err(invalid(
            "compression rows require supported scalar properties including thickness, without pressure-dependent areal mass",
        ));
    }
    let mut normalized = Vec::with_capacity(input.samples.len());
    for (i, sample) in input.samples.iter().enumerate() {
        if !sample.pressure_pa.is_finite()
            || sample.pressure_pa < 0.0
            || (i > 0 && sample.pressure_pa <= input.samples[i - 1].pressure_pa)
            || !sample.properties.keys().eq(first.properties.keys())
        {
            return Err(invalid(
                "compression rows need strictly increasing nonnegative pressures and identical property keysets",
            ));
        }
        for (key, q) in &sample.properties {
            qualified(input, &q.source, q.evidence, &q.method, &q.conditions)?;
            if pressure_conditions(q, sample.pressure_pa)?
                != pressure_conditions(&first.properties[key], first.pressure_pa)?
            {
                return Err(invalid(format!(
                    "{key}: pressure interpolation cannot mix different acquisition conditions"
                )));
            }
        }
        let material = normalize(&scalar_record(&input.material, sample.properties.clone()))?;
        for (key, q) in &material.properties {
            positive_property(key, q)?;
        }
        if normalized.last().is_some_and(|prior: &Material| {
            material.properties["thickness"].value > prior.properties["thickness"].value
        }) {
            return Err(invalid(
                "compression loft must be nonincreasing with increasing pressure on the declared branch",
            ));
        }
        normalized.push(material);
    }
    Ok(normalized)
}

fn weaker(a: Evidence, b: Evidence) -> Evidence {
    match (a, b) {
        (Evidence::Estimate, _) | (_, Evidence::Estimate) => Evidence::Estimate,
        (Evidence::Published, _) | (_, Evidence::Published) => Evidence::Published,
        _ => Evidence::Measured,
    }
}

fn retained(
    input: &Input,
    domain: [f64; 2],
    varied_loft: bool,
) -> Result<BTreeMap<String, Quantity>, Refusal> {
    let base = normalize(&input.material)?;
    let mut result = BTreeMap::new();
    for (key, declaration) in &input.invariants {
        let q =
            input.material.properties.get(key).ok_or_else(|| {
                invalid(format!("{key}: no original scalar observation to retain"))
            })?;
        if property_unit(key).is_none()
            || input.samples[0].properties.contains_key(key)
            || (key == "bulk_density" && varied_loft)
        {
            return Err(invalid(format!(
                "{key}: unsupported, multiply specified, or incompatible pressure-invariant property"
            )));
        }
        let [lo, hi] = declaration.pressure_range_pa;
        if !lo.is_finite() || !hi.is_finite() || lo < 0.0 || lo > domain[0] || hi < domain[1] {
            return Err(invalid(format!(
                "{key}: pressure invariance must cover the entire characterized interval"
            )));
        }
        if declaration.conditions.contains_key("pressure_pa") {
            return Err(invalid(format!(
                "{key}: invariance uses pressure_range_pa, not a fixed pressure_pa condition"
            )));
        }
        if let Some(original) = q.conditions.get("pressure_pa")
            && !original.parse::<f64>().ok().is_some_and(|pressure| {
                pressure.is_finite() && pressure >= 0.0 && pressure >= lo && pressure <= hi
            })
        {
            return Err(invalid(format!(
                "{key}: original pressure_pa must be finite, nonnegative and covered by the invariant interval"
            )));
        }
        qualified(
            input,
            &declaration.source,
            declaration.evidence,
            &declaration.method,
            &declaration.conditions,
        )?;
        if (q.evidence == Evidence::Estimate && !input.allow_estimates)
            || q.conditions
                .iter()
                .any(|(k, v)| k != "pressure_pa" && declaration.conditions.get(k) != Some(v))
        {
            return Err(invalid(format!(
                "{key}: invariance must preserve original conditions and admit the original evidence"
            )));
        }
        positive_property(key, &base.properties[key])?;
        let mut retained = q.clone();
        retained.evidence = weaker(q.evidence, declaration.evidence);
        retained.conditions = declaration.conditions.clone();
        retained.conditions.extend([
            ("pressure_pa".into(), input.pressure_pa.to_string()),
            (
                "compression_invariance_source".into(),
                declaration.source.clone(),
            ),
            (
                "compression_invariance_method".into(),
                declaration.method.clone(),
            ),
            (
                "compression_invariance_range_pa".into(),
                serde_json::to_string(&declaration.pressure_range_pa)
                    .map_err(|e| invalid(e.to_string()))?,
            ),
        ]);
        // In particular, retain the exact original areal-mass value, units and bounds.
        result.insert(key.clone(), retained);
    }
    Ok(result)
}

fn lerp(a: f64, b: f64, weight: f64) -> f64 {
    (1.0 - weight) * a + weight * b
}

/// Condition an exact specimen at a prescribed pressure and freeze its admitted physical inputs.
/// Unqualified source properties and response models are named in the report, never silently reused.
///
/// # Errors
/// Invalid provenance/units/pressure/loft, mixed conditioning, unsupported invariance, ambiguous
/// transport, extrapolation, denied estimates, nonrepresentable arithmetic or prior conditioning.
pub fn condition(input: &Input) -> Result<Report, Refusal> {
    input.material.validate()?;
    if input.schema != SCHEMA
        || matches!(input.material.form, Form::Fiber | Form::Yarn)
        || !(2..=128).contains(&input.samples.len())
        || [
            &input.specimen,
            &input.conditioning,
            &input.loading_branch,
            &input.method,
            &input.conditioned_on,
        ]
        .iter()
        .any(|value| !text_ok(value))
        || !input.pressure_pa.is_finite()
        || input.pressure_pa < 0.0
        || input
            .material
            .structure
            .keys()
            .any(|key| key.starts_with("compression_"))
    {
        return Err(invalid(
            "compression needs its supported schema, original sheet/fill record, 2..128 rows, fixed conditions and finite pressure; recover the original input before reconditioning",
        ));
    }
    let normalized = rows(input)?;
    let last = input.samples.len() - 1;
    let pressure_range_pa = [
        input.samples[0].pressure_pa,
        input.samples[last].pressure_pa,
    ];
    if input.pressure_pa < pressure_range_pa[0] || input.pressure_pa > pressure_range_pa[1] {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            "requested pressure is outside the characterized compression interval; no extrapolation",
        ));
    }
    let mut properties = retained(
        input,
        pressure_range_pa,
        normalized[0].properties["thickness"].value
            != normalized[last].properties["thickness"].value,
    )?;
    let exact = input
        .samples
        .iter()
        .position(|row| row.pressure_pa == input.pressure_pa);
    let interpolated = exact.is_none();
    if interpolated && !input.allow_estimates {
        return Err(invalid(
            "pressure interpolation is an estimate; explicit allow_estimates is required",
        ));
    }
    let bytes = serde_json::to_vec(input).map_err(|e| invalid(e.to_string()))?;
    let input_sha256 = sha256_hex(&bytes);
    let source_id = format!("compression/{input_sha256}");
    if input.material.sources.contains_key(&source_id) {
        return Err(invalid(
            "compression receipt source ID cannot replace an existing source",
        ));
    }
    let bracket_pa = if let Some(index) = exact {
        properties.extend(input.samples[index].properties.clone());
        [input.pressure_pa; 2]
    } else {
        let hi = input
            .samples
            .partition_point(|row| row.pressure_pa < input.pressure_pa);
        let lo = hi - 1;
        let bracket = [input.samples[lo].pressure_pa, input.samples[hi].pressure_pa];
        let weight = (input.pressure_pa - bracket[0]) / (bracket[1] - bracket[0]);
        if !weight.is_finite() || weight <= 0.0 || weight >= 1.0 {
            return Err(invalid(
                "compression interpolation weight is not representable",
            ));
        }
        for (key, a) in &normalized[lo].properties {
            let b = &normalized[hi].properties[key];
            let mut conditions =
                pressure_conditions(&input.samples[lo].properties[key], bracket[0])?;
            conditions.insert("pressure_pa".into(), input.pressure_pa.to_string());
            properties.insert(key.clone(), Quantity {
                value: lerp(a.value, b.value, weight),
                unit: a.unit.clone(),
                evidence: Evidence::Estimate,
                source: source_id.clone(),
                method: "Piecewise-linear interpolation in prescribed pressure of source observations in canonical units; any bounds are interpolated endpoint envelopes conditional on that same assumed linear law, not new confidence intervals.".into(),
                conditions,
                bounds: a.bounds.zip(b.bounds).map(|(a, b)| std::array::from_fn(|i| lerp(a[i], b[i], weight))),
            });
        }
        bracket
    };
    if properties.contains_key("thermal_resistance")
        && properties.contains_key("thermal_conductivity")
    {
        return Err(invalid(
            "conditioned material requires one thermal transport authority, resistance or conductivity",
        ));
    }
    let discarded_properties = input
        .material
        .properties
        .keys()
        .filter(|key| !properties.contains_key(*key))
        .cloned()
        .collect();
    let discarded_responses = [
        (
            "membrane_response",
            input.material.membrane_response.is_some(),
        ),
        (
            "membrane_relaxation",
            input.material.membrane_relaxation.is_some(),
        ),
        (
            "thermal_response",
            input.material.thermal_response.is_some(),
        ),
        ("optical_faces", input.material.optical_faces.is_some()),
    ]
    .into_iter()
    .filter(|(_, present)| *present)
    .map(|(key, _)| key.into())
    .collect();
    let mut material = scalar_record(&input.material, properties);
    let discarded_structure = material
        .structure
        .keys()
        .filter(|key| key.ends_with("_model"))
        .cloned()
        .collect();
    material.structure.retain(|key, _| !key.ends_with("_model"));
    material.structure.extend([
        (MODEL_KEY.into(), MODEL.into()),
        (PRESSURE_KEY.into(), input.pressure_pa.to_string()),
        ("compression_input_sha256".into(), input_sha256.clone()),
    ]);
    material.sources.insert(source_id, Source {
        reference: format!("local:ftx-material-compression/{input_sha256}"),
        accessed_on: input.conditioned_on.clone(),
        note: format!("Pressure-conditioning receipt for {}: {}. Domain {:?} Pa; selected {} Pa on {} branch. Original observations, invariant qualifications and discarded inputs are retained in the complete report. No contact-pressure, moisture, mass-loss or presser-foot law is inferred.", input.specimen, input.method, pressure_range_pa, input.pressure_pa, input.loading_branch),
    });
    let canonical = normalize(&material)?;
    for (key, q) in &canonical.properties {
        positive_property(key, q)?;
    }
    let selection = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: Vec::new(),
    }
    .resolve(&Selection {
        id: material.id.clone(),
        record: Some(material.clone()),
        allow_estimates: input.allow_estimates,
        ..Selection::default()
    })?
    .frozen_selection();
    let selection_json = serde_json::to_string(&selection).map_err(|e| invalid(e.to_string()))?;
    Ok(Report {
        schema: "ftx-material-compression-report/1".into(),
        input: input.clone(),
        input_sha256,
        material,
        selection,
        selection_json,
        pressure_range_pa,
        bracket_pa,
        interpolated,
        discarded_properties,
        discarded_responses,
        discarded_structure,
    })
}

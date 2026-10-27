//! Source-qualified before/after treatment of a particular sheet, without a second material model.
//!
//! Directional shrinkage defines a homogeneous, freely treated reference geometry. It does not
//! infer washing forces, constrained seam shrinkage, a constitutive history or material conservation.
//! Both material states explicitly qualify the same specimen under their own observed conditioning.
//! The caller supplies an independently qualified AFTER selection: no original mechanical, thermal,
//! optical or purchasing observation is copied across the treatment merely because it existed.

use std::collections::BTreeMap;

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use serde::{Deserialize, Serialize};

use crate::material_catalog::{ResolvedMaterial, Selection, normalize};
use crate::materials::{CATALOG_SCHEMA, Catalog, Evidence, Form, Material, Quantity, Source};

pub mod geometry;

/// Portable observation of one exact treatment and its before/after material states.
pub const SCHEMA: &str = "ftx-material-treatment/1";

/// A source-complete directional dimensional-change observation, not a family shrinkage default.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    /// Must equal [`SCHEMA`].
    pub schema: String,
    /// Embedded, content-locked initial state, without overlays or live catalog lookup.
    pub before: Selection,
    /// Embedded, content-locked treated state. Its own missing properties remain missing.
    pub after: Selection,
    /// Exact fabric variant/lot or specimen shared by dimensional and both material-state observations.
    pub specimen: String,
    /// Exact treatment protocol, including washing/drying/finishing conditions as applicable.
    pub process: String,
    /// Positive number of cycles measured as one complete transition; never exponentiated or scaled.
    pub cycles: u32,
    /// State/protocol in which the initial reference dimensions and properties were measured.
    pub conditioning_before: String,
    /// State/protocol in which the final reference dimensions and properties were measured.
    pub conditioning_after: String,
    /// `(initial - final) / initial` along warp/wale. Unit `1` or `%`; negative means expansion.
    pub shrinkage_warp: Quantity,
    /// `(initial - final) / initial` along weft/course. Unit `1` or `%`; negative means expansion.
    pub shrinkage_weft: Quantity,
    /// Primary protocol/dimensional observations, separate from each material's own source map.
    pub sources: BTreeMap<String, Source>,
    /// Admit estimated dimensional observations only. Each material keeps its own evidence policy.
    #[serde(default)]
    pub allow_estimates: bool,
}

/// Frozen transition and its nominal material-axis map for ordinary pattern/physics consumers.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    /// `ftx-material-treatment-report/1`.
    pub schema: String,
    /// Complete original observations and both independently supplied, locked material states.
    pub input: Input,
    /// Digest of the complete typed request, including protocol, cycle count and evidence policy.
    pub input_sha256: String,
    /// Normal source-complete initial material selection.
    pub before: Selection,
    /// Normal source-complete treated material selection; nothing is inherited from `before`.
    pub after: Selection,
    /// Exact identity of the initial material and its own estimate policy.
    pub before_sha256: String,
    /// Exact identity of the treated material and its own estimate policy.
    pub after_sha256: String,
    /// Exact Rust-serialized selection, suitable for download without numeric reconstruction.
    pub before_selection_json: String,
    /// Exact Rust-serialized selection, suitable for download without numeric reconstruction.
    pub after_selection_json: String,
    /// Warp/wale observation in canonical dimensionless units; original units remain in `input`.
    pub shrinkage_warp: Quantity,
    /// Weft/course observation in canonical dimensionless units; original units remain in `input`.
    pub shrinkage_weft: Quantity,
    /// Positive nominal final/initial lengths, ordered warp/wale then weft/course.
    pub stretch: [f64; 2],
    /// Reciprocal nominal length factors for explicitly compensated cutting geometry.
    pub compensation: [f64; 2],
    /// Product of the two nominal stretches. No mass or areal density is inferred from this ratio.
    pub area_scale: f64,
    /// Reversed source endpoints `[1 - upper_shrinkage, 1 - lower_shrinkage]`, when reported.
    /// These are source intervals, not confidence bounds or uncertainty propagated through a garment.
    pub reported_stretch_bounds: [Option<[f64; 2]>; 2],
    /// Original scalar keys absent from the AFTER record. No old value filled these gaps.
    pub properties_no_longer_available: Vec<String>,
    /// Original optional response blocks absent from the AFTER record.
    pub responses_no_longer_available: Vec<String>,
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

fn text_ok(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 4096
}

fn conditions_ok(conditions: &BTreeMap<String, String>) -> bool {
    conditions.len() <= 64
        && conditions
            .iter()
            .all(|(key, value)| text_ok(key) && text_ok(value))
}

fn frozen(selection: &Selection, state: &str) -> Result<ResolvedMaterial, Refusal> {
    if selection.record.is_none()
        || selection.expected_sha256.is_none()
        || !selection.overrides.is_empty()
        || !selection.sources.is_empty()
    {
        return Err(invalid(format!(
            "treatment {state} must be an embedded digest-locked selection without overlays"
        )));
    }
    Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: Vec::new(),
    }
    .resolve(selection)
}

fn dimension_conditions(input: &Input, quantity: &Quantity) -> Result<(), Refusal> {
    let cycles = input.cycles.to_string();
    if !conditions_ok(&quantity.conditions)
        || (quantity.evidence == Evidence::Estimate && !input.allow_estimates)
    {
        return Err(invalid(
            "treatment dimensions require valid conditions and explicit admission of estimates",
        ));
    }
    for (key, expected) in [
        ("specimen", &input.specimen),
        ("process", &input.process),
        ("cycles", &cycles),
        ("conditioning_before", &input.conditioning_before),
        ("conditioning_after", &input.conditioning_after),
    ] {
        if quantity.conditions.get(key) != Some(expected) {
            return Err(invalid(format!(
                "treatment dimension observation must match the exact {key}"
            )));
        }
    }
    Ok(())
}

fn material_state(
    input: &Input,
    material: &Material,
    state: &str,
    conditioning: &str,
) -> Result<(), Refusal> {
    let check_conditions = |conditions: &BTreeMap<String, String>, label: &str| {
        if !conditions_ok(conditions)
            || conditions.get("specimen") != Some(&input.specimen)
            || conditions.get("conditioning").map(String::as_str) != Some(conditioning)
        {
            return Err(invalid(format!(
                "treatment {state} {label} must explicitly qualify its specimen and conditioning"
            )));
        }
        Ok(())
    };
    let check_identity = |specimen: &str, actual_conditioning: &str| {
        if specimen != input.specimen || actual_conditioning != conditioning {
            return Err(invalid(format!(
                "treatment {state} response must match the exact specimen and conditioning"
            )));
        }
        Ok(())
    };
    for (key, quantity) in &material.properties {
        check_conditions(&quantity.conditions, key)?;
    }
    if let Some(law) = &material.membrane_response {
        for (name, conditions) in [
            ("warp response", &law.warp.conditions),
            ("weft response", &law.weft.conditions),
            ("shear response", &law.shear.conditions),
            ("membrane coupling", &law.coupling.conditions),
        ] {
            check_conditions(conditions, name)?;
        }
        if let Some(area) = &law.area_response {
            check_conditions(&area.conditions, "area response")?;
            check_conditions(&area.quadratic.conditions, "area quadratic coefficient")?;
            check_conditions(&area.log_squared.conditions, "area log coefficient")?;
        }
    }
    if let Some(law) = &material.membrane_relaxation {
        check_conditions(&law.conditions, "relaxation response")?;
        for branch in &law.branches {
            check_conditions(&branch.stiffness_ratio.conditions, "relaxation ratio")?;
            check_conditions(&branch.relaxation_time.conditions, "relaxation time")?;
        }
        for branch in &law.orthotropic_branches {
            for quantity in [
                &branch.stiffness.warp,
                &branch.stiffness.weft,
                &branch.stiffness.coupling,
                &branch.stiffness.shear,
                &branch.relaxation_time,
            ] {
                check_conditions(&quantity.conditions, "directional relaxation")?;
            }
        }
    }
    if let Some(law) = &material.thermal_response {
        check_identity(&law.specimen, &law.conditioning)?;
        // ThermalResponse's own validator binds its scalar companions to this identity. Each
        // retained series still needs explicit applicability to this observed conditioning state.
        for series in [&law.conductivity, &law.specific_heat]
            .into_iter()
            .flatten()
        {
            check_conditions(&series.conditions, "temperature response")?;
        }
    }
    if let Some(faces) = &material.optical_faces {
        check_identity(&faces.specimen, &faces.conditioning)?;
    }
    Ok(())
}

fn post_state(input: &Input, material: &Material) -> Result<(), Refusal> {
    if material.structure.get("treatment_process") != Some(&input.process)
        || material.structure.get("treatment_cycles") != Some(&input.cycles.to_string())
    {
        return Err(invalid(
            "treatment AFTER record must name its exact treatment_process and treatment_cycles",
        ));
    }
    material_state(input, material, "AFTER", &input.conditioning_after)
}

fn stretches(quantity: &Quantity) -> Result<(f64, Option<[f64; 2]>), Refusal> {
    let value = 1.0 - quantity.value;
    let bounds = quantity.bounds.map(|[lo, hi]| [1.0 - hi, 1.0 - lo]);
    if !value.is_finite()
        || value <= 0.0
        || bounds.is_some_and(|b| b.iter().any(|v| !v.is_finite() || *v <= 0.0))
    {
        return Err(invalid(
            "treatment dimensions and every reported bound must leave finite positive lengths",
        ));
    }
    Ok((value, bounds))
}

/// Qualify one observed treatment and freeze its ordinary material states and dimensional map.
///
/// Cycle count is descriptive evidence for the supplied transition; a five-cycle observation is
/// never interpreted as five repetitions of a one-cycle factor. The geometry map uses nominal
/// observations and does not claim a treated sewn garment follows unconstrained coupon shrinkage.
///
/// # Errors
/// Missing/stale material locks, unresolved or mismatched evidence, unsupported sheet levels,
/// unqualified state observations, nonpositive dimensions or nonfinite derived scales. No input
/// or catalog is changed; physics readiness remains the responsibility of each ordinary consumer.
pub fn evaluate(input: &Input) -> Result<Report, Refusal> {
    if input.schema != SCHEMA
        || input.cycles == 0
        || ![
            &input.specimen,
            &input.process,
            &input.conditioning_before,
            &input.conditioning_after,
        ]
        .into_iter()
        .all(|value| text_ok(value))
        || input.sources.is_empty()
        || input.sources.len() > 128
        || input.sources.iter().any(|(id, source)| {
            ![id, &source.reference, &source.accessed_on, &source.note]
                .into_iter()
                .all(|value| text_ok(value))
        })
    {
        return Err(invalid(
            "treatment needs its schema, exact specimen/process/conditioning, positive cycle count and complete sources",
        ));
    }
    let before = frozen(&input.before, "BEFORE")?;
    let after = frozen(&input.after, "AFTER")?;
    if before.sha256() == after.sha256()
        || serde_json::to_value(before.authored()).map_err(|error| invalid(error.to_string()))?
            == serde_json::to_value(after.authored()).map_err(|error| invalid(error.to_string()))?
    {
        return Err(invalid(
            "treatment AFTER must be an explicitly qualified state, not the unchanged BEFORE snapshot",
        ));
    }
    for material in [before.material(), after.material()] {
        if matches!(material.form, Form::Fiber | Form::Yarn | Form::Fill) {
            return Err(invalid(
                "directional sheet treatment cannot infer geometry for a fiber, yarn or loose fill",
            ));
        }
    }
    dimension_conditions(input, &input.shrinkage_warp)?;
    dimension_conditions(input, &input.shrinkage_weft)?;
    material_state(
        input,
        before.material(),
        "BEFORE",
        &input.conditioning_before,
    )?;
    post_state(input, after.material())?;
    // Reuse the material authority's source, bounds and dimensional normalization checks. This
    // observation record is not published as an extra textile or passed into a physics consumer.
    let observations = normalize(&Material {
        id: "treatment/dimensional-observations".into(),
        name: "Directional treatment observations".into(),
        form: Form::Sheet,
        composition: Vec::new(),
        composition_complete: false,
        structure: BTreeMap::new(),
        properties: BTreeMap::from([
            ("shrinkage_warp".into(), input.shrinkage_warp.clone()),
            ("shrinkage_weft".into(), input.shrinkage_weft.clone()),
        ]),
        membrane_response: None,
        membrane_relaxation: None,
        thermal_response: None,
        optical_faces: None,
        sources: input.sources.clone(),
    })?;
    let shrinkage_warp = observations.properties["shrinkage_warp"].clone();
    let shrinkage_weft = observations.properties["shrinkage_weft"].clone();
    let warp = stretches(&shrinkage_warp)?;
    let weft = stretches(&shrinkage_weft)?;
    let stretch = [warp.0, weft.0];
    let compensation = stretch.map(|value| 1.0 / value);
    let area_scale = stretch[0] * stretch[1];
    if !area_scale.is_finite()
        || area_scale <= 0.0
        || !(1.0 / area_scale).is_finite()
        || compensation
            .iter()
            .any(|value| !value.is_finite() || *value <= 0.0)
    {
        return Err(invalid(
            "treatment nominal area and reciprocal compensation must remain finite and positive",
        ));
    }
    let properties_no_longer_available = before
        .material()
        .properties
        .keys()
        .filter(|key| !after.material().properties.contains_key(*key))
        .cloned()
        .collect();
    let responses_no_longer_available = [
        (
            "membrane_response",
            before.material().membrane_response.is_some(),
            after.material().membrane_response.is_some(),
        ),
        (
            "membrane_relaxation",
            before.material().membrane_relaxation.is_some(),
            after.material().membrane_relaxation.is_some(),
        ),
        (
            "thermal_response",
            before.material().thermal_response.is_some(),
            after.material().thermal_response.is_some(),
        ),
        (
            "optical_faces",
            before.material().optical_faces.is_some(),
            after.material().optical_faces.is_some(),
        ),
    ]
    .into_iter()
    .filter(|(_, present, retained)| *present && !retained)
    .map(|(name, _, _)| name.into())
    .collect();
    let before_selection = before.frozen_selection();
    let after_selection = after.frozen_selection();
    Ok(Report {
        schema: "ftx-material-treatment-report/1".into(),
        input: input.clone(),
        input_sha256: sha256_hex(
            &serde_json::to_vec(input).map_err(|error| invalid(error.to_string()))?,
        ),
        before_selection_json: serde_json::to_string(&before_selection)
            .map_err(|error| invalid(error.to_string()))?,
        after_selection_json: serde_json::to_string(&after_selection)
            .map_err(|error| invalid(error.to_string()))?,
        before: before_selection,
        after: after_selection,
        before_sha256: before.sha256().into(),
        after_sha256: after.sha256().into(),
        shrinkage_warp,
        shrinkage_weft,
        stretch,
        compensation,
        area_scale,
        reported_stretch_bounds: [warp.1, weft.1],
        properties_no_longer_available,
        responses_no_longer_available,
    })
}

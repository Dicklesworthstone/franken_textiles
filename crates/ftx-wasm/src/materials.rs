//! Local material discovery and substitution over the canonical catalog and production consumers.

use ftx_core::{Refusal, RefusalCode};
use ftx_drape::{material, run::DrapeOptions};
use ftx_pattern::{
    Pattern,
    material_binding::Changes,
    material_catalog::{ResolvedMaterial, Selection},
    textile_import,
};
use serde_json::{Value, json};

pub mod outfit;
pub mod scenario;

const MAX_BYTES: usize = 64 * 1024 * 1024;

fn invalid(e: impl std::fmt::Display) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, e.to_string())
}

fn budget(inputs: &[&str]) -> Result<(), Refusal> {
    let n = inputs
        .iter()
        .try_fold(0usize, |n, s| n.checked_add(s.len()));
    if n.is_none_or(|n| n > MAX_BYTES) {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "combined material input exceeds 64 MiB",
        ));
    }
    Ok(())
}

fn readiness<T: serde::Serialize>(result: Result<T, Refusal>) -> Value {
    match result {
        Ok(value) => json!({"ready":true,"value":value}),
        Err(refusal) => json!({"ready":false,"refusal":refusal}),
    }
}

fn detail(m: &ResolvedMaterial) -> Result<Value, Refusal> {
    let material = m.material();
    let policy = m.allows_estimates();
    let mechanical = (|| {
        let mechanics = m.mechanics()?;
        m.membrane_relaxation()?;
        m.membrane_plasticity()?;
        m.bending_response()?;
        m.bending_plasticity()?;
        Ok(mechanics)
    })();
    let steady = ftx_pattern::material_thermal::ConductionLaw::from_material(material, policy)
        .map(|law| law.domain());
    let transient = ftx_pattern::material_thermal::ThermalLaw::from_material(material, policy)
        .map(|law| law.domain());
    let sewing = if material.structure.contains_key("compression_model") {
        Err(invalid(
            "Pressure-conditioned thickness does not establish uncompressed sewing-stack clearance.",
        ))
    } else {
        material.require("thickness", "m", policy)
    };
    let selection = m.frozen_selection();
    let selection_json = serde_json::to_string(&selection).map_err(invalid)?;
    Ok(
        json!({"snapshot":m,"selection":selection,"selection_json":selection_json,"missing_mechanics":material.missing_mechanics(),
        "capabilities":{"mechanical_coefficients":readiness(mechanical),
            "steady_conduction":readiness(steady),"transient_thermal":readiness(transient),
            "stock_width_m":readiness(material.require("width","m",policy)),
            "sewing_thickness_m":readiness(sewing)},
        "note":"Coefficient availability is not successful draping, process qualification, empirical fit or a safety rating."}),
    )
}

/// Discover the existing built-in catalog; original observations remain accessible through inspect.
///
/// # Errors
/// Oversized query or invalid built-in records. No network request or implicit property fill occurs.
pub fn catalog(query: &str) -> Result<String, Refusal> {
    budget(&[query])?;
    let needle = query.to_lowercase();
    let db = textile_import::catalog()?;
    let rows: Vec<_> = db
        .materials
        .iter()
        .filter(|m| {
            [&m.id, &m.name]
                .into_iter()
                .chain(m.structure.values())
                .any(|s| s.to_lowercase().contains(&needle))
                || m.composition
                    .iter()
                    .any(|c| c.fiber.to_lowercase().contains(&needle))
        })
        .map(|m| {
            json!({"id":m.id,"name":m.name,"form":m.form,
        "property_count":m.properties.len()})
        })
        .collect();
    serde_json::to_string(&json!({"schema":"ftx-material-discovery/1","materials":rows}))
        .map_err(invalid)
}

/// Inspect an exact ID or embedded source-bearing selection without elevating evidence consent.
///
/// # Errors
/// Oversized/invalid JSON, unknown identity, source/unit conflicts or a failed material lock.
pub fn inspect(selection: &str) -> Result<String, Refusal> {
    budget(&[selection])?;
    let s: Selection = serde_json::from_str(selection).map_err(invalid)?;
    let m = textile_import::catalog()?.resolve(&s)?;
    serde_json::to_string(&detail(&m)?).map_err(invalid)
}

/// Prepare the existing frozen material selection at an explicitly characterized normal pressure.
/// The input owns its specimen, pressure observations, invariance evidence and estimate consent.
///
/// # Errors
/// Input over 64 MiB, unsupported observations, missing qualification or pressure extrapolation.
pub fn pressure_condition(request: &str) -> Result<String, Refusal> {
    budget(&[request])?;
    let input: ftx_pattern::material_compression::Input =
        serde_json::from_str(request).map_err(invalid)?;
    serde_json::to_string(&ftx_pattern::material_compression::condition(&input)?).map_err(invalid)
}

/// Browser pressure conditioning through the same material preparation operation as both CLIs.
///
/// # Errors
/// Same refusals as [`pressure_condition`].
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn material_pressure_condition(request: &str) -> Result<String, wasm_bindgen::JsError> {
    pressure_condition(request).map_err(|e| wasm_bindgen::JsError::new(&e.to_string()))
}

/// Infer common normal pressure from qualified layer thicknesses and rigid-face stack clearance.
/// Each layer returns its ordinary frozen selection, without guessing missing mass or transport.
///
/// # Errors
/// Input over 64 MiB or any refusal from the shared inverse compression/conditioning operation.
pub fn compression_stack(request: &str) -> Result<String, Refusal> {
    budget(&[request])?;
    let input: ftx_pattern::material_compression::stack::Input =
        serde_json::from_str(request).map_err(invalid)?;
    serde_json::to_string(&ftx_pattern::material_compression::stack::close(&input)?)
        .map_err(invalid)
}

/// Browser stack closure through the identical operation used by both native material commands.
///
/// # Errors
/// Same refusals as [`compression_stack`].
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn material_compression_stack(request: &str) -> Result<String, wasm_bindgen::JsError> {
    compression_stack(request).map_err(|e| wasm_bindgen::JsError::new(&e.to_string()))
}

/// Inspect complete retained strip acquisitions without converting grip extension to local strain.
/// Original source text reaches the shared parser without a numerical JSON round trip.
///
/// # Errors
/// Input over the adapter's 64 MiB budget or parser's 2 MiB source budget, invalid identities,
/// unsupported acquisition schema or altered source data.
pub fn strip_acquisition(request: &str) -> Result<String, Refusal> {
    budget(&[request])?;
    serde_json::to_string(&ftx_pattern::material_catalog::knit_strips::parse_acquisition(request)?)
        .map_err(invalid)
}

/// Browser inspection of original source curves through the same parser as both native commands.
///
/// # Errors
/// Same refusals as [`strip_acquisition`].
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn material_strip_acquisition(request: &str) -> Result<String, wasm_bindgen::JsError> {
    strip_acquisition(request).map_err(|e| wasm_bindgen::JsError::new(&e.to_string()))
}

/// Identify the production nonlinear material law from source-bearing nominal swatch forces.
/// Observation consent in the input and admission of the fitted estimates are independent.
///
/// # Errors
/// Oversized/invalid input, unidentifiable or unstable response, inadmissible evidence, or failed
/// fit/held-out residual budgets. Missing unrelated mechanics remain reported, never invented.
pub fn calibrate_nonlinear(input: &str, allow_estimates: bool) -> Result<String, Refusal> {
    budget(&[input])?;
    let input = serde_json::from_str(input).map_err(invalid)?;
    let report = ftx_pattern::material_calibration::nonlinear::calibrate(&input)?;
    let snapshot = textile_import::catalog()?.resolve(&Selection {
        id: report.material.id.clone(),
        record: Some(report.material.clone()),
        allow_estimates,
        ..Selection::default()
    })?;
    let selection = snapshot.frozen_selection();
    let selection_json = serde_json::to_string(&selection).map_err(invalid)?;
    serde_json::to_string(
        &json!({"calibration":report,"selection":selection,"selection_json":selection_json,
        "material":detail(&snapshot)?,"fit_claim":false}),
    )
    .map_err(invalid)
}

/// Browser nonlinear swatch calibration using the native identification and material authority.
///
/// # Errors
/// Same refusals as [`calibrate_nonlinear`].
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn material_calibrate_nonlinear(
    input: &str,
    allow_estimates: bool,
) -> Result<String, wasm_bindgen::JsError> {
    calibrate_nonlinear(input, allow_estimates)
        .map_err(|e| wasm_bindgen::JsError::new(&e.to_string()))
}

/// Identify the existing orthotropic membrane using the actual finite clamped-strip solver.
/// Unknown, ambiguous or rejected fits retain their report without publishing a material selection.
///
/// # Errors
/// Oversized or invalid input, unsupported evidence, or a shared acquisition-admission refusal.
pub fn calibrate_strip(input: &str, allow_estimates: bool) -> Result<String, Refusal> {
    budget(&[input])?;
    let mut input: ftx_drape::strip::fit::Input = serde_json::from_str(input).map_err(invalid)?;
    input.allow_estimates = allow_estimates;
    let report = ftx_drape::strip::fit::calibrate(&input)?;
    if !report.accepted {
        return serde_json::to_string(&json!({"calibration":report,"selection":null,
            "selection_json":null,"material":null,"simulation_ready":false,
            "fit_claim":false,"empirical_validation_claim":false}))
        .map_err(invalid);
    }
    let selection = report
        .selection
        .as_ref()
        .ok_or_else(|| invalid("accepted strip fit lacks its frozen selection"))?;
    let snapshot = textile_import::catalog()?.resolve(selection)?;
    let readiness = (|| {
        snapshot.mechanics()?;
        snapshot.membrane_relaxation()?;
        snapshot.membrane_plasticity()?;
        snapshot.bending_response()?;
        snapshot.bending_plasticity()?;
        Ok::<(), Refusal>(())
    })();
    serde_json::to_string(&json!({"calibration":report,"selection":selection,
        "selection_json":report.selection_json,"material":detail(&snapshot)?,
        "simulation_ready":readiness.is_ok(),"mechanics_refusal":readiness.err(),
        "fit_claim":false,"empirical_validation_claim":false}))
    .map_err(invalid)
}

/// Browser clamped-strip identification through the same physical inverse solver as native code.
///
/// # Errors
/// Same refusals as [`calibrate_strip`].
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn material_calibrate_strip(
    input: &str,
    allow_estimates: bool,
) -> Result<String, wasm_bindgen::JsError> {
    calibrate_strip(input, allow_estimates).map_err(|e| wasm_bindgen::JsError::new(&e.to_string()))
}

/// Evaluate independent finite-strip equilibria with a source-locked existing membrane law.
/// Unrelated bending, contact, density and transport observations are not needed or inferred.
///
/// # Errors
/// Input budget, source-policy, constitutive-domain, geometry or equilibrium refusals.
pub fn strip_experiment(input: &str) -> Result<String, Refusal> {
    budget(&[input])?;
    let input = serde_json::from_str(input).map_err(invalid)?;
    serde_json::to_string(&ftx_drape::strip::evaluate(&input)?).map_err(invalid)
}

/// Browser forward strip experiment using the shared physical material evaluator.
///
/// # Errors
/// Same refusals as [`strip_experiment`].
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn material_strip_experiment(input: &str) -> Result<String, wasm_bindgen::JsError> {
    strip_experiment(input).map_err(|e| wasm_bindgen::JsError::new(&e.to_string()))
}

/// Run the production membrane through a prescribed grip history with material memory and inertia.
/// The frozen input and complete accepted history are returned only after every step succeeds.
///
/// # Errors
/// Input budget, source/domain, loading-program, time/iteration budget or solver refusals.
pub fn strip_history(input: &str) -> Result<String, Refusal> {
    budget(&[input])?;
    let input = serde_json::from_str(input).map_err(invalid)?;
    serde_json::to_string(&ftx_drape::strip::history::evaluate(&input)?).map_err(invalid)
}

/// Browser timed-strip experiment using the same production material history as native code.
///
/// # Errors
/// Same refusals as [`strip_history`].
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn material_strip_history(input: &str) -> Result<String, wasm_bindgen::JsError> {
    strip_history(input).map_err(|e| wasm_bindgen::JsError::new(&e.to_string()))
}

/// Identify a source-qualified recovery spectrum using the same timed histories as native code.
/// Observation consent and admission of fitted estimates remain independent policies.
///
/// # Errors
/// Oversized/invalid input, unidentifiable histories, inadmissible evidence, or failed residual
/// budgets. An identified spectrum does not supply missing mass, bending, friction or transport.
pub fn calibrate_relaxation(input: &str, allow_estimates: bool) -> Result<String, Refusal> {
    budget(&[input])?;
    use ftx_pattern::material_relaxation::calibration::{self, comparison};
    let document: Value = serde_json::from_str(input).map_err(invalid)?;
    let comparison = if document["schema"] == comparison::SCHEMA {
        let input = serde_json::from_str(input).map_err(invalid)?;
        Some(comparison::compare(&input)?)
    } else {
        None
    };
    let report = if let Some(comparison) = &comparison {
        let Some(report) = comparison.selected_calibration() else {
            return serde_json::to_string(&json!({"comparison":comparison,"calibration":null,
                "selection":null,"selection_json":null,"material":null,
                "fit_claim":false,"empirical_validation_claim":false}))
            .map_err(invalid);
        };
        report.clone()
    } else {
        let input = serde_json::from_str(input).map_err(invalid)?;
        calibration::calibrate(&input)?
    };
    let snapshot = textile_import::catalog()?.resolve(&Selection {
        id: report.material.id.clone(),
        record: Some(report.material.clone()),
        allow_estimates,
        ..Selection::default()
    })?;
    let selection = snapshot.frozen_selection();
    let selection_json = serde_json::to_string(&selection).map_err(invalid)?;
    let mut output = json!({"calibration":report,"selection":selection,"selection_json":selection_json,
        "material":detail(&snapshot)?,"fit_claim":false,"empirical_validation_claim":false,
        "time_integration":"Calibration analytically integrates piecewise-linear Green strain; the drape solver uses backward Euler and requires time-step refinement."});
    if let Some(comparison) = comparison {
        output["comparison"] = json!(comparison);
    }
    serde_json::to_string(&output).map_err(invalid)
}

/// Browser recovery identification through the same material authority and timed-history fitter.
///
/// # Errors
/// Same refusals as [`calibrate_relaxation`].
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn material_calibrate_relaxation(
    input: &str,
    allow_estimates: bool,
) -> Result<String, wasm_bindgen::JsError> {
    calibrate_relaxation(input, allow_estimates)
        .map_err(|e| wasm_bindgen::JsError::new(&e.to_string()))
}

/// Identify permanent stretch and hardening from complete source-qualified cyclic histories.
/// Nonaccepted fits retain their diagnostics but cannot supply an assignable material selection.
///
/// # Errors
/// Oversized or invalid input, unsupported initial state, incompatible specimen conditions or
/// inadmissible source evidence. Missing unrelated properties remain explicit capability gaps.
pub fn calibrate_membrane_plasticity(
    input: &str,
    allow_estimates: bool,
) -> Result<String, Refusal> {
    budget(&[input])?;
    let input = serde_json::from_str(input).map_err(invalid)?;
    let report = ftx_pattern::material_membrane_plasticity::calibration::calibrate(&input)?;
    let Some(material) = &report.material else {
        return serde_json::to_string(&json!({"calibration":report,"selection":null,
            "selection_json":null,"material":null,"simulation_ready":false,
            "fit_claim":false,"empirical_validation_claim":false}))
        .map_err(invalid);
    };
    let snapshot = textile_import::catalog()?.resolve(&Selection {
        id: material.id.clone(),
        record: Some(material.clone()),
        allow_estimates,
        ..Selection::default()
    })?;
    let selection = snapshot.frozen_selection();
    let selection_json = serde_json::to_string(&selection).map_err(invalid)?;
    serde_json::to_string(&json!({"calibration":report,"selection":selection,
        "selection_json":selection_json,"material":detail(&snapshot)?,
        "fit_claim":false,"empirical_validation_claim":false}))
    .map_err(invalid)
}

/// Browser permanent-stretch identification through the shared production material law.
///
/// # Errors
/// Same refusals as [`calibrate_membrane_plasticity`].
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn material_calibrate_membrane_plasticity(
    input: &str,
    allow_estimates: bool,
) -> Result<String, wasm_bindgen::JsError> {
    calibrate_membrane_plasticity(input, allow_estimates)
        .map_err(|e| wasm_bindgen::JsError::new(&e.to_string()))
}

/// Screen source-locked alternatives under explicit constraints, evidence policy and objectives.
///
/// # Errors
/// Input over 64 MiB or an invalid request. Per-candidate missing data remains visible in the report.
pub fn screen(request: &str) -> Result<String, Refusal> {
    budget(&[request])?;
    let request: ftx_pattern::material_screen::Request =
        serde_json::from_str(request).map_err(invalid)?;
    serde_json::to_string(&request.evaluate(&textile_import::catalog()?)?).map_err(invalid)
}

/// Browser constraint screening over the same canonical material authority as native code.
///
/// # Errors
/// Same refusals as [`screen`].
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn material_screen(request: &str) -> Result<String, wasm_bindgen::JsError> {
    screen(request).map_err(|e| wasm_bindgen::JsError::new(&e.to_string()))
}

/// Compare unchanged-pattern material substitutions using the actual manufacturing consumers.
///
/// # Errors
/// Combined input over 64 MiB, invalid source/request or aggregate work budget exceeded.
pub fn study(pattern: &str, request: &str) -> Result<String, Refusal> {
    budget(&[pattern, request])?;
    let pattern = Pattern::from_json(pattern).map_err(invalid)?;
    let request: ftx_export::material_study::Request =
        serde_json::from_str(request).map_err(invalid)?;
    serde_json::to_string(&request.evaluate(&pattern)?).map_err(invalid)
}

/// Browser material study; no network, file write or hardware execution.
///
/// # Errors
/// Same refusals as [`study`].
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn material_study(pattern: &str, request: &str) -> Result<String, wasm_bindgen::JsError> {
    study(pattern, request).map_err(|e| wasm_bindgen::JsError::new(&e.to_string()))
}

/// Freeze exact default/per-piece edits and report the existing drape resolver's capability refusals.
/// No body geometry, measured fit or successful solver run is implied by a resolved material set.
///
/// # Errors
/// Combined input over 64 MiB, invalid pattern/edits or any source-selection refusal.
pub fn bind(pattern: &str, changes: &str) -> Result<String, Refusal> {
    budget(&[pattern, changes])?;
    let p = Pattern::from_json(pattern).map_err(invalid)?;
    let c: Changes = serde_json::from_str(changes).map_err(invalid)?;
    let mut result = c.apply(&p)?;
    let bound = Pattern::new(result["pattern"].clone());
    let mut opts = DrapeOptions {
        self_contact: None,
        ..DrapeOptions::default()
    };
    result["drape_materials_without_self_contact"] =
        readiness(material::resolve(&bound, &opts).map(|m| m.report));
    opts.self_contact = Some(opts.contact);
    result["drape_materials_with_self_contact"] =
        readiness(material::resolve(&bound, &opts).map(|m| m.report));
    serde_json::to_string(&result).map_err(invalid)
}

/// Browser catalog discovery; all observations stay local.
///
/// # Errors
/// Same refusals as [`catalog`].
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn material_catalog(query: &str) -> Result<String, wasm_bindgen::JsError> {
    catalog(query).map_err(|e| wasm_bindgen::JsError::new(&e.to_string()))
}

/// Browser inspection of a frozen or catalog selection.
///
/// # Errors
/// Same refusals as [`inspect`].
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn material_inspect(selection: &str) -> Result<String, wasm_bindgen::JsError> {
    inspect(selection).map_err(|e| wasm_bindgen::JsError::new(&e.to_string()))
}

/// Browser source-piece material binding, sharing canonical selection and drape resolution.
///
/// # Errors
/// Same refusals as [`bind`].
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn material_bind(pattern: &str, changes: &str) -> Result<String, wasm_bindgen::JsError> {
    bind(pattern, changes).map_err(|e| wasm_bindgen::JsError::new(&e.to_string()))
}

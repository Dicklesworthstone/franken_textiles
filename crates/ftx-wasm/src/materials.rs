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

fn detail(m: &ResolvedMaterial) -> Value {
    let material = m.material();
    let policy = m.allows_estimates();
    let mechanical = (|| {
        let mechanics = m.mechanics()?;
        m.membrane_relaxation()?;
        m.bending_response()?;
        m.bending_plasticity()?;
        Ok(mechanics)
    })();
    let steady = ftx_pattern::material_thermal::ConductionLaw::from_material(material, policy)
        .map(|law| law.domain());
    let transient = ftx_pattern::material_thermal::ThermalLaw::from_material(material, policy)
        .map(|law| law.domain());
    json!({"snapshot":m,"selection":m.frozen_selection(),"missing_mechanics":material.missing_mechanics(),
        "capabilities":{"mechanical_coefficients":readiness(mechanical),
            "steady_conduction":readiness(steady),"transient_thermal":readiness(transient),
            "stock_width_m":readiness(material.require("width","m",policy)),
            "sewing_thickness_m":readiness(material.require("thickness","m",policy))},
        "note":"Coefficient availability is not successful draping, process qualification, empirical fit or a safety rating."})
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
    serde_json::to_string(&detail(&m)).map_err(invalid)
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

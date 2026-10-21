//! Finished-band substitutions share material authority but never substitute casing cloth.
//! This resolves assignment identities only; the drape consumer still checks casing geometry,
//! load domains and numerical options. No casing is synthesized by binding a material.

use std::collections::BTreeMap;

use ftx_core::Refusal;
use serde_json::{Value, json};

use crate::{
    Pattern, material_catalog::Selection, material_elastic::ElasticBand, materials::Catalog, truthy,
};

use super::invalid;

// None locates the single legacy selector; Some(i) locates an existing explicit declaration.
fn targets(pattern: &Pattern) -> Result<BTreeMap<String, Option<usize>>, Refusal> {
    let solved = &pattern.raw()["meta"]["solved"];
    let mut targets = BTreeMap::new();
    if let Some(raw) = solved.get("elastic_casings") {
        if solved.get("elastic_cut_mm").is_some() || solved.get("elastic_material").is_some() {
            return Err(invalid(
                "explicit casing bindings cannot coexist with legacy selectors",
            ));
        }
        let entries = raw
            .as_array()
            .ok_or_else(|| invalid("elastic_casings must be a list"))?;
        if entries.is_empty() || entries.len() > 64 {
            return Err(invalid("elastic binding requires 1..64 existing casings"));
        }
        for (i, entry) in entries.iter().enumerate() {
            let id = entry["instance"]
                .as_str()
                .ok_or_else(|| invalid("casing needs a physical instance ID"))?;
            if targets.insert(id.to_owned(), Some(i)).is_some() {
                return Err(invalid("ambiguous repeated casing instance"));
            }
        }
    } else {
        let cut = solved["elastic_cut_mm"].as_f64().ok_or_else(|| {
            invalid("band binding needs an existing casing, not a new inferred waistband")
        })?;
        let overlap = pattern.raw()["meta"]["style"]["elastic_overlap"]
            .as_f64()
            .unwrap_or(0.0);
        if !cut.is_finite() || !(cut - overlap).is_finite() || cut <= overlap {
            return Err(invalid(
                "legacy elastic relaxed length must be finite and positive",
            ));
        }
        let pieces = pattern.raw()["pieces"]
            .as_array()
            .expect("validated source pieces");
        let bands: Vec<_> = pieces.iter().filter(|p| p["role"] == "waistband").collect();
        if bands.len() != 1 {
            return Err(invalid(
                "legacy elastic binding needs exactly one waistband piece",
            ));
        }
        let instances: Vec<_> = pattern
            .instances
            .iter()
            .filter(|i| bands[0]["id"] == i.piece.as_str())
            .collect();
        if instances.len() != 1 {
            return Err(invalid(
                "legacy elastic binding needs exactly one physical waistband",
            ));
        }
        targets.insert(instances[0].id.clone(), None);
    }
    for id in targets.keys() {
        let instance = pattern
            .instance(id)
            .ok_or_else(|| invalid(format!("unknown casing instance {id}")))?;
        let piece = &pattern.raw()["pieces"][instance.piece_index];
        if piece["sim"] == false || !truthy(&piece["placement"]) {
            return Err(invalid(format!(
                "casing {id} must be active and explicitly placed"
            )));
        }
    }
    Ok(targets)
}

pub(super) fn apply(
    pattern: &Pattern,
    raw: &mut Value,
    changes: &BTreeMap<String, Selection>,
    catalog: &Catalog,
) -> Result<BTreeMap<String, Selection>, Refusal> {
    let targets = targets(pattern)?;
    for id in changes.keys() {
        if !targets.contains_key(id) {
            return Err(invalid(format!(
                "unknown physical casing target {id}; source-piece IDs and cloth roles are not band selectors"
            )));
        }
    }
    let mut frozen = BTreeMap::new();
    for (id, index) in &targets {
        let source = match index {
            Some(i) => &pattern.raw()["meta"]["solved"]["elastic_casings"][*i],
            None => &pattern.raw()["meta"]["solved"],
        };
        let authored;
        let selection = if let Some(replacement) = changes.get(id) {
            replacement
        } else if let Some(value) = source.get("elastic_material") {
            authored = serde_json::from_value(value.clone())
                .map_err(|e| invalid(format!("{id}: invalid elastic_material: {e}")))?;
            &authored
        } else {
            // Preserve a caller-EA fallback, not an invented band record or coefficient.
            continue;
        };
        let band = ElasticBand::resolve(catalog, selection)?;
        frozen.insert(id.clone(), band.frozen_selection());
    }
    // Publish the prepared edits only after every replacement and retained source selection resolves.
    for (id, selection) in &frozen {
        let destination = match targets[id] {
            Some(i) => &mut raw["meta"]["solved"]["elastic_casings"][i],
            None => &mut raw["meta"]["solved"],
        };
        destination["elastic_material"] = json!(selection);
    }
    Ok(frozen)
}

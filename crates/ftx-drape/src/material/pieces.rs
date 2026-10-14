//! Source-piece assignment for the existing sheet-element solver. No remeshing or material mixing.
//! This first contact scope requires equal sheet thickness and skin friction. Unlike-cloth friction
//! is not inferred from the two self-friction observations; unsupported self-contact refuses.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::Refusal;
use ftx_core::receipt::sha256_hex;
use ftx_pattern::Pattern;
use serde_json::json;

use super::{MaterialInputs, invalid, resolve_uniform};

pub(super) fn resolve(
    pattern: &Pattern,
    mut base: MaterialInputs,
) -> Result<MaterialInputs, Refusal> {
    let raw = pattern.raw();
    if raw["fabric"].get("layers").is_some() || raw["fabric"].get("interfaces").is_some() {
        return Err(invalid(
            "layer/interface assignments are not supported by this contact scope",
        ));
    }
    let Some(pieces) = raw["pieces"].as_array() else {
        return Ok(base);
    };
    for p in pieces {
        if p.get("material").is_some_and(|m| !m.is_string()) || p.get("material_id").is_some() {
            return Err(invalid(
                "piece.material is a cutting label; put characterized selections in piece.fabric.material",
            ));
        }
    }
    if !pieces.iter().any(|p| p.get("fabric").is_some()) {
        return Ok(base);
    }
    if pieces.len() > 4096 || base.report["basis"] != "characterized_record" {
        return Err(invalid(
            "per-piece mechanics need a characterized pattern.fabric default and at most 4096 pieces",
        ));
    }
    let mut records = BTreeMap::new();
    let default_hash = base.report["record_sha256"]
        .as_str()
        .ok_or_else(|| invalid("default material has no source identity"))?
        .to_owned();
    records.insert(default_hash.clone(), base.report.clone());
    let mut hashes = BTreeSet::new();
    let mut bindings = Vec::with_capacity(pieces.len());
    let mut fabrics = Vec::with_capacity(pieces.len());
    for (piece_index, p) in pieces.iter().enumerate() {
        let (fabric, hash) = if let Some(f) = p.get("fabric") {
            let object = f
                .as_object()
                .ok_or_else(|| invalid("piece.fabric must be an object"))?;
            if object.is_empty() || object.keys().any(|k| k != "material" && k != "material_id") {
                return Err(invalid(
                    "piece.fabric must contain only a complete material OR material_id selection; partial coefficients/layers are not inherited",
                ));
            }
            let resolved = resolve_uniform(&Pattern::new(json!({"fabric":f})), &base.options)?;
            if resolved.options.contact.thickness != base.options.contact.thickness
                || resolved.options.contact.mu != base.options.contact.mu
            {
                return Err(invalid(format!(
                    "piece {piece_index}: different thickness/skin friction needs per-feature contact; no global average is used",
                )));
            }
            if resolved.options.self_contact != base.options.self_contact {
                return Err(invalid(format!(
                    "piece {piece_index}: different self-friction needs pairwise contact",
                )));
            }
            let hash = resolved.report["record_sha256"]
                .as_str()
                .ok_or_else(|| invalid("piece material has no source identity"))?
                .to_owned();
            records.insert(hash.clone(), resolved.report);
            (resolved.fabric, hash)
        } else {
            (base.fabric, default_hash.clone())
        };
        // Include unplaced pieces in the receipt and validation. No named material is silently
        // ignored merely because this particular drape did not simulate its source piece.
        hashes.insert(hash.clone());
        bindings.push(json!({"piece_index":piece_index, "piece_id":p["id"],
            "record_sha256":hash, "inherited_default":p.get("fabric").is_none()}));
        fabrics.push(fabric);
    }
    if base.options.self_contact.is_some() && hashes.len() > 1 {
        return Err(invalid(
            "unlike sheets require explicit pairwise friction evidence before self-contact; equal self-friction values do not establish their mutual friction",
        ));
    }
    let mut report = json!({
        "basis":"per_piece_characterization",
        "model":"orthotropic_green_strain_scalar_bending",
        "default_record_sha256":default_hash,
        "pieces":bindings, "records":records,
        "contact_policy":"equal physical thickness and skin friction; unlike self-contact is refused",
        "body_offset_m":base.options.contact.thickness,
        "self_contact_enabled":base.options.self_contact.is_some(),
        "hinge_policy":"shared curvature with each adjacent triangle's bending-area share; not an empirical seam law",
        "not_simulated":["layer generation", "unlike interface friction", "different sheet thicknesses", "directional bending", "seam bulk", "nonlinear textile response"],
        "fit_claim":false
    });
    report["assignment_sha256"] = json!(sha256_hex(
        &serde_json::to_vec(&report).map_err(|e| invalid(e.to_string()))?
    ));
    base.report = report;
    base.piece_fabrics = Some(fabrics);
    Ok(base)
}

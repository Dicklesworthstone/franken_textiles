//! Source-piece assignment for the existing sheet-element solver. No remeshing or material mixing.
//! Unlike-cloth friction is never inferred from two same-cloth observations. Source-bound pair data
//! feeds the existing feature-local contact kernel, including the conservative welded-seam envelope.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::Refusal;
use ftx_core::receipt::sha256_hex;
use ftx_pattern::Pattern;
use ftx_pattern::textile_import::piece_fabric;
use serde_json::json;

use super::{ContactMaterials, MaterialInputs, interfaces, invalid, resolve_uniform};
use crate::contact::SurfaceMaterial;

pub(super) fn resolve(
    pattern: &Pattern,
    mut base: MaterialInputs,
) -> Result<MaterialInputs, Refusal> {
    let raw = pattern.raw();
    if raw["fabric"].get("layers").is_some() {
        return Err(invalid(
            "layer generation is not supported; use explicit pattern pieces",
        ));
    }
    let interface_data = raw["fabric"].get("interfaces");
    let Some(pieces) = raw["pieces"].as_array() else {
        if interface_data.is_some() {
            return Err(invalid("material interfaces require source pieces"));
        }
        return Ok(base);
    };
    for i in 0..pieces.len() {
        piece_fabric(pattern, i)?;
    }
    if !pieces.iter().any(|p| p.get("fabric").is_some()) && interface_data.is_none() {
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
    let mut mechanics = BTreeMap::new();
    let mut piece_hashes = Vec::with_capacity(pieces.len());
    let mut bindings = Vec::with_capacity(pieces.len());
    let mut fabrics = Vec::with_capacity(pieces.len());
    let mut responses = Vec::with_capacity(pieces.len());
    for (piece_index, p) in pieces.iter().enumerate() {
        let mut explicit_role: Option<String> = None;
        let (fabric, response, hash, mechanical) = if p.get("fabric").is_some() {
            let f = piece_fabric(pattern, piece_index)?;
            explicit_role = f
                .get("role")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
            let resolved = resolve_uniform(&Pattern::new(json!({"fabric":f})), &base.options)?;
            if explicit_role.as_deref() == Some("laminate")
                && resolved.report["record"]["authored"]["form"] != "laminate"
            {
                return Err(invalid(
                    "piece.fabric.role \"laminate\" requires a material with form \"laminate\"",
                ));
            }
            let hash = resolved.report["record_sha256"]
                .as_str()
                .ok_or_else(|| invalid("piece material has no source identity"))?
                .to_owned();
            records.insert(hash.clone(), resolved.report);
            (resolved.fabric, resolved.response, hash, resolved.mechanics)
        } else {
            (
                base.fabric,
                base.response.clone(),
                default_hash.clone(),
                base.mechanics,
            )
        };
        // Include unplaced pieces in the receipt and validation. No named material is silently
        // ignored merely because this particular drape did not simulate its source piece.
        hashes.insert(hash.clone());
        if hashes.len() > 64 {
            return Err(invalid(
                "contact supports at most 64 distinct source snapshots",
            ));
        }
        mechanics.insert(
            hash.clone(),
            mechanical.ok_or_else(|| invalid("missing sheet mechanics"))?,
        );
        piece_hashes.push(hash.clone());
        let mut binding = json!({"piece_index":piece_index, "piece_id":p["id"],
            "record_sha256":hash, "inherited_default":p.get("fabric").is_none()});
        if let Some(role) = explicit_role {
            binding["role"] = json!(role);
        }
        bindings.push(binding);
        fabrics.push(fabric);
        responses.push(response);
    }
    let hashes: Vec<_> = hashes.into_iter().collect();
    let diagonal: Vec<_> = hashes.iter().map(|h| mechanics[h].friction_self).collect();
    let interfaces = interfaces::resolve(
        interface_data,
        &hashes,
        &diagonal,
        base.options.self_contact.is_some(),
    )?;
    let surfaces: Vec<_> = hashes
        .iter()
        .map(|h| SurfaceMaterial {
            half_thickness_m: 0.5 * mechanics[h].thickness_m,
            friction_body: mechanics[h].friction_skin,
        })
        .collect();
    let piece_ids = piece_hashes
        .iter()
        .map(|h| hashes.binary_search(h).expect("assigned hash is retained"))
        .collect();
    base.options.contact.thickness = surfaces
        .iter()
        .map(|s| s.half_thickness_m)
        .fold(0.0, f64::max);
    base.options.contact.mu = surfaces.iter().map(|s| s.friction_body).fold(0.0, f64::max);
    if let Some(p) = &mut base.options.self_contact {
        p.thickness = 2.0 * base.options.contact.thickness;
        p.mu = interfaces
            .matrix
            .as_ref()
            .expect("complete enabled interfaces")
            .iter()
            .flatten()
            .copied()
            .fold(0.0, f64::max);
    }
    base.options.validate()?;
    let mut report = json!({
        "basis":"per_piece_characterization",
        "model":"per_piece_green_strain_scalar_bending",
        "default_record_sha256":default_hash,
        "pieces":bindings, "records":records,
        "contact_policy":"feature-local thickness/body friction and source-bound pairwise friction; welded features use maximum-adjacent envelopes",
        "body_offset_envelope_m":base.options.contact.thickness,
        "contact_material_order":hashes,
        "self_friction_matrix":interfaces.matrix,
        "interfaces":interfaces.report,
        "self_contact_enabled":base.options.self_contact.is_some(),
        "hinge_policy":"shared curvature with each adjacent triangle's bending-area share; not an empirical seam law",
        "not_simulated":["layer generation", "directional bending", "resolved seam cross-section", "irreversible hysteresis/damage"],
        "fit_claim":false
    });
    report["assignment_sha256"] = json!(sha256_hex(
        &serde_json::to_vec(&report).map_err(|e| invalid(e.to_string()))?
    ));
    base.report = report;
    base.piece_fabrics = Some(fabrics);
    base.piece_responses = responses.iter().any(Option::is_some).then_some(responses);
    // One identity keeps the original homogeneous start/ramp and numerical path exactly.
    base.contact_materials = (hashes.len() > 1).then_some(ContactMaterials {
        piece_ids,
        surfaces,
        friction: interfaces.matrix,
    });
    Ok(base)
}

//! Source-piece assignment for the existing sheet-element solver. No remeshing or material mixing.
//! Unlike-cloth friction is never inferred from two same-cloth observations. Source-bound pair data
//! feeds the existing feature-local contact kernel, including the conservative welded-seam envelope.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::Refusal;
use ftx_core::receipt::sha256_hex;
use ftx_pattern::Pattern;
use ftx_pattern::textile_import::{piece_fabric, resolve_fabric};
use serde_json::{Value, json};

use super::{
    ContactMaterials, Fabric, MaterialInputs, check_grains, grain_direction, interfaces, invalid,
    resolve_uniform,
};
use crate::contact::SurfaceMaterial;
use crate::run::DrapeOptions;

pub(super) fn resolve(
    pattern: &Pattern,
    options: &DrapeOptions,
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
        return resolve_uniform(pattern, options);
    };
    for i in 0..pieces.len() {
        piece_fabric(pattern, i)?;
    }
    if !pieces.iter().any(|p| p.get("fabric").is_some()) && interface_data.is_none() {
        return resolve_uniform(pattern, options);
    }
    if pieces.len() > 4096 {
        return Err(invalid("per-piece mechanics support at most 4096 pieces"));
    }
    // Resolve every declared source, but compile mechanics only for the explicit simulation scope.
    // A source-only default remains useful for cutting/BOM even when every active piece overrides it.
    let default_record = resolve_fabric(&raw["fabric"])?;
    if default_record.is_none() {
        Fabric::of(pattern)?; // supplied legacy numbers must still be structurally/physically valid
    }
    let default_hash = default_record
        .as_ref()
        .map(|record| record.sha256().to_owned());
    let mut records = BTreeMap::new();
    if let Some(record) = &default_record {
        records.insert(record.sha256().to_owned(), source_receipt(record));
    }
    let uses_default = pieces
        .iter()
        .any(|piece| piece["sim"] != false && piece.get("fabric").is_none());
    let legacy_active = default_record.is_none()
        && pieces
            .iter()
            .all(|piece| piece["sim"] == false || piece.get("fabric").is_none());
    let mut base = if uses_default {
        if default_record.is_none() && !legacy_active {
            return Err(invalid(
                "active pieces without explicit selections need a source-bearing pattern.fabric default",
            ));
        }
        Some(resolve_uniform(
            &Pattern::new(json!({"fabric":raw["fabric"]})),
            options,
        )?)
    } else {
        None
    };
    let mut hashes = BTreeSet::new();
    let mut declared_hashes = BTreeSet::new();
    let mut mechanics = BTreeMap::new();
    let mut piece_hashes = Vec::with_capacity(pieces.len());
    let mut bindings = Vec::with_capacity(pieces.len());
    let mut fabrics = Vec::with_capacity(pieces.len());
    let mut responses = Vec::with_capacity(pieces.len());
    let mut relaxations = Vec::with_capacity(pieces.len());
    let mut membrane_plasticities = Vec::with_capacity(pieces.len());
    let mut plasticities = Vec::with_capacity(pieces.len());
    for (piece_index, p) in pieces.iter().enumerate() {
        let f = piece_fabric(pattern, piece_index)?;
        let explicit = p.get("fabric").is_some();
        let record = if explicit {
            Some(
                resolve_fabric(f)?
                    .ok_or_else(|| invalid("piece material has no source identity"))?,
            )
        } else {
            default_record.clone()
        };
        let explicit_role = explicit
            .then(|| f.get("role").and_then(Value::as_str))
            .flatten();
        if explicit_role == Some("laminate")
            && record.as_ref().is_none_or(|record| {
                record.material().form != ftx_pattern::materials::Form::Laminate
            })
        {
            return Err(invalid(
                "piece.fabric.role \"laminate\" requires a material with form \"laminate\"",
            ));
        }
        let hash = record.as_ref().map(|record| record.sha256().to_owned());
        if let Some(record) = &record {
            declared_hashes.insert(record.sha256().to_owned());
            if declared_hashes.len() > 64 {
                return Err(invalid(
                    "contact supports at most 64 distinct source snapshots",
                ));
            }
            records
                .entry(record.sha256().to_owned())
                .or_insert_with(|| source_receipt(record));
        }
        let mut binding = json!({"piece_index":piece_index, "piece_id":p["id"],
            "record_sha256":hash, "inherited_default":!explicit});
        if let Some(role) = explicit_role {
            binding["role"] = json!(role);
        }
        if p["sim"] == false {
            binding["mechanics_consumed"] = json!(false);
            binding["exclusion_reason"] = json!("sim_false");
            bindings.push(binding);
            piece_hashes.push(None);
            fabrics.push(None);
            responses.push(None);
            relaxations.push(None);
            membrane_plasticities.push(None);
            plasticities.push(None);
            continue;
        }
        if legacy_active {
            continue;
        }
        // Absence of a placement is not an exclusion. The ordinary scope gate must still refuse
        // intended pieces with unresolved placements rather than letting them disappear here.
        let hash = hash.ok_or_else(|| invalid("active piece material has no source identity"))?;
        let owned = if explicit {
            Some(resolve_uniform(
                &Pattern::new(json!({"fabric":f})),
                options,
            )?)
        } else {
            None
        };
        let resolved = owned
            .as_ref()
            .or(base.as_ref())
            .ok_or_else(|| invalid("missing active sheet mechanics"))?;
        if p.get("placement").is_some_and(ftx_pattern::truthy) {
            grain_direction(p, resolved.fabric.requires_grain())?;
        }
        hashes.insert(hash.clone());
        mechanics.insert(
            hash.clone(),
            resolved
                .mechanics
                .ok_or_else(|| invalid("missing sheet mechanics"))?,
        );
        records.insert(hash.clone(), resolved.report.clone());
        piece_hashes.push(Some(hash));
        bindings.push(binding);
        fabrics.push(Some(resolved.fabric));
        responses.push(resolved.response.clone());
        relaxations.push(resolved.relaxation.clone());
        membrane_plasticities.push(resolved.membrane_plasticity.clone());
        plasticities.push(resolved.plasticity.clone());
        if base.is_none() {
            base = owned;
        }
    }
    let mut base = base.ok_or_else(|| invalid("nothing to drape: no active source pieces"))?;
    let hashes: Vec<_> = hashes.into_iter().collect();
    let declared_hashes: Vec<_> = declared_hashes.into_iter().collect();
    if legacy_active {
        check_grains(pattern, &base.fabric)?;
        let interfaces = interfaces::resolve(interface_data, &[], &[], &declared_hashes, false)?;
        base.report["excluded_piece_materials"] =
            json!({"pieces":bindings,"records":records,"interfaces":interfaces.report});
        return Ok(base);
    }
    let diagonal: Vec<_> = hashes.iter().map(|h| mechanics[h].friction_self).collect();
    let interfaces = interfaces::resolve(
        interface_data,
        &hashes,
        &diagonal,
        &declared_hashes,
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
        .map(|h| {
            h.as_ref()
                .map(|h| hashes.binary_search(h).expect("active hash is retained"))
        })
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
        "model":if fabrics.iter().flatten().any(|f| f.bending_response().is_some()) {"per_piece_green_strain_tensor_panel_bending"} else {"per_piece_green_strain_scalar_bending"},
        "default_record_sha256":default_hash,
        "pieces":bindings, "records":records,
        "contact_policy":"feature-local thickness/body friction and source-bound pairwise friction; welded features use maximum-adjacent envelopes",
        "body_offset_envelope_m":base.options.contact.thickness,
        "contact_material_order":hashes,
        "self_friction_matrix":interfaces.matrix,
        "interfaces":interfaces.report,
        "self_contact_enabled":base.options.self_contact.is_some(),
        "hinge_policy":"shared curvature with each adjacent triangle's bending-area share; not an empirical seam law",
        "panel_bending_piece_count":fabrics.iter().flatten().filter(|f| f.bending_response().is_some()).count(),
        "panel_bending_policy":"selected panels use source-topology quadratic tensor patches; scalar seam approximation is retained independently",
        "not_simulated":["layer generation", "conforming shell discretization", "resolved seam cross-section", "plastic set/damage"],
        "fit_claim":false
    });
    if default_hash.is_none() {
        report["unused_default_fabric"] = raw["fabric"].clone();
    }
    if plasticities.iter().any(Option::is_some) {
        report["plastic_bending_piece_count"] =
            json!(plasticities.iter().filter(|p| p.is_some()).count());
        report["not_simulated"] = json!([
            "layer generation",
            "conforming shell discretization",
            "resolved seam cross-section",
            "membrane plasticity/damage",
            "seam plasticity",
            "rate/temperature-dependent creasing"
        ]);
    }
    if membrane_plasticities.iter().any(Option::is_some) {
        report["membrane_plasticity_piece_count"] =
            json!(membrane_plasticities.iter().filter(|p| p.is_some()).count());
        let excluded = report["not_simulated"]
            .as_array_mut()
            .expect("authored list");
        excluded.retain(|s| s != "plastic set/damage" && s != "membrane plasticity/damage");
        excluded.extend([
            json!("membrane damage/rupture"),
            json!("plastic-work thermal feedback"),
        ]);
        if !plasticities.iter().any(Option::is_some) {
            excluded.push(json!("bending plasticity"));
        }
    }
    report["assignment_sha256"] = json!(sha256_hex(
        &serde_json::to_vec(&report).map_err(|e| invalid(e.to_string()))?
    ));
    base.report = report;
    base.piece_fabrics = Some(fabrics);
    base.piece_responses = responses.iter().any(Option::is_some).then_some(responses);
    // Keep explicit None entries: an equilibrium-only cuff must not inherit a transient shell law.
    base.piece_relaxations = Some(relaxations);
    // Explicitly elastic/Maxwell pieces never inherit another piece's permanent membrane state.
    base.piece_membrane_plasticities = Some(membrane_plasticities);
    // An explicitly elastic lining/cuff must not inherit its shell's irreversible curvature law.
    base.piece_plasticities = Some(plasticities);
    // One identity keeps the original homogeneous start/ramp and numerical path exactly.
    base.contact_materials = (hashes.len() > 1).then_some(ContactMaterials {
        piece_ids,
        surfaces,
        friction: interfaces.matrix,
    });
    Ok(base)
}

fn source_receipt(record: &ftx_pattern::material_catalog::ResolvedMaterial) -> Value {
    json!({"basis":"source_record", "record_sha256":record.sha256(),
        "record":record, "mechanics_consumed":false})
}

//! Joint draping of independently authored garments through the ordinary cloth/contact solver.
//! Namespacing prevents accidental inter-garment welds; placement and pair friction are never guessed.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use ftx_pattern::{
    Pattern, material_friction::Interface, material_mass::Attachments, textile_import,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::run::{DrapeBody, DrapeOptions, DrapeRun};

/// Source-pattern composition, not completed frozen garment meshes.
pub const SCHEMA: &str = "ftx-outfit-drape/1";

/// A garment with its own piece/seam namespace. Placements share the one supplied body frame.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Garment {
    /// Unique ASCII letter/digit/underscore/hyphen label, at most 64 bytes.
    pub id: String,
    /// Original fabric-pattern/1 document, including source-qualified material selections.
    pub pattern: Value,
}

/// An explicit, bounded joint simulation request. Garment order does not imply layer ordering.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    /// Must equal [`SCHEMA`].
    pub schema: String,
    /// Two to sixteen independently authored garments; original pieces are not edited in place.
    pub garments: Vec<Garment>,
    /// Additional exact-snapshot contact pairs. Internal garment pairs are retained automatically.
    #[serde(default)]
    pub interfaces: Vec<Value>,
}

/// A frozen composed pattern and its source correspondence. No numerical state exists until drape.
pub struct Prepared {
    pattern: Pattern,
    receipt: Value,
    instance_garment: BTreeMap<String, usize>,
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

fn digest(value: &impl Serialize) -> Result<String, Refusal> {
    let bytes = serde_json::to_vec(value).map_err(|e| invalid(e.to_string()))?;
    if bytes.len() > 64 * 1024 * 1024 {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "outfit document exceeds 64 MiB",
        ));
    }
    Ok(sha256_hex(&bytes))
}

fn check_options(options: &DrapeOptions) -> Result<(), Refusal> {
    options.validate()?;
    if options.self_contact.is_none() {
        return Err(invalid(
            "joint outfit draping requires explicit cloth-cloth self-contact",
        ));
    }
    Ok(())
}

fn collect_interfaces(
    entries: &[Value],
    pairs: &mut BTreeMap<[String; 2], Value>,
) -> Result<(), Refusal> {
    if entries.len() > 2016 {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "too many outfit contact pairs",
        ));
    }
    let mut seen = BTreeSet::new();
    for value in entries {
        let mut pair = Interface::parse(value)?;
        pair.validate()?;
        pair.materials.sort();
        if !seen.insert(pair.materials.clone()) {
            return Err(invalid("duplicate contact pair within one outfit source"));
        }
        let canonical = pair.report();
        if let Some(previous) = pairs.get(&pair.materials) {
            if *previous != canonical {
                return Err(invalid(
                    "conflicting outfit contact evidence for the same material snapshots",
                ));
            }
        } else {
            pairs.insert(pair.materials, canonical);
        }
        if pairs.len() > 2016 {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "too many distinct outfit contact pairs",
            ));
        }
    }
    Ok(())
}

// Validate against the original physical instances before introducing a namespace. Only identities
// change: preserve authored mass, fractions, source records, bounds and per-group estimate policy.
fn collect_attached_mass(
    pattern: &Pattern,
    garment: &str,
    groups: &mut Vec<Value>,
) -> Result<Option<Value>, Refusal> {
    let Some(attachments) = Attachments::from_pattern(pattern)? else {
        return Ok(None);
    };
    if attachments.groups.is_empty() {
        return Ok(None);
    }
    let original = &pattern.raw()["attached_mass"];
    let name = |id: &str| format!("{garment}::{id}");
    let mut correspondence = BTreeMap::new();
    for (i, group) in attachments.groups.iter().enumerate() {
        // An unknown/denied mass must refuse before body fitting, not become a weightless accessory.
        group.extra.evaluate()?;
        let mut raw = original["groups"][i].clone();
        raw["extra"]["id"] = json!(name(&group.extra.id));
        for (j, target) in group.targets.iter().enumerate() {
            let instance = pattern
                .instance(&target.instance)
                .expect("validated target");
            if pattern.raw()["pieces"][instance.piece_index]["sim"] == Value::Bool(false) {
                return Err(invalid(format!(
                    "{garment}: attached mass target {} is excluded from simulation",
                    target.instance
                )));
            }
            raw["targets"][j]["instance"] = json!(name(&target.instance));
        }
        correspondence.insert(group.extra.id.clone(), name(&group.extra.id));
        groups.push(raw);
    }
    Ok(Some(
        json!({"source_input_sha256":digest(original)?,"groups":correspondence}),
    ))
}

/// Freeze material selections, namespace source references, and preflight the actual contact resolver.
/// No garment is moved, scaled, assigned invented friction, or sewn to another garment.
///
/// # Errors
/// Invalid/oversized inputs, unresolvable casings or root extensions, missing placement/material
/// evidence, unknown seam references, conflicting contact sources, or missing unlike-sheet friction.
pub fn prepare(input: &Input, options: &DrapeOptions) -> Result<Prepared, Refusal> {
    check_options(options)?;
    if input.schema != SCHEMA || !(2..=16).contains(&input.garments.len()) {
        return Err(invalid(
            "joint outfit needs schema ftx-outfit-drape/1 and 2..16 garments",
        ));
    }
    let input_sha256 = digest(input)?;
    let mut ids = BTreeSet::new();
    let mut pieces = Vec::new();
    let mut seams = Vec::new();
    let mut sources = Vec::new();
    let mut interfaces = BTreeMap::new();
    let mut attached_groups = Vec::new();
    let mut elastic_casings = Vec::new();
    let mut instance_garment = BTreeMap::new();
    let mut default = None;
    for (index, garment) in input.garments.iter().enumerate() {
        if garment.id.is_empty()
            || garment.id.len() > 64
            || !garment
                .id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
            || !ids.insert(&garment.id)
        {
            return Err(invalid(
                "outfit garment IDs must be unique bounded ASCII names",
            ));
        }
        let original = &garment.pattern;
        let object = original
            .as_object()
            .ok_or_else(|| invalid("garment pattern must be an object"))?;
        if object.keys().any(|k| {
            ![
                "schema",
                "units",
                "style",
                "size",
                "meta",
                "fabric",
                "pieces",
                "seams",
                "attached_mass",
            ]
            .contains(&k.as_str())
        }) {
            return Err(invalid(format!(
                "{}: unsupported root pattern extension; it cannot be discarded during outfit assembly",
                garment.id
            )));
        }
        let pattern = Pattern::new(original.clone());
        let errors = pattern.structure_errors();
        if !errors.is_empty() {
            return Err(invalid(format!("{}: {}", garment.id, errors.join("; "))));
        }
        // Freeze the same relaxed length as the standalone garment, independently of the run's
        // elastic enable switch. The source cut/overlap remains unchanged in the source receipt.
        let casings = crate::scene::elastic_casings(&pattern, 1.0)?;
        if original["meta"]["solved"].get("elastic_cut_mm").is_some() && casings.is_empty() {
            return Err(invalid(format!("{}: legacy casing cannot resolve to one positive-length waistband", garment.id)));
        }
        for casing in casings {
            elastic_casings.push(json!({
                "instance":format!("{}::{}", garment.id, casing.instance),
                "relaxed_m":casing.length,
                "load_instances":casing.load_instances.iter()
                    .map(|id| format!("{}::{id}", garment.id)).collect::<Vec<_>>()
            }));
        }
        let source_pieces = original["pieces"]
            .as_array()
            .ok_or_else(|| invalid("garment needs pieces"))?;
        if pieces.len().saturating_add(source_pieces.len()) > 4096
            || seams.len().saturating_add(pattern.seams().len()) > 4096
        {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "outfit exceeds 4096 source pieces or seams",
            ));
        }
        let fabric = original["fabric"].as_object().ok_or_else(|| {
            invalid("each garment needs an explicit characterized fabric default")
        })?;
        if fabric
            .keys()
            .any(|k| !["material", "material_id", "interfaces", "role", "id"].contains(&k.as_str()))
        {
            return Err(invalid(
                "outfit fabric cannot contain legacy coefficients, inferred layers or unsupported extensions",
            ));
        }
        let frozen_default = textile_import::resolve_fabric(&original["fabric"])?
            .ok_or_else(|| invalid("each garment needs a characterized fabric default"))?
            .frozen_selection();
        if default.is_none() {
            default = Some(frozen_default);
        }
        if let Some(raw) = fabric.get("interfaces") {
            collect_interfaces(
                raw.as_array()
                    .ok_or_else(|| invalid("garment interfaces must be a list"))?,
                &mut interfaces,
            )?;
        }
        let name = |id: &str| format!("{}::{id}", garment.id);
        let mut source_map = BTreeMap::new();
        let mut simulated = 0;
        for (i, source) in source_pieces.iter().enumerate() {
            let id = source["id"]
                .as_str()
                .ok_or_else(|| invalid("source piece needs a string ID"))?;
            if source.get("sim") != Some(&Value::Bool(false)) {
                if !source["placement"].is_object()
                    || source["placement"]
                        .as_object()
                        .is_some_and(|p| p.is_empty())
                {
                    return Err(invalid(format!(
                        "{}: every active piece needs explicit placement or sim:false",
                        name(id)
                    )));
                }
                simulated += 1;
            }
            let fabric = textile_import::piece_fabric(&pattern, i)?;
            let resolved = textile_import::resolve_fabric(fabric)?
                .ok_or_else(|| invalid("outfit pieces need explicit characterized materials"))?;
            let mut frozen = json!({"material":resolved.frozen_selection()});
            if let Some(role) = fabric.get("role") {
                frozen["role"] = role.clone();
            }
            let mut piece = source.clone();
            piece["id"] = json!(name(id));
            piece["fabric"] = frozen;
            source_map.insert(id.to_owned(), name(id));
            pieces.push(piece);
        }
        if simulated == 0 {
            return Err(invalid(format!(
                "{}: garment has no active placed pieces",
                garment.id
            )));
        }
        for instance in &pattern.instances {
            instance_garment.insert(name(&instance.id), index);
        }
        let mut seam_map = Vec::new();
        for (i, source) in pattern.seams().iter().enumerate() {
            let mut seam = serde_json::to_value(source).map_err(|e| invalid(e.to_string()))?;
            for side in ["a", "b"] {
                let reference = source
                    .side_ref(side)
                    .ok_or_else(|| invalid("outfit seam needs two explicit side references"))?;
                pattern
                    .side(reference)
                    .map_err(|e| invalid(e.to_string()))?;
                seam[side] = json!(name(reference));
            }
            let id = format!("{}::seam-{i}", garment.id);
            seam_map.push(json!({"source_id":source.id_text(),"assembled_id":id}));
            seam["id"] = json!(id);
            seams.push(seam);
        }
        let mut source = json!({"id":garment.id,"source_pattern_sha256":digest(original)?,
            "style":original["style"],"meta":original["meta"],"pieces":source_map,"seams":seam_map});
        if let Some(mapping) = collect_attached_mass(&pattern, &garment.id, &mut attached_groups)? {
            source["attached_mass"] = mapping;
        }
        sources.push(source);
    }
    collect_interfaces(&input.interfaces, &mut interfaces)?;
    let mut raw = json!({"schema":"fabric-pattern/1","units":"mm","style":"JOINT_OUTFIT",
        "fabric":{"material":default,"interfaces":interfaces.into_values().collect::<Vec<_>>()},
        "pieces":pieces,"seams":seams});
    if !attached_groups.is_empty() {
        raw["attached_mass"] = json!({"schema":"ftx-attached-mass/1","groups":attached_groups});
    }
    if !elastic_casings.is_empty() {
        raw["meta"] = json!({"solved":{"elastic_casings":elastic_casings}});
    }
    let composed_sha256 = digest(&raw)?;
    let pattern = Pattern::new(raw);
    let errors = pattern.structure_errors();
    if !errors.is_empty() {
        return Err(invalid(format!("composed outfit: {}", errors.join("; "))));
    }
    // Reuse the shared combined group/target budgets and validate every transformed physical name.
    Attachments::from_pattern(&pattern)?;
    crate::scene::elastic_casings(&pattern, options.elastic_ea)?;
    // One authority checks grain, constitutive domains, evidence, pair coverage and contact thickness.
    crate::material::resolve(&pattern, options)?;
    Ok(Prepared {
        pattern,
        instance_garment,
        receipt: json!({"schema":"ftx-outfit-composition/1","input_sha256":input_sha256,
            "composed_pattern_sha256":composed_sha256,"garments":sources,
            "placement_changed":false,"cross_garment_seams":false}),
    })
}

impl Prepared {
    /// The frozen source-mapped fabric-pattern/1 document used by meshing and rendering.
    #[must_use]
    pub fn pattern(&self) -> &Pattern {
        &self.pattern
    }

    /// Original piece/seam identities and source-document digests.
    #[must_use]
    pub fn receipt(&self) -> &Value {
        &self.receipt
    }

    /// Simulate all garments together on one body with the ordinary material-aware contact solver.
    ///
    /// # Errors
    /// Disabled self-contact, invalid/intersecting start, or any ordinary meshing/solver refusal.
    /// Failed runs never return a partially accepted joint result.
    pub fn drape(
        &self,
        fit_spec: &Value,
        body: &DrapeBody,
        options: &DrapeOptions,
        progress: impl FnMut(&str),
    ) -> Result<DrapeRun, Refusal> {
        check_options(options)?;
        let mut run = crate::run::drape(&self.pattern, fit_spec, body, options, progress)?;
        let owners = run
            .scene
            .mesh
            .instances
            .iter()
            .map(|id| {
                self.instance_garment
                    .get(id)
                    .copied()
                    .ok_or_else(|| invalid("outfit lost source instance ownership"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let triangle_garment: Vec<_> = run
            .scene
            .triangle_owner
            .iter()
            .map(|&owner| owners[owner])
            .collect();
        let mut welded = vec![None; run.scene.solver.model.mass.len()];
        for (v, &w) in run.scene.weld.iter().enumerate() {
            let garment = owners[run.scene.mesh.owner[v]];
            if welded[w].is_some_and(|old| old != garment) {
                return Err(invalid("unexpected inter-garment weld"));
            }
            welded[w] = Some(garment);
        }
        let mut mass = vec![
            0.0;
            self.receipt["garments"]
                .as_array()
                .expect("prepared garments")
                .len()
        ];
        for (owner, value) in welded.iter().zip(&run.scene.solver.model.mass) {
            mass[owner.ok_or_else(|| invalid("unowned outfit vertex"))?] += value;
        }
        let mut receipt = self.receipt.clone();
        receipt["instance_garment"] = json!(owners);
        receipt["triangle_garment"] = json!(triangle_garment);
        receipt["garment_mass_kg"] = json!(mass);
        if let Some(attached) = run.scene.solver.model.attached_mass_report()? {
            let mut extras = vec![0.0; mass.len()];
            for group in &attached.groups {
                let mut owner = None;
                for target in &group.targets {
                    let garment = self.instance_garment.get(&target.instance).copied();
                    if garment.is_none() || owner.is_some_and(|old| Some(old) != garment) {
                        return Err(invalid("attached mass lost its source garment ownership"));
                    }
                    owner = garment;
                }
                extras
                    [owner.ok_or_else(|| invalid("attached group has no physical targets"))?] +=
                    group.mass_kg;
            }
            receipt["garment_attached_mass_kg"] = json!(extras);
            receipt["garment_mass_basis"] = json!(
                "garment_mass_kg is total simulated inertia including attached groups; mass_kg remains sheet mass."
            );
        }
        receipt["contact_basis"] = json!(
            "All garments in one cloth solver with full cross-garment self-contact; not frozen overlay or sequential dressing."
        );
        receipt["fit_claim"] = json!(false);
        run.report["outfit"] = receipt;
        Ok(run)
    }
}

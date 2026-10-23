//! Explicit finite garment alterations composed with the existing material-study consumers.
//! Geometry is authored through the canonical operators; neither seam repair nor wearer fit is
//! inferred. Every sequence starts from the same source, including the unaltered baseline.

use std::collections::BTreeSet;

use ftx_core::{Refusal, RefusalCode, curve::OUTLINE_DENSE};
use ftx_pattern::{Pattern, alter, validate};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Outcome, PreparedVariant, Request, budget, hash, invalid};
use crate::cutting::geometry::{Work, simple};

const BASELINE: &str = "baseline";
const MAX_DESIGNS: usize = 31;
const MAX_OPERATIONS: usize = 16;
const MAX_EXPANDED_OPERATIONS: usize = 256;
const MAX_DOCUMENT_WORK_BYTES: usize = 256 * 1024 * 1024;

/// One existing canonical alteration, applied in the stated order and millimetre frame.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    /// Shift coordinates strictly beyond the stated x/y cut, using the canonical mark semantics.
    SlashSpread {
        /// Drafted source-piece ID, not a mirrored instance.
        piece: String,
        /// `x` adds/removes width; `y` adds/removes length.
        axis: String,
        /// Cut coordinate in millimetres.
        at_mm: f64,
        /// Signed spread in millimetres; this is not an inferred body-ease allowance.
        add_mm: f64,
    },
    /// Move one drafted vertex. Relative curves follow it; marks do not move.
    Move {
        /// Drafted source-piece ID.
        piece: String,
        /// Existing drafted vertex ID.
        vertex: String,
        /// Signed horizontal displacement in millimetres.
        dx_mm: f64,
        /// Signed vertical displacement in millimetres.
        dy_mm: f64,
    },
    /// Explicitly slide a seam's B-side drafted endpoint using the existing bounded root search.
    TrueSeam {
        /// Existing unique seam ID; its ratio/band remains authoritative.
        seam: String,
        /// `start` or `end` in the existing operator's B-side convention.
        end: String,
    },
    /// Scale an existing sleeve cap above the underarm to a declared aggregate cap/armhole ratio.
    CapEase {
        /// Drafted cap source-piece ID.
        piece: String,
        /// Existing unique cap/armhole seam IDs; each half is still validated independently.
        seams: Vec<String>,
        /// Positive target total cap length divided by total armhole length.
        target_ratio: f64,
    },
}

impl Operation {
    fn document_passes(&self) -> usize {
        match self {
            // The canonical root solver evaluates two endpoints, sixty bisections and the root,
            // then produces its final document. Reserve those trials even when it refuses early.
            Self::TrueSeam { .. } | Self::CapEase { .. } => 64,
            Self::SlashSpread { .. } | Self::Move { .. } => 1,
        }
    }

    fn apply(&self, pattern: &Value) -> Result<Value, Refusal> {
        match self {
            Self::SlashSpread {
                piece,
                axis,
                at_mm,
                add_mm,
            } => alter::slash_spread(pattern, piece, axis, *at_mm, *add_mm),
            Self::Move {
                piece,
                vertex,
                dx_mm,
                dy_mm,
            } => alter::move_vertex(pattern, piece, vertex, *dx_mm, *dy_mm),
            Self::TrueSeam { seam, end } => alter::true_seam(pattern, seam, end),
            Self::CapEase {
                piece,
                seams,
                target_ratio,
            } => alter::cap_ease(pattern, piece, seams, *target_ratio),
        }
    }
}

/// One independently authored geometry candidate. Material choices remain in the ordinary study.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Variant {
    /// Unique nonempty design identity; `baseline` is reserved for the original pattern.
    pub key: String,
    /// One to sixteen ordered canonical alterations. No implicit geometry repair follows them.
    pub operations: Vec<Operation>,
}

/// Existing flat validation and full sampled-outline checks, retained even on refusal.
#[derive(Clone, Debug, Serialize)]
pub struct Validation {
    /// Exact spec passed to the canonical flat validator; no body points are synthesized.
    pub spec: Value,
    /// Unmodified structural/seam/truing/completeness report from `ftx_pattern::validate`.
    pub flat: Value,
    /// All checked physical instance IDs, or the first malformed/self-intersecting outline.
    pub outlines: Outcome<Vec<String>>,
    /// Existing canonical outline sampling density, not an analytic curved-intersection proof.
    pub outline_samples_per_curve: usize,
    /// Both the canonical flat report and every sampled outline passed.
    pub passed: bool,
}

/// Geometry provenance for one design/material pair, including failed sequences and validation.
#[derive(Clone, Debug, Serialize)]
pub struct Receipt {
    /// Authored design key, or the automatically retained `baseline`.
    pub key: String,
    /// Original material variant key, before collision-free pair encoding.
    pub material_key: String,
    /// Exact ordered operators, empty only for the unaltered baseline.
    pub operations: Vec<Operation>,
    /// Identity of the original unchanged pattern, before alteration or material binding.
    pub source_pattern_sha256: String,
    /// Identity of the complete altered document, before material binding. Null on operator refusal.
    pub altered_pattern_sha256: Option<String>,
    /// Lossless altered document for export/replay, including refused validation for inspection.
    /// Replay the row's frozen material changes on this exact document, not on the original source.
    pub altered_pattern_json: Option<String>,
    /// Whether pieces/marks or seams changed; history-only no-ops are false, operator failures null.
    pub geometry_changed: Option<bool>,
    /// Complete canonical flat/sampled-outline gate, absent only if an operator refused.
    pub validation: Option<Validation>,
}

pub(super) struct Prepared {
    variant: Variant,
    material_key: String,
}

pub(super) fn validate_request(request: &Request) -> Result<(), Refusal> {
    if !request.is_design() {
        if request.designs.is_some() {
            return Err(invalid(
                "designs require explicit ftx-design-study/1; ftx-material-study/1 keeps geometry fixed",
            ));
        }
        return Ok(());
    }
    let designs = request
        .designs
        .as_ref()
        .filter(|designs| !designs.is_empty())
        .ok_or_else(|| invalid("design study needs at least one explicit design"))?;
    if designs.len() > MAX_DESIGNS {
        return Err(budget("design study exceeds 31 designs plus its baseline"));
    }
    if request
        .sewing
        .as_ref()
        .or_else(|| request.sewing_selection.as_ref().map(|s| &s.options))
        .is_some_and(|s| !s.seam_tolerance_mm.is_finite() || s.seam_tolerance_mm < 0.0)
    {
        return Err(invalid(
            "design validation needs finite nonnegative seam tolerance",
        ));
    }
    let mut keys = BTreeSet::new();
    for design in designs {
        if design.key.trim().is_empty()
            || design.key == BASELINE
            || !keys.insert(&design.key)
            || design.operations.is_empty()
        {
            return Err(invalid(
                "design keys must be unique and nonempty, baseline is reserved, and each design needs operations",
            ));
        }
        if design.operations.len() > MAX_OPERATIONS {
            return Err(budget("design sequence exceeds 16 alteration operations"));
        }
    }
    Ok(())
}

/// Reserve the full cross product before any alterations, manufacturing or scenario callbacks.
pub(super) fn expand(
    request: &Request,
    pattern: &Pattern,
    materials: Vec<PreparedVariant>,
) -> Result<Vec<PreparedVariant>, Refusal> {
    if !request.is_design() {
        return Ok(materials);
    }
    let designs = request.designs.as_ref().expect("validated design request");
    let mut keys = BTreeSet::new();
    if materials
        .iter()
        .any(|row| row.variant.key.trim().is_empty() || !keys.insert(&row.variant.key))
    {
        return Err(invalid("material variant keys must be unique and nonempty"));
    }
    let rows = (designs.len() + 1).saturating_mul(materials.len());
    if rows > 32 {
        return Err(budget(
            "design study exceeds 32 expanded design/material alternatives, including baseline",
        ));
    }
    let operations = designs
        .iter()
        .map(|design| design.operations.len())
        .sum::<usize>()
        .saturating_mul(materials.len());
    if operations > MAX_EXPANDED_OPERATIONS {
        return Err(budget(
            "design study exceeds 256 expanded alteration operations",
        ));
    }
    let document_passes = designs
        .iter()
        .flat_map(|design| &design.operations)
        .map(Operation::document_passes)
        .sum::<usize>()
        .saturating_mul(materials.len());
    // Reserve source clones/retained documents and the authored operation payload before running
    // any operation. This bounds large-document work in addition to the existing geometry budgets.
    let source_bytes = serde_json::to_vec(pattern.raw())
        .map_err(|e| invalid(e.to_string()))?
        .len();
    let operation_bytes = serde_json::to_vec(designs)
        .map_err(|e| invalid(e.to_string()))?
        .len();
    let material_bytes = materials.iter().try_fold(0usize, |sum, material| {
        let bytes = serde_json::to_vec(&material.variant)
            .map_err(|e| invalid(e.to_string()))?
            .len();
        Ok::<_, Refusal>(sum.saturating_add(bytes))
    })?;
    if source_bytes
        .saturating_add(operation_bytes)
        .saturating_mul(rows.saturating_add(document_passes))
        .saturating_add(material_bytes.saturating_mul(designs.len() + 1))
        > MAX_DOCUMENT_WORK_BYTES
    {
        return Err(budget(
            "design study exceeds 256 MiB aggregate document-processing reservation",
        ));
    }
    let baseline = Variant {
        key: BASELINE.into(),
        operations: Vec::new(),
    };
    let mut expanded = Vec::with_capacity(rows);
    for design in std::iter::once(&baseline).chain(designs) {
        for material in &materials {
            let key = serde_json::to_string(&(&design.key, &material.variant.key))
                .map_err(|e| invalid(e.to_string()))?;
            let mut variant = material.variant.clone();
            variant.key = key;
            expanded.push(PreparedVariant {
                variant,
                choices: material.choices.clone(),
                design: Some(Prepared {
                    variant: design.clone(),
                    material_key: material.variant.key.clone(),
                }),
            });
        }
    }
    Ok(expanded)
}

impl Prepared {
    pub(super) fn apply(
        self,
        source: &Pattern,
        seam_tolerance_mm: f64,
        work: &mut Work,
    ) -> Result<(Receipt, Result<Pattern, Refusal>), Refusal> {
        let mut receipt = Receipt {
            key: self.variant.key,
            material_key: self.material_key,
            operations: self.variant.operations,
            source_pattern_sha256: hash(source.raw())?,
            altered_pattern_sha256: None,
            altered_pattern_json: None,
            geometry_changed: None,
            validation: None,
        };
        let altered = receipt
            .operations
            .iter()
            .try_fold(source.raw().clone(), |pattern, operation| {
                operation.apply(&pattern)
            });
        let altered = match altered {
            Ok(pattern) => pattern,
            Err(refusal) => return Ok((receipt, Err(refusal))),
        };
        receipt.altered_pattern_sha256 = Some(hash(&altered)?);
        receipt.altered_pattern_json =
            Some(serde_json::to_string(&altered).map_err(|e| invalid(e.to_string()))?);
        let pattern = Pattern::new(altered);
        receipt.geometry_changed = Some(geometry_changed(source, &pattern));
        let spec = json!({"seam_tol_mm":seam_tolerance_mm,"truing_tol_deg":8.0,"points":[]});
        let flat = serde_json::to_value(validate::validate(
            &pattern,
            &spec,
            &validate::Measurements(json!({})),
        ))
        .map_err(|e| invalid(e.to_string()))?;
        let outlines = if flat["status"] == "BLOCKED" {
            Err(invalid(
                "design outline checks require valid canonical pattern structure",
            ))
        } else {
            check_outlines(&pattern, work)
        };
        let refused = match &outlines {
            Err(refusal) => Some(refusal.clone()),
            Ok(_) if flat["status"] != "CONVERGED" => Some(invalid(
                "design failed canonical flat validation; inspect design.validation.flat for seam, truing or completeness failures",
            )),
            Ok(_) => None,
        };
        receipt.validation = Some(Validation {
            spec,
            flat,
            outlines: Outcome::from_result(outlines),
            outline_samples_per_curve: OUTLINE_DENSE,
            passed: refused.is_none(),
        });
        Ok((receipt, refused.map_or(Ok(pattern), Err)))
    }
}

fn geometry_changed(source: &Pattern, altered: &Pattern) -> bool {
    let (Some(source_doc), Some(altered_doc)) = (&source.doc, &altered.doc) else {
        return source.raw()["pieces"] != altered.raw()["pieces"]
            || source.raw()["seams"] != altered.raw()["seams"];
    };
    source_doc.seams != altered_doc.seams
        || source_doc.pieces.len() != altered_doc.pieces.len()
        || source_doc
            .pieces
            .iter()
            .zip(&altered_doc.pieces)
            .any(|(source, altered)| {
                // Typed coordinates compare actual f64 values, so 10 versus 10.0 is not a change.
                // Slash-spread also normalizes legacy grain objects to arrays without moving them.
                if source == altered {
                    return false;
                }
                if source.grain.as_ref().and_then(|g| g.points())
                    != altered.grain.as_ref().and_then(|g| g.points())
                {
                    return true;
                }
                let mut normalized = source.clone();
                normalized.grain.clone_from(&altered.grain);
                normalized != *altered
            })
}

fn check_outlines(pattern: &Pattern, work: &mut Work) -> Result<Vec<String>, Refusal> {
    let mut checked = Vec::with_capacity(pattern.instances.len());
    for instance in &pattern.instances {
        let ring = instance
            .outline(OUTLINE_DENSE)
            .map_err(|e| invalid(e.to_string()))?;
        if ring.len() < 3
            || ring.iter().any(|p| !p.is_finite())
            || !simple(&ring, work)
                .ok_or_else(|| budget("design study exhausted aggregate outline-validation work"))?
        {
            return Err(Refusal::new(
                RefusalCode::DegenerateGeometry,
                format!(
                    "{}: altered sampled outline is not a simple finite polygon",
                    instance.id
                ),
            ));
        }
        checked.push(instance.id.clone());
    }
    Ok(checked)
}

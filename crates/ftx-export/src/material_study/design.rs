//! Explicit finite garment alterations composed with the existing material-study consumers.
//! Geometry is authored through the canonical operators; neither seam repair nor wearer fit is
//! inferred. Every sequence starts from the same source, including the unaltered baseline.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode, curve::OUTLINE_DENSE};
use ftx_pattern::{Pattern, alter, validate};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Outcome, PreparedVariant, Request, budget, hash, invalid};
use crate::cutting::geometry::{Work, simple};

pub mod fit;
pub mod grid;

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
    /// Canonical flat structure, every sampled outline and any requested body-fit gate passed.
    pub passed: bool,
}

/// Geometry provenance for one design/material pair, including failed sequences and validation.
#[derive(Clone, Debug, Serialize)]
pub struct Receipt {
    /// Authored design key, or the automatically retained `baseline`.
    pub key: String,
    /// Original material variant key, before collision-free pair encoding. Absent on the
    /// independent flat-screening receipt, before any material is selected or bound.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub material_key: Option<String>,
    /// Exact ordered operators, empty only for the unaltered baseline.
    pub operations: Vec<Operation>,
    /// Declared grid coordinates; absent for the baseline and hand-authored designs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variables_mm: Option<BTreeMap<String, f64>>,
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
    /// Explicit fit targets, fixed body identity and the canonical report or unavailable evidence.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fit: Option<Outcome<fit::Report>>,
}

pub(super) struct Prepared {
    variant: Variant,
    material_key: Option<String>,
    variables_mm: Option<BTreeMap<String, f64>>,
    evaluated: Option<(Receipt, Result<Pattern, Refusal>)>,
}

pub(super) fn validate_request(request: &Request) -> Result<(), Refusal> {
    if !request.is_design() {
        if request.designs.is_some()
            || request.design_grid.is_some()
            || request.design_fit.is_some()
        {
            return Err(invalid(
                "designs, design_grid and design_fit require explicit ftx-design-study/1; ftx-material-study/1 keeps geometry fixed",
            ));
        }
        return Ok(());
    }
    if request.designs.is_some() == request.design_grid.is_some() {
        return Err(invalid(
            "design study needs exactly one of designs or design_grid",
        ));
    }
    if let Some(grid) = &request.design_grid {
        grid.validate()?;
        if grid.fit_screening.is_some() && request.design_fit.is_none() {
            return Err(invalid(
                "design grid fit_screening requires an explicit design_fit contract",
            ));
        }
    }
    if let Some(fit) = &request.design_fit {
        fit.validate()?;
    }
    let designs = request.designs.as_deref().unwrap_or(&[]);
    if request.designs.is_some() && designs.is_empty() {
        return Err(invalid("design study needs at least one explicit design"));
    }
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
    fit_context: Option<&fit::Context<'_>>,
) -> Result<(Vec<PreparedVariant>, Option<grid::Receipt>), Refusal> {
    if !request.is_design() {
        return Ok((materials, None));
    }
    let screened = request
        .design_grid
        .as_ref()
        .is_some_and(|grid| grid.fit_screening.is_some());
    if screened {
        fit_context
            .ok_or_else(|| {
                invalid("design grid fit_screening needs the selected scenario's fitted-body measurement context")
                    .repair("Evaluate this design request with a model-backed scenario entry point.")
            })?
            .validate()?;
    }
    let mut search = request
        .design_grid
        .as_ref()
        .map(grid::Request::receipt)
        .transpose()?;
    let designs: Vec<_> = if let Some(grid) = &request.design_grid {
        grid.generate()?
            .into_iter()
            .map(|(design, values)| (design, Some(values)))
            .collect()
    } else {
        request
            .designs
            .as_ref()
            .expect("validated design request")
            .iter()
            .cloned()
            .map(|design| (design, None))
            .collect()
    };
    let mut keys = BTreeSet::new();
    if materials
        .iter()
        .any(|row| row.variant.key.trim().is_empty() || !keys.insert(&row.variant.key))
    {
        return Err(invalid("material variant keys must be unique and nonempty"));
    }
    let selected_bound = if screened {
        designs.len().min(
            request
                .design_grid
                .as_ref()
                .expect("screened grid")
                .max_designs,
        )
    } else {
        designs.len()
    };
    let rows = (selected_bound + 1).saturating_mul(materials.len());
    if rows > 32 {
        return Err(budget(
            "design study exceeds 32 expanded design/material alternatives, including baseline",
        ));
    }
    if screened {
        // Existing operators move coordinates but retain edge/control-point topology. Bound the
        // entire canonical simplicity pass before any candidate operation or sampling allocates.
        let outline_work = pattern
            .instances
            .iter()
            .fold(0usize, |total, instance| {
                let points = instance.edges.iter().fold(0usize, |count, edge| {
                    count.saturating_add(edge.curve.as_ref().map_or(1, |curve| {
                        if curve.is_line() {
                            1
                        } else {
                            OUTLINE_DENSE.max(curve.controls().len().saturating_add(1))
                        }
                    }))
                });
                total.saturating_add(points.saturating_mul(points.saturating_add(1)) / 2)
            })
            .saturating_mul(designs.len() + 1);
        if outline_work > 40_000_000 {
            return Err(budget(
                "design fit_screening exceeds 40000000 reserved outline-validation work units",
            ));
        }
    }
    if let Some(fit) = &request.design_fit {
        // Canonical points may resample an outline or walk several selected edges. Reserve a
        // conservative full-instance edge pass per selector, before alterations or consumers.
        let dense = ftx_core::curve::MEASURE_DENSE.max(ftx_core::curve::EDGE_DENSE);
        let samples = pattern
            .instances
            .iter()
            .flat_map(|instance| &instance.edges)
            .fold(0usize, |n, edge| {
                n.saturating_add(if screened {
                    edge.curve.as_ref().map_or(dense, |curve| {
                        dense.max(curve.controls().len().saturating_add(2))
                    })
                } else {
                    dense
                })
            });
        let work = fit
            .measurement_work_units()
            .saturating_mul(samples.max(dense))
            .saturating_mul(if screened { designs.len() + 1 } else { rows });
        if work > 40_000_000 {
            return Err(budget(
                "design fit exceeds 40000000 aggregate measurement work units",
            ));
        }
    }
    let operations = designs
        .iter()
        .map(|(design, _)| design.operations.len())
        .sum::<usize>()
        .saturating_mul(if screened { 1 } else { materials.len() });
    let max_operations = if screened {
        4096
    } else {
        MAX_EXPANDED_OPERATIONS
    };
    if operations > max_operations {
        return Err(budget(if screened {
            "design fit_screening exceeds 4096 reserved alteration operations"
        } else {
            "design study exceeds 256 expanded alteration operations"
        }));
    }
    let document_passes = designs
        .iter()
        .flat_map(|(design, _)| &design.operations)
        .map(Operation::document_passes)
        .sum::<usize>()
        .saturating_mul(if screened { 1 } else { materials.len() });
    // Reserve source clones/retained documents and the authored operation payload before running
    // any operation. This bounds large-document work in addition to the existing geometry budgets.
    let source_bytes = serde_json::to_vec(pattern.raw())
        .map_err(|e| invalid(e.to_string()))?
        .len();
    let operation_bytes = serde_json::to_vec(&(&designs, &request.design_fit))
        .map_err(|e| invalid(e.to_string()))?
        .len();
    let material_bytes = materials.iter().try_fold(0usize, |sum, material| {
        let bytes = serde_json::to_vec(&material.variant)
            .map_err(|e| invalid(e.to_string()))?
            .len();
        Ok::<_, Refusal>(sum.saturating_add(bytes))
    })?;
    let body_identity_bytes = if screened {
        serde_json::to_vec(fit_context.expect("validated screening context").identity)
            .map_err(|e| invalid(e.to_string()))?
            .len()
    } else {
        0
    };
    let geometry_evaluations = if screened { designs.len() + 1 } else { rows };
    if source_bytes
        .saturating_add(operation_bytes)
        .saturating_mul(
            // Each screening receipt retains its altered document and canonical validation;
            // selected candidates additionally retain cached geometry for material expansion.
            rows.saturating_add(document_passes)
                .saturating_add(if screened { geometry_evaluations } else { 0 })
                .saturating_add(
                    usize::from(request.design_fit.is_some()).saturating_mul(geometry_evaluations),
                ),
        )
        .saturating_add(material_bytes.saturating_mul(selected_bound + 1))
        .saturating_add(
            body_identity_bytes.saturating_mul(geometry_evaluations.saturating_add(rows)),
        )
        > MAX_DOCUMENT_WORK_BYTES
    {
        return Err(budget(
            "design study exceeds 256 MiB aggregate document-processing reservation",
        ));
    }
    let baseline = (
        Variant {
            key: BASELINE.into(),
            operations: Vec::new(),
        },
        None,
    );
    let mut candidates = Vec::with_capacity(selected_bound + 1);
    if screened {
        let mut work = Work::new(40_000_000);
        let seam_tolerance = request
            .sewing
            .as_ref()
            .or_else(|| request.sewing_selection.as_ref().map(|s| &s.options))
            .map_or(3.0, |s| s.seam_tolerance_mm);
        let mut evaluate = |design: &Variant, variables_mm: &Option<BTreeMap<String, f64>>| {
            let result = Prepared {
                variant: design.clone(),
                material_key: None,
                variables_mm: variables_mm.clone(),
                evaluated: None,
            }
            .apply(
                pattern,
                seam_tolerance,
                &mut work,
                request.design_fit.as_ref(),
                fit_context,
            )?;
            if work.exhausted {
                return Err(budget(
                    "design fit_screening exceeds 40000000 aggregate outline-validation work units",
                ));
            }
            Ok(result)
        };
        let initial = evaluate(&baseline.0, &baseline.1)?;
        candidates.push((baseline.0, baseline.1, Some(initial)));
        let mut rows = Vec::with_capacity(designs.len());
        let mut qualified = 0;
        let mut selected = 0;
        for (design, variables_mm) in designs {
            let evaluated = evaluate(&design, &variables_mm)?;
            let passed = evaluated.1.is_ok();
            qualified += usize::from(passed);
            let retain = passed && selected < selected_bound;
            rows.push(grid::ScreenedDesign {
                design: evaluated.0.clone(),
                selected: retain,
                refusal: evaluated.1.as_ref().err().cloned(),
            });
            if retain {
                selected += 1;
                candidates.push((design, variables_mm, Some(evaluated)));
            }
        }
        let receipt = search.as_mut().expect("screened grid receipt");
        let unvisited = receipt.total_grid_points - rows.len();
        let omitted = qualified - selected;
        receipt.generated_designs = selected;
        receipt.omitted_designs = receipt.total_grid_points - selected;
        receipt.complete_grid_enumeration = unvisited == 0 && omitted == 0;
        receipt.budget_exhausted = !receipt.complete_grid_enumeration;
        receipt.fit_screening = Some(grid::FitScreeningReport {
            screened_designs: rows.len(),
            qualified_designs: qualified,
            rejected_designs: rows.len() - qualified,
            selected_designs: selected,
            qualified_designs_omitted: omitted,
            unvisited_designs: unvisited,
            complete_fit_screening: unvisited == 0,
            all_qualified_designs_selected: omitted == 0,
            rows,
        });
    } else {
        candidates.extend(
            std::iter::once(baseline)
                .chain(designs)
                .map(|(design, values)| (design, values, None)),
        );
    }
    let mut expanded = Vec::with_capacity(rows);
    for (design, variables_mm, evaluated) in &candidates {
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
                    material_key: Some(material.variant.key.clone()),
                    variables_mm: variables_mm.clone(),
                    evaluated: evaluated.clone(),
                }),
            });
        }
    }
    Ok((expanded, search))
}

impl Prepared {
    pub(super) fn apply(
        self,
        source: &Pattern,
        seam_tolerance_mm: f64,
        work: &mut Work,
        fit_request: Option<&fit::Request>,
        fit_context: Option<&fit::Context<'_>>,
    ) -> Result<(Receipt, Result<Pattern, Refusal>), Refusal> {
        if let Some((mut receipt, pattern)) = self.evaluated {
            receipt.material_key = self.material_key;
            return Ok((receipt, pattern));
        }
        let mut receipt = Receipt {
            key: self.variant.key,
            material_key: self.material_key,
            operations: self.variant.operations,
            variables_mm: self.variables_mm,
            source_pattern_sha256: hash(source.raw())?,
            altered_pattern_sha256: None,
            altered_pattern_json: None,
            geometry_changed: None,
            validation: None,
            fit: None,
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
        let mut refused = match &outlines {
            Err(refusal) => Some(refusal.clone()),
            Ok(_) if flat["status"] != "CONVERGED" => Some(invalid(
                "design failed canonical flat validation; inspect design.validation.flat for seam, truing or completeness failures",
            )),
            Ok(_) => None,
        };
        if let Some(request) = fit_request {
            let outcome = request.evaluate(&pattern, fit_context);
            if refused.is_none() {
                refused = match &outcome {
                    Outcome::Available { result } if result.passed => None,
                    Outcome::Available { .. } => Some(invalid(
                        "design failed explicit flat fit targets on the selected body; inspect design.fit.result.validation",
                    )),
                    Outcome::Refused { refusal } => Some(refusal.clone()),
                    Outcome::NotRequested => {
                        Some(invalid("requested design fit was not evaluated"))
                    }
                };
            }
            receipt.fit = Some(outcome);
        }
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

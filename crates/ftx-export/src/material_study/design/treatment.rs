//! Canonical treatment inside an explicit design sequence, with final material-state protection.

use std::collections::BTreeMap;

use ftx_core::{Curve, Refusal, curve::EDGE_DENSE};
use ftx_pattern::{Pattern, material_treatment::geometry};
use serde::Serialize;
use serde_json::Value;

use super::{Operation, budget, invalid};

const MAX_GEOMETRY_WORK: usize = 40_000_000;

/// Additional bounded geometry and document work introduced by treatment, beyond ordinary designs.
#[derive(Clone, Copy, Default)]
pub(super) struct Reservation {
    pub geometry: usize,
    pub bytes: usize,
}

impl Reservation {
    pub(super) fn add(&mut self, other: Self, executions: usize) -> Result<(), Refusal> {
        self.geometry = self
            .geometry
            .saturating_add(other.geometry.saturating_mul(executions));
        self.bytes = self
            .bytes
            .saturating_add(other.bytes.saturating_mul(executions));
        if self.geometry > MAX_GEOMETRY_WORK {
            return Err(budget(
                "design treatment exceeds 40000000 reserved geometry work units",
            ));
        }
        Ok(())
    }
}

fn samples(curve: Option<&Curve>) -> usize {
    curve.map_or(2, |curve| {
        if curve.is_line() {
            2
        } else {
            EDGE_DENSE.max(curve.controls().len().saturating_add(2))
        }
    })
}

// Operators preserve source/instance topology. Lengths may change, so reserve the canonical
// seam_pairs maximum (<100000 mm per side) instead of trusting the original seam length.
fn seam_work(pattern: &Pattern) -> usize {
    let mut work = 0usize;
    for seam in pattern.seams() {
        let mut side_work = 0usize;
        let mut breaks = 2usize;
        for side in ["a", "b"] {
            let Some(reference) = seam.side_ref(side) else {
                continue;
            };
            let Ok((instance, edges)) = pattern.side(reference) else {
                continue;
            };
            let notches = pattern
                .doc
                .as_ref()
                .and_then(|doc| doc.pieces.get(instance.piece_index))
                .map_or(0, |piece| piece.notches().len());
            let points = edges
                .iter()
                .filter_map(|id| instance.edge(id).ok())
                .fold(0usize, |sum, edge| {
                    sum.saturating_add(samples(edge.curve.as_ref()))
                });
            // notch_fractions visits every path edge and may sum its prefix for each match.
            side_work = side_work
                .saturating_add(points.saturating_mul(4))
                .saturating_add(
                    notches
                        .saturating_mul(edges.len())
                        .saturating_mul(edges.len()),
                );
            breaks = breaks.saturating_add(edges.len()).saturating_add(notches);
        }
        // Piecewise easing can require the sum of both side lengths, not merely the longer side.
        let stations = 40_001usize.saturating_add(breaks);
        work = work.saturating_add(
            side_work
                .saturating_add(stations.saturating_mul(2))
                .saturating_mul(2),
        );
    }
    work
}

/// Reserve every attempted operation and the accumulated per-piece selection/history expansion.
/// Full canonical reports retain several copies of both patterns and source requests. In particular,
/// a large AFTER record assigned to many pieces must not be counted only once as request payload.
pub(super) fn reserve(pattern: &Pattern, operations: &[Operation]) -> Result<Reservation, Refusal> {
    if !operations
        .iter()
        .any(|op| matches!(op, Operation::Treatment { .. }))
    {
        return Ok(Reservation::default());
    }
    let source_bytes = serde_json::to_vec(pattern.raw())
        .map_err(|e| invalid(e.to_string()))?
        .len();
    let seams = seam_work(pattern);
    let mut added = 0usize;
    let mut reservation = Reservation::default();
    for operation in operations {
        if let Operation::Treatment { request } = operation {
            let request_bytes = serde_json::to_vec(request)
                .map_err(|e| invalid(e.to_string()))?
                .len();
            let selection_bytes = request
                .transitions
                .iter()
                .map(|(name, input)| {
                    let selection = match request.mode {
                        geometry::Mode::Treat => &input.after,
                        geometry::Mode::Compensate => &input.before,
                    };
                    Ok((
                        name.as_str(),
                        serde_json::to_vec(selection)
                            .map_err(|e| invalid(e.to_string()))?
                            .len(),
                    ))
                })
                .collect::<Result<BTreeMap<_, _>, Refusal>>()?;
            let mut work = seams;
            for (id, transition) in &request.pieces {
                added = added.saturating_add(
                    selection_bytes
                        .get(transition.as_str())
                        .copied()
                        .unwrap_or(0),
                );
                if let Some(piece) = pattern.doc.as_ref().and_then(|doc| doc.piece(id)) {
                    let edges: BTreeMap<_, _> = piece
                        .edges
                        .iter()
                        .map(|edge| (edge.id.as_str(), samples(edge.curve.as_ref())))
                        .collect();
                    work = work
                        .saturating_add(
                            edges
                                .values()
                                .fold(0usize, |sum, n| sum.saturating_add(*n))
                                .saturating_mul(4),
                        )
                        .saturating_add(piece.vertices.0.len())
                        .saturating_add(piece.drills.as_ref().map_or(0, Vec::len));
                    for notch in piece.notches() {
                        work = work
                            .saturating_add(edges.get(notch.edge.as_str()).copied().unwrap_or(0));
                    }
                    for marks in [&piece.internal, &piece.decor, &piece.trace_marks]
                        .into_iter()
                        .flatten()
                    {
                        work = work.saturating_add(
                            marks
                                .iter()
                                .fold(0usize, |sum, mark| sum.saturating_add(mark.pts.len())),
                        );
                    }
                }
            }
            // Bound retained assignment hashes, coordinates, names, interfaces and history even
            // when several operations successively grow the same source document.
            added = added
                .saturating_add(request_bytes.saturating_mul(4))
                .saturating_add(request.pieces.len().saturating_mul(1024));
            reservation.geometry = reservation
                .geometry
                .saturating_add(work)
                .saturating_add(request.transitions.len().saturating_mul(2))
                .saturating_add(request.pieces.len().saturating_mul(2))
                .saturating_add(pattern.doc.as_ref().map_or(0, |doc| doc.pieces.len()));
            reservation.bytes = reservation.bytes.saturating_add(
                source_bytes
                    .saturating_add(added)
                    .saturating_add(request_bytes)
                    .saturating_mul(8),
            );
        } else {
            reservation.bytes = reservation
                .bytes
                .saturating_add(added.saturating_mul(operation.document_passes()));
        }
    }
    // Final design/fit admission, frozen binding, and retained final ordinary pattern.
    reservation.bytes = reservation.bytes.saturating_add(added.saturating_mul(4));
    Ok(reservation)
}

/// One completed treatment operation. Geometry and source evidence remain in the canonical report.
#[derive(Clone, Debug, Serialize)]
pub struct Applied {
    /// Zero-based index in the design's complete ordered operation list.
    pub operation_index: usize,
    /// Unmodified `ftx-pattern-treatment-report/1`, including exact intermediate pattern bytes.
    pub report: Value,
}

pub(super) fn apply(
    request: &geometry::Input,
    pattern: &Value,
    operation_index: usize,
    treatments: &mut Vec<Applied>,
) -> Result<Value, Refusal> {
    let report = request.apply(&Pattern::new(pattern.clone()))?;
    let pattern = report["pattern"].clone();
    treatments.push(Applied {
        operation_index,
        report,
    });
    Ok(pattern)
}

/// The ordinary binder may alter unrelated pieces, but cannot detach treated geometry from the
/// final material state promised by its last treatment operation. Use the binder's resolved hashes.
pub(crate) fn protect_bindings(treatments: &[Applied], binding: &Value) -> Result<(), Refusal> {
    if treatments.is_empty() {
        return Ok(());
    }
    let mut expected = BTreeMap::new();
    for treatment in treatments {
        let field = match treatment.report["mode"].as_str() {
            Some("treat") => "after_material_sha256",
            Some("compensate") => "before_material_sha256",
            _ => return Err(invalid("completed treatment lost its state mode")),
        };
        for piece in treatment.report["pieces"]
            .as_array()
            .ok_or_else(|| invalid("completed treatment lost its source-piece assignments"))?
        {
            let id = piece["piece_id"]
                .as_str()
                .ok_or_else(|| invalid("completed treatment lost its source-piece ID"))?;
            let hash = piece[field]
                .as_str()
                .ok_or_else(|| invalid("completed treatment lost its material identity"))?;
            expected.insert(id, hash);
        }
    }
    let assignments = binding["pieces"]
        .as_array()
        .ok_or_else(|| invalid("final material binding lost its source-piece assignments"))?
        .iter()
        .filter_map(|entry| {
            Some((
                entry["piece_id"].as_str()?,
                entry["material_sha256"].as_str()?,
            ))
        })
        .collect::<BTreeMap<_, _>>();
    for (piece, hash) in expected {
        if assignments.get(piece).copied() != Some(hash) {
            return Err(invalid(format!(
                "final material binding replaces {piece}'s qualified treatment state; retain its last treatment material or supply the matching transition"
            )));
        }
    }
    Ok(())
}

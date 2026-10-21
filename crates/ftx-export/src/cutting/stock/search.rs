//! Finite-stock backtracking and optional stock-use minimization over the declared lattice.
//! Every candidate uses the same complete polygon checks. A budget may end optimization, but
//! never turn an incomplete allocation into a plan. No continuous-space optimality is asserted.
use ftx_core::{Refusal, RefusalCode, Vec2};
use ftx_pattern::material_catalog::ResolvedMaterial;
use serde::{Deserialize, Serialize};

use super::super::Shape;
use super::{
    CutPlacement, Marker, Options, Work, bad, checked, half_turn, locate_next, place, same_material,
};

/// Explicit stock-use objective. Purchase cost requires qualified whole-item quotes; no objective
/// infers reusable-offcut value or manufacturing/handling costs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Objective {
    /// Minimize the number of distinct physical stock items receiving new cuts.
    MinimizeStockCount,
    /// Minimize whole polygon area of stock items receiving new cuts, counted once per item.
    MinimizeGrossArea,
    /// Minimize admitted whole-item order costs, counting each physical item once.
    MinimizePurchaseCost,
}

/// Stock use for a complete checked layout, in stable inventory order.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Usage {
    /// Physical items receiving at least one new cut; old occupied regions alone do not count.
    pub stock_count: usize,
    /// Sum of whole original stock polygon areas, including defects and prior cuts, in m².
    pub gross_area_m2: f64,
    /// Sum of admitted whole-item costs in `purchasing.currency`; present only for cost optimization.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub purchase_cost: Option<f64>,
}

impl Objective {
    fn improves(self, proposed: Usage, incumbent: Usage) -> bool {
        match self {
            Self::MinimizeStockCount => proposed.stock_count < incumbent.stock_count,
            Self::MinimizeGrossArea => proposed.gross_area_m2 < incumbent.gross_area_m2,
            Self::MinimizePurchaseCost => proposed
                .purchase_cost
                .zip(incumbent.purchase_cost)
                .is_some_and(|(a, b)| a < b),
        }
    }
}

/// Why the bounded improvement search stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Termination {
    /// All strictly better layouts on the declared lattice were explored or bounded out.
    LatticeExhausted,
    /// The shared geometry/search work allowance was exhausted after a complete layout existed.
    WorkLimit,
    /// The explicit number of undone placement decisions was reached.
    BacktrackLimit,
}

/// A complete feasible incumbent, with optimization scope and budget status kept separate.
#[derive(Clone, Debug, Serialize)]
pub struct Optimization {
    /// Declared objective; ties retain the first complete incumbent without a secondary objective.
    pub objective: Objective,
    /// First complete layout encountered, under the same constraints and search order.
    pub baseline: Usage,
    /// Stock use of the returned complete layout.
    pub selected: Usage,
    /// Complete feasible leaves visited; prefixes and bound-pruned possibilities do not count.
    pub complete_layouts: usize,
    /// Strict improvements after the first complete incumbent.
    pub improvements: usize,
    /// Stopping cause, including an explicit exhausted budget when proof is incomplete.
    pub termination: Termination,
    /// Objective minimality on this exact lattice/orientation set, not in continuous placement space.
    pub lattice_optimal: bool,
    /// Unexamined possibilities remain because one of the declared work limits was reached.
    pub budget_exhausted: bool,
}

pub(super) struct Allocation {
    pub undone: usize,
    pub optimization: Option<Optimization>,
}

// Adding an item can only increase either objective. The hypothetical item need not fit: ignoring
// geometry gives a valid lower bound, never an acceptance shortcut. Sum in stock order just as the
// final plan does, including when comparing partially allocated inventories.
fn usage(
    markers: &[Marker],
    add: Option<usize>,
    costs: Option<&[Option<f64>]>,
) -> Result<Usage, Refusal> {
    let mut use_ = Usage {
        stock_count: 0,
        gross_area_m2: 0.0,
        purchase_cost: costs.map(|_| 0.0),
    };
    for (i, marker) in markers.iter().enumerate() {
        if !marker.placements.is_empty() || add == Some(i) {
            use_.stock_count += 1;
            use_.gross_area_m2 += marker.gross_area_m2;
            if let Some(costs) = costs {
                let cost = costs[i].ok_or_else(|| bad("unpriced stock entered cost search"))?;
                *use_.purchase_cost.as_mut().expect("cost accumulator") += cost;
            }
        }
    }
    if !use_.gross_area_m2.is_finite() {
        return Err(bad("finite-stock objective area overflows"));
    }
    if use_.purchase_cost.is_some_and(|v| !v.is_finite()) {
        return Err(bad("finite-stock purchase objective overflows"));
    }
    Ok(use_)
}

// Save only mutable placement/area state, not another copy of material sources and stock geometry.
struct Incumbent {
    usage: Usage,
    allocations: Vec<(f64, Vec<CutPlacement>)>,
}
impl Incumbent {
    fn restore(self, markers: &mut [Marker]) {
        for (marker, (area, placements)) in markers.iter_mut().zip(self.allocations) {
            marker.external_cut_area_m2 = area;
            marker.placements = placements;
        }
    }
}

fn copy_work(markers: &[Marker]) -> Result<usize, Refusal> {
    let mut points = markers.len();
    for p in markers.iter().flat_map(|m| &m.placements) {
        points = checked(points.checked_add(p.cut_mm.len()))?;
        points = checked(points.checked_add(2))?;
        points = checked(points.checked_add(p.drills_mm.len()))?;
        for (path, _) in &p.internal_cut_mm {
            points = checked(points.checked_add(path.len()))?;
        }
    }
    Ok(points)
}

struct Progress {
    objective: Objective,
    baseline: Option<Usage>,
    best: Option<Incumbent>,
    complete: usize,
    improvements: usize,
}
impl Progress {
    fn result(&self, selected: Usage, undone: usize, termination: Termination) -> Allocation {
        Allocation {
            undone,
            optimization: Some(Optimization {
                objective: self.objective,
                // Called only after a complete candidate is known; the fallback is that candidate.
                baseline: self.baseline.unwrap_or(selected),
                selected,
                complete_layouts: self.complete,
                improvements: self.improvements,
                termination,
                lattice_optimal: termination == Termination::LatticeExhausted,
                budget_exhausted: termination != Termination::LatticeExhausted,
            }),
        }
    }

    fn finish(
        &mut self,
        markers: &mut [Marker],
        undone: usize,
        termination: Termination,
        failure: Refusal,
    ) -> Result<Allocation, Refusal> {
        let Some(best) = self.best.take() else {
            return Err(failure);
        };
        let result = self.result(best.usage, undone, termination);
        best.restore(markers);
        Ok(result)
    }
}

#[derive(Clone, Copy, Default)]
pub(super) struct Cursor {
    pub x: usize,
    pub y: usize,
}

pub(super) struct Task<'a> {
    pub shape: &'a Shape,
    pub material: &'a ResolvedMaterial,
    pub copy: usize,
    pub zone: Option<&'a str>,
    pub reversible: bool,
}

struct Frame {
    forward: CutPlacement,
    turned: Option<CutPlacement>,
    stock: usize,
    reversed: bool,
    cursor: Cursor,
}
impl Frame {
    fn new(task: &Task<'_>, work: &mut Work) -> Result<Self, Refusal> {
        // Reconsidering an earlier decision may rebuild a descendant. Charge its geometry again,
        // including annotations, rather than giving failed branches free allocation/copy work.
        let points = checked(
            task.shape
                .ring
                .len()
                .checked_add(2)
                .and_then(|n| n.checked_add(task.shape.geom.drills.len())),
        )?;
        let points = checked(
            task.shape
                .geom
                .internal_cut
                .iter()
                .try_fold(points, |sum, (p, _)| sum.checked_add(p.len())),
        )?;
        checked(work.spend(points))?;
        let forward = place(task.shape, task.copy, Vec2::ZERO)?;
        let turned = if task.reversible {
            checked(work.spend(points))?;
            Some(half_turn(&forward).ok_or_else(|| bad("grain reversal overflows"))?)
        } else {
            None
        };
        Ok(Self {
            forward,
            turned,
            stock: 0,
            reversed: false,
            cursor: Cursor::default(),
        })
    }

    fn next(
        &mut self,
        task: &Task<'_>,
        markers: &[Marker],
        options: &Options,
        work: &mut Work,
        bound: Option<(Objective, Usage)>,
        costs: Option<&[Option<f64>]>,
    ) -> Result<Option<(usize, CutPlacement)>, Refusal> {
        while let Some(marker) = markers.get(self.stock) {
            checked(work.tick())?;
            // Missing/stale/unqualified prices are not a free-stock fallback. This exclusion applies
            // even before the first complete candidate exists, not only during bound pruning.
            if costs.is_some_and(|c| c[self.stock].is_none()) {
                self.stock += 1;
                self.reversed = false;
                self.cursor = Cursor::default();
                continue;
            }
            let worth_opening = if let Some((objective, incumbent)) = bound {
                checked(work.spend(markers.len()))?;
                objective.improves(usage(markers, Some(self.stock), costs)?, incumbent)
            } else {
                true
            };
            if worth_opening && same_material(&marker.material, task.material) {
                let cut = if self.reversed {
                    self.turned
                        .as_ref()
                        .ok_or_else(|| bad("missing admitted half-turn"))?
                } else {
                    &self.forward
                };
                if let Some(p) = locate_next(
                    cut,
                    task.shape.area,
                    marker,
                    task.zone,
                    options,
                    work,
                    &mut self.cursor,
                )? {
                    return Ok(Some((self.stock, p)));
                }
                if !self.reversed && self.turned.is_some() {
                    self.reversed = true;
                    self.cursor = Cursor::default();
                    continue;
                }
            }
            self.stock += 1;
            self.reversed = false;
            self.cursor = Cursor::default();
        }
        Ok(None)
    }
}

struct Decision {
    stock: usize,
    previous_area: f64,
}

pub(super) fn allocate(
    tasks: &[Task<'_>],
    markers: &mut [Marker],
    options: &Options,
    limit: usize,
    work: &mut Work,
    costs: Option<&[Option<f64>]>,
) -> Result<Allocation, Refusal> {
    if (options.optimize == Some(Objective::MinimizePurchaseCost)) != costs.is_some()
        || costs.is_some_and(|c| {
            c.len() != markers.len() || c.iter().flatten().any(|v| !v.is_finite() || *v <= 0.0)
        })
    {
        return Err(bad(
            "purchase optimization requires admitted positive per-item costs",
        ));
    }
    let mut frames = Vec::new();
    let mut decisions: Vec<Decision> = Vec::new();
    let mut undone = 0;
    let mut progress = options.optimize.map(|objective| Progress {
        objective,
        baseline: None,
        best: None,
        complete: 0,
        improvements: 0,
    });
    loop {
        let depth = decisions.len();
        if depth == tasks.len() {
            let Some(p) = &mut progress else {
                return Ok(Allocation {
                    undone,
                    optimization: None,
                });
            };
            let selected = usage(markers, None, costs)?;
            p.complete += 1;
            p.baseline.get_or_insert(selected);
            if p.best
                .as_ref()
                .is_none_or(|b| p.objective.improves(selected, b.usage))
            {
                p.improvements += usize::from(p.best.is_some());
                // If copying the newly improved layout exceeds the work allowance, it is already
                // complete in `markers`: return it directly, with no uncharged backup allocation.
                if work.spend(copy_work(markers)?).is_none() {
                    return Ok(p.result(selected, undone, Termination::WorkLimit));
                }
                p.best = Some(Incumbent {
                    usage: selected,
                    allocations: markers
                        .iter()
                        .map(|m| (m.external_cut_area_m2, m.placements.clone()))
                        .collect(),
                });
            }
        } else {
            let task = &tasks[depth];
            let bound = progress
                .as_ref()
                .and_then(|p| p.best.as_ref().map(|b| (p.objective, b.usage)));
            let next = (|| {
                if frames.len() == depth {
                    frames.push(Frame::new(task, work)?);
                }
                frames[depth].next(task, markers, options, work, bound, costs)
            })();
            match next {
                Ok(Some((stock, placement))) => {
                    let marker = &mut markers[stock];
                    decisions.push(Decision {
                        stock,
                        previous_area: marker.external_cut_area_m2,
                    });
                    marker.external_cut_area_m2 += task.shape.area;
                    marker.placements.push(placement);
                    continue;
                }
                Ok(None) => {
                    frames.pop();
                }
                Err(e) => {
                    if e.code == RefusalCode::BudgetExceeded
                        && let Some(p) = &mut progress
                    {
                        return p.finish(markers, undone, Termination::WorkLimit, e);
                    }
                    return Err(e);
                }
            }
        }
        let Some(previous) = decisions.pop() else {
            let failure = bad(
                "no checked complete finite-stock layout in the declared lattice and grain choices; not a proof of physical infeasibility",
            );
            return match &mut progress {
                Some(p) => p.finish(markers, undone, Termination::LatticeExhausted, failure),
                None => Err(failure),
            };
        };
        if undone == limit {
            let failure = Refusal::new(
                RefusalCode::BudgetExceeded,
                "finite-stock backtrack limit exhausted; no complete plan published",
            );
            return match &mut progress {
                Some(p) => p.finish(markers, undone, Termination::BacktrackLimit, failure),
                None => Err(failure),
            };
        }
        undone += 1;
        let marker = &mut markers[previous.stock];
        marker
            .placements
            .pop()
            .ok_or_else(|| bad("lost finite-stock search decision"))?;
        // Restore the saved sum exactly; repeated subtraction would accumulate rollback roundoff.
        marker.external_cut_area_m2 = previous.previous_area;
    }
}

//! Optional finite-stock backtracking. Every candidate uses the same complete polygon checks.
//! Iterative frames bound call-stack use; failed trials never reset the shared work counter.
use ftx_core::{Refusal, RefusalCode, Vec2};
use ftx_pattern::material_catalog::ResolvedMaterial;

use super::super::Shape;
use super::{
    CutPlacement, Marker, Options, Work, bad, checked, half_turn, locate_next, place, same_material,
};

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
    ) -> Result<Option<(usize, CutPlacement)>, Refusal> {
        while let Some(marker) = markers.get(self.stock) {
            checked(work.tick())?;
            if same_material(&marker.material, task.material) {
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
) -> Result<usize, Refusal> {
    let mut frames = Vec::new();
    let mut decisions: Vec<Decision> = Vec::new();
    let mut undone = 0;
    loop {
        let depth = decisions.len();
        if depth == tasks.len() {
            return Ok(undone);
        }
        let task = &tasks[depth];
        if frames.len() == depth {
            frames.push(Frame::new(task, work)?);
        }
        if let Some((stock, placement)) = frames[depth].next(task, markers, options, work)? {
            let marker = &mut markers[stock];
            decisions.push(Decision {
                stock,
                previous_area: marker.external_cut_area_m2,
            });
            marker.external_cut_area_m2 += task.shape.area;
            marker.placements.push(placement);
            continue;
        }
        frames.pop();
        let Some(previous) = decisions.pop() else {
            return Err(bad(
                "no checked complete finite-stock layout in the declared lattice and grain choices; not a proof of physical infeasibility",
            ));
        };
        if undone == limit {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "finite-stock backtrack limit exhausted; no complete plan published",
            ));
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

//! Complete qualified cutting effort retained once by the actual manufacturing producers.
//! The private seal admits cached results without repeating nesting, geometry search or scheduling.
//! Imported JSON cannot create this receipt: `Alternative` is deliberately producer-only.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, Vec2};
use ftx_pattern::{Pattern, material_catalog::ResolvedMaterial};
use serde_json::json;

use super::{Alternative, Metrics, comparison::Consumer, finite_sum, hash, invalid};
use crate::cutting::{CutPlacement, CutPlan, job, program, stock};

const CONSUMERS: [Consumer; 3] = [
    Consumer::CuttingProgram,
    Consumer::FiniteStockProgram,
    Consumer::CuttingJobProgram,
];

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Totals {
    pub seconds: f64,
    pub cut_m: f64,
    pub travel_m: f64,
}

impl Totals {
    fn array(self) -> [f64; 3] {
        [self.seconds, self.cut_m, self.travel_m]
    }

    fn add(&mut self, other: Self) -> Result<(), Refusal> {
        self.seconds = finite_sum([self.seconds, other.seconds].into_iter())?;
        self.cut_m = finite_sum([self.cut_m, other.cut_m].into_iter())?;
        self.travel_m = finite_sum([self.travel_m, other.travel_m].into_iter())?;
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub(super) struct Receipt {
    pub totals: Totals,
    pub protocol: String,
    fingerprint: String,
}

#[derive(Clone, Debug, Default)]
pub(super) struct Receipts(BTreeMap<Consumer, Result<Receipt, Refusal>>);

#[derive(Default)]
struct Summary {
    totals: Totals,
    protocols: BTreeMap<String, BTreeSet<String>>,
    inventory: BTreeSet<job::Item>,
}

fn slots(metrics: &mut Metrics, consumer: Consumer) -> [&mut Option<f64>; 3] {
    match consumer {
        Consumer::CuttingProgram => [
            &mut metrics.cutting_modeled_path_seconds,
            &mut metrics.cutting_cut_length_m,
            &mut metrics.cutting_raised_travel_length_m,
        ],
        Consumer::FiniteStockProgram => [
            &mut metrics.finite_stock_modeled_path_seconds,
            &mut metrics.finite_stock_cut_length_m,
            &mut metrics.finite_stock_raised_travel_length_m,
        ],
        Consumer::CuttingJobProgram => [
            &mut metrics.cutting_job_modeled_path_seconds,
            &mut metrics.cutting_job_cut_length_m,
            &mut metrics.cutting_job_raised_travel_length_m,
        ],
        _ => unreachable!("cutting effort consumer"),
    }
}

fn values(metrics: &Metrics, consumer: Consumer) -> [Option<f64>; 3] {
    match consumer {
        Consumer::CuttingProgram => [
            metrics.cutting_modeled_path_seconds,
            metrics.cutting_cut_length_m,
            metrics.cutting_raised_travel_length_m,
        ],
        Consumer::FiniteStockProgram => [
            metrics.finite_stock_modeled_path_seconds,
            metrics.finite_stock_cut_length_m,
            metrics.finite_stock_raised_travel_length_m,
        ],
        Consumer::CuttingJobProgram => [
            metrics.cutting_job_modeled_path_seconds,
            metrics.cutting_job_cut_length_m,
            metrics.cutting_job_raised_travel_length_m,
        ],
        _ => unreachable!("cutting effort consumer"),
    }
}

fn fingerprint(row: &Alternative, consumer: Consumer) -> Result<String, Refusal> {
    let sources = (&row.binding, &row.frozen_changes);
    match consumer {
        Consumer::CuttingProgram => hash(&(sources, &row.cutting)),
        Consumer::FiniteStockProgram => hash(&(sources, &row.finite_stock)),
        Consumer::CuttingJobProgram => hash(&(sources, &row.cutting_job)),
        _ => unreachable!("cutting effort consumer"),
    }
}

/// Called only after actual consumers and the exact binding have completed. An independent
/// process refusal leaves geometry, pricing and every unrelated consumer untouched.
pub(super) fn capture(row: &mut Alternative, pattern: &Pattern) {
    let mut receipts = BTreeMap::new();
    for consumer in CONSUMERS {
        let receipt = seal(row, pattern, consumer);
        if let Ok(receipt) = &receipt {
            for (slot, value) in slots(&mut row.metrics, consumer)
                .into_iter()
                .zip(receipt.totals.array())
            {
                *slot = Some(value);
            }
        }
        receipts.insert(consumer, receipt);
    }
    row.cutting_effort = Receipts(receipts);
}

/// Recheck immutable source/output identity and the public metric cache, without solving again.
pub(super) fn admit(row: &Alternative, consumer: Consumer) -> Result<&Receipt, Refusal> {
    let receipt = row
        .cutting_effort
        .0
        .get(&consumer)
        .ok_or_else(|| invalid("qualified cutting comparison lacks its actual producer receipt"))?
        .as_ref()
        .map_err(Clone::clone)?;
    if fingerprint(row, consumer)? != receipt.fingerprint
        || values(&row.metrics, consumer) != receipt.totals.array().map(Some)
    {
        return Err(invalid(
            "qualified cutting source, complete output or metric cache changed after production",
        ));
    }
    Ok(receipt)
}

fn seal(row: &Alternative, pattern: &Pattern, consumer: Consumer) -> Result<Receipt, Refusal> {
    let expected = row
        .binding
        .value()
        .and_then(|bound| bound["pattern_sha256"].as_str())
        .ok_or_else(|| invalid("qualified cutting needs an exact bound pattern"))?;
    let (summary, copies) = match consumer {
        Consumer::CuttingProgram => {
            let plan = row
                .cutting
                .value()
                .ok_or_else(|| invalid("roll cutting unavailable"))?;
            if plan.source_piece_ids.is_some() {
                return Err(invalid(
                    "standalone roll effort cannot admit a partial source-piece plan",
                ));
            }
            (roll(plan, expected)?, plan.options.copies)
        }
        Consumer::FiniteStockProgram => {
            let plan = row
                .finite_stock
                .value()
                .ok_or_else(|| invalid("finite-stock cutting unavailable"))?;
            if plan.source_piece_ids().is_some() {
                return Err(invalid(
                    "standalone finite-stock effort cannot admit a partial source-piece plan",
                ));
            }
            (finite(plan, expected)?, plan.request().options.copies)
        }
        Consumer::CuttingJobProgram => {
            let plan = row
                .cutting_job
                .value()
                .ok_or_else(|| invalid("complete cutting job unavailable"))?;
            if !plan.complete_inventory
                || plan.hardware_execution_authorized
                || plan.pattern_sha256 != expected
            {
                return Err(invalid(
                    "qualified cutting job needs complete inventory and the exact bound pattern",
                ));
            }
            let mut combined = Summary::default();
            let mut routes = Vec::new();
            if let Some(plan) = &plan.roll {
                routes.push(roll(plan, expected)?);
            }
            if let Some(plan) = &plan.finite_stock {
                routes.push(finite(plan, expected)?);
            }
            for route in routes {
                combined.totals.add(route.totals)?;
                for (piece, protocols) in route.protocols {
                    combined
                        .protocols
                        .entry(piece)
                        .or_default()
                        .extend(protocols);
                }
                for item in route.inventory {
                    if !combined.inventory.insert(item) {
                        return Err(invalid(
                            "qualified cutting job duplicates a physical cut item",
                        ));
                    }
                }
            }
            let inventory: BTreeSet<_> = plan.inventory.iter().cloned().collect();
            if inventory.len() != plan.inventory.len() || inventory != combined.inventory {
                return Err(invalid(
                    "qualified cutting programs do not cover the complete routed job inventory",
                ));
            }
            (combined, plan.request.copies())
        }
        _ => unreachable!("cutting effort consumer"),
    };
    let actual: BTreeSet<_> = summary
        .inventory
        .iter()
        .map(|item| (item.piece.clone(), item.instance.clone(), item.copy))
        .collect();
    let complete: BTreeSet<_> = pattern
        .instances
        .iter()
        .flat_map(|instance| {
            (0..copies).map(move |copy| (instance.piece.clone(), instance.id.clone(), copy))
        })
        .collect();
    if actual.is_empty() || actual.len() != summary.inventory.len() || actual != complete {
        return Err(invalid(
            "qualified cutting effort requires every physical instance and copy exactly once",
        ));
    }
    Ok(Receipt {
        totals: summary.totals,
        protocol: hash(&summary.protocols)?,
        fingerprint: fingerprint(row, consumer)?,
    })
}

fn roll(plan: &CutPlan, expected: &str) -> Result<Summary, Refusal> {
    if plan.pattern_sha256 != expected {
        return Err(invalid(
            "roll knife plan differs from the exact bound pattern",
        ));
    }
    let report = plan.program.as_ref().ok_or_else(|| {
        plan.program_refusal
            .clone()
            .unwrap_or_else(|| invalid("complete qualified roll knife program unavailable"))
    })?;
    let policy = plan
        .options
        .program
        .as_ref()
        .ok_or_else(|| invalid("roll knife policy unavailable"))?;
    summarize(
        report,
        policy,
        "ftx-knife-cut-program/1",
        job::StockKind::Roll,
        plan.rolls
            .iter()
            .enumerate()
            .map(|(i, marker)| (i, &marker.material, marker.placements.as_slice())),
        |index| {
            let marker = &plan.rolls[index];
            let width = marker
                .material
                .material()
                .properties
                .get("width")
                .ok_or_else(|| invalid("knife source stock width is missing"))?
                .bounds
                .map_or(marker.width_m, |bounds| bounds[1]);
            Ok([0., 0., width * 1000., marker.length_m * 1000.])
        },
    )
}

fn finite(plan: &stock::Plan, expected: &str) -> Result<Summary, Refusal> {
    if plan.pattern_sha256() != expected {
        return Err(invalid(
            "finite-stock knife plan differs from the exact bound pattern",
        ));
    }
    let report = plan.program().ok_or_else(|| {
        plan.program_refusal()
            .cloned()
            .unwrap_or_else(|| invalid("complete qualified finite-stock knife program unavailable"))
    })?;
    let policy = plan
        .request()
        .options
        .program
        .as_ref()
        .ok_or_else(|| invalid("finite-stock knife policy unavailable"))?;
    summarize(
        report,
        policy,
        "ftx-knife-cut-program/2",
        job::StockKind::FiniteStock,
        plan.markers()
            .iter()
            .enumerate()
            .filter(|(_, marker)| !marker.placements.is_empty())
            .map(|(i, marker)| (i, &marker.material, marker.placements.as_slice())),
        |index| {
            Ok(plan.markers()[index].outline_mm.iter().fold(
                [
                    f64::INFINITY,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    f64::NEG_INFINITY,
                ],
                |[xmin, ymin, xmax, ymax], p| {
                    [xmin.min(p.x), ymin.min(p.y), xmax.max(p.x), ymax.max(p.y)]
                },
            ))
        },
    )
}

fn near(a: f64, b: f64) -> bool {
    a.is_finite() && b.is_finite() && (a - b).abs() <= 1e-10 * a.abs().max(b.abs()).max(1.)
}

fn summarize<'a>(
    report: &program::Report,
    policy: &program::Policy,
    schema: &str,
    stock: job::StockKind,
    markers: impl Iterator<Item = (usize, &'a ResolvedMaterial, &'a [CutPlacement])>,
    stock_bounds: impl Fn(usize) -> Result<[f64; 4], Refusal>,
) -> Result<Summary, Refusal> {
    let markers: BTreeMap<_, _> = markers
        .map(|(i, material, placements)| (i, (material, placements)))
        .collect();
    if report.schema != schema
        || report.hardware_execution_authorized
        || report.physical_cutting_validated
        || report.optimality_proven
        || report.machine_sha256 != program::machine_sha256(&policy.machine)?
        || report.motion_profile != policy.machine.motion_profile
        || hash(&report.drilling)? != hash(&policy.machine.drilling)?
        || hash(&report.fixture_clearance)? != hash(&policy.machine.fixture_clearance)?
        || hash(&report.travel_routing)? != hash(&policy.travel_routing)?
        || report.stock_loading != policy.stock_loading
        || markers.is_empty()
        || markers.len() != report.markers.len()
    {
        return Err(invalid(
            "knife effort needs the complete source-qualified modeled program",
        ));
    }
    let mut summary = Summary::default();
    let mut seen = BTreeSet::new();
    for marker in &report.markers {
        let &(material, placements) = markers
            .get(&marker.marker)
            .ok_or_else(|| invalid("knife program names an unknown final marker"))?;
        if !seen.insert(marker.marker) || marker.material_sha256 != material.sha256() {
            return Err(invalid(
                "knife program repeats or changes its exact material marker",
            ));
        }
        let bounds = stock_bounds(marker.marker)?;
        let mut loadings = policy
            .stock_loading
            .iter()
            .filter(|item| item.marker == marker.marker);
        let loading = loadings.next();
        let origin = loading.map_or(Vec2::ZERO, |item| item.lower_left_mm);
        let offset = match stock {
            job::StockKind::FiniteStock => Some(if loading.is_some() {
                origin - Vec2::new(bounds[0], bounds[1])
            } else {
                Vec2::new(-bounds[0], -bounds[1])
            }),
            job::StockKind::Roll => loading.map(|_| origin),
        };
        let expected_loading = loading.map(|_| program::Loading {
            lower_left_mm: origin,
            marker_to_machine_offset_mm: offset.unwrap(),
            stock_bounds_mm: [
                origin.x,
                origin.y,
                origin.x + (bounds[2] - bounds[0]),
                origin.y + (bounds[3] - bounds[1]),
            ],
        });
        if bounds.iter().any(|value| !value.is_finite())
            || bounds[2] <= bounds[0]
            || bounds[3] <= bounds[1]
            || marker.loading != expected_loading
            || loadings.next().is_some()
            || (stock == job::StockKind::FiniteStock
                && marker
                    .finite_stock
                    .as_ref()
                    .map(|frame| frame.marker_to_machine_offset_mm)
                    != offset)
            || (stock == job::StockKind::Roll && marker.finite_stock.is_some())
        {
            return Err(invalid(
                "knife effort lost its authored whole-stock loading frame",
            ));
        }
        let mut candidates = policy.qualifications.iter().filter(|q| {
            q.id == marker.qualification
                && q.material_sha256 == marker.material_sha256
                && q.machine_sha256 == report.machine_sha256
        });
        let qualification = candidates.next().ok_or_else(|| {
            invalid("knife source qualification does not match the actual marker")
        })?;
        if candidates.next().is_some() {
            return Err(invalid("ambiguous knife source qualification"));
        }
        let record = material.material();
        let thickness = record
            .properties
            .get("unloaded_thickness")
            .or_else(|| record.properties.get("thickness"))
            .ok_or_else(|| invalid("consumed knife clearance observation unavailable"))?;
        // Specimen identity and source references are sealed above, not experimental protocols.
        // Feed and thickness values may legitimately differ between the compared materials.
        let conditions: BTreeMap<_, _> = thickness
            .conditions
            .iter()
            .filter(|(key, _)| key.as_str() != "specimen_id")
            .collect();
        let process_conditions: BTreeMap<_, _> = qualification
            .conditions
            .iter()
            .filter(|(key, _)| key.as_str() != "specimen_id")
            .collect();
        let mut protocol = json!({"model":policy.model,"machine":policy.machine,
            "method":qualification.method,"conditions":process_conditions,"directions":qualification.directions,
            "evidence":qualification.evidence,"clearance":{"unit":thickness.unit,"method":thickness.method,
            "conditions":conditions,"evidence":thickness.evidence}});
        if let Some(routing) = &policy.travel_routing {
            protocol["travel_routing"] = json!(routing);
        }
        if !policy.stock_loading.is_empty() {
            protocol["stock_loading"] = json!(policy.stock_loading);
        }
        let expected_drills: Vec<_> = placements
            .iter()
            .enumerate()
            .flat_map(|(placement, item)| {
                (0..item.drills_mm.len()).map(move |drill| (placement, drill))
            })
            .collect();
        let drilling = if expected_drills.is_empty() {
            if marker.drill_qualification.is_some() {
                return Err(invalid(
                    "a marker without holes cannot claim a drilling qualification",
                ));
            }
            None
        } else {
            let tool = policy.machine.drilling.as_ref().ok_or_else(|| {
                invalid("complete cutting effort requires an explicit tool for every drill hole")
            })?;
            let qualified = qualification.drilling.as_ref().ok_or_else(|| {
                invalid("complete cutting effort requires independently qualified drilling")
            })?;
            if marker.drill_qualification.as_deref() != Some(qualified.id.as_str()) {
                return Err(invalid(
                    "drilling qualification differs from the actual marker",
                ));
            }
            let conditions: BTreeMap<_, _> = qualified
                .conditions
                .iter()
                .filter(|(key, _)| key.as_str() != "specimen_id")
                .collect();
            // Actual source identities and rates are sealed in the producer receipt. Different
            // materials may have different cycle times under one declared measurement protocol.
            protocol["drilling"] = json!({"method":qualified.method,
                "conditions":conditions,"evidence":qualified.evidence});
            Some((tool, qualified))
        };
        let protocol = hash(&protocol)?;
        let expected: BTreeSet<_> = placements
            .iter()
            .enumerate()
            .flat_map(|(i, placement)| {
                (0..placement.internal_cut_mm.len())
                    .map(move |feature| (i, Some(feature)))
                    .chain(std::iter::once((i, None)))
            })
            .collect();
        let route = &marker.selected;
        if route.drills.len() != expected_drills.len() {
            return Err(invalid(
                "cutting effort must retain every declared drill hole exactly once",
            ));
        }
        let (drilling_seconds, tool_change_seconds) = if let Some((tool, qualified)) = drilling {
            for (run, &(placement_index, drill_index)) in route.drills.iter().zip(&expected_drills)
            {
                let placement = &placements[placement_index];
                let (center, diameter) = placement.drills_mm[drill_index];
                let center = offset.map_or(center, |offset| center + offset);
                if run.placement != placement_index
                    || run.drill != drill_index
                    || run.piece != placement.piece
                    || run.instance != placement.instance
                    || run.copy != placement.copy
                    || run.center_mm != center
                    || run.diameter_mm != diameter
                    || run.diameter_mm != tool.diameter_mm
                    || !center.is_finite()
                    || !diameter.is_finite()
                    || diameter <= 0.
                {
                    return Err(invalid(
                        "drilling must retain exact physical copies, authored hole order, coordinates and diameters",
                    ));
                }
            }
            (
                finite_sum(std::iter::repeat_n(
                    qualified.cycle_seconds()?,
                    route.drills.len(),
                ))?,
                finite_sum([tool.select_drill_seconds, tool.select_knife_seconds].into_iter())?,
            )
        } else {
            (0., 0.)
        };
        let mut paths = BTreeSet::new();
        let mut exterior_started = false;
        let mut accelerated_cutting_seconds = 0.;
        for run in &route.runs {
            let placement = placements
                .get(run.path.placement)
                .ok_or_else(|| invalid("knife path names an unknown placement"))?;
            let (points, closed) = match run.path.internal_cut {
                Some(feature) => placement
                    .internal_cut_mm
                    .get(feature)
                    .map(|(points, closed)| (points.as_slice(), *closed))
                    .ok_or_else(|| invalid("knife path names an unknown internal cut"))?,
                None => (placement.cut_mm.as_slice(), true),
            };
            let start = *points
                .first()
                .ok_or_else(|| invalid("knife path is empty"))?;
            let end = if closed {
                start
            } else {
                *points.last().unwrap()
            };
            let loaded = |point: Vec2| offset.map_or(point, |offset| point + offset);
            if !paths.insert((run.path.placement, run.path.internal_cut))
                || run.piece != placement.piece
                || run.instance != placement.instance
                || run.copy != placement.copy
                || run.closed != closed
                || run.cut_endpoints_mm != [loaded(start), loaded(end)]
                || !run.cut_length_mm.is_finite()
                || run.cut_length_mm <= 0.
                || (run.path.internal_cut.is_some() && exterior_started)
            {
                return Err(invalid(
                    "knife paths must retain complete unique physical parts and internal-before-exterior order",
                ));
            }
            exterior_started |= run.path.internal_cut.is_none();
            if policy.machine.motion_profile.is_some() {
                accelerated_cutting_seconds = finite_sum(
                    [
                        accelerated_cutting_seconds,
                        policy
                            .machine
                            .cutting_seconds(points, closed, qualification.feed_mm_s)?,
                    ]
                    .into_iter(),
                )?;
            }
        }
        let cut = finite_sum(route.runs.iter().map(|run| run.cut_length_mm))?;
        let travel = route.checked_travel_length()?;
        let (cutting_seconds, travel_seconds) = if policy.machine.motion_profile.is_some() {
            (
                accelerated_cutting_seconds,
                policy.machine.travel_seconds(route)?,
            )
        } else {
            (
                cut / qualification.feed_mm_s,
                if route.drills.is_empty() {
                    travel / policy.machine.travel_mm_s
                } else {
                    policy.machine.travel_seconds(route)?
                },
            )
        };
        if paths != expected
            || paths.is_empty()
            || route
                .drills
                .first()
                .map(|run| run.travel_mm[0])
                .or_else(|| route.runs.first().map(|run| run.travel_mm[0]))
                != Some(policy.machine.start_mm)
            || route.return_mm.is_some() != policy.machine.return_to_start
            || (policy.travel_routing.is_none()
                && (!route.return_waypoints_mm.is_empty()
                    || route
                        .drills
                        .iter()
                        .any(|run| !run.travel_waypoints_mm.is_empty())
                    || route
                        .runs
                        .iter()
                        .any(|run| !run.travel_waypoints_mm.is_empty())))
            || !near(cut, route.cut_length_mm)
            || !near(travel, route.travel_length_mm)
            || !near(cutting_seconds, route.cutting_seconds)
            || !near(travel_seconds, route.travel_seconds)
            || !near(drilling_seconds, route.drilling_seconds)
            || !near(tool_change_seconds, route.tool_change_seconds)
            || !near(
                route.runs.len() as f64 * policy.machine.lift_lower_seconds,
                route.lift_lower_seconds,
            )
            || !near(
                route.cutting_seconds
                    + route.travel_seconds
                    + route.lift_lower_seconds
                    + route.drilling_seconds
                    + route.tool_change_seconds,
                route.modeled_path_seconds,
            )
        {
            return Err(invalid(
                "knife effort requires every exact path and its complete declared timing phases",
            ));
        }
        summary.totals.add(Totals {
            seconds: route.modeled_path_seconds,
            cut_m: cut / 1000.,
            travel_m: travel / 1000.,
        })?;
        for placement in placements {
            if !summary.inventory.insert(job::Item {
                piece: placement.piece.clone(),
                instance: placement.instance.clone(),
                copy: placement.copy,
                stock,
                material_sha256: marker.material_sha256.clone(),
            }) {
                return Err(invalid(
                    "knife marker inventory duplicates a physical cut item",
                ));
            }
            summary
                .protocols
                .entry(placement.piece.clone())
                .or_default()
                .insert(protocol.clone());
        }
    }
    Ok(summary)
}

#[cfg(test)]
mod tests;

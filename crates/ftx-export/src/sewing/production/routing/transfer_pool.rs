//! Independently qualified unloaded carriers share a route without sharing an exclusive clock.
//! Source durations include positioning/return; no worker ownership or implicit travel is inferred.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::Refusal;
use ftx_pattern::materials::{Evidence, Source};
use serde::{Deserialize, Serialize};

use super::{
    MAX_EXACT_MILLISECONDS, Policy as RoutePolicy, Timeline, TransferPolicy, add, source_valid,
};
use crate::sewing::bad;

pub(super) const MAX_RESOURCES: usize = 8;

/// Explicit simultaneous independence of the primary carrier and up to seven additional carriers.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    /// `independent_unloaded_transfer_resources/1`.
    pub model: String,
    /// Authored carrier order breaks equal-score ties after the primary resource. Each carrier
    /// qualifies a nonempty subset of the route's directed edges and its own complete occupancy.
    pub additional_resources: Vec<TransferPolicy>,
    /// Evidence for simultaneous independence, distinct physical identities and unloaded access.
    pub source: Source,
    /// Qualification method, separate from each carrier's observed complete handoff durations.
    pub method: String,
    /// Source conditions retained without extrapolation.
    #[serde(default)]
    pub conditions: BTreeMap<String, String>,
    /// Independent classification of the pool's applicability evidence.
    pub evidence: Evidence,
    /// Separate consent for estimated simultaneous independence.
    #[serde(default)]
    pub allow_estimates: bool,
}

/// Independently reconstructed use of one carrier, including explicitly admitted idle resources.
#[derive(Clone, Debug, Serialize)]
pub struct Usage {
    /// Exact primary or additional resource ID.
    pub resource: String,
    /// Complete handoffs assigned to this carrier.
    pub handoffs: usize,
    /// Sum of qualified occupancy, separate from elapsed completion and inactive gaps.
    pub occupied_milliseconds: u64,
    /// Earliest occupied start; absent for an unused carrier.
    pub first_start_milliseconds: Option<u64>,
    /// Latest complete finish; absent for an unused carrier.
    pub last_finish_milliseconds: Option<u64>,
}

/// Original qualifications and complete per-carrier replay, without a physical factory claim.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// `ftx-sewing-transfer-pool/1`.
    pub schema: &'static str,
    /// Exact additional carrier policies and their common independence qualification.
    pub policy: Policy,
    /// Exact full-domain primary policy used by the copy-major seed.
    pub primary: TransferPolicy,
    /// Complete retained baseline, which may be a searched replacement of an unavailable seed.
    pub baseline: Vec<Usage>,
    /// Selected complete timeline, checked independently for carrier exclusivity and source timing.
    pub selected: Vec<Usage>,
    /// Software replay does not validate measured simultaneous access or factory execution.
    pub physical_pool_validated: bool,
}

impl Policy {
    pub(super) fn validate(
        &self,
        route: &RoutePolicy,
        cells: &BTreeSet<&str>,
        required: &BTreeSet<(&str, &str)>,
    ) -> Result<(), Refusal> {
        if self.model != "independent_unloaded_transfer_resources/1"
            || !(1..MAX_RESOURCES).contains(&self.additional_resources.len())
            || route.transfer.attending_worker.is_some()
            || !source_valid(&self.source, &self.method, &self.conditions)
        {
            return Err(bad(
                "transfer pools need a qualified independent model and one to seven additional unloaded carriers; worker ownership is not admitted",
            ));
        }
        if self.evidence == Evidence::Estimate && !self.allow_estimates {
            return Err(bad(
                "estimated transfer independence needs its own explicit consent",
            ));
        }
        let mut identities = cells.clone();
        identities.extend(
            route
                .attending_labor
                .iter()
                .flat_map(|labor| &labor.workers)
                .map(|worker| worker.id.as_str()),
        );
        for carrier in route.transfer_resources() {
            if carrier.attending_worker.is_some() || !identities.insert(carrier.id.as_str()) {
                return Err(bad(
                    "independent transfer carriers need distinct cell, worker and resource identities without shared worker ownership",
                ));
            }
        }
        for carrier in &self.additional_resources {
            let supplied = carrier.validate_resource(cells)?;
            if supplied.is_empty() || !supplied.is_subset(required) {
                return Err(bad(
                    "each additional carrier must qualify a nonempty subset of the route's directed handoff edges",
                ));
            }
        }
        Ok(())
    }

    fn usage(&self, route: &RoutePolicy, timeline: &Timeline) -> Result<Vec<Usage>, Refusal> {
        let carriers: Vec<_> = route.transfer_resources().collect();
        let mut usage: Vec<_> = carriers
            .iter()
            .map(|carrier| Usage {
                resource: carrier.id.clone(),
                handoffs: 0,
                occupied_milliseconds: 0,
                first_start_milliseconds: None,
                last_finish_milliseconds: None,
            })
            .collect();
        let mut intervals = vec![Vec::new(); carriers.len()];
        let operations: BTreeMap<_, _> = timeline
            .operations
            .iter()
            .map(|operation| ((operation.copy, operation.operation_id.as_str()), operation))
            .collect();
        if operations.len() != timeline.operations.len() {
            return Err(bad("transfer-pool replay duplicates an operation identity"));
        }
        let mut arrivals = BTreeSet::new();
        let mut total = 0;
        for transfer in &timeline.transfers {
            let carrier_index = carriers
                .iter()
                .position(|carrier| transfer.resource.as_deref() == Some(carrier.id.as_str()))
                .ok_or_else(|| bad("transfer-pool replay omits or changes the actual carrier"))?;
            let carrier = carriers[carrier_index];
            let source = operations
                .get(&(transfer.copy, transfer.from_operation.as_str()))
                .ok_or_else(|| bad("transfer-pool replay omits its source operation"))?;
            let destination = operations
                .get(&(transfer.copy, transfer.to_operation.as_str()))
                .ok_or_else(|| bad("transfer-pool replay omits its destination operation"))?;
            let start = transfer.start_milliseconds;
            let finish = transfer.finish_milliseconds;
            if start > finish
                || finish > MAX_EXACT_MILLISECONDS
                || start < u64::from(carrier.available_at_milliseconds)
                || carrier
                    .available_until_milliseconds
                    .is_some_and(|end| finish > end)
                || source.cell != transfer.from
                || destination.cell != transfer.to
                || source.route_index.checked_add(1) != Some(destination.route_index)
                || start < source.finish_milliseconds
                || finish > destination.loading_start_milliseconds
                || !arrivals.insert((transfer.copy, transfer.to_operation.as_str()))
                || !carrier.transfers.iter().any(|edge| {
                    edge.from == transfer.from
                        && edge.to == transfer.to
                        && u64::from(edge.milliseconds) == finish - start
                })
            {
                return Err(bad(
                    "transfer-pool replay changed a qualified complete handoff",
                ));
            }
            if let Some(calendars) = &route.availability_calendars
                && calendars.earliest_start(&carrier.id, start, finish - start)? != Some(start)
            {
                return Err(bad(
                    "transfer-pool replay crosses a carrier calendar closure",
                ));
            }
            let row = &mut usage[carrier_index];
            row.handoffs += 1;
            row.occupied_milliseconds = add(row.occupied_milliseconds, finish - start)?;
            row.first_start_milliseconds = Some(
                row.first_start_milliseconds
                    .map_or(start, |old| old.min(start)),
            );
            row.last_finish_milliseconds = Some(
                row.last_finish_milliseconds
                    .map_or(finish, |old| old.max(finish)),
            );
            total = add(total, finish - start)?;
            if start < finish {
                intervals[carrier_index].push((start, finish));
            }
        }
        for operation in &timeline.operations {
            let arrival = arrivals.contains(&(operation.copy, operation.operation_id.as_str()));
            let needs_handoff = if operation.route_index == 0 {
                false
            } else {
                let previous = route
                    .route
                    .get(operation.route_index - 1)
                    .and_then(|step| operations.get(&(operation.copy, step.operation.as_str())))
                    .ok_or_else(|| {
                        bad("transfer-pool replay changed the complete garment route")
                    })?;
                previous.cell != operation.cell
            };
            if arrival != needs_handoff {
                return Err(bad(
                    "transfer-pool replay adds or omits an actual inter-cell move",
                ));
            }
        }
        for resource in &mut intervals {
            resource.sort_unstable();
            if resource.windows(2).any(|pair| pair[0].1 > pair[1].0) {
                return Err(bad("transfer-pool replay overlaps one exclusive carrier"));
            }
        }
        if total != timeline.transfer_milliseconds {
            return Err(bad(
                "transfer-pool replay changed total occupied handoff work",
            ));
        }
        Ok(usage)
    }

    pub(super) fn report(
        &self,
        route: &RoutePolicy,
        baseline: &Timeline,
        selected: &Timeline,
    ) -> Result<Report, Refusal> {
        Ok(Report {
            schema: "ftx-sewing-transfer-pool/1",
            policy: self.clone(),
            primary: route.transfer.clone(),
            baseline: self.usage(route, baseline)?,
            selected: self.usage(route, selected)?,
            physical_pool_validated: false,
        })
    }
}

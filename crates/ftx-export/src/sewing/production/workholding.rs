//! Qualified finite fixture lifecycles on one machine. Source evidence admits the declared holds;
//! occupancy enforcement does not establish fixture geometry, seam access or hardware permission.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use ftx_pattern::materials::{Evidence, Source};
use serde::{Deserialize, Serialize};

use super::super::{
    SewPlan, SewingMachine, SewingQualification, bad, ordering, qualified_text, sha256_text,
};
use super::Operation;

const MAX_RESOURCES: usize = 16;
const MAX_HOLDS: usize = 64;
const MAX_EXPANDED_HOLDS: usize = 1024;
const MAX_QUALIFICATIONS: usize = 256;
const MAX_UNITS: u32 = 4096;

/// Exact independently observed machine/process domain for a finite set of reusable fixtures.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    /// Canonical digest of the complete declared machine, not merely a reused machine ID.
    pub machine_sha256: String,
    /// Admitted source qualifications by original operation ID. Every operation needs coverage;
    /// alternatives permit explicitly supported recipe choices and material stacks.
    pub qualifications: BTreeMap<String, Vec<QualificationRef>>,
    /// Finite observed inventory; no resource is inferred from its name.
    pub resources: Vec<Resource>,
    /// Each lifecycle is repeated independently for every physical garment copy.
    pub holds: Vec<Hold>,
    /// Source covering this inventory and its declared applicability/lifecycles.
    pub source: Source,
    /// Acquisition/qualification protocol, including what workholding was actually assessed.
    pub method: String,
    /// Retained applicability conditions; not measurements from live machine sensors.
    #[serde(default)]
    pub conditions: BTreeMap<String, String>,
    /// Evidence class of this resource policy, independent of sewing and setup-time evidence.
    pub evidence: Evidence,
    /// Separate explicit permission for estimated fixture inventory/applicability.
    #[serde(default)]
    pub allow_estimates: bool,
}

/// Full content lock for an existing sewing qualification. A matching ID alone is insufficient.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationRef {
    /// Exact ID in the owning sewing options, also used by the admitted operation.
    pub id: String,
    /// Digest includes ordered material identities, process limits, machine ID and source evidence.
    pub sha256: String,
}

/// Named reusable fixture/tool inventory. Availability is integer and finite, never a reservation.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resource {
    /// Exact source resource identity.
    pub id: String,
    /// Available units, in 1..=4096.
    pub capacity: u32,
}

/// Per-copy resource lifecycle, acquired before an operation and released after another completes.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hold {
    /// Unique lifecycle identity, independent of the resource identity.
    pub id: String,
    /// Existing resource ID.
    pub resource: String,
    /// Units held continuously, in 1..=4096.
    pub units: u32,
    /// Original operation ID before which availability is required.
    pub acquire: String,
    /// Original operation ID after which units become available again. May equal `acquire`.
    pub release: String,
}

/// One physical copy's lifecycle, using the same scoped identities as the sewing schedule.
#[derive(Clone, Debug, Serialize)]
pub struct ExpandedHold {
    /// Unambiguous `copy/hold_id` identity.
    pub id: String,
    /// Zero-based physical garment copy.
    pub copy: usize,
    /// Original lifecycle ID retained for source lookup.
    pub hold_id: String,
    /// Source resource ID.
    pub resource: String,
    /// Exact integer units held.
    pub units: u32,
    /// Scoped acquisition operation.
    pub acquire: String,
    /// Scoped release operation, never before acquisition.
    pub release: String,
}

/// Resource state at one complete operation boundary; no unmodeled handling duration is assigned.
#[derive(Clone, Debug, Serialize)]
pub struct Step {
    /// Scoped physical sewing operation.
    pub operation: String,
    /// Occupancy immediately before acquiring this operation's holds.
    pub occupied_before: BTreeMap<String, u32>,
    /// Scoped holds acquired before operation start.
    pub acquired: Vec<String>,
    /// Peak state during this operation, before any completion-time release.
    pub occupied_during: BTreeMap<String, u32>,
    /// Scoped holds released after operation completion.
    pub released: Vec<String>,
    /// Occupancy after completion-time releases.
    pub occupied_after: BTreeMap<String, u32>,
}

/// Complete resource-feasible traversal and peak integer occupancies.
#[derive(Clone, Debug, Serialize)]
pub struct Timeline {
    /// Every complete operation exactly once, in the corresponding schedule order.
    pub operations: Vec<Step>,
    /// Maximum occupancy during operation execution, including single-operation holds.
    pub peak_occupancy: BTreeMap<String, u32>,
}

/// Source-qualified finite resource enforcement, not geometric fixture or seam-access validation.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// `ftx-sewing-workholding/1`.
    pub schema: &'static str,
    /// Admitted complete machine configuration digest.
    pub machine_sha256: String,
    /// Actual admitted qualification/content lock for each original source operation.
    pub qualifications: BTreeMap<String, QualificationRef>,
    /// All declared finite resources, including their exact capacities.
    pub resources: Vec<Resource>,
    /// Every repeated physical hold and its scoped start/completion boundaries.
    pub holds: Vec<ExpandedHold>,
    /// Admitted serial-copy baseline, established before any objective shortcut or pruning.
    pub baseline: Timeline,
    /// Complete selected schedule using the same resource authority as search admission.
    pub selected: Timeline,
    /// Resource accounting does not establish geometric fixture fit or physical seam access.
    pub fixture_geometry_and_access_validated: bool,
}

fn fingerprint<T: Serialize>(value: &T) -> Result<String, Refusal> {
    // Value uses sorted object keys; typed serialization supplies explicit defaults and field types.
    let value = serde_json::to_value(value).map_err(|e| bad(e.to_string()))?;
    Ok(sha256_hex(
        &serde_json::to_vec(&value).map_err(|e| bad(e.to_string()))?,
    ))
}

/// Compute the content lock used by [`Policy::machine_sha256`] from the complete typed machine.
/// Hashes do not themselves validate source evidence or make an invalid machine admissible.
///
/// # Errors
/// The machine cannot be serialized as JSON.
pub fn machine_sha256(machine: &SewingMachine) -> Result<String, Refusal> {
    fingerprint(machine)
}

/// Compute a full qualification lock, including its exact ordered material stack and source.
/// Hashes do not replace ordinary material/process qualification or estimate permissions.
///
/// # Errors
/// The qualification cannot be serialized as JSON.
pub fn qualification_sha256(qualification: &SewingQualification) -> Result<String, Refusal> {
    fingerprint(qualification)
}

fn budget() -> Refusal {
    Refusal::new(
        RefusalCode::BudgetExceeded,
        "workholding supports 1..16 resources, 1..64 source holds, at most 1024 expanded holds and 256 qualification references",
    )
}

impl Policy {
    fn validate(&self, plan: &SewPlan) -> Result<BTreeMap<String, QualificationRef>, Refusal> {
        let reference_count: usize = self.qualifications.values().map(Vec::len).sum();
        if self.resources.is_empty()
            || self.resources.len() > MAX_RESOURCES
            || self.holds.is_empty()
            || self.holds.len() > MAX_HOLDS
            || self.qualifications.len() > 63
            || reference_count > MAX_QUALIFICATIONS
        {
            return Err(budget());
        }
        if !sha256_text(&self.machine_sha256)
            || self.machine_sha256 != machine_sha256(&plan.options.machine)?
            || !qualified_text(&self.source.reference)
            || !qualified_text(&self.source.accessed_on)
            || !qualified_text(&self.source.note)
            || !qualified_text(&self.method)
            || self.conditions.len() > 64
            || self
                .conditions
                .iter()
                .any(|(key, value)| !qualified_text(key) || !qualified_text(value))
        {
            return Err(bad(
                "invalid workholding source metadata or full machine content lock",
            ));
        }
        if self.evidence == Evidence::Estimate && !self.allow_estimates {
            return Err(bad("estimated workholding needs its own explicit consent"));
        }
        let mut resources = BTreeSet::new();
        for resource in &self.resources {
            if !qualified_text(&resource.id)
                || !(1..=MAX_UNITS).contains(&resource.capacity)
                || !resources.insert(resource.id.as_str())
            {
                return Err(bad("invalid or duplicate finite workholding resource"));
            }
        }
        let operations: BTreeSet<_> = plan.seams.iter().map(|s| s.pass.operation_id()).collect();
        let mut holds = BTreeSet::new();
        for hold in &self.holds {
            if !qualified_text(&hold.id)
                || !holds.insert(hold.id.as_str())
                || !resources.contains(hold.resource.as_str())
                || !(1..=MAX_UNITS).contains(&hold.units)
                || !operations.contains(hold.acquire.as_str())
                || !operations.contains(hold.release.as_str())
            {
                return Err(bad(
                    "invalid workholding hold, resource or operation anchor",
                ));
            }
        }
        if self
            .qualifications
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>()
            != operations
        {
            return Err(bad(
                "workholding qualification coverage must name every original operation exactly",
            ));
        }
        let qualified: BTreeMap<_, _> = plan
            .options
            .qualifications
            .iter()
            .map(|q| (q.id.as_str(), q))
            .collect();
        let mut admitted = BTreeMap::new();
        for seam in &plan.seams {
            let operation = seam.pass.operation_id();
            let references = &self.qualifications[operation];
            if references.is_empty() {
                return Err(bad(
                    "workholding requires an actual admitted sewing qualification for every operation",
                ));
            }
            let mut seen = BTreeSet::new();
            for reference in references {
                let Some(qualification) = qualified.get(reference.id.as_str()) else {
                    return Err(bad("workholding names an absent sewing qualification"));
                };
                if !sha256_text(&reference.sha256)
                    || !seen.insert(reference.id.as_str())
                    || reference.sha256 != qualification_sha256(qualification)?
                {
                    return Err(bad(
                        "workholding sewing qualification content lock mismatch or duplicate reference",
                    ));
                }
            }
            let reference = seam
                .qualification_id
                .as_ref()
                .and_then(|id| references.iter().find(|r| &r.id == id));
            let Some(reference) = reference else {
                return Err(bad(format!(
                    "{operation}: actual admitted sewing qualification is not covered by workholding"
                )));
            };
            admitted.insert(operation.to_string(), reference.clone());
        }
        Ok(admitted)
    }
}

pub(super) struct Compiled {
    pub(super) limits: ordering::ResourceLimits,
    policy: Policy,
    qualifications: BTreeMap<String, QualificationRef>,
    holds: Vec<ExpandedHold>,
    baseline: Timeline,
}

impl Compiled {
    fn timeline(&self, order: &[String], baseline: bool) -> Result<Timeline, Refusal> {
        let ids: BTreeMap<_, _> = self
            .limits
            .ids
            .iter()
            .enumerate()
            .map(|(i, id)| (id.as_str(), i))
            .collect();
        let named = |values: &[u32]| {
            self.policy
                .resources
                .iter()
                .zip(values)
                .map(|(resource, &value)| (resource.id.clone(), value))
                .collect()
        };
        let mut mask = 0;
        let mut peaks = vec![0; self.policy.resources.len()];
        let mut steps = Vec::with_capacity(order.len());
        for operation in order {
            let next = *ids
                .get(operation.as_str())
                .ok_or_else(|| bad("unknown workholding timeline operation"))?;
            if mask & (1_u64 << next) != 0 {
                return Err(bad("repeated workholding timeline operation"));
            }
            let before = self.limits.occupied(mask);
            if !self.limits.admits(&before, next) {
                return Err(if baseline {
                    Refusal::new(
                        RefusalCode::NotConverged,
                        format!(
                            "workholding serial-copy baseline is unsupported: {operation} cannot acquire its complete resources before start; declare a resource-feasible source order or qualified capacity; no global infeasibility is proved"
                        ),
                    )
                } else {
                    bad(format!(
                        "selected workholding schedule exceeds qualified resources before {operation}"
                    ))
                });
            }
            let during: Vec<_> = before
                .iter()
                .zip(&self.limits.acquire[next])
                .map(|(held, acquire)| held + acquire)
                .collect();
            for (peak, &held) in peaks.iter_mut().zip(&during) {
                *peak = (*peak).max(held);
            }
            mask |= 1_u64 << next;
            steps.push(Step {
                operation: operation.clone(),
                occupied_before: named(&before),
                acquired: self
                    .holds
                    .iter()
                    .filter(|hold| hold.acquire == *operation)
                    .map(|hold| hold.id.clone())
                    .collect(),
                occupied_during: named(&during),
                released: self
                    .holds
                    .iter()
                    .filter(|hold| hold.release == *operation)
                    .map(|hold| hold.id.clone())
                    .collect(),
                occupied_after: named(&self.limits.occupied(mask)),
            });
        }
        if mask != (1_u64 << self.limits.ids.len()) - 1
            || self.limits.occupied(mask).iter().any(|&units| units != 0)
        {
            return Err(bad(
                "workholding timeline must complete every operation and release every hold",
            ));
        }
        Ok(Timeline {
            operations: steps,
            peak_occupancy: named(&peaks),
        })
    }

    pub(super) fn report(self, selected: &[String]) -> Result<Report, Refusal> {
        let selected = self.timeline(selected, false)?;
        Ok(Report {
            schema: "ftx-sewing-workholding/1",
            machine_sha256: self.policy.machine_sha256,
            qualifications: self.qualifications,
            resources: self.policy.resources,
            holds: self.holds,
            baseline: self.baseline,
            selected,
            fixture_geometry_and_access_validated: false,
        })
    }
}

pub(super) fn compile(
    plan: &SewPlan,
    policy: &Policy,
    operations: &mut [Operation],
) -> Result<Compiled, Refusal> {
    let qualifications = policy.validate(plan)?;
    let copies = operations.len() / plan.seams.len();
    if policy
        .holds
        .len()
        .checked_mul(copies)
        .is_none_or(|n| n > MAX_EXPANDED_HOLDS)
    {
        return Err(budget());
    }
    let mut ids: Vec<_> = operations.iter().map(|op| op.id.clone()).collect();
    ids.sort();
    let indices: BTreeMap<_, _> = ids
        .iter()
        .enumerate()
        .map(|(i, id)| (id.as_str(), i))
        .collect();
    let resource_ids: BTreeMap<_, _> = policy
        .resources
        .iter()
        .enumerate()
        .map(|(i, resource)| (resource.id.as_str(), i))
        .collect();
    let mut acquire = vec![vec![0_u32; policy.resources.len()]; operations.len()];
    let mut occupancy = Vec::with_capacity(policy.holds.len() * copies);
    let mut holds = Vec::with_capacity(policy.holds.len() * copies);
    for copy in 0..copies {
        for hold in &policy.holds {
            let start = format!("{copy}/{}", hold.acquire);
            let end = format!("{copy}/{}", hold.release);
            let resource = resource_ids[hold.resource.as_str()];
            let start_index = indices[start.as_str()];
            acquire[start_index][resource] += hold.units;
            occupancy.push(ordering::ResourceHold {
                resource,
                units: hold.units,
                acquire: 1_u64 << start_index,
                release: 1_u64 << indices[end.as_str()],
            });
            if start != end {
                let release = operations
                    .iter_mut()
                    .find(|op| op.id == end)
                    .expect("validated hold anchor");
                if !release.after.contains(&start) {
                    release.after.push(start.clone());
                    release.after.sort();
                }
            }
            holds.push(ExpandedHold {
                id: format!("{copy}/{}", hold.id),
                copy,
                hold_id: hold.id.clone(),
                resource: hold.resource.clone(),
                units: hold.units,
                acquire: start,
                release: end,
            });
        }
    }
    // Added lifecycle edges must not turn an otherwise admitted sewing graph into a cycle.
    let mut done = BTreeSet::new();
    loop {
        let before = done.len();
        for operation in operations.iter() {
            if operation.after.iter().all(|id| done.contains(id)) {
                done.insert(operation.id.clone());
            }
        }
        if done.len() == operations.len() {
            break;
        }
        if done.len() == before {
            return Err(bad(
                "workholding release-after-acquire constraints introduce a dependency cycle",
            ));
        }
    }
    // Preserve the existing serial-copy seed. An unsupported seed is an explicit admission limit,
    // not proof that no other resource-feasible source order exists.
    done.clear();
    for operation in operations.iter() {
        if !operation.after.iter().all(|id| done.contains(id)) {
            return Err(Refusal::new(
                RefusalCode::NotConverged,
                "workholding serial-copy baseline is unsupported by release-after-acquire prerequisites; declare a compatible source order; no global infeasibility is proved",
            ));
        }
        done.insert(operation.id.clone());
    }
    let mut compiled = Compiled {
        limits: ordering::ResourceLimits {
            ids,
            capacity: policy.resources.iter().map(|r| r.capacity).collect(),
            acquire,
            holds: occupancy,
        },
        policy: policy.clone(),
        qualifications,
        holds,
        baseline: Timeline {
            operations: Vec::new(),
            peak_occupancy: BTreeMap::new(),
        },
    };
    compiled.baseline = compiled.timeline(
        &operations
            .iter()
            .map(|op| op.id.clone())
            .collect::<Vec<_>>(),
        true,
    )?;
    Ok(compiled)
}

//! Cross-copy scheduling on one admitted machine, reusing the ordinary dependency/setup authority.
//! Repeated operations borrow source station geometry; no handling or fixture transition is inferred.

use std::borrow::Cow;

use ftx_core::{Refusal, RefusalCode};
use serde::{Deserialize, Serialize};

use super::{SewOptions, SewPlan, bad, ordering};

/// Optional production schedule. The expanded graph has at most 63 operations and one machine.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    /// Garment quantity. Omission inherits explicit thread quantity, or one outside a cutting study.
    /// Material studies also inherit their complete cutting quantity; conflicting counts refuse.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub production_copies: Option<usize>,
    /// Count changes or source-qualified directed durations across the complete expanded graph.
    pub ordering: ordering::Policy,
}

/// One physical garment operation, referencing the single retained geometry and qualified recipe.
#[derive(Clone, Debug, Serialize)]
pub struct Operation {
    /// Unambiguous `copy/operation_id` identity used by the ordering report.
    pub id: String,
    /// Zero-based garment copy, using the same convention as cutting placements.
    pub copy: usize,
    /// Original sewing pass identity, preserved even when it contains slashes or other punctuation.
    pub operation_id: String,
    /// Index into the owning plan's `seams`; station geometry is retained once per source operation.
    pub seam_index: usize,
    /// Expanded predecessor IDs within this same garment copy.
    pub after: Vec<String>,
    /// Existing exact material/machine/process qualification, absent on unqualified legacy recipes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qualification_id: Option<String>,
    /// Actual operation intervals including repeated lock/reinforcement traversal.
    pub stitch_intervals: usize,
    /// This operation's continuous feed duration, excluding unmodeled handling and acceleration.
    pub feed_seconds: f64,
}

/// Complete bounded schedule over every repeated operation. No geometry or source evidence is guessed.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// `ftx-sewing-production/1`.
    pub schema: &'static str,
    /// Actual garment quantity shared with any requested thread inventory.
    pub production_copies: usize,
    /// Exact single machine/configuration from the owning qualified sewing plan.
    pub machine: String,
    /// Complete repeated operation graph, initially ordered by copy then original topological order.
    pub operations: Vec<Operation>,
    /// Complete baseline/selected schedules, counts, qualified durations and bounded-search status.
    /// Initial setup is charged once across the batch; no final reset or fixture change is inferred.
    pub ordering: ordering::Report,
    /// Complete batch traversal count. The owning plan's count retains its one-garment meaning.
    pub stitch_intervals: usize,
    /// Complete batch continuous feed duration. Setup, if qualified, is reported separately.
    pub feed_seconds: f64,
}

fn count(value: usize) -> Result<usize, Refusal> {
    if !(1..=4096).contains(&value) {
        return Err(bad("sewing production_copies must be in 1..4096"));
    }
    Ok(value)
}

/// Freeze one effective quantity into requested scheduling and thread-consumption inputs.
/// Existing one-garment plans are unchanged when scheduling is absent.
pub(super) fn resolve_options(options: &SewOptions) -> Result<Cow<'_, SewOptions>, Refusal> {
    let Some(production) = &options.production else {
        return Ok(Cow::Borrowed(options));
    };
    if options.ordering.is_some() {
        return Err(bad(
            "choose one-garment ordering or production scheduling, not both",
        ));
    }
    let thread_copies = options
        .consumables
        .as_ref()
        .and_then(|c| c.production_copies);
    if let (Some(schedule), Some(thread)) = (production.production_copies, thread_copies)
        && schedule != thread
    {
        return Err(bad(
            "sewing production_copies must match thread production_copies",
        ));
    }
    let copies = count(production.production_copies.or(thread_copies).unwrap_or(1))?;
    let expanded = options.passes.len().checked_mul(copies).ok_or_else(|| {
        Refusal::new(
            RefusalCode::BudgetExceeded,
            "production operation count overflows",
        )
    })?;
    production
        .ordering
        .validate_production(expanded, &options.machine.id)?;
    if production.production_copies == Some(copies)
        && options
            .consumables
            .as_ref()
            .is_none_or(|c| c.production_copies == Some(copies))
    {
        return Ok(Cow::Borrowed(options));
    }
    let mut resolved = options.clone();
    resolved
        .production
        .as_mut()
        .expect("requested production")
        .production_copies = Some(copies);
    if let Some(consumables) = &mut resolved.consumables {
        consumables.production_copies = Some(copies);
    }
    Ok(Cow::Owned(resolved))
}

pub(super) fn evaluate(plan: &SewPlan, policy: &Policy) -> Result<Report, Refusal> {
    let copies = count(policy.production_copies.unwrap_or(1))?;
    let expanded = plan.seams.len().checked_mul(copies).ok_or_else(|| {
        Refusal::new(
            RefusalCode::BudgetExceeded,
            "production operation count overflows",
        )
    })?;
    policy
        .ordering
        .validate_production(expanded, &plan.options.machine.id)?;
    let stitch_intervals = plan.stitch_intervals.checked_mul(copies).ok_or_else(|| {
        Refusal::new(
            RefusalCode::BudgetExceeded,
            "production interval count overflows",
        )
    })?;
    let feed_seconds = plan.feed_seconds * copies as f64;
    if !feed_seconds.is_finite() {
        return Err(bad("production feed duration overflows"));
    }
    let scoped = |copy: usize, operation: &str| format!("{copy}/{operation}");
    let mut operations = Vec::with_capacity(expanded);
    for copy in 0..copies {
        for (seam_index, seam) in plan.seams.iter().enumerate() {
            operations.push(Operation {
                id: scoped(copy, seam.pass.operation_id()),
                copy,
                operation_id: seam.pass.operation_id().to_string(),
                seam_index,
                after: seam.pass.after.iter().map(|id| scoped(copy, id)).collect(),
                qualification_id: seam.qualification_id.clone(),
                stitch_intervals: seam.stitch_intervals,
                feed_seconds: seam.feed_seconds,
            });
        }
    }
    let views: Vec<_> = operations
        .iter()
        .map(|operation| {
            let pass = &plan.seams[operation.seam_index].pass;
            ordering::Operation {
                id: &operation.id,
                after: &operation.after,
                needle: &pass.needle,
                thread: &pass.thread,
            }
        })
        .collect();
    let mut ordering = ordering::optimize_operations(&policy.ordering, &views, feed_seconds)?;
    ordering.tie_break = if ordering.timing.is_some() {
        "Retain the incumbent on equal integer setup milliseconds; start with complete garments in ascending copy order, then explore by immediate duration and scoped operation ID."
    } else {
        "Retain the incumbent on equal total changes; start with complete garments in ascending copy order, then explore by immediate total changes and scoped operation ID."
    };
    Ok(Report {
        schema: "ftx-sewing-production/1",
        production_copies: copies,
        machine: plan.options.machine.id.clone(),
        operations,
        ordering,
        stitch_intervals,
        feed_seconds,
    })
}

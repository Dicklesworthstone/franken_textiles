//! Directed traversal of the existing paired seam stations. A reverse move keeps side b / side a
//! as the feed convention: reversing both sides does not exchange their material identities.

use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::SeamPairs;
use serde::{Deserialize, Serialize};

use super::{SewingMachine, SewingPass, bad, positive, range, within};

/// Feed direction relative to increasing canonical paired-station index.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedDirection {
    /// Increasing station index; this is independent of the seam's own `reverse` attribute.
    #[default]
    Forward,
    /// Decreasing station index on both matched sides.
    Reverse,
}

impl FeedDirection {
    /// Whether this is the legacy traversal.
    #[must_use]
    pub fn is_forward(&self) -> bool {
        *self == Self::Forward
    }
}

/// Separately declared reverse-feed capability. Unknown capability is not admitted.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReverseFeed {
    /// Permitted absolute advance per reverse stitch on each side, in mm.
    pub stitch_mm: [f64; 2],
    /// Permitted side-b / side-a absolute advance ratio during reverse feed.
    pub feed_b_over_a: [f64; 2],
    /// Maximum continuous reverse stitch rate, not an acceleration or handling model.
    pub max_stitches_per_minute: f64,
}

impl ReverseFeed {
    pub(super) fn validate(&self) -> Result<(), Refusal> {
        if !range(self.stitch_mm)
            || !range(self.feed_b_over_a)
            || !positive(self.max_stitches_per_minute)
        {
            return Err(bad("invalid reverse sewing machine limits"));
        }
        Ok(())
    }
}

/// Lock lengths in canonical stitch intervals, not millimetres or invented thread consumption.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Backtack {
    /// First travel this many intervals, return to the start, then sew the complete seam.
    #[serde(default)]
    pub start_stitches: usize,
    /// After the complete seam, return this many intervals and sew back to its endpoint.
    #[serde(default)]
    pub end_stitches: usize,
}

impl Backtack {
    /// No additional lock travel.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.start_stitches == 0 && self.end_stitches == 0
    }

    pub(super) fn extra_intervals(&self) -> Result<usize, Refusal> {
        self.start_stitches
            .checked_add(self.end_stitches)
            .and_then(|n| n.checked_mul(2))
            .ok_or_else(|| Refusal::new(RefusalCode::BudgetExceeded, "backtack budget overflow"))
    }
}

/// Purpose of a continuous directed run. Changes of direction require unmodeled deceleration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunPhase {
    /// Initial outward leg of the start lock, in the requested seam direction.
    StartLockOut,
    /// Return to the initial station before sewing the whole seam.
    StartLockReturn,
    /// Complete traversal of the seam, without omitting any interval or notch.
    Seam,
    /// Return from the endpoint into the sewn seam to begin the end lock.
    EndLockOut,
    /// Final traversal to the endpoint, in the requested seam direction.
    EndLockReturn,
}

/// Compact stitch program over the immutable `PlannedSeam::stations` arrays.
/// Visit every consecutive integer index from `from_station` through `to_station`, inclusive.
/// Adjacent runs share one station; that shared station is not an extra stitch interval.
#[derive(Clone, Debug, Serialize)]
pub struct StitchRun {
    /// Role in this pass's explicitly requested seam/lock program.
    pub phase: RunPhase,
    /// Index in both `stations.a` and `stations.b`, where this run begins.
    pub from_station: usize,
    /// Inclusive endpoint index, never equal to `from_station`.
    pub to_station: usize,
    /// Actual traversal direction, also selecting the relevant machine envelope.
    pub direction: FeedDirection,
    /// Number of traversed station intervals, including repeated lock travel.
    pub stitch_intervals: usize,
    /// Continuous feed time only; handling, turns, acceleration and thread use are not modeled.
    pub feed_seconds: f64,
}

fn limits(
    machine: &SewingMachine,
    direction: FeedDirection,
) -> Result<([f64; 2], [f64; 2], f64), Refusal> {
    match direction {
        FeedDirection::Forward => Ok((
            machine.stitch_mm,
            machine.feed_b_over_a,
            machine.max_stitches_per_minute,
        )),
        FeedDirection::Reverse => machine
            .reverse
            .as_ref()
            .map(|r| (r.stitch_mm, r.feed_b_over_a, r.max_stitches_per_minute))
            .ok_or_else(|| bad("reverse sewing/backtack needs explicit reverse machine limits")),
    }
}

pub(super) fn validate_primary_feed(
    pass: &SewingPass,
    machine: &SewingMachine,
) -> Result<(), Refusal> {
    let (stitch, _, speed) = limits(machine, pass.direction)?;
    if !within(pass.max_stitch_mm, stitch) || pass.stitches_per_minute > speed {
        return Err(bad(format!(
            "{}: requested stitch advance or rate exceeds primary feed limits",
            pass.operation_id()
        )));
    }
    // Refuse missing reverse capability before station allocation, including a forward seam's locks.
    if !pass.backtack.is_empty() {
        limits(machine, FeedDirection::Reverse)?;
    }
    Ok(())
}

pub(super) fn compile(
    stations: &SeamPairs,
    lengths: [f64; 2],
    pass: &SewingPass,
    machine: &SewingMachine,
) -> Result<Vec<StitchRun>, Refusal> {
    let count = stations.fa.len();
    if count < 2
        || stations.fb.len() != count
        || stations.a.len() != count
        || stations.b.len() != count
    {
        return Err(bad("sewing needs equal, nonempty paired station tables"));
    }
    let last = count - 1;
    let start_lock = pass.backtack.start_stitches;
    let end_lock = pass.backtack.end_stitches;
    if start_lock > last || end_lock > last {
        return Err(bad(format!(
            "{}: backtack length exceeds the seam's {last} stitch intervals",
            pass.operation_id()
        )));
    }
    let station = |progress: usize| {
        if pass.direction.is_forward() {
            progress
        } else {
            last - progress
        }
    };
    let mut runs = Vec::with_capacity(5);
    let mut append = |phase, from: usize, to: usize| -> Result<(), Refusal> {
        let from_station = station(from);
        let to_station = station(to);
        let direction = if from_station < to_station {
            FeedDirection::Forward
        } else {
            FeedDirection::Reverse
        };
        let (stitch, feed, speed) = limits(machine, direction)?;
        if pass.stitches_per_minute > speed {
            return Err(bad(format!(
                "{}: {direction:?} stitch rate exceeds machine limits",
                pass.operation_id()
            )));
        }
        for i in from_station.min(to_station)..from_station.max(to_station) {
            let da = (stations.fa[i + 1] - stations.fa[i]) * lengths[0];
            let db = (stations.fb[i + 1] - stations.fb[i]) * lengths[1];
            if !within(da, stitch)
                || !within(db, stitch)
                || !within(db / da, feed)
                || da > pass.max_stitch_mm * (1.0 + 1e-10)
                || db > pass.max_stitch_mm * (1.0 + 1e-10)
            {
                return Err(bad(format!(
                    "{}: {direction:?} local stitch advance or differential feed outside machine limits at interval {i}",
                    pass.operation_id()
                )));
            }
        }
        let stitch_intervals = from.abs_diff(to);
        let feed_seconds = stitch_intervals as f64 * 60.0 / pass.stitches_per_minute;
        if !feed_seconds.is_finite() {
            return Err(bad("non-finite sewing run time"));
        }
        runs.push(StitchRun {
            phase,
            from_station,
            to_station,
            direction,
            stitch_intervals,
            feed_seconds,
        });
        Ok(())
    };
    if start_lock > 0 {
        append(RunPhase::StartLockOut, 0, start_lock)?;
        append(RunPhase::StartLockReturn, start_lock, 0)?;
    }
    append(RunPhase::Seam, 0, last)?;
    if end_lock > 0 {
        append(RunPhase::EndLockOut, last, last - end_lock)?;
        append(RunPhase::EndLockReturn, last - end_lock, last)?;
    }
    Ok(runs)
}

//! Take-down exposure replayed from the independently decoded physical pass stream.
//!
//! A nominal course uses the pinned upstream backend's default 100 roller units.
//! This is a software budget convention, not a measured distance, tension, dwell time,
//! or hardware calibration. Actual positive pass controls are counted without rounding.

use std::collections::BTreeMap;

use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::knit::{MachineBackendKind, MachineProfile, Needle};
use serde::{Deserialize, Serialize};

use super::{Decoded, Record, decode};

const NOMINAL_COURSE_ADVANCE_UNITS: u32 = 100;

/// Source-bound audit of actual KCode roller controls and live held-loop exposure.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KCodeTakedownReport {
    /// Exact independently decoded KCode file identity.
    pub kc_sha256: String,
    /// Exact profile supplying the existing imbalance and hook-stack limits.
    pub machine_sha256: String,
    /// Physical passes with a knit, tuck, or split, including zero-advance passes.
    pub forming_pass_count: usize,
    /// All physical passes with positive roller advance, including empty moves.
    pub roller_pass_count: usize,
    /// Sum of actual nonnegative KCode roller controls, in nominal machine units.
    pub roller_advance_units: u64,
    /// Greatest accumulated advance while any original loop remained unknitted.
    pub max_held_advance_units: u64,
    /// Existing profile course-imbalance limit multiplied by the nominal course budget.
    pub limit_units: u64,
    /// Pinned upstream default roller budget per ordinary course; not physical calibration.
    pub nominal_course_advance_units: u32,
    /// One-based physical pass with greatest held exposure, absent if exposure stays zero.
    pub worst_pass_index: Option<usize>,
    /// Live hook containing the oldest loop at that pass.
    pub worst_needle: Option<Needle>,
    /// Hooks still occupied at the end; an operator pause does not pretend they were cleared.
    pub live_hooks_at_end: usize,
    /// Always false: software roller units do not qualify real fabric take-down.
    pub physical_takedown_calibrated: bool,
}

/// Replays actual physical passes from an initially empty bed and checks held-loop exposure.
///
/// A pass's roller advance occurs after its needle actions in this software model.
/// Knitting replaces the old loops; tucking preserves their age; transfers and splits
/// carry old ages to the receiving hooks. Empty and transfer passes with positive
/// controls advance the same budget. Pauses do not advance it or clear live loops.
///
/// # Errors
/// Propagates independent format refusals. Returns `UnqualifiedProcess` when actual
/// held exposure exceeds the existing profile budget, and `MachineStateInfeasible`
/// for an incompatible profile or impossible hook transfers/stacks.
pub fn verify_kniterate_takedown(
    text: &str,
    profile: &MachineProfile,
) -> Result<KCodeTakedownReport, Refusal> {
    decode(text)?.verify_takedown(profile)
}

fn state_error(pass_index: usize, message: impl std::fmt::Display) -> Refusal {
    Refusal::new(
        RefusalCode::MachineStateInfeasible,
        format!("KCode take-down replay at physical pass {pass_index}: {message}"),
    )
}

pub(super) fn check_decoded(
    decoded: &Decoded,
    profile: &MachineProfile,
) -> Result<KCodeTakedownReport, Refusal> {
    profile.validate()?;
    if profile.backend_kind != MachineBackendKind::KniterateKc || profile.needle_count != 252 {
        return Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            "KCode take-down replay requires a 252-needle Kniterate profile",
        ));
    }
    let mut report = KCodeTakedownReport {
        kc_sha256: decoded.sha256.clone(),
        machine_sha256: profile.canonical_sha256()?,
        forming_pass_count: 0,
        roller_pass_count: 0,
        roller_advance_units: 0,
        max_held_advance_units: 0,
        limit_units: u64::from(profile.max_roller_imbalance_courses)
            * u64::from(NOMINAL_COURSE_ADVANCE_UNITS),
        nominal_course_advance_units: NOMINAL_COURSE_ADVANCE_UNITS,
        worst_pass_index: None,
        worst_needle: None,
        live_hooks_at_end: 0,
        physical_takedown_calibrated: false,
    };
    // Each entry is a real loop's cumulative-advance origin. Keeping complete stacks
    // makes a tuck unable to renew the old loop, and preserves ages through transfers.
    let mut hooks = BTreeMap::<Needle, Vec<u64>>::new();
    let mut pass_index = 0;
    for record in &decoded.entries {
        let Record::Pass(pass) = record else {
            continue;
        };
        pass_index += 1;
        let advance = u64::try_from(pass.roller).map_err(|_| {
            state_error(pass_index, "negative roller advance cannot erase exposure")
        })?;
        let after_pass = report
            .roller_advance_units
            .checked_add(advance)
            .ok_or_else(|| state_error(pass_index, "cumulative roller advance overflow"))?;
        report.forming_pass_count += usize::from(
            pass.actions
                .iter()
                .any(|action| matches!(action.code, "knit" | "tuck" | "split")),
        );
        report.roller_pass_count += usize::from(advance > 0);

        for action in &pass.actions {
            match action.code {
                "knit" => {
                    hooks.insert(action.needle, vec![after_pass]);
                }
                "tuck" => {
                    hooks.entry(action.needle).or_default().push(after_pass);
                }
                "xfer" | "split" => {
                    let target = action
                        .to
                        .ok_or_else(|| state_error(pass_index, "transfer has no receiving hook"))?;
                    let mut source = hooks.remove(&action.needle).ok_or_else(|| {
                        state_error(
                            pass_index,
                            format!("transfer source {} is empty", action.needle.knitout_token()),
                        )
                    })?;
                    source.reverse();
                    hooks.entry(target).or_default().extend(source);
                    if action.code == "split" {
                        hooks.insert(action.needle, vec![after_pass]);
                    }
                }
                "drop" => {
                    hooks.remove(&action.needle);
                }
                "amiss" => {}
                _ => return Err(state_error(pass_index, "unknown decoded needle action")),
            }
            for needle in [action.needle, action.to.unwrap_or(action.needle)] {
                if hooks
                    .get(&needle)
                    .is_some_and(|stack| stack.len() > usize::from(profile.max_loops_per_hook))
                {
                    return Err(state_error(
                        pass_index,
                        "live loop stack exceeds the profile hook limit",
                    ));
                }
            }
        }
        report.roller_advance_units = after_pass;
        for (&needle, stack) in &hooks {
            let oldest = *stack.iter().min().expect("live hook has at least one loop");
            let held = after_pass - oldest;
            if held > report.max_held_advance_units {
                report.max_held_advance_units = held;
                report.worst_pass_index = Some(pass_index);
                report.worst_needle = Some(needle);
            }
        }
        if report.max_held_advance_units > report.limit_units {
            return Err(Refusal::new(
                RefusalCode::UnqualifiedProcess,
                format!(
                    "actual KCode holds a loop through {} nominal roller units at physical pass {}, exceeding the profile budget {} (100 units per reference course)",
                    report.max_held_advance_units,
                    report.worst_pass_index.expect("positive held exposure has a pass"),
                    report.limit_units,
                ),
            )
            .repair("retain the verified Knitout for review; explicitly budget split forming passes so bind-off or anchor-only operations do not multiply take-down")
            .repair("do not raise the limit or treat nominal roller units as physical calibration without independent machine and material evidence"));
        }
    }
    report.live_hooks_at_end = hooks.len();
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ftx_pattern::knit::{Bed, CourseDir};

    use super::super::{Action, ReadPass};

    fn action(code: &'static str, bed: Bed, needle: i32, target: Option<(Bed, i32)>) -> Action {
        Action {
            code,
            needle: Needle::new(bed, needle),
            to: target.map(|(bed, index)| Needle::new(bed, index)),
            dir: None,
            carrier: 1,
            cam: 5,
            rack: 0,
            speed: 100,
        }
    }

    fn pass(roller: i32, actions: Vec<Action>) -> Record {
        Record::Pass(ReadPass {
            mode: "Kn-Kn".into(),
            dir: CourseDir::Plus,
            carrier: 1,
            rack: 0,
            speed: 100,
            roller,
            front: BTreeMap::new(),
            back: BTreeMap::new(),
            carriage: [-23, 525],
            yarn: None,
            actions,
        })
    }

    fn decoded(entries: Vec<Record>) -> Decoded {
        Decoded {
            entries,
            sha256: "synthetic:decoded-pass-unit-test".into(),
        }
    }

    #[test]
    fn segmented_bind_off_cannot_multiply_default_take_down() {
        use Bed::{Back, Front};
        let history = |zero_extra: bool| {
            let mut entries = vec![
                pass(
                    100,
                    (0..10).map(|n| action("tuck", Front, n, None)).collect(),
                ),
                pass(
                    100,
                    (0..10).map(|n| action("knit", Front, n, None)).collect(),
                ),
            ];
            for needle in 0..10 {
                if needle > 0 {
                    entries.push(pass(
                        0,
                        vec![action("xfer", Front, needle - 1, Some((Back, needle - 1)))],
                    ));
                    entries.push(pass(
                        0,
                        vec![action("xfer", Back, needle - 1, Some((Front, needle)))],
                    ));
                }
                entries.push(pass(
                    if zero_extra && needle > 0 { 0 } else { 100 },
                    vec![action("knit", Front, needle, None)],
                ));
            }
            decoded(entries)
        };
        let profile = MachineProfile::kniterate_7e();
        let refusal = check_decoded(&history(false), &profile).unwrap_err();
        assert_eq!(refusal.code, RefusalCode::UnqualifiedProcess);
        assert!(refusal.message.contains("900 nominal roller units"));
        let report = check_decoded(&history(true), &profile).unwrap();
        assert_eq!(report.forming_pass_count, 12);
        assert_eq!(report.roller_pass_count, 3);
        assert_eq!(report.roller_advance_units, 300);
        assert_eq!(report.max_held_advance_units, 100);
        assert_eq!(report.limit_units, 800);
        assert_eq!(report.live_hooks_at_end, 1);
        assert!(!report.physical_takedown_calibrated);
    }

    #[test]
    fn tucked_and_transferred_loops_keep_their_oldest_age() {
        use Bed::{Back, Front};
        let entries = vec![
            pass(100, vec![action("tuck", Front, 0, None)]),
            pass(100, vec![action("tuck", Front, 0, None)]),
            pass(0, vec![action("xfer", Front, 0, Some((Back, 0)))]),
            pass(300, Vec::new()),
            pass(0, vec![action("split", Back, 0, Some((Front, 0)))]),
            pass(200, Vec::new()),
        ];
        let report = check_decoded(&decoded(entries), &MachineProfile::kniterate_7e()).unwrap();
        assert_eq!(report.forming_pass_count, 3);
        assert_eq!(report.roller_pass_count, 4);
        assert_eq!(report.roller_advance_units, 700);
        assert_eq!(report.max_held_advance_units, 600);
        assert_eq!(report.worst_needle, Some(Needle::new(Front, 0)));
        assert_eq!(report.live_hooks_at_end, 2);
    }

    #[test]
    fn empty_positive_roller_passes_count_but_drops_and_pauses_do_not_fake_loops() {
        use Bed::Front;
        let history = || {
            vec![
                pass(100, vec![action("tuck", Front, 0, None)]),
                Record::Pause(Some("Inspect held loop".into())),
                pass(801, Vec::new()),
            ]
        };
        let profile = MachineProfile::kniterate_7e();
        assert_eq!(
            check_decoded(&decoded(history()), &profile)
                .unwrap_err()
                .code,
            RefusalCode::UnqualifiedProcess
        );
        let mut entries = history();
        entries.insert(2, pass(0, vec![action("drop", Front, 0, None)]));
        let report = check_decoded(&decoded(entries), &profile).unwrap();
        assert_eq!(report.max_held_advance_units, 0);
        assert_eq!(report.roller_advance_units, 901);
        assert_eq!(report.live_hooks_at_end, 0);
    }

    #[test]
    fn original_upstream_file_reaches_public_audit() {
        let text = include_str!("../../../../tests/fixtures/kniterate/position-keep.kc");
        let profile = MachineProfile::kniterate_7e();
        let report = verify_kniterate_takedown(text, &profile).unwrap();
        assert_eq!(
            report.kc_sha256,
            ftx_core::receipt::sha256_hex(text.as_bytes())
        );
        assert_eq!(report.forming_pass_count, 4);
        assert_eq!(report.roller_pass_count, 5);
        assert_eq!(report.roller_advance_units, 500);
        assert_eq!(report.live_hooks_at_end, 0);
        assert_eq!(report.max_held_advance_units, 100);
    }
}

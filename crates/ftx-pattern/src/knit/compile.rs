//! Transfer planner & `KnitDoc` -> lossless CMU Knitout v2 compiler (`FTX-K04`).
//!
//! Compiles validated `ftx-knit/1` documents into machine-profile-adapted `KnitoutProgram`s
//! (branching carrier insertion cleanly on `has_yarn_inserting_hooks`, planning sliderless
//! bed-to-bed decrease/rib transfers within `±max_racking_quarter_pitches`, and verifying every
//! emitted stream through the V-bed VM before sealing `ftx-machine-cert/1`).

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode};

use super::ir::{
    Bed, CableGroup, CarrierId, CourseDir, KNITOUT_SCHEMA, KnitDoc, KnitoutHeader, KnitoutOp,
    KnitoutProgram, LoopKind, Needle, StitchNode,
};
use super::profile::{CalibrationSwatchCaps, MachineProfile, WearerAgeClass};
use super::receipts::{FourClaimsVerdict, KnitMachineCert, KnitSwatchQualification};
use super::vm::{KnitSafetyContext, VBedVmReport, profile_racking_strain, verify_knitout_program};

/// Complete verified compilation bundle returned by [`compile_knit_doc`].
#[derive(Clone, Debug)]
pub struct CompiledKnitBundle {
    /// Lossless CMU Knitout v2 AST.
    pub program: KnitoutProgram,
    /// V-bed VM verification report.
    pub vm_report: VBedVmReport,
    /// Sealed `ftx-machine-cert/1` (`hardware_execution_authorized: false`, `physical_knitting_validated: false`).
    pub cert: KnitMachineCert,
}

/// Live loop identities belong to needle addresses, not just wale indices: a circular
/// course has independent front and back stacks at every wale.
struct LoopCompiler<'a> {
    profile: &'a MachineProfile,
    live: BTreeMap<Needle, Vec<u32>>,
    /// Rack positions at which currently live cross-bed bridges were established.
    /// Mirrors the VM; every emitted transfer returns the carriage beds to rack zero.
    bridge_racks: BTreeSet<i32>,
    ops: Vec<KnitoutOp>,
}

fn topology_refusal(node: &StitchNode, detail: impl std::fmt::Display) -> Refusal {
    Refusal::new(
        RefusalCode::MachineStateInfeasible,
        format!(
            "stitch {} (course {}, {:?} wale {}): {detail}",
            node.id, node.course, node.bed, node.wale
        ),
    )
    .repair("supply exact live parent loops in bottom-to-top order; split or cable operations require a supported transfer schedule")
}

impl LoopCompiler<'_> {
    fn live_bed_counts(&self) -> [usize; 2] {
        let mut counts = [0, 0];
        for needle in self.live.keys() {
            counts[usize::from(needle.bed == Bed::Back)] += 1;
        }
        counts
    }

    fn observe_transfer_bridges(&mut self, rack: i32) {
        let counts = self.live_bed_counts();
        if counts.contains(&0) {
            self.bridge_racks.clear();
        } else if self.bridge_racks.is_empty() {
            self.bridge_racks.insert(rack);
        }
    }

    /// Simulate only a candidate's local crossings and return its peak rack delta.
    /// Occupancy overrides are limited to these few hooks; no full program replay
    /// or copy of the stitch graph is needed to check bridge strain and stack caps.
    fn route_cost(&self, route: &[(Needle, Needle)]) -> Option<u32> {
        let mut counts = self.live_bed_counts();
        let mut bridges = self.bridge_racks.clone();
        let mut changed = BTreeMap::new();
        let mut cost = 0;
        let rack_cost = |rack: i32, counts: [usize; 2], bridges: &BTreeSet<i32>| {
            if counts.contains(&0) {
                return Some(rack.unsigned_abs());
            }
            let delta = bridges
                .iter()
                .map(|&born| rack.abs_diff(born))
                .max()
                .unwrap_or_else(|| rack.unsigned_abs());
            let (_, strain) = profile_racking_strain(self.profile, delta);
            (strain <= self.profile.racking_yarn_breaking_elongation / 2.0)
                .then_some(delta.max(rack.unsigned_abs()))
        };
        for &(from, to) in route {
            let rack = match (from.bed, to.bed) {
                (Bed::Front, Bed::Back) => (i64::from(from.index) - i64::from(to.index)) * 4,
                (Bed::Back, Bed::Front) => (i64::from(to.index) - i64::from(from.index)) * 4,
                _ => return None,
            };
            if rack.abs() > i64::from(self.profile.max_racking_quarter_pitches)
                || [from.index, to.index]
                    .iter()
                    .any(|&index| index < 0 || index as u32 >= self.profile.needle_count)
            {
                return None;
            }
            let rack = rack as i32;
            cost = cost.max(rack_cost(rack, counts, &bridges)?);
            let source = changed
                .get(&from)
                .copied()
                .unwrap_or_else(|| self.live.get(&from).map_or(0, Vec::len));
            let destination = changed
                .get(&to)
                .copied()
                .unwrap_or_else(|| self.live.get(&to).map_or(0, Vec::len));
            if source == 0 || source + destination > usize::from(self.profile.max_loops_per_hook) {
                return None;
            }
            counts[usize::from(from.bed == Bed::Back)] -= 1;
            if destination == 0 {
                counts[usize::from(to.bed == Bed::Back)] += 1;
            }
            changed.insert(from, 0);
            changed.insert(to, source + destination);
            if counts.contains(&0) {
                bridges.clear();
            } else if bridges.is_empty() {
                bridges.insert(rack);
            }
            cost = cost.max(rack_cost(0, counts, &bridges)?);
        }
        Some(cost)
    }

    fn cross(&mut self, from: Needle, to: Needle) -> Result<(), Refusal> {
        let rack = if from.bed == Bed::Front && to.bed == Bed::Back {
            (i64::from(from.index) - i64::from(to.index)) * 4
        } else if from.bed == Bed::Back && to.bed == Bed::Front {
            (i64::from(to.index) - i64::from(from.index)) * 4
        } else {
            return Err(Refusal::new(
                RefusalCode::MachineStateInfeasible,
                "parent transfer requires opposite main beds",
            ));
        };
        if rack.abs() > i64::from(self.profile.max_racking_quarter_pitches) {
            return Err(Refusal::new(
                RefusalCode::MachineStateInfeasible,
                format!("parent transfer racking {rack} quarter-pitches exceeds the machine limit"),
            )
            .repair("use a machine with the required racking range or reduce the shaping step"));
        }
        let mut source = self.live.get(&from).cloned().ok_or_else(|| {
            Refusal::new(
                RefusalCode::MachineStateInfeasible,
                "parent transfer source has no live loops",
            )
        })?;
        let destination_len = self.live.get(&to).map_or(0, Vec::len);
        if source.len() + destination_len > usize::from(self.profile.max_loops_per_hook) {
            return Err(Refusal::new(
                RefusalCode::MachineStateInfeasible,
                "parent transfer would exceed the machine hook-stack limit",
            ));
        }
        if self.route_cost(&[(from, to)]).is_none() {
            return Err(Refusal::new(
                RefusalCode::MaterialProcessHazard,
                "parent transfer exceeds the profile's live cross-bed racking strain envelope",
            )
            .repair(
                "use smaller staged transfers through available hooks or reduce the shaping step",
            ));
        }
        self.ops.push(KnitoutOp::Rack {
            quarter_pitches: rack as i32,
        });
        self.ops.push(KnitoutOp::Xfer { from, to });
        self.ops.push(KnitoutOp::Rack { quarter_pitches: 0 });
        self.live.remove(&from);
        // Crossing beds reverses the transferred hook stack. A same-bed move
        // crosses twice; temporary parking also restores the original order.
        source.reverse();
        self.live.entry(to).or_default().extend(source);
        self.observe_transfer_bridges(rack as i32);
        Ok(())
    }

    fn transfer(&mut self, from: Needle, to: Needle) -> Result<(), Refusal> {
        if from == to {
            return Ok(());
        }
        if !self.profile.has_back_bed {
            return Err(Refusal::new(
                RefusalCode::MachineStateInfeasible,
                "parent transfer requires a second bed",
            )
            .repair("use a two-bed profile or an explicitly manual shaping workflow"));
        }
        if from.bed != to.bed {
            return self.cross(from, to);
        }

        // A same-bed move crosses twice. An intermediate near the midpoint splits
        // the rack displacement instead of placing the whole move on one crossing.
        // Admit the complete local route before emitting it, including rack-zero
        // returns and temporary parking of unrelated live stacks.
        let reach = self.profile.max_racking_quarter_pitches / 4;
        let lo = (i64::from(from.index) - i64::from(reach))
            .max(i64::from(to.index) - i64::from(reach))
            .max(0);
        let hi = (i64::from(from.index) + i64::from(reach))
            .min(i64::from(to.index) + i64::from(reach))
            .min(i64::from(self.profile.needle_count) - 1);
        if lo <= hi {
            const MAX_ROUTE_CANDIDATES: usize = 16_384;
            if hi - lo + 1 > MAX_ROUTE_CANDIDATES as i64 {
                return Err(Refusal::new(
                    RefusalCode::BudgetExceeded,
                    "parent transfer candidate range exceeds the bounded local route search",
                ));
            }
            let mut middles: Vec<i64> = (lo..=hi).collect();
            middles.sort_by_key(|&index| {
                (
                    (index - i64::from(from.index))
                        .abs()
                        .max((index - i64::from(to.index)).abs()),
                    (index - i64::from(from.index)).abs(),
                    index,
                )
            });
            let mut best: Option<(u32, Vec<(Needle, Needle)>)> = None;
            for &index in &middles {
                let middle = Needle::new(from.bed.opposite(), index as i32);
                if !self.live.contains_key(&middle) {
                    let route = vec![(from, middle), (middle, to)];
                    if let Some(cost) = self.route_cost(&route)
                        && best.as_ref().is_none_or(|(previous, _)| cost < *previous)
                    {
                        best = Some((cost, route));
                    }
                }
            }
            if let Some((_, route)) = best.take() {
                for (source, target) in route {
                    self.cross(source, target)?;
                }
                return Ok(());
            }

            // Full-gauge tubes occupy both beds. Temporarily park the opposite
            // loop on a free nearby hook, move the requested stack, then restore
            // that unrelated loop before the next stitch is formed.
            let mut candidates = middles.len();
            for index in middles {
                let middle = Needle::new(from.bed.opposite(), index as i32);
                if !self.live.contains_key(&middle) {
                    continue;
                }
                let spare_lo = (index - i64::from(reach)).max(0);
                let spare_hi =
                    (index + i64::from(reach)).min(i64::from(self.profile.needle_count) - 1);
                for spare_index in spare_lo..=spare_hi {
                    candidates += 1;
                    if candidates > MAX_ROUTE_CANDIDATES {
                        return Err(Refusal::new(
                            RefusalCode::BudgetExceeded,
                            "parent transfer parking search exceeded its bounded local route work",
                        ));
                    }
                    let spare = Needle::new(from.bed, spare_index as i32);
                    if spare == from || spare == to || self.live.contains_key(&spare) {
                        continue;
                    }
                    let route = vec![
                        (middle, spare),
                        (from, middle),
                        (middle, to),
                        (spare, middle),
                    ];
                    if let Some(cost) = self.route_cost(&route)
                        && best.as_ref().is_none_or(|(previous, _)| cost < *previous)
                    {
                        best = Some((cost, route));
                    }
                }
            }
            if let Some((_, route)) = best {
                for (source, target) in route {
                    self.cross(source, target)?;
                }
                return Ok(());
            }
        }
        Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            format!(
                "no parent-preserving transfer from {} to {} within the available hooks, racking range and live bridge-strain envelope",
                from.knitout_token(),
                to.knitout_token()
            ),
        )
        .repair("leave a free transfer hook near the shaping edge or reduce the shaping step"))
    }

    /// Move complete earlier-course parent stacks before the yarn-forming pass.
    /// A transfer into a decrease frees the next target in a sleeve shift; resolving
    /// this dependency first avoids making carrier direction dictate garment topology.
    fn prepare_course(
        &mut self,
        nodes: &[&StitchNode],
        map_needle: &impl Fn(Bed, i32) -> Needle,
    ) -> Result<(), Refusal> {
        let mut used = BTreeSet::new();
        for node in nodes {
            for parent in &node.parents {
                if !used.insert(parent.stitch_id) {
                    return Err(topology_refusal(
                        node,
                        "two stitches in one course claim the same live parent",
                    ));
                }
            }
        }
        let mut pending = nodes.to_vec();
        while !pending.is_empty() {
            let count = pending.len();
            let mut blocked = Vec::new();
            for node in pending {
                let target = map_needle(node.bed, node.wale);
                let mut parents = node.parents.clone();
                parents.sort_by_key(|parent| parent.stack_pos);
                let expected: Vec<_> = parents.iter().map(|parent| parent.stitch_id).collect();
                let present = self.live.get(&target).map_or(&[][..], Vec::as_slice);
                if expected.starts_with(present) {
                    self.gather(node, target)?;
                } else {
                    blocked.push(node);
                }
            }
            if blocked.len() == count {
                return Err(topology_refusal(
                    blocked[0],
                    "parent placement has a cycle or a target contains undeclared loops; crossings require an explicit cable group",
                ));
            }
            pending = blocked;
        }
        Ok(())
    }

    /// Park a declared front-bed cable on empty opposite hooks. Returning the
    /// over-group first leaves it on the front bed while the under-group crosses
    /// behind it during the second rack. Every source owns one identified loop.
    fn cable(
        &mut self,
        cable: &CableGroup,
        nodes: &[&StitchNode],
        map_needle: &impl Fn(Bed, i32) -> Needle,
    ) -> Result<(), Refusal> {
        let mut left_moves = Vec::new();
        let mut right_moves = Vec::new();
        for (&left, &right) in cable.left_wales.iter().zip(&cable.right_wales) {
            for (from_wale, to_wale, moves) in [
                (left, right, &mut left_moves),
                (right, left, &mut right_moves),
            ] {
                let from = map_needle(Bed::Front, from_wale);
                let to = map_needle(Bed::Front, to_wale);
                let back = map_needle(Bed::Back, from_wale);
                let node = nodes.iter().copied().find(|node| node.bed == Bed::Front && node.wale == to_wale)
                    .ok_or_else(|| Refusal::new(
                        RefusalCode::MachineStateInfeasible,
                        "cable crossing target must be a front-bed stitch in its declared course",
                    ))?;
                let source = self.live.get(&from).map_or(&[][..], Vec::as_slice);
                if node.kind != LoopKind::Cable
                    || source.len() != 1
                    || node.parents.len() != 1
                    || node.parents[0].stack_pos != 0
                    || node.parents[0].stitch_id != source[0]
                    || self.live.contains_key(&back)
                {
                    return Err(topology_refusal(
                        node,
                        "cable target must consume the opposite block's single live loop and have an empty backing hook",
                    ));
                }
                moves.push((from, back, to));
            }
        }
        for &(front, back, _) in left_moves.iter().chain(&right_moves) {
            self.cross(front, back)?;
        }
        let (over, under) = if cable.left_in_front {
            (&left_moves, &right_moves)
        } else {
            (&right_moves, &left_moves)
        };
        for &(_, back, target) in over.iter().chain(under) {
            self.cross(back, target)?;
        }
        Ok(())
    }

    fn gather(&mut self, node: &StitchNode, target: Needle) -> Result<(), Refusal> {
        let mut declared = node.parents.clone();
        declared.sort_by_key(|parent| parent.stack_pos);
        if declared
            .iter()
            .enumerate()
            .any(|(i, parent)| usize::from(parent.stack_pos) != i)
        {
            return Err(topology_refusal(
                node,
                "parent stack positions must start at zero without gaps",
            ));
        }
        let expected: Vec<u32> = declared.iter().map(|parent| parent.stitch_id).collect();
        let unique: BTreeSet<u32> = expected.iter().copied().collect();
        if unique.len() != expected.len() {
            return Err(topology_refusal(
                node,
                "the same parent loop is listed more than once",
            ));
        }
        if node.kind == LoopKind::CastOn {
            if !expected.is_empty() {
                return Err(topology_refusal(
                    node,
                    "cast-on cannot consume existing parent loops",
                ));
            }
        } else if expected.is_empty() {
            return Err(topology_refusal(
                node,
                "a knitted stitch requires at least one live parent",
            ));
        }
        if expected.len() > usize::from(self.profile.max_loops_per_hook) {
            return Err(topology_refusal(
                node,
                "declared parents exceed the machine hook-stack limit",
            ));
        }
        let present = self.live.get(&target).cloned().unwrap_or_default();
        if !expected.starts_with(&present) {
            return Err(topology_refusal(
                node,
                "target hook contains undeclared loops or a different bottom-to-top parent order",
            ));
        }
        let mut placed = present.len();
        while placed < expected.len() {
            let parent = expected[placed];
            let (source, stack) = self
                .live
                .iter()
                .find(|(_, stack)| stack.contains(&parent))
                .map(|(needle, stack)| (*needle, stack.clone()))
                .ok_or_else(|| {
                    topology_refusal(
                        node,
                        format!("parent {parent} was consumed already or has not been formed"),
                    )
                })?;
            if source == target || !expected[placed..].starts_with(&stack) {
                return Err(topology_refusal(
                    node,
                    "requested parents cannot be separated from their current hook stack",
                ));
            }
            self.transfer(source, target)?;
            placed += stack.len();
        }
        if self.live.get(&target).map_or(&[][..], Vec::as_slice) != expected.as_slice() {
            return Err(topology_refusal(
                node,
                "bed transfers cannot preserve the declared parent stack order",
            ));
        }
        Ok(())
    }
}

/// A bounded flat neckline: the center stays on hold while two independently carried
/// shoulders are worked. These identities are checked against the actual live hooks;
/// metadata alone never exempts an arbitrary stranded loop from the final graph check.
struct ShoulderSchedule {
    start: u32,
    held: BTreeSet<u32>,
    left_carrier: CarrierId,
    right_carrier: CarrierId,
    left_wale: i32,
    right_wale: i32,
}

impl ShoulderSchedule {
    fn read(doc: &KnitDoc) -> Result<Option<Self>, Refusal> {
        let Some(start) = doc.metadata.get("shoulder_start_course") else {
            return Ok(None);
        };
        let invalid = || {
            Refusal::new(
            RefusalCode::MachineStateInfeasible,
            "neckline schedule must identify two separate shoulder carriers and every held center loop",
        ).repair("redraft the neckline with separate carriers and explicit held-neck stitch identities")
        };
        let start = start.parse::<u32>().map_err(|_| invalid())?;
        let previous_course = doc
            .stitches
            .iter()
            .filter(|node| node.course < start)
            .map(|node| node.course)
            .max()
            .ok_or_else(invalid)?;
        if !doc.stitches.iter().any(|node| node.course == start) {
            return Err(invalid());
        }
        let carrier = |key: &str| {
            doc.metadata
                .get(key)
                .ok_or_else(invalid)
                .and_then(|value| CarrierId::new(value.clone()))
        };
        let left_carrier = carrier("left_shoulder_carrier")?;
        let right_carrier = carrier("right_shoulder_carrier")?;
        if left_carrier == right_carrier {
            return Err(invalid());
        }
        for (carrier, direction) in [(&left_carrier, "plus"), (&right_carrier, "minus")] {
            if doc
                .metadata
                .get(&format!("shoulder_initial_direction_{}", carrier.as_str()))
                .map(String::as_str)
                != Some(direction)
            {
                return Err(invalid());
            }
        }
        let mut held = BTreeSet::new();
        let mut wales = BTreeSet::new();
        for value in doc
            .metadata
            .get("held_neck_stitch_ids")
            .ok_or_else(invalid)?
            .split(',')
        {
            let id = value.parse::<u32>().map_err(|_| invalid())?;
            if !held.insert(id) {
                return Err(invalid());
            }
        }
        let mut found = 0;
        for node in doc.stitches.iter().filter(|node| held.contains(&node.id)) {
            if node.course != previous_course || node.bed != Bed::Front {
                return Err(invalid());
            }
            found += 1;
            wales.insert(node.wale);
        }
        if found != held.len() {
            return Err(invalid());
        }
        let left_wale = *wales.first().ok_or_else(invalid)?;
        let right_wale = *wales.last().ok_or_else(invalid)?;
        if i64::from(right_wale) - i64::from(left_wale) + 1 != wales.len() as i64 {
            return Err(invalid());
        }
        for node in doc.stitches.iter().filter(|node| node.course >= start) {
            let carrier = if node.wale < left_wale {
                &left_carrier
            } else if node.wale > right_wale {
                &right_carrier
            } else {
                return Err(invalid());
            };
            if node.bed != Bed::Front
                || node.carrier_ids.as_slice() != std::slice::from_ref(carrier)
                || node
                    .parents
                    .iter()
                    .any(|parent| held.contains(&parent.stitch_id))
            {
                return Err(invalid());
            }
        }
        Ok(Some(Self {
            start,
            held,
            left_carrier,
            right_carrier,
            left_wale,
            right_wale,
        }))
    }
}

fn validate_qualified_gauge(
    doc: &KnitDoc,
    profile: &MachineProfile,
    qualification: &KnitSwatchQualification,
) -> Result<(), Refusal> {
    if !doc.gauge.relaxed_after_wash {
        return Err(Refusal::new(
            RefusalCode::UnqualifiedProcess,
            "production knit sizing must use the washed and blocked swatch gauge",
        )
        .repair("draft with the relaxed gauge measured in the selected swatch qualification"));
    }
    for (field, actual, qualified) in [
        (
            "needle_pitch_mm",
            doc.gauge.needle_pitch_mm,
            profile.needle_pitch_mm,
        ),
        (
            "wales_per_100mm",
            doc.gauge.wales_per_100mm,
            qualification.wales_per_100mm,
        ),
        (
            "courses_per_100mm",
            doc.gauge.courses_per_100mm,
            qualification.courses_per_100mm,
        ),
    ] {
        if actual != qualified {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                format!(
                    "production knit {field} ({actual}) disagrees with the qualified machine/swatch value ({qualified})",
                ),
            )
            .repair("use the selected machine and measured swatch values, or qualify the changed process independently"));
        }
    }
    if let Some(thickness) = doc.gauge.thickness_mm
        && thickness != qualification.thickness_mm
    {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            "production knit thickness disagrees with the selected swatch qualification",
        )
        .repair(
            "retain the measured swatch thickness or leave unprovided document thickness unknown",
        ));
    }
    if let Some(stitch) = doc
        .stitches
        .iter()
        .find(|stitch| stitch.cam_stitch_setting.setting != qualification.cam_stitch_setting)
    {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            format!(
                "stitch {} uses cam setting {}, but its swatch was qualified at {}",
                stitch.id,
                stitch.cam_stitch_setting.setting,
                qualification.cam_stitch_setting,
            ),
        )
        .repair("use the qualified stitch-cam setting throughout this document, or qualify the other settings independently"));
    }
    if let Some(stitch) = doc
        .stitches
        .iter()
        .find(|stitch| stitch.cam_stitch_setting.machine_family != profile.id)
    {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            format!(
                "stitch {} declares machine family '{}', but the selected machine is '{}'",
                stitch.id, stitch.cam_stitch_setting.machine_family, profile.id,
            ),
        )
        .repair("use stitch-cam settings from the selected machine family"));
    }
    Ok(())
}

fn validate_short_row_turns(doc: &KnitDoc) -> Result<(), Refusal> {
    if doc.short_row_turns.is_empty() {
        return Ok(());
    }
    if doc.passes.is_empty() {
        return Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            "short rows require explicit ordered passes and real turnaround stitches",
        )
        .repair(
            "provide a tuck on a held loop at each anchored turn, then knit it off on rejoining",
        ));
    }
    let nodes: BTreeMap<_, _> = doc.stitches.iter().map(|node| (node.id, node)).collect();
    let mut seen = BTreeSet::new();
    for turn in &doc.short_row_turns {
        if !seen.insert((turn.course, turn.turn_wale)) {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                "duplicate short-row turn",
            ));
        }
        let candidates: Vec<_> = doc
            .passes
            .iter()
            .enumerate()
            .filter(|(_, pass)| {
                let last = nodes[pass.stitch_ids.last().expect("validated nonempty pass")];
                last.course == turn.course && last.wale == turn.turn_wale
            })
            .collect();
        let [(index, pass)] = candidates.as_slice() else {
            return Err(Refusal::new(
                RefusalCode::MachineStateInfeasible,
                "short-row turn must identify exactly one pass endpoint",
            ));
        };
        let last = nodes[pass.stitch_ids.last().expect("validated nonempty pass")];
        let next = doc
            .passes
            .iter()
            .skip(*index + 1)
            .find(|next| nodes[&next.stitch_ids[0]].carrier_ids == last.carrier_ids)
            .ok_or_else(|| {
                topology_refusal(last, "short-row yarn never returns from its declared turn")
            })?;
        let first = nodes[&next.stitch_ids[0]];
        let returns_inside = match next.direction {
            CourseDir::Plus => first.wale > last.wale,
            CourseDir::Minus => first.wale < last.wale,
        };
        if turn.dir_after != pass.direction.reversed()
            || next.direction != turn.dir_after
            || !returns_inside
            || first.bed != last.bed
            || (turn.anchored && (last.kind != LoopKind::Tuck || last.parents.is_empty()))
        {
            return Err(topology_refusal(
                last,
                "short-row anchor and return direction disagree with the actual yarn-forming passes",
            ));
        }
    }
    Ok(())
}

/// Compiles a [`KnitDoc`] into a verified [`KnitoutProgram`] and sealed [`KnitMachineCert`].
///
/// # Errors
/// Fails closed if qualification is required and missing/mismatched, if target bed width is exceeded,
/// or if any of the 7 V-bed VM safety/child-safety checks fail.
pub fn compile_knit_doc(
    doc: &KnitDoc,
    profile: &MachineProfile,
    qualification: Option<&KnitSwatchQualification>,
    expected_material_sha256: Option<&str>,
    drape_receipt_sha256: Option<String>,
) -> Result<CompiledKnitBundle, Refusal> {
    doc.validate()?;
    profile.validate()?;
    let machine_sha256 = profile.canonical_sha256()?;

    if profile.needle_count > i32::MAX as u32 {
        return Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            "machine bed exceeds the signed needle-address range",
        ));
    }
    let production = doc.wearer_age_class != WearerAgeClass::CalibrationSwatchOnly;
    if let Some(retained) = doc.metadata.get("material_sha256")
        && expected_material_sha256 != Some(retained.as_str())
    {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            "the selected material digest disagrees with the knit document's retained material identity",
        )
        .repair("select the original material or draft a new document with the replacement qualification"));
    }
    let qual_sha = if let Some(q) = qualification {
        let material = if production {
            expected_material_sha256
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| {
                    Refusal::new(
                        RefusalCode::UnqualifiedProcess,
                        "production qualification requires the independently selected material digest",
                    )
                    .repair("supply the exact active material digest; do not copy an unrelated swatch identity")
                })?
        } else {
            expected_material_sha256.unwrap_or(&q.material_sha256)
        };
        q.validate_for_production(material, &machine_sha256)?;
        if let Some(retained_lot) = doc.metadata.get("yarn_lot_id")
            && retained_lot != &q.yarn_lot_id
        {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                "the knit document's retained yarn lot disagrees with the supplied qualification",
            ));
        }
        if production {
            validate_qualified_gauge(doc, profile, q)?;
        }
        Some(q.canonical_sha256()?)
    } else if production && profile.requires_calibration_swatch {
        return Err(Refusal::new(
            RefusalCode::UnqualifiedProcess,
            format!(
                "production knit compilation for machine '{}' requires a washed KnitSwatchQualification",
                profile.id
            ),
        )
        .repair("measure a washed calibration swatch and provide its qualification and exact active material digest"));
    } else {
        None
    };

    validate_short_row_turns(doc)?;
    let mut cables_by_course: BTreeMap<u32, Vec<&CableGroup>> = BTreeMap::new();
    let mut cable_targets = BTreeSet::new();
    for cable in &doc.cable_groups {
        cables_by_course
            .entry(cable.course)
            .or_default()
            .push(cable);
        for &wale in cable.left_wales.iter().chain(&cable.right_wales) {
            cable_targets.insert((cable.course, Bed::Front, wale));
        }
    }
    for node in &doc.stitches {
        if (node.kind == LoopKind::Cable)
            != cable_targets.remove(&(node.course, node.bed, node.wale))
        {
            return Err(topology_refusal(
                node,
                "every cable stitch must match exactly one declared cable target",
            ));
        }
        let setting = node.cam_stitch_setting.setting;
        let tenths = setting * 10.0;
        if !tenths.is_finite()
            || tenths < f64::from(i32::MIN)
            || tenths > f64::from(i32::MAX)
            || setting != tenths.round() / 10.0
        {
            return Err(topology_refusal(
                node,
                "cam setting cannot be represented exactly in the Knitout AST's integer tenths",
            ));
        }
        if node.bed.is_slider()
            || !matches!(
                node.kind,
                LoopKind::CastOn
                    | LoopKind::Tuck
                    | LoopKind::Knit
                    | LoopKind::Purl
                    | LoopKind::Cable
                    | LoopKind::Decrease
                    | LoopKind::BindOff
            )
        {
            return Err(topology_refusal(
                node,
                "this loop operation has no supported parent-preserving lowering",
            ));
        }
    }

    if !cable_targets.is_empty() {
        return Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            "a declared cable crossing has no matching front-bed stitch",
        ));
    }

    // Group stitches deterministically by course
    let mut by_course: BTreeMap<u32, Vec<&StitchNode>> = BTreeMap::new();
    let mut min_wale = i32::MAX;
    let mut max_wale = i32::MIN;
    let mut used_carriers: BTreeSet<CarrierId> = BTreeSet::new();
    let mut last_carrier_course = BTreeMap::new();
    for st in &doc.stitches {
        by_course.entry(st.course).or_default().push(st);
        min_wale = min_wale.min(st.wale);
        max_wale = max_wale.max(st.wale);
        for c in &st.carrier_ids {
            used_carriers.insert(c.clone());
            last_carrier_course
                .entry(c.clone())
                .and_modify(|last: &mut u32| {
                    *last = (*last).max(st.course);
                })
                .or_insert(st.course);
        }
    }
    if used_carriers.is_empty() {
        used_carriers.insert(profile.carrier_ids[0].clone());
    }
    let carriers_vec: Vec<CarrierId> = used_carriers.into_iter().collect();
    let mut explicit_schedule: BTreeMap<u32, Vec<(CourseDir, &StitchNode)>> = BTreeMap::new();
    if !doc.passes.is_empty() {
        let nodes: BTreeMap<_, _> = doc.stitches.iter().map(|node| (node.id, node)).collect();
        for pass in &doc.passes {
            for id in &pass.stitch_ids {
                let node = nodes[id];
                explicit_schedule
                    .entry(node.course)
                    .or_default()
                    .push((pass.direction, node));
            }
        }
    }

    // Center pattern on the machine bed (`0..needle_count`) without i32 subtraction overflow
    let span_i64 = i64::from(max_wale) - i64::from(min_wale) + 1;
    if span_i64 > i64::from(profile.needle_count) {
        return Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            format!(
                "pattern requires {span_i64} needles but machine '{}' has only {}",
                profile.id, profile.needle_count
            ),
        ));
    }
    let span_needles = span_i64 as u32;
    let bed_offset_i64 = i64::from((profile.needle_count - span_needles) / 2) - i64::from(min_wale);
    let map_needle = |bed: Bed, wale: i32| -> Needle {
        Needle::new(bed, (i64::from(wale) + bed_offset_i64) as i32)
    };

    let mut ops: Vec<KnitoutOp> = Vec::new();
    ops.push(KnitoutOp::Comment {
        text: format!("FrankenTextiles ftx-knit/1 -> Knitout v2 ({})", doc.name),
    });

    if profile.min_carrier_spacing > 0 {
        ops.push(KnitoutOp::CarrierSpacing {
            needles: profile.min_carrier_spacing,
        });
    }
    if doc.wearer_age_class == WearerAgeClass::CalibrationSwatchOnly {
        if profile.automated_carriage_control {
            ops.push(KnitoutOp::SpeedNumber { speed: 150 });
        }
        ops.push(KnitoutOp::Comment {
            text: format!(
                "CalibrationSwatchOnly safety caps: speed <= {:.0}%, takedown <= {:.0}%",
                CalibrationSwatchCaps::MAX_SPEED_FRACTION * 100.0,
                CalibrationSwatchCaps::MAX_TAKEDOWN_FRACTION * 100.0
            ),
        });
    }

    let shoulders = ShoulderSchedule::read(doc)?;
    if let Some(schedule) = &shoulders {
        let last_course = *by_course
            .last_key_value()
            .expect("validated nonempty document")
            .0;
        let held_courses = last_course.saturating_sub(schedule.start).saturating_add(1);
        if held_courses > profile.max_roller_imbalance_courses {
            return Err(Refusal::new(
                RefusalCode::MaterialProcessHazard,
                format!("neckline roller imbalance spans {held_courses} source courses, exceeding the machine holding cap {}", profile.max_roller_imbalance_courses),
            ).repair("reduce the held neckline depth or use a separately verified manual holding/finishing workflow"));
        }
    }
    let mut compiler = LoopCompiler {
        profile,
        live: BTreeMap::new(),
        bridge_racks: BTreeSet::new(),
        ops,
    };
    let mut flat_dir = CourseDir::Plus;
    let mut last_cam_int: Option<i32> = None;
    let mut active_carriers = BTreeSet::new();
    let mut hooked_since: BTreeMap<CarrierId, _> = BTreeMap::new();
    let mut next_carrier_direction = BTreeMap::new();
    let mut last_pass_dir = None;
    let mut previous_layout: Option<bool> = None;
    let source_nodes: BTreeMap<_, _> = doc.stitches.iter().map(|node| (node.id, node)).collect();
    let mut last_infant_pass: BTreeMap<Vec<CarrierId>, (&StitchNode, CourseDir, BTreeSet<u32>)> =
        BTreeMap::new();
    let mut last_forming_segment: Option<(CourseDir, Vec<CarrierId>, bool, i32)> = None;

    for (&course_idx, nodes) in &by_course {
        let explicit = explicit_schedule.get(&course_idx);
        let mut addresses = BTreeSet::new();
        let mut wales = BTreeSet::new();
        for node in nodes {
            if !addresses.insert((node.bed, node.wale)) {
                return Err(topology_refusal(
                    node,
                    "duplicate needle address in one course",
                ));
            }
            wales.insert(node.wale);
        }
        let circular = explicit.is_none() && nodes.len() > wales.len();
        if circular && nodes.len() != 2 * wales.len() {
            return Err(Refusal::new(
                RefusalCode::MachineStateInfeasible,
                format!("course {course_idx} mixes partial tubes and flat rows"),
            )
            .repair("supply complete paired front/back courses or separate flat panels"));
        }
        if explicit.is_none() && previous_layout.is_some_and(|previous| previous != circular) {
            return Err(Refusal::new(
                RefusalCode::MachineStateInfeasible,
                "tube-to-panel joins and splits require an explicit supported schedule",
            )
            .repair("compile a complete tube or flat panel as one document"));
        }
        if explicit.is_none() {
            previous_layout = Some(circular);
        }

        let mut sorted: Vec<&StitchNode> = nodes.clone();
        let shoulder = shoulders
            .as_ref()
            .filter(|schedule| course_idx >= schedule.start);
        let mut directions = BTreeMap::new();
        if let Some(schedule) = shoulder
            && explicit.is_some()
            && course_idx == schedule.start
        {
            let held: BTreeSet<_> = compiler
                .live
                .iter()
                .filter(|(needle, _)| {
                    needle.bed == Bed::Front
                        && needle.index >= map_needle(Bed::Front, schedule.left_wale).index
                        && needle.index <= map_needle(Bed::Front, schedule.right_wale).index
                })
                .flat_map(|(_, stack)| stack.iter().copied())
                .collect();
            if held != schedule.held {
                return Err(topology_refusal(
                    nodes[0],
                    "declared held neckline identities do not match the live center hooks",
                ));
            }
        }
        if let Some(schedule) = explicit {
            sorted.clear();
            for &(direction, node) in schedule {
                directions.insert(node.id, direction);
                sorted.push(node);
            }
        } else if let Some(schedule) = shoulder {
            if circular {
                return Err(topology_refusal(
                    nodes[0],
                    "held-center shoulders require a flat front-bed panel",
                ));
            }
            if course_idx == schedule.start {
                let held: BTreeSet<_> = compiler
                    .live
                    .iter()
                    .filter(|(needle, _)| {
                        needle.bed == Bed::Front
                            && needle.index >= map_needle(Bed::Front, schedule.left_wale).index
                            && needle.index <= map_needle(Bed::Front, schedule.right_wale).index
                    })
                    .flat_map(|(_, stack)| stack.iter().copied())
                    .collect();
                if held != schedule.held {
                    return Err(topology_refusal(
                        nodes[0],
                        "declared held neckline identities do not match the live center hooks",
                    ));
                }
            }
            let mut sections = Vec::new();
            for (carrier, initial) in [
                (&schedule.left_carrier, CourseDir::Plus),
                (&schedule.right_carrier, CourseDir::Minus),
            ] {
                let dir = next_carrier_direction
                    .get(carrier)
                    .copied()
                    .unwrap_or(initial);
                if course_idx == schedule.start && dir != initial {
                    return Err(topology_refusal(
                        nodes[0],
                        "the shoulder carrier must begin at its existing outer edge",
                    ));
                }
                let mut section: Vec<_> = nodes
                    .iter()
                    .copied()
                    .filter(|node| node.carrier_ids.as_slice() == std::slice::from_ref(carrier))
                    .collect();
                section.sort_by_key(|node| node.wale);
                if section.is_empty()
                    || section
                        .windows(2)
                        .any(|pair| i64::from(pair[1].wale) - i64::from(pair[0].wale) != 1)
                {
                    return Err(topology_refusal(
                        nodes[0],
                        "each shoulder needs one contiguous pass with its own carrier",
                    ));
                }
                if dir == CourseDir::Minus {
                    section.reverse();
                }
                sections.push((dir, section));
            }
            // Return with the last-used carrier first, then work the other shoulder.
            // Keeping two directional passes per shoulder course avoids understating held
            // loop age by merging disconnected same-direction sections in the VM.
            sections.sort_by_key(|(dir, _)| last_pass_dir.is_some_and(|last| last == *dir));
            sorted.clear();
            for (dir, section) in sections {
                for node in section {
                    directions.insert(node.id, dir);
                    sorted.push(node);
                }
            }
        } else if circular {
            sorted.sort_by(|a, b| {
                a.bed.cmp(&b.bed).then_with(|| {
                    if a.bed == Bed::Front {
                        a.wale.cmp(&b.wale)
                    } else {
                        b.wale.cmp(&a.wale)
                    }
                })
            });
        } else {
            sorted.sort_by(|a, b| match flat_dir {
                CourseDir::Plus => a.wale.cmp(&b.wale),
                CourseDir::Minus => b.wale.cmp(&a.wale),
            });
        }

        let binding_off = sorted.iter().any(|node| node.kind == LoopKind::BindOff);
        if binding_off
            && (by_course.last_key_value().map(|(course, _)| *course) != Some(course_idx)
                || sorted.iter().any(|node| node.kind != LoopKind::BindOff))
        {
            return Err(Refusal::new(
                RefusalCode::MachineStateInfeasible,
                "bind-off must form one complete final course",
            )
            .repair(
                "finish shaping before the final chain or supply a separate manual completion",
            ));
        }

        if let Some(cables) = cables_by_course.get(&course_idx) {
            for cable in cables {
                compiler.cable(cable, nodes, &map_needle)?;
            }
        }
        if !binding_off {
            compiler.prepare_course(&sorted, &map_needle)?;
        }

        let mut shoulder_bind_chains = BTreeMap::new();
        for (position, node) in sorted.iter().copied().enumerate() {
            if binding_off {
                let mut parents = node.parents.clone();
                parents.sort_by_key(|parent| parent.stack_pos);
                let preceding = if shoulder.is_some() || explicit.is_some() {
                    shoulder_bind_chains.insert(node.carrier_ids[0].clone(), node.id)
                } else {
                    position.checked_sub(1).map(|index| sorted[index].id)
                };
                let expected_count = if preceding.is_none() { 1 } else { 2 };
                if parents.len() != expected_count
                    || (preceding.is_some()
                        && parents.last().map(|parent| parent.stitch_id) != preceding)
                {
                    return Err(topology_refusal(
                        node,
                        "bind-off must consume its own old loop, then the preceding same-course bind-off loop",
                    ));
                }
            }
            let dir = if let Some(dir) = directions.get(&node.id) {
                *dir
            } else if circular {
                if node.bed == Bed::Front {
                    CourseDir::Plus
                } else {
                    CourseDir::Minus
                }
            } else {
                flat_dir
            };
            if doc.wearer_age_class.is_infant()
                && let Some((previous, previous_dir, held)) =
                    last_infant_pass.get(&node.carrier_ids)
                && *previous_dir != dir
                && previous.bed == node.bed
                && (previous.kind != LoopKind::Tuck || previous.parents.is_empty())
                && held
                    .iter()
                    .any(|id| compiler.live.values().any(|stack| stack.contains(id)))
            {
                return Err(Refusal::new(
                    RefusalCode::MaterialProcessHazard,
                    format!("infant short-row return after stitch {} leaves live interior loops without a real tuck anchor", previous.id),
                ).repair("tuck into a held loop before reversing, then knit the old loop and anchor off together"));
            }
            for carrier in &node.carrier_ids {
                if active_carriers.insert(carrier.clone()) {
                    if profile.has_yarn_inserting_hooks {
                        // A V-bed machine has one physical yarn-inserting hook: release any
                        // previously held carrier before inserting a new carrier on the hook.
                        let held_others: Vec<_> = hooked_since.keys().cloned().collect();
                        for prev in held_others {
                            compiler.ops.push(KnitoutOp::ReleaseHook {
                                carriers: vec![prev.clone()],
                            });
                            hooked_since.remove(&prev);
                        }
                        hooked_since.insert(carrier.clone(), course_idx);
                        compiler.ops.push(KnitoutOp::InHook {
                            carriers: vec![carrier.clone()],
                        });
                    } else {
                        compiler.ops.push(KnitoutOp::In {
                            carriers: vec![carrier.clone()],
                        });
                    }
                }
                next_carrier_direction.insert(carrier.clone(), dir.reversed());
            }
            let needle = map_needle(node.bed, node.wale);
            compiler.gather(node, needle)?;
            let cam_int = (node.cam_stitch_setting.setting * 10.0).round() as i32;
            if last_cam_int != Some(cam_int) {
                compiler.ops.push(KnitoutOp::Stitch {
                    leading: cam_int,
                    trailing: cam_int,
                });
                last_cam_int = Some(cam_int);
            }
            let is_tuck = matches!(node.kind, LoopKind::CastOn | LoopKind::Tuck);
            if profile.backend_kind == super::profile::MachineBackendKind::KniterateKc
                && last_forming_segment.as_ref().is_some_and(
                    |(previous_dir, carriers, tucked, cam)| {
                        *previous_dir == dir
                            && carriers == &node.carrier_ids
                            && (*tucked != is_tuck || *cam != cam_int || binding_off)
                    },
                )
            {
                // Kniterate's default is 100 roller units per physical forming
                // segment. Extra tuck/knit or transferred bind-off segments in this
                // logical pass must not repeatedly pull held fabric down.
                compiler
                    .ops
                    .push(KnitoutOp::AddRollerAdvance { units: -100 });
            }
            last_forming_segment = Some((dir, node.carrier_ids.clone(), is_tuck, cam_int));
            compiler.ops.push(if is_tuck {
                KnitoutOp::Tuck {
                    dir,
                    needle,
                    carriers: node.carrier_ids.clone(),
                }
            } else {
                KnitoutOp::Knit {
                    dir,
                    needle,
                    carriers: node.carrier_ids.clone(),
                }
            });
            if node.kind == LoopKind::Tuck {
                compiler.live.entry(needle).or_default().push(node.id);
            } else {
                compiler.live.insert(needle, vec![node.id]);
            }
            if !compiler.live_bed_counts().contains(&0) {
                compiler.bridge_racks.insert(0);
            }
            if doc.wearer_age_class.is_infant() {
                let held = compiler
                    .live
                    .iter()
                    .filter(|(address, _)| {
                        if address.bed != needle.bed {
                            return false;
                        }
                        if let Some(schedule) = shoulder {
                            if node.wale < schedule.left_wale
                                && address.index >= map_needle(Bed::Front, schedule.left_wale).index
                            {
                                return false;
                            }
                            if node.wale > schedule.right_wale
                                && address.index
                                    <= map_needle(Bed::Front, schedule.right_wale).index
                            {
                                return false;
                            }
                        }
                        match dir {
                            CourseDir::Plus => address.index > needle.index,
                            CourseDir::Minus => address.index < needle.index,
                        }
                    })
                    .flat_map(|(_, stack)| stack.iter().copied())
                    .filter(|id| {
                        source_nodes[id].course < node.course
                            && !shoulders
                                .as_ref()
                                .is_some_and(|schedule| schedule.held.contains(id))
                    })
                    .collect();
                last_infant_pass.insert(node.carrier_ids.clone(), (node, dir, held));
            }
            last_pass_dir = Some(dir);
        }

        let ready_hooks: Vec<_> = hooked_since
            .iter()
            .filter(|(_, start)| **start < course_idx)
            .map(|(carrier, _)| carrier.clone())
            .collect();
        for carrier in ready_hooks {
            compiler.ops.push(KnitoutOp::ReleaseHook {
                carriers: vec![carrier.clone()],
            });
            hooked_since.remove(&carrier);
        }
        if by_course.last_key_value().map(|(last, _)| *last) != Some(course_idx) {
            let retired: Vec<_> = active_carriers
                .iter()
                .filter(|carrier| last_carrier_course.get(*carrier) == Some(&course_idx))
                .cloned()
                .collect();
            for carrier in retired {
                if hooked_since.remove(&carrier).is_some() {
                    compiler.ops.push(KnitoutOp::ReleaseHook {
                        carriers: vec![carrier.clone()],
                    });
                }
                compiler.ops.push(if profile.has_yarn_inserting_hooks {
                    KnitoutOp::OutHook {
                        carriers: vec![carrier.clone()],
                    }
                } else {
                    KnitoutOp::Out {
                        carriers: vec![carrier.clone()],
                    }
                });
                active_carriers.remove(&carrier);
            }
        }
        if !circular {
            flat_dir = flat_dir.reversed();
        }
    }

    for carrier in hooked_since.keys() {
        compiler.ops.push(KnitoutOp::ReleaseHook {
            carriers: vec![carrier.clone()],
        });
    }

    let final_ids: BTreeSet<u32> = by_course
        .last_key_value()
        .into_iter()
        .flat_map(|(_, nodes)| nodes.iter().map(|node| node.id))
        .collect();
    if compiler.live.values().flatten().any(|id| {
        !final_ids.contains(id)
            && !shoulders
                .as_ref()
                .is_some_and(|schedule| schedule.held.contains(id))
    }) {
        return Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            "the final course strands unconsumed loops from an earlier course",
        )
        .repair(
            "include every removed edge loop in a decrease or explicit bind-off parent chain",
        ));
    }

    // A bind-off chain consumes its predecessor as an explicit same-course parent.
    // Secure its last loop before dropping it; ordinary held loops require a manual
    // bind-off, including calibration swatches whose transfer prohibition is retained.
    let bind_off_ids: BTreeSet<u32> = doc
        .stitches
        .iter()
        .filter(|node| node.kind == LoopKind::BindOff)
        .map(|node| node.id)
        .collect();
    let finished: Vec<Needle> = compiler
        .live
        .iter()
        .filter(|(_, stack)| stack.len() == 1 && bind_off_ids.contains(&stack[0]))
        .map(|(needle, _)| *needle)
        .collect();
    for needle in finished {
        compiler.ops.push(KnitoutOp::Pause {
            note: Some(format!(
                "Secure the final bind-off loop on {} with the yarn tail before continuing.",
                needle.knitout_token()
            )),
        });
        if profile.backend_kind == super::profile::MachineBackendKind::KniterateKc {
            compiler
                .ops
                .push(KnitoutOp::AddRollerAdvance { units: -100 });
        }
        compiler.ops.push(KnitoutOp::Drop { needle });
        compiler.live.remove(&needle);
    }
    let requires_manual_bind_off = !compiler.live.is_empty();
    if requires_manual_bind_off {
        compiler.ops.push(KnitoutOp::Pause {
            note: Some(format!(
                "Manually secure and bind off the {} occupied hooks before removing the fabric; no automatic bind-off was compiled for these loops.",
                compiler.live.len()
            )),
        });
    }
    let mut ops = compiler.ops;

    // Park all carriers cleanly
    if profile.has_yarn_inserting_hooks {
        for c in &active_carriers {
            ops.push(KnitoutOp::OutHook {
                carriers: vec![c.clone()],
            });
        }
    } else {
        for c in &active_carriers {
            ops.push(KnitoutOp::Out {
                carriers: vec![c.clone()],
            });
        }
    }

    let program = KnitoutProgram {
        schema: KNITOUT_SCHEMA.to_string(),
        header: KnitoutHeader {
            machine: profile.id.clone(),
            gauge: format!("{:.1}", profile.gauge_e),
            width: profile.needle_count,
            position: "Center".to_string(),
            carriers: carriers_vec,
            extra: BTreeMap::new(),
        },
        ops,
    };

    let ctx = KnitSafetyContext {
        wearer_age_class: doc.wearer_age_class,
        blocked_wale_pitch_mm: doc.gauge.blocked_wale_pitch_mm(),
        source_doc: Some(doc),
        speed_fraction: if doc.wearer_age_class == WearerAgeClass::CalibrationSwatchOnly {
            Some(CalibrationSwatchCaps::MAX_SPEED_FRACTION)
        } else {
            None
        },
        takedown_fraction: if doc.wearer_age_class == WearerAgeClass::CalibrationSwatchOnly {
            Some(CalibrationSwatchCaps::MAX_TAKEDOWN_FRACTION)
        } else {
            None
        },
    };
    let vm_report = verify_knitout_program(&program, profile, &ctx)?;

    let mut limitations = Vec::new();
    if requires_manual_bind_off {
        limitations.push(
            "Live loops remain on the machine; the final operator pause requires manual bind-off before fabric removal.".to_string(),
        );
    }
    if shoulders.is_some() {
        limitations.push("The center neckline loops remain on hold while separately carried shoulders are knitted and chain-bound off. Finish the held neckline manually; no physical-fit claim is made.".to_string());
    }
    if profile.backend_kind.is_opaque_proprietary() {
        limitations.push(
            "Tier-B proprietary target: emitted Knitout v2 handoff IR only; vendor software must perform machine binary compilation.".to_string(),
        );
    }
    if !profile.automated_carriage_control {
        limitations.push(
            "Manual-carriage machine: operator advances carriage and performs manual rib/shaping transfers per synchronized checklist.".to_string(),
        );
    }

    let cert = KnitMachineCert::seal(
        vm_report.program_sha256.clone(),
        machine_sha256,
        qual_sha,
        drape_receipt_sha256,
        doc.wearer_age_class,
        profile.automated_carriage_control,
        vm_report.checks_passed.clone(),
        limitations,
        FourClaimsVerdict {
            parity_basis: "knitout_v2_vm_verified".to_string(),
            physics_basis: "discrete_vbed_kinematics_and_safety_bounds".to_string(),
            fit_claimed: false,
            performance_basis: "unbenchmarked".to_string(),
        },
    )?;

    Ok(CompiledKnitBundle {
        program,
        vm_report,
        cert,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knit::draft::{ParametricPanelSpec, draft_calibration_swatch, draft_shaped_panel};
    use crate::knit::ir::GaugeSpec;
    use crate::materials::{Evidence, Source};

    #[test]
    fn calibration_swatch_compiles_for_kniterate_without_inhook_and_passes_vm() {
        let prof = MachineProfile::kniterate_7e();
        let doc = draft_calibration_swatch(&prof, "wool-200tex", 30, 12, &[6.0, 8.0]).unwrap();
        let bundle = compile_knit_doc(&doc, &prof, None, None, None).unwrap();
        let text = bundle.program.to_knitout_text().unwrap();
        assert!(text.contains("\nin 1\n"));
        assert!(!text.contains("inhook"));
        assert!(!bundle.cert.hardware_execution_authorized);
        assert!(!bundle.cert.physical_knitting_validated);
        assert_eq!(bundle.vm_report.total_transfers, 0);
        bundle.cert.verify_integrity().unwrap();
    }

    #[test]
    fn shaped_panel_with_transfers_and_child_gates_compiles_and_enforces_drawstring_ban() {
        let prof = MachineProfile::kniterate_7e();
        let gauge = GaugeSpec {
            wales_per_100mm: 24.0,
            courses_per_100mm: 32.0,
            needle_pitch_mm: prof.needle_pitch_mm,
            yarn_tex: Some(180.0),
            thickness_mm: Some(2.0),
            relaxed_after_wash: true,
        };
        let mut spec = ParametricPanelSpec {
            name: "toddler_panel".to_string(),
            wearer_age_class: WearerAgeClass::Child3To14,
            hem_width_mm: 180.0,
            chest_width_mm: 180.0,
            length_mm: 120.0,
            armhole_depth_mm: 40.0,
            armhole_inset_total_mm: 30.0,
            neck_width_mm: 0.0,
            neck_depth_mm: 0.0,
            rib_hem_courses: 4,
            cam_setting: 8.0,
            yarn_id: "merino-180tex".to_string(),
            carrier_id: CarrierId("1".to_string()),
            has_hood_or_neck_drawstring: false,
        };
        let doc = draft_shaped_panel(&spec, &gauge, &prof).unwrap();
        // Without qualification -> UnqualifiedProcess!
        let err = compile_knit_doc(&doc, &prof, None, None, None).unwrap_err();
        assert_eq!(err.code, RefusalCode::UnqualifiedProcess);

        let qual = KnitSwatchQualification {
            id: "sw-merino".to_string(),
            material_sha256: "mat-123".to_string(),
            machine_sha256: prof.canonical_sha256().unwrap(),
            yarn_lot_id: "lot-1".to_string(),
            was_washed_and_blocked: true,
            wales_per_100mm: 24.0,
            courses_per_100mm: 32.0,
            thickness_mm: 2.0,
            cam_stitch_setting: 8.0,
            wick_dispersion_mm: None,
            shrinkage_axial_pct: None,
            yarn_intake_drift_eps: Some(0.002),
            yarn_intake_drift_sigma_eta: Some(0.015),
            evidence: Evidence::Measured,
            source: Source {
                reference: "local:sw-merino".to_string(),
                accessed_on: "2026-11-27".to_string(),
                note: "washed swatch".to_string(),
            },
            method: "flat dry 24h".to_string(),
            conditioning: BTreeMap::new(),
            allow_estimates: false,
        };
        let bundle = compile_knit_doc(&doc, &prof, Some(&qual), Some("mat-123"), None).unwrap();
        assert!(bundle.vm_report.total_transfers > 0);
        bundle.cert.verify_integrity().unwrap();
        // A swatch belonging to this machine is not evidence for a different sizing gauge,
        // thickness, or cam setting. Every ordinary compiler consumer shares this admission.
        for field in [
            "wales",
            "courses",
            "pitch",
            "thickness",
            "cam",
            "machine_family",
            "washed",
        ] {
            let mut changed = doc.clone();
            match field {
                "wales" => changed.gauge.wales_per_100mm += 1.0,
                "courses" => changed.gauge.courses_per_100mm += 1.0,
                "pitch" => changed.gauge.needle_pitch_mm += 0.1,
                "thickness" => changed.gauge.thickness_mm = Some(3.0),
                "cam" => {
                    // Check a non-first stitch: a matching initial cam does not qualify a
                    // different later setting that the old compiler would silently admit.
                    changed
                        .stitches
                        .last_mut()
                        .unwrap()
                        .cam_stitch_setting
                        .setting += 1.0;
                }
                "machine_family" => {
                    changed
                        .stitches
                        .last_mut()
                        .unwrap()
                        .cam_stitch_setting
                        .machine_family = "different-machine".to_string();
                }
                "washed" => changed.gauge.relaxed_after_wash = false,
                _ => unreachable!(),
            }
            let refusal =
                compile_knit_doc(&changed, &prof, Some(&qual), Some("mat-123"), None).unwrap_err();
            assert_eq!(
                refusal.code,
                if field == "washed" {
                    RefusalCode::UnqualifiedProcess
                } else {
                    RefusalCode::VerificationMismatch
                },
                "changed {field} must not reuse the original swatch qualification",
            );
            assert!(!refusal.repairs.is_empty());
        }
        let mut unknown_thickness = doc.clone();
        unknown_thickness.gauge.thickness_mm = None;
        assert!(
            compile_knit_doc(
                &unknown_thickness,
                &prof,
                Some(&qual),
                Some("mat-123"),
                None
            )
            .is_ok()
        );
        let unchanged = compile_knit_doc(&doc, &prof, Some(&qual), Some("mat-123"), None).unwrap();
        assert_eq!(bundle.cert.cert_sha256, unchanged.cert.cert_sha256);

        // Calibration must remain able to explore several cam settings before a relaxed
        // garment gauge exists; an optional existing qualification does not change its role.
        let stepped = draft_calibration_swatch(&prof, "merino-180tex", 12, 4, &[6.0, 8.0]).unwrap();
        assert!(!stepped.gauge.relaxed_after_wash);
        assert!(compile_knit_doc(&stepped, &prof, Some(&qual), Some("mat-123"), None).is_ok());

        assert_eq!(
            compile_knit_doc(&doc, &prof, Some(&qual), None, None)
                .unwrap_err()
                .code,
            RefusalCode::UnqualifiedProcess
        );

        // Drawstring on child garment MUST fail closed!
        spec.has_hood_or_neck_drawstring = true;
        let bad_doc = draft_shaped_panel(&spec, &gauge, &prof).unwrap();
        let err =
            compile_knit_doc(&bad_doc, &prof, Some(&qual), Some("mat-123"), None).unwrap_err();
        assert_eq!(err.code, RefusalCode::MaterialProcessHazard);
    }

    #[test]
    fn shima_profile_uses_inhook_and_releasehook() {
        let prof = MachineProfile::shima_seiki_12e_handoff();
        let doc = draft_calibration_swatch(&prof, "sportswear-68tex", 24, 10, &[20.0]).unwrap();
        let bundle = compile_knit_doc(&doc, &prof, None, None, None).unwrap();
        let txt = bundle.program.to_knitout_text().unwrap();
        assert!(txt.contains("inhook 1\n"));
        assert!(txt.contains("releasehook 1\n"));
        assert!(txt.contains("outhook 1\n"));
    }

    #[test]
    fn compile_validates_optional_swatch_qual_and_handles_extreme_wale_span() {
        let prof = MachineProfile::kniterate_7e();
        let mut doc = draft_calibration_swatch(&prof, "merino-180tex", 12, 4, &[7.0]).unwrap();
        let bad_qual = KnitSwatchQualification {
            id: "bad".to_string(),
            material_sha256: "mat".to_string(),
            machine_sha256: "wrong-machine-hash".to_string(),
            yarn_lot_id: "lot".to_string(),
            was_washed_and_blocked: true,
            wales_per_100mm: 24.0,
            courses_per_100mm: 32.0,
            thickness_mm: 2.0,
            cam_stitch_setting: 7.0,
            wick_dispersion_mm: None,
            shrinkage_axial_pct: None,
            yarn_intake_drift_eps: None,
            yarn_intake_drift_sigma_eta: None,
            evidence: Evidence::Measured,
            source: Source {
                reference: "local:sw".to_string(),
                accessed_on: "2026-11-27".to_string(),
                note: "sw".to_string(),
            },
            method: "flat dry".to_string(),
            conditioning: BTreeMap::new(),
            allow_estimates: false,
        };
        assert_eq!(
            compile_knit_doc(&doc, &prof, Some(&bad_qual), None, None)
                .unwrap_err()
                .code,
            RefusalCode::VerificationMismatch
        );

        // Extreme wale indices must return MachineStateInfeasible cleanly, never overflow i32
        doc.stitches[0].wale = i32::MIN + 1;
        doc.stitches[1].wale = i32::MAX - 1;
        assert_eq!(
            compile_knit_doc(&doc, &prof, None, None, None)
                .unwrap_err()
                .code,
            RefusalCode::MachineStateInfeasible
        );
    }

    #[test]
    fn compile_refuses_decrease_exceeding_machine_racking_limit() {
        let mut prof = MachineProfile::kniterate_7e();
        prof.max_racking_quarter_pitches = 0; // double-bed machine with zero racking
        let gauge = GaugeSpec {
            wales_per_100mm: 24.0,
            courses_per_100mm: 32.0,
            needle_pitch_mm: prof.needle_pitch_mm,
            yarn_tex: Some(180.0),
            thickness_mm: Some(2.0),
            relaxed_after_wash: true,
        };
        let doc = draft_shaped_panel(
            &ParametricPanelSpec {
                name: "shaped_zero_rack".to_string(),
                wearer_age_class: WearerAgeClass::Adult,
                hem_width_mm: 220.0,
                chest_width_mm: 220.0,
                length_mm: 180.0,
                armhole_depth_mm: 60.0,
                armhole_inset_total_mm: 40.0,
                neck_width_mm: 0.0,
                neck_depth_mm: 0.0,
                rib_hem_courses: 0,
                cam_setting: 8.0,
                yarn_id: "merino-180tex".to_string(),
                carrier_id: CarrierId("1".to_string()),
                has_hood_or_neck_drawstring: false,
            },
            &gauge,
            &prof,
        )
        .unwrap();
        let qual = KnitSwatchQualification {
            id: "sw-zero-rack".to_string(),
            material_sha256: "mat-123".to_string(),
            machine_sha256: prof.canonical_sha256().unwrap(),
            yarn_lot_id: "lot-1".to_string(),
            was_washed_and_blocked: true,
            wales_per_100mm: 24.0,
            courses_per_100mm: 32.0,
            thickness_mm: 2.0,
            cam_stitch_setting: 8.0,
            wick_dispersion_mm: None,
            shrinkage_axial_pct: None,
            yarn_intake_drift_eps: Some(0.002),
            yarn_intake_drift_sigma_eta: Some(0.015),
            evidence: Evidence::Measured,
            source: Source {
                reference: "local:sw".to_string(),
                accessed_on: "2026-11-27".to_string(),
                note: "washed swatch".to_string(),
            },
            method: "flat dry".to_string(),
            conditioning: BTreeMap::new(),
            allow_estimates: false,
        };
        let err = compile_knit_doc(&doc, &prof, Some(&qual), Some("mat-123"), None).unwrap_err();
        assert_eq!(err.code, RefusalCode::MachineStateInfeasible);
        assert!(err.message.contains("racking"));
    }
    fn topology_node(
        id: u32,
        course: u32,
        wale: i32,
        bed: Bed,
        kind: LoopKind,
        parents: &[u32],
    ) -> StitchNode {
        StitchNode {
            id,
            course,
            wale,
            bed,
            kind,
            parents: parents
                .iter()
                .enumerate()
                .map(|(position, parent)| crate::knit::ParentRef {
                    stitch_id: *parent,
                    stack_pos: position as u8,
                })
                .collect(),
            carrier_ids: vec![CarrierId("1".to_string())],
            yarn_id: "synthetic".to_string(),
            cam_stitch_setting: crate::knit::CamStitchSetting {
                setting: 4.0,
                machine_family: MachineProfile::hand_knit(4.0).id,
            },
            yarn_length_mm: None,
        }
    }

    fn topology_doc(stitches: Vec<StitchNode>) -> KnitDoc {
        KnitDoc {
            schema: crate::knit::KNIT_SCHEMA.to_string(),
            name: "synthetic loop topology".to_string(),
            wearer_age_class: WearerAgeClass::Adult,
            gauge: GaugeSpec {
                wales_per_100mm: 25.0,
                courses_per_100mm: 25.0,
                needle_pitch_mm: 4.0,
                yarn_tex: None,
                thickness_mm: Some(1.0),
                relaxed_after_wash: true,
            },
            stitches,
            short_row_turns: Vec::new(),
            passes: Vec::new(),
            cable_groups: Vec::new(),
            yarn_runs: Vec::new(),
            has_hood_or_neck_drawstring: false,
            metadata: BTreeMap::new(),
        }
    }

    // Independent replay associates each emitted loop-forming operation with a
    // hand-authored expected node order. Transfers must preserve every identity,
    // and each knit must consume exactly the source graph's ordered parents.
    fn replay_parentage(
        doc: &KnitDoc,
        program: &KnitoutProgram,
        order: &[u32],
    ) -> BTreeMap<Needle, Vec<u32>> {
        let nodes: BTreeMap<_, _> = doc.stitches.iter().map(|node| (node.id, node)).collect();
        let mut hooks: BTreeMap<Needle, Vec<u32>> = BTreeMap::new();
        let mut formed = 0;
        for op in &program.ops {
            match op {
                KnitoutOp::Tuck { needle, .. } | KnitoutOp::Knit { needle, .. } => {
                    let node = nodes[&order[formed]];
                    let actual = hooks.remove(needle).unwrap_or_default();
                    let mut parents = node.parents.clone();
                    parents.sort_by_key(|parent| parent.stack_pos);
                    let expected: Vec<_> = parents.iter().map(|parent| parent.stitch_id).collect();
                    assert_eq!(actual, expected, "parents of stitch {}", node.id);
                    assert_eq!(
                        matches!(op, KnitoutOp::Tuck { .. }),
                        matches!(node.kind, LoopKind::CastOn | LoopKind::Tuck)
                    );
                    let mut result = if node.kind == LoopKind::Tuck {
                        actual
                    } else {
                        Vec::new()
                    };
                    result.push(node.id);
                    hooks.insert(*needle, result);
                    formed += 1;
                }
                KnitoutOp::Xfer { from, to } => {
                    let mut stack = hooks.remove(from).expect("transfer must move live loops");
                    stack.reverse();
                    hooks.entry(*to).or_default().extend(stack);
                }
                KnitoutOp::Drop { needle } => {
                    assert!(hooks.remove(needle).is_some());
                }
                _ => {}
            }
        }
        assert_eq!(formed, order.len());
        hooks
    }

    fn anchored_short_row_doc() -> KnitDoc {
        use Bed::Front as F;
        use LoopKind::{BindOff, CastOn, Knit, Tuck};
        let mut stitches = Vec::new();
        for wale in 0..6 {
            stitches.push(topology_node(wale, 0, wale as i32, F, CastOn, &[]));
            stitches.push(topology_node(wale + 6, 1, wale as i32, F, Knit, &[wale]));
        }
        stitches.extend([
            topology_node(12, 2, 0, F, Knit, &[6]),
            topology_node(13, 2, 1, F, Knit, &[7]),
            topology_node(14, 2, 2, F, Knit, &[8]),
            topology_node(15, 2, 3, F, Knit, &[9]),
            // This anchor retains loop 10. It does not consume it or replace it.
            topology_node(16, 2, 4, F, Tuck, &[10]),
            topology_node(17, 3, 3, F, Knit, &[15]),
            topology_node(18, 3, 2, F, Knit, &[14]),
            topology_node(19, 3, 1, F, Knit, &[13]),
            topology_node(20, 3, 0, F, Knit, &[12]),
            topology_node(21, 4, 0, F, Knit, &[20]),
            topology_node(22, 4, 1, F, Knit, &[19]),
            topology_node(23, 4, 2, F, Knit, &[18]),
            topology_node(24, 4, 3, F, Knit, &[17]),
            // Rejoining must knit the held loop and its tuck together, in that order.
            topology_node(25, 4, 4, F, Knit, &[10, 16]),
            topology_node(26, 4, 5, F, Knit, &[11]),
            topology_node(27, 5, 5, F, BindOff, &[26]),
            topology_node(28, 5, 4, F, BindOff, &[25, 27]),
            topology_node(29, 5, 3, F, BindOff, &[24, 28]),
            topology_node(30, 5, 2, F, BindOff, &[23, 29]),
            topology_node(31, 5, 1, F, BindOff, &[22, 30]),
            topology_node(32, 5, 0, F, BindOff, &[21, 31]),
        ]);
        let mut doc = topology_doc(stitches);
        doc.passes = [
            (CourseDir::Plus, vec![0, 1, 2, 3, 4, 5]),
            (CourseDir::Minus, vec![11, 10, 9, 8, 7, 6]),
            (CourseDir::Plus, vec![12, 13, 14, 15, 16]),
            (CourseDir::Minus, vec![17, 18, 19, 20]),
            (CourseDir::Plus, vec![21, 22, 23, 24, 25, 26]),
            (CourseDir::Minus, vec![27, 28, 29, 30, 31, 32]),
        ]
        .into_iter()
        .map(|(direction, stitch_ids)| crate::knit::KnitPass {
            direction,
            stitch_ids,
        })
        .collect();
        doc.short_row_turns.push(crate::knit::ShortRowTurn {
            course: 2,
            turn_wale: 4,
            dir_after: CourseDir::Minus,
            anchored: true,
        });
        doc
    }

    #[test]
    fn anchored_short_rows_retain_the_held_loop_and_knit_off_the_real_tuck() {
        let doc = anchored_short_row_doc();
        let profile = MachineProfile::hand_knit(4.0);
        let bundle = compile_knit_doc(&doc, &profile, None, None, None).unwrap();
        let order = [
            0, 1, 2, 3, 4, 5, 11, 10, 9, 8, 7, 6, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            24, 25, 26, 27, 28, 29, 30, 31, 32,
        ];
        assert!(replay_parentage(&doc, &bundle.program, &order).is_empty());
        assert_eq!(bundle.vm_report.estimated_courses, 6);
        assert_eq!(
            bundle
                .program
                .ops
                .iter()
                .filter(|op| matches!(op, KnitoutOp::Tuck { .. }))
                .count(),
            7
        );
        let mut missing_anchor = doc.clone();
        missing_anchor
            .stitches
            .iter_mut()
            .find(|node| node.id == 16)
            .unwrap()
            .kind = LoopKind::Knit;
        assert!(
            compile_knit_doc(&missing_anchor, &profile, None, None, None)
                .unwrap_err()
                .message
                .contains("anchor")
        );
        let mut omitted_held_loop = doc.clone();
        omitted_held_loop
            .stitches
            .iter_mut()
            .find(|node| node.id == 25)
            .unwrap()
            .parents
            .remove(0);
        assert_eq!(
            compile_knit_doc(&omitted_held_loop, &profile, None, None, None)
                .unwrap_err()
                .code,
            RefusalCode::MachineStateInfeasible
        );
        let mut excessive_hold = profile;
        excessive_hold.max_roller_imbalance_courses = 1;
        assert!(compile_knit_doc(&doc, &excessive_hold, None, None, None).is_err());
    }

    #[test]
    fn explicit_passes_require_complete_causal_directional_order() {
        let original = anchored_short_row_doc();
        for defect in [
            "missing",
            "duplicate",
            "wrong_direction",
            "hidden_return",
            "wrong_course",
        ] {
            let mut doc = original.clone();
            match defect {
                "missing" => {
                    doc.passes[0].stitch_ids.pop();
                }
                "duplicate" => {
                    doc.passes[0].stitch_ids.push(5);
                }
                "wrong_direction" => {
                    doc.passes[0].stitch_ids.reverse();
                }
                "hidden_return" => {
                    doc.passes[1].direction = CourseDir::Plus;
                    doc.passes[1].stitch_ids.reverse();
                }
                "wrong_course" => {
                    doc.passes[0].stitch_ids[0] = 6;
                }
                _ => unreachable!(),
            }
            assert_eq!(
                doc.validate().unwrap_err().code,
                RefusalCode::InvalidDocument,
                "{defect}"
            );
        }
        // Old documents omit the additive schedule field and keep their canonical form.
        let legacy = topology_doc(vec![topology_node(
            0,
            0,
            0,
            Bed::Front,
            LoopKind::CastOn,
            &[],
        )]);
        let value = serde_json::to_value(&legacy).unwrap();
        assert!(value.get("passes").is_none());
        let decoded: KnitDoc = serde_json::from_value(value).unwrap();
        assert!(decoded.passes.is_empty());
        assert_eq!(
            decoded.canonical_sha256().unwrap(),
            legacy.canonical_sha256().unwrap()
        );
    }

    #[test]
    fn infant_interior_returns_require_real_anchors_even_without_metadata_or_passes() {
        let profile = MachineProfile::hand_knit(4.0);
        let mut anchored = anchored_short_row_doc();
        anchored.wearer_age_class = WearerAgeClass::InfantUnder3;
        compile_knit_doc(&anchored, &profile, None, None, None).unwrap();
        anchored.short_row_turns.clear();
        compile_knit_doc(&anchored, &profile, None, None, None).unwrap();
        let mut unanchored = anchored.clone();
        unanchored.stitches.retain(|node| node.id != 16);
        unanchored.passes[2].stitch_ids.retain(|id| *id != 16);
        unanchored
            .stitches
            .iter_mut()
            .find(|node| node.id == 25)
            .unwrap()
            .parents
            .retain(|parent| parent.stitch_id != 16);
        for legacy in [false, true] {
            let mut doc = unanchored.clone();
            if legacy {
                doc.passes.clear();
            }
            let refusal = compile_knit_doc(&doc, &profile, None, None, None).unwrap_err();
            assert_eq!(refusal.code, RefusalCode::MaterialProcessHazard);
            assert!(refusal.message.contains("real tuck anchor"));
        }
        let mut manual_center = neckline_doc();
        manual_center.wearer_age_class = WearerAgeClass::InfantUnder3;
        compile_knit_doc(&manual_center, &profile, None, None, None).unwrap();
    }

    #[test]
    fn kniterate_extra_forming_segments_and_final_drops_have_explicit_zero_takedown() {
        use Bed::Front as F;
        use LoopKind::{CastOn, Knit};
        let profile = MachineProfile::kniterate_7e();
        let qualification = topology_qualification(&profile);
        let widening = topology_doc(vec![
            topology_node(0, 0, 1, F, CastOn, &[]),
            topology_node(1, 0, 2, F, CastOn, &[]),
            topology_node(2, 1, 3, F, CastOn, &[]),
            topology_node(3, 1, 2, F, Knit, &[1]),
            topology_node(4, 1, 1, F, Knit, &[0]),
            topology_node(5, 1, 0, F, CastOn, &[]),
            topology_node(6, 2, 0, F, Knit, &[5]),
            topology_node(7, 2, 1, F, Knit, &[4]),
            topology_node(8, 2, 2, F, Knit, &[3]),
            topology_node(9, 2, 3, F, Knit, &[2]),
        ]);
        for (mut doc, expected_corrections) in [(anchored_short_row_doc(), 7), (widening, 2)] {
            doc.gauge.needle_pitch_mm = profile.needle_pitch_mm;
            for node in &mut doc.stitches {
                node.cam_stitch_setting.machine_family = profile.id.clone();
            }
            let bundle = compile_knit_doc(
                &doc,
                &profile,
                Some(&qualification),
                Some("synthetic-topology-material"),
                None,
            )
            .unwrap();
            let corrections: Vec<_> = bundle
                .program
                .ops
                .iter()
                .enumerate()
                .filter_map(|(index, op)| match op {
                    KnitoutOp::AddRollerAdvance { units } => Some((index, *units)),
                    _ => None,
                })
                .collect();
            assert_eq!(corrections.len(), expected_corrections);
            for (index, units) in corrections {
                assert_eq!(units, -100);
                assert!(matches!(
                    bundle.program.ops[index + 1],
                    KnitoutOp::Knit { .. } | KnitoutOp::Tuck { .. } | KnitoutOp::Drop { .. }
                ));
            }
            // The short-row specimen has six logical passes: its additional tuck,
            // five bind-off segments, and final drop explicitly add no take-down.
            if !doc.short_row_turns.is_empty() {
                assert_eq!(bundle.vm_report.estimated_courses, 6);
            }
        }
        let swatch = draft_calibration_swatch(&profile, "synthetic", 8, 2, &[4.0]).unwrap();
        let bundle = compile_knit_doc(&swatch, &profile, None, None, None).unwrap();
        assert!(
            !bundle
                .program
                .ops
                .iter()
                .any(|op| matches!(op, KnitoutOp::AddRollerAdvance { .. }))
        );
    }

    #[test]
    fn explicit_passes_cannot_hide_same_slot_or_changed_carrier_sweeps() {
        let original = anchored_short_row_doc();
        let mut same_slot = original.clone();
        same_slot
            .stitches
            .push(topology_node(33, 0, 0, Bed::Back, LoopKind::CastOn, &[]));
        same_slot.passes[0].stitch_ids.insert(1, 33);
        assert_eq!(
            same_slot.validate().unwrap_err().code,
            RefusalCode::InvalidDocument
        );

        let mut continued = original.clone();
        continued.passes[0].stitch_ids = vec![0, 1, 2];
        continued.passes.insert(
            1,
            crate::knit::KnitPass {
                direction: CourseDir::Plus,
                stitch_ids: vec![3, 4, 5],
            },
        );
        continued.validate().unwrap();
        let mut different_carrier = continued.clone();
        for node in &mut different_carrier.stitches {
            if (3..=5).contains(&node.id) {
                node.carrier_ids = vec![CarrierId("2".to_string())];
            }
        }
        assert_eq!(
            different_carrier.validate().unwrap_err().code,
            RefusalCode::InvalidDocument
        );
        let mut different_cam = continued;
        for node in &mut different_cam.stitches {
            if (3..=5).contains(&node.id) {
                node.cam_stitch_setting.setting = 5.0;
            }
        }
        assert_eq!(
            different_cam.validate().unwrap_err().code,
            RefusalCode::InvalidDocument
        );
    }

    #[test]
    fn compiler_preserves_tenth_cam_settings_and_refuses_finer_rounding() {
        let profile = MachineProfile::kniterate_7e();
        let doc = draft_calibration_swatch(&profile, "synthetic", 8, 2, &[7.5]).unwrap();
        let bundle = compile_knit_doc(&doc, &profile, None, None, None).unwrap();
        assert!(bundle.program.ops.iter().any(|op| matches!(
            op,
            KnitoutOp::Stitch {
                leading: 75,
                trailing: 75
            }
        )));
        let mut too_precise = doc;
        too_precise.stitches[0].cam_stitch_setting.setting = 6.04;
        assert!(
            compile_knit_doc(&too_precise, &profile, None, None, None)
                .unwrap_err()
                .message
                .contains("integer tenths")
        );
    }

    #[test]
    fn flat_decrease_and_bind_off_preserve_exact_parent_loops() {
        use Bed::Front as F;
        use LoopKind::{BindOff, CastOn, Decrease, Knit};
        let doc = topology_doc(vec![
            topology_node(0, 0, 0, F, CastOn, &[]),
            topology_node(1, 0, 1, F, CastOn, &[]),
            topology_node(2, 0, 2, F, CastOn, &[]),
            topology_node(3, 1, 1, F, Decrease, &[1, 0]),
            topology_node(4, 1, 2, F, Knit, &[2]),
            topology_node(5, 2, 1, F, BindOff, &[3]),
            topology_node(6, 2, 2, F, BindOff, &[4, 5]),
        ]);
        let bundle =
            compile_knit_doc(&doc, &MachineProfile::hand_knit(4.0), None, None, None).unwrap();
        assert!(replay_parentage(&doc, &bundle.program, &[0, 1, 2, 4, 3, 5, 6]).is_empty());
        assert!(bundle.program.ops.windows(2).any(|pair| {
            matches!(pair[0], KnitoutOp::Pause { .. }) && matches!(pair[1], KnitoutOp::Drop { .. })
        }));
    }

    #[test]
    fn tube_rounds_and_bind_off_keep_both_beds_and_restore_parked_loops() {
        use Bed::{Back as B, Front as F};
        use LoopKind::{BindOff, CastOn, Knit, Purl};
        let mut doc = topology_doc(vec![
            topology_node(0, 0, 0, F, CastOn, &[]),
            topology_node(1, 0, 1, F, CastOn, &[]),
            topology_node(2, 0, 0, B, CastOn, &[]),
            topology_node(3, 0, 1, B, CastOn, &[]),
            topology_node(4, 1, 0, F, Knit, &[0]),
            topology_node(5, 1, 1, F, Knit, &[1]),
            topology_node(6, 1, 0, B, Purl, &[2]),
            topology_node(7, 1, 1, B, Purl, &[3]),
            topology_node(8, 2, 0, F, BindOff, &[4]),
            topology_node(9, 2, 1, F, BindOff, &[5, 8]),
            topology_node(10, 2, 1, B, BindOff, &[7, 9]),
            topology_node(11, 2, 0, B, BindOff, &[6, 10]),
        ]);
        let profile = MachineProfile::hand_knit(4.0);
        let bundle = compile_knit_doc(&doc, &profile, None, None, None).unwrap();
        assert!(
            replay_parentage(
                &doc,
                &bundle.program,
                &[0, 1, 3, 2, 4, 5, 7, 6, 8, 9, 10, 11]
            )
            .is_empty()
        );
        let passes: Vec<_> = bundle
            .program
            .ops
            .iter()
            .filter_map(|op| match op {
                KnitoutOp::Tuck { needle, dir, .. } | KnitoutOp::Knit { needle, dir, .. } => {
                    Some((needle.bed, *dir))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            &passes[..4],
            &[
                (F, CourseDir::Plus),
                (F, CourseDir::Plus),
                (B, CourseDir::Minus),
                (B, CourseDir::Minus)
            ]
        );
        assert_eq!(bundle.vm_report.estimated_courses, 6);
        doc.stitches.reverse();
        let reordered = compile_knit_doc(&doc, &profile, None, None, None).unwrap();
        assert_eq!(bundle.program, reordered.program);
    }

    #[test]
    fn tubular_rib_keeps_logical_sides_separate_while_alternating_physical_beds() {
        use crate::knit::draft::{RaglanPulloverSpec, draft_seamless_raglan_tube};
        let profile = MachineProfile::hand_knit(4.0);
        let gauge = topology_doc(vec![]).gauge;
        let spec = RaglanPulloverSpec {
            name: "synthetic tubular rib topology".to_string(),
            wearer_age_class: WearerAgeClass::Child3To14,
            neck_girth_mm: 64.0,
            chest_girth_mm: 80.0,
            yoke_depth_mm: 16.0,
            body_length_below_arm_mm: 16.0,
            collar_rib_courses: 2,
            hem_rib_courses: 2,
            cam_setting: 4.0,
            yarn_id: "synthetic".to_string(),
            carrier_id: CarrierId("1".to_string()),
            has_hood_or_neck_drawstring: false,
        };
        let doc = draft_seamless_raglan_tube(&spec, &gauge, &profile).unwrap();
        let nodes: BTreeMap<_, _> = doc.stitches.iter().map(|node| (node.id, node)).collect();
        for (index, pass) in doc.passes.iter().enumerate() {
            assert_eq!(
                pass.direction,
                if index % 2 == 0 {
                    CourseDir::Plus
                } else {
                    CourseDir::Minus
                }
            );
            for id in &pass.stitch_ids {
                let node = nodes[id];
                assert_eq!(node.wale.rem_euclid(2), (index % 2) as i32);
                for parent in &node.parents {
                    let older = nodes[&parent.stitch_id];
                    if older.course < node.course {
                        assert_eq!(older.wale, node.wale, "rib transfer cannot swap tube sides");
                    }
                }
            }
        }
        for pass in &doc.passes[2..6] {
            let beds: BTreeSet<_> = pass.stitch_ids.iter().map(|id| nodes[id].bed).collect();
            assert_eq!(beds, BTreeSet::from([Bed::Front, Bed::Back]));
        }
        let bundle = compile_knit_doc(&doc, &profile, None, None, None).unwrap();
        let order: Vec<_> = doc
            .passes
            .iter()
            .flat_map(|pass| pass.stitch_ids.iter().copied())
            .collect();
        assert!(replay_parentage(&doc, &bundle.program, &order).is_empty());
        assert_eq!(bundle.vm_report.estimated_courses, doc.passes.len() as u32);
        assert!(bundle.vm_report.total_transfers > 0);
        let actual_dirs: Vec<_> = bundle
            .program
            .ops
            .iter()
            .filter_map(|op| match op {
                KnitoutOp::Knit { dir, .. } | KnitoutOp::Tuck { dir, .. } => Some(*dir),
                _ => None,
            })
            .collect();
        let expected_dirs: Vec<_> = doc
            .passes
            .iter()
            .flat_map(|pass| std::iter::repeat_n(pass.direction, pass.stitch_ids.len()))
            .collect();
        assert_eq!(actual_dirs, expected_dirs);
    }

    fn branched_spec() -> crate::knit::draft::BranchedPulloverSpec {
        crate::knit::draft::BranchedPulloverSpec {
            torso: crate::knit::draft::RaglanPulloverSpec {
                name: "synthetic three-branch pullover".to_string(),
                wearer_age_class: WearerAgeClass::Child3To14,
                neck_girth_mm: 64.0,
                chest_girth_mm: 96.0,
                yoke_depth_mm: 32.0,
                body_length_below_arm_mm: 32.0,
                collar_rib_courses: 2,
                hem_rib_courses: 2,
                cam_setting: 4.0,
                yarn_id: "synthetic".to_string(),
                carrier_id: CarrierId("1".to_string()),
                has_hood_or_neck_drawstring: false,
            },
            sleeve_cuff_girth_mm: 32.0,
            sleeve_upper_arm_girth_mm: 48.0,
            sleeve_length_mm: 48.0,
            cuff_rib_courses: 2,
            left_sleeve_carrier: CarrierId("2".to_string()),
            right_sleeve_carrier: CarrierId("3".to_string()),
        }
    }

    #[test]
    fn three_tube_pullover_joins_real_sleeve_parents_and_retires_unused_carriers() {
        use crate::knit::draft::draft_branched_raglan_pullover;
        let mut profile = MachineProfile::hand_knit(4.0);
        profile.min_carrier_spacing = 2;
        let gauge = topology_doc(Vec::new()).gauge;
        let spec = branched_spec();
        let doc = draft_branched_raglan_pullover(&spec, &gauge, &profile).unwrap();
        assert_eq!(doc.metadata["body_start_course"], "6");
        assert_eq!(doc.metadata["sleeve_start_course"], "0");
        assert_eq!(
            doc.metadata["boundary_rings"],
            "waist_hem,neck,left_wrist_cuff,right_wrist_cuff"
        );
        let join_course: u32 = doc.metadata["join_course"].parse().unwrap();
        assert_eq!(join_course, 15);
        let nodes: BTreeMap<_, _> = doc.stitches.iter().map(|node| (node.id, node)).collect();
        let join_nodes: Vec<_> = doc
            .stitches
            .iter()
            .filter(|node| node.course == join_course && node.parents.len() == 2)
            .collect();
        assert_eq!(join_nodes.len(), 4);
        for node in join_nodes {
            assert_eq!(node.kind, LoopKind::Decrease);
            let parent_carriers: BTreeSet<_> = node
                .parents
                .iter()
                .map(|parent| nodes[&parent.stitch_id].carrier_ids[0].clone())
                .collect();
            assert_eq!(parent_carriers.len(), 2);
            assert!(parent_carriers.contains(&spec.torso.carrier_id));
            assert!(
                parent_carriers.contains(&spec.left_sleeve_carrier)
                    || parent_carriers.contains(&spec.right_sleeve_carrier)
            );
        }
        let mut uses = BTreeMap::<u32, usize>::new();
        for node in &doc.stitches {
            for parent in &node.parents {
                *uses.entry(parent.stitch_id).or_default() += 1;
            }
        }
        let terminal_id = doc.stitches.last().unwrap().id;
        for node in &doc.stitches {
            assert_eq!(
                uses.get(&node.id).copied().unwrap_or(0),
                usize::from(node.id != terminal_id),
                "branch loop {} is neither copied nor discarded",
                node.id
            );
        }
        let bundle = compile_knit_doc(&doc, &profile, None, None, None).unwrap();
        let order: Vec<_> = doc
            .passes
            .iter()
            .flat_map(|pass| pass.stitch_ids.iter().copied())
            .collect();
        assert!(replay_parentage(&doc, &bundle.program, &order).is_empty());
        assert_eq!(bundle.vm_report.estimated_courses, doc.passes.len() as u32);
        // A sleeve widens one logical wale: the previous odd back edge to the
        // new even front edge spans three slots, hence two skipped positions.
        let effective_pitch = gauge.blocked_wale_pitch_mm().max(profile.needle_pitch_mm);
        let mut last_wale = BTreeMap::<CarrierId, i32>::new();
        let mut largest_gap = 0;
        for id in &order {
            let node = nodes[id];
            if let Some(previous) = last_wale.insert(node.carrier_ids[0].clone(), node.wale) {
                largest_gap = largest_gap.max((node.wale - previous).abs().saturating_sub(1));
            }
        }
        assert_eq!(largest_gap, 2);
        assert_eq!(
            bundle.vm_report.max_float_mm,
            f64::from(largest_gap) * effective_pitch
        );
        assert!(bundle.vm_report.max_float_mm <= profile.child_float_cap_mm);
        let mut formed = 0;
        let mut retired = BTreeSet::new();
        for op in &bundle.program.ops {
            match op {
                KnitoutOp::Out { carriers } | KnitoutOp::OutHook { carriers } => {
                    retired.extend(carriers.iter().cloned());
                }
                KnitoutOp::Knit { .. } | KnitoutOp::Tuck { .. } => {
                    if nodes[&order[formed]].course == join_course {
                        assert!(retired.contains(&spec.torso.carrier_id));
                        assert!(retired.contains(&spec.right_sleeve_carrier));
                        assert!(!retired.contains(&spec.left_sleeve_carrier));
                    }
                    formed += 1;
                }
                _ => {}
            }
        }
        bundle.cert.verify_integrity().unwrap();
        assert!(!bundle.cert.hardware_execution_authorized);
    }

    fn topology_qualification(profile: &MachineProfile) -> KnitSwatchQualification {
        KnitSwatchQualification {
            id: "synthetic-topology-qualification".to_string(),
            material_sha256: "synthetic-topology-material".to_string(),
            machine_sha256: profile.canonical_sha256().unwrap(),
            yarn_lot_id: "synthetic-test-lot".to_string(),
            was_washed_and_blocked: true,
            wales_per_100mm: 25.0,
            courses_per_100mm: 25.0,
            thickness_mm: 1.0,
            cam_stitch_setting: 4.0,
            wick_dispersion_mm: None,
            shrinkage_axial_pct: None,
            yarn_intake_drift_eps: Some(0.0),
            yarn_intake_drift_sigma_eta: Some(0.01),
            evidence: Evidence::Measured,
            source: Source {
                reference: "test:synthetic-topology-qualification".to_string(),
                accessed_on: "2026-11-27".to_string(),
                note: "Synthetic admission fixture only; no physical machine experiment."
                    .to_string(),
            },
            method: "synthetic test values".to_string(),
            conditioning: BTreeMap::new(),
            allow_estimates: false,
        }
    }

    #[test]
    fn new_topologies_compile_for_qualified_gripper_and_hooked_machine_profiles() {
        use crate::knit::draft::{
            apply_cable_crossings, draft_branched_raglan_pullover, draft_short_row_neckline_panel,
        };
        let mut generic = MachineProfile::shima_seiki_12e_handoff();
        generic.id = "test-generic-vbed-12e".to_string();
        generic.name = "Synthetic hooked V-bed 12E capability fixture".to_string();
        generic.backend_kind = crate::knit::MachineBackendKind::KnitoutV2;
        generic.has_sliders = false;
        for profile in [MachineProfile::kniterate_7e(), generic] {
            let mut gauge = topology_doc(Vec::new()).gauge;
            gauge.needle_pitch_mm = profile.needle_pitch_mm;
            let qualification = topology_qualification(&profile);
            let branch =
                draft_branched_raglan_pullover(&branched_spec(), &gauge, &profile).unwrap();
            let mut panel_spec = ParametricPanelSpec {
                name: "qualified topology fixture".to_string(),
                wearer_age_class: WearerAgeClass::Child3To14,
                hem_width_mm: 120.0,
                chest_width_mm: 120.0,
                length_mm: 80.0,
                armhole_depth_mm: 48.0,
                armhole_inset_total_mm: 16.0,
                neck_width_mm: 40.0,
                neck_depth_mm: 16.0,
                rib_hem_courses: 2,
                cam_setting: 4.0,
                yarn_id: "synthetic".to_string(),
                carrier_id: CarrierId("1".to_string()),
                has_hood_or_neck_drawstring: false,
            };
            let neckline =
                draft_short_row_neckline_panel(&panel_spec, &gauge, &profile, 1).unwrap();
            panel_spec.neck_width_mm = 0.0;
            panel_spec.neck_depth_mm = 0.0;
            let plain = draft_shaped_panel(&panel_spec, &gauge, &profile).unwrap();
            let cable = apply_cable_crossings(
                &plain,
                &[CableGroup {
                    course: 4,
                    left_wales: vec![-2, -1],
                    right_wales: vec![0, 1],
                    left_in_front: true,
                }],
            )
            .unwrap();
            for doc in [branch, neckline, cable] {
                let bundle = compile_knit_doc(
                    &doc,
                    &profile,
                    Some(&qualification),
                    Some("synthetic-topology-material"),
                    None,
                )
                .unwrap();
                assert!(bundle.vm_report.total_transfers > 0);
                assert!(!bundle.cert.physical_knitting_validated);
                assert!(!bundle.cert.hardware_execution_authorized);
                assert!(bundle.program.ops.iter().any(|op| {
                    if profile.has_yarn_inserting_hooks {
                        matches!(op, KnitoutOp::InHook { .. })
                    } else {
                        matches!(op, KnitoutOp::In { .. })
                    }
                }));
                let mut reordered = doc.clone();
                reordered.stitches.reverse();
                let second = compile_knit_doc(
                    &reordered,
                    &profile,
                    Some(&qualification),
                    Some("synthetic-topology-material"),
                    None,
                )
                .unwrap();
                assert_eq!(
                    bundle.program.to_knitout_text().unwrap(),
                    second.program.to_knitout_text().unwrap()
                );
            }
        }
    }

    #[test]
    fn branched_pullover_refuses_missing_carriers_width_and_unformable_yoke_steps() {
        use crate::knit::draft::draft_branched_raglan_pullover;
        let profile = MachineProfile::hand_knit(4.0);
        let gauge = topology_doc(Vec::new()).gauge;
        let mut spec = branched_spec();
        spec.right_sleeve_carrier = spec.torso.carrier_id.clone();
        assert_eq!(
            draft_branched_raglan_pullover(&spec, &gauge, &profile)
                .unwrap_err()
                .code,
            RefusalCode::MachineStateInfeasible
        );
        spec = branched_spec();
        let mut too_short = spec.clone();
        too_short.torso.body_length_below_arm_mm = 4.0;
        assert_eq!(
            draft_branched_raglan_pullover(&too_short, &gauge, &profile)
                .unwrap_err()
                .code,
            RefusalCode::OutOfRange
        );
        let mut too_small = spec.clone();
        too_small.sleeve_cuff_girth_mm = 4.0;
        assert_eq!(
            draft_branched_raglan_pullover(&too_small, &gauge, &profile)
                .unwrap_err()
                .code,
            RefusalCode::OutOfRange
        );
        let mut long_hem = spec.clone();
        long_hem.torso.hem_rib_courses = 9;
        assert_eq!(
            draft_branched_raglan_pullover(&long_hem, &gauge, &profile)
                .unwrap_err()
                .code,
            RefusalCode::OutOfRange
        );
        let mut narrow = profile.clone();
        narrow.needle_count = 40;
        assert_eq!(
            draft_branched_raglan_pullover(&spec, &gauge, &narrow)
                .unwrap_err()
                .code,
            RefusalCode::MachineStateInfeasible
        );
        spec.torso.chest_girth_mm = 400.0;
        spec.sleeve_upper_arm_girth_mm = 240.0;
        spec.torso.yoke_depth_mm = 16.0;
        assert_eq!(
            draft_branched_raglan_pullover(&spec, &gauge, &profile)
                .unwrap_err()
                .code,
            RefusalCode::MachineStateInfeasible
        );
    }

    #[test]
    fn cables_exchange_real_parents_and_preserve_the_declared_over_under_order() {
        use crate::knit::draft::apply_cable_crossings;
        use Bed::Front as F;
        use LoopKind::{BindOff, CastOn, Knit};
        let mut stitches = Vec::new();
        for wale in 0..6 {
            stitches.push(topology_node(wale, 0, wale as i32, F, CastOn, &[]));
            stitches.push(topology_node(wale + 6, 1, wale as i32, F, Knit, &[wale]));
            stitches.push(topology_node(
                wale + 12,
                2,
                wale as i32,
                F,
                Knit,
                &[wale + 6],
            ));
        }
        for position in 0..6 {
            let id = 18 + position;
            let wale = 5 - position;
            let mut parents = vec![12 + wale];
            if position > 0 {
                parents.push(id - 1);
            }
            stitches.push(topology_node(id, 3, wale as i32, F, BindOff, &parents));
        }
        let plain = topology_doc(stitches);
        let profile = MachineProfile::hand_knit(4.0);
        let mut programs = Vec::new();
        for left_in_front in [true, false] {
            let group = CableGroup {
                course: 1,
                left_wales: vec![1, 2],
                right_wales: vec![3, 4],
                left_in_front,
            };
            let doc = apply_cable_crossings(&plain, &[group]).unwrap();
            let expected = [(7, 3), (8, 4), (9, 1), (10, 2)];
            for (id, parent) in expected {
                let node = doc.stitches.iter().find(|node| node.id == id).unwrap();
                assert_eq!(node.kind, LoopKind::Cable);
                assert_eq!(node.parents[0].stitch_id, parent);
            }
            let bundle = compile_knit_doc(&doc, &profile, None, None, None).unwrap();
            assert!(
                replay_parentage(
                    &doc,
                    &bundle.program,
                    &[
                        0, 1, 2, 3, 4, 5, 11, 10, 9, 8, 7, 6, 12, 13, 14, 15, 16, 17, 18, 19, 20,
                        21, 22, 23
                    ]
                )
                .is_empty()
            );
            let first_return = bundle
                .program
                .ops
                .iter()
                .find_map(|op| match op {
                    KnitoutOp::Xfer { from, to } if from.bed == Bed::Back => Some((*from, *to)),
                    _ => None,
                })
                .unwrap();
            assert_eq!(
                first_return.1.index - first_return.0.index,
                if left_in_front { 2 } else { -2 }
            );
            let mut too_little_rack = profile.clone();
            too_little_rack.max_racking_quarter_pitches = 4;
            assert!(compile_knit_doc(&doc, &too_little_rack, None, None, None).is_err());
            let mut unstated_crossing = doc.clone();
            unstated_crossing.cable_groups.clear();
            for node in &mut unstated_crossing.stitches {
                if node.kind == LoopKind::Cable {
                    node.kind = LoopKind::Knit;
                }
            }
            assert!(
                compile_knit_doc(&unstated_crossing, &profile, None, None, None)
                    .unwrap_err()
                    .message
                    .contains("cycle")
            );
            programs.push(bundle.program);
        }
        assert_ne!(programs[0].ops, programs[1].ops);
    }

    #[test]
    fn cable_groups_refuse_overlap_irregular_blocks_and_false_parent_maps() {
        use crate::knit::draft::apply_cable_crossings;
        use Bed::Front as F;
        use LoopKind::{CastOn, Knit};
        let mut stitches = Vec::new();
        for wale in 0..4 {
            stitches.push(topology_node(wale, 0, wale as i32, F, CastOn, &[]));
            stitches.push(topology_node(wale + 4, 1, wale as i32, F, Knit, &[wale]));
        }
        let plain = topology_doc(stitches);
        let group = CableGroup {
            course: 1,
            left_wales: vec![0, 1],
            right_wales: vec![2, 3],
            left_in_front: true,
        };
        let mut duplicate = apply_cable_crossings(&plain, std::slice::from_ref(&group)).unwrap();
        duplicate.cable_groups.push(group.clone());
        assert_eq!(
            duplicate.validate().unwrap_err().code,
            RefusalCode::InvalidDocument
        );
        let mut irregular = group.clone();
        irregular.right_wales = vec![2, 4];
        assert_eq!(
            irregular.validate().unwrap_err().code,
            RefusalCode::InvalidDocument
        );
        let mut false_map = apply_cable_crossings(&plain, &[group]).unwrap();
        false_map
            .stitches
            .iter_mut()
            .find(|node| node.id == 4)
            .unwrap()
            .parents[0]
            .stitch_id = 0;
        assert!(
            compile_knit_doc(
                &false_map,
                &MachineProfile::hand_knit(4.0),
                None,
                None,
                None
            )
            .unwrap_err()
            .message
            .contains("opposite block")
        );
    }

    #[test]
    fn compiler_refuses_reused_and_stranded_parents_and_incomplete_bind_off() {
        use Bed::Front as F;
        use LoopKind::{BindOff, CastOn, Knit};
        let profile = MachineProfile::hand_knit(4.0);
        let mut doc = topology_doc(vec![
            topology_node(0, 0, 0, F, CastOn, &[]),
            topology_node(1, 0, 1, F, CastOn, &[]),
            topology_node(2, 1, 0, F, Knit, &[0]),
            topology_node(3, 1, 1, F, Knit, &[0]),
        ]);
        assert_eq!(
            compile_knit_doc(&doc, &profile, None, None, None)
                .unwrap_err()
                .code,
            RefusalCode::MachineStateInfeasible
        );
        doc.stitches.pop();
        assert!(
            compile_knit_doc(&doc, &profile, None, None, None)
                .unwrap_err()
                .message
                .contains("strands")
        );
        doc.stitches.push(topology_node(3, 1, 1, F, Knit, &[1]));
        doc.stitches[2].kind = BindOff;
        doc.stitches[3].kind = BindOff;
        assert!(
            compile_knit_doc(&doc, &profile, None, None, None)
                .unwrap_err()
                .message
                .contains("preceding")
        );
    }

    #[test]
    fn opposite_bed_transfers_reverse_stacks_and_same_bed_moves_restore_order() {
        let profile = MachineProfile::hand_knit(4.0);
        let front = Needle::new(Bed::Front, 10);
        let back = Needle::new(Bed::Back, 10);
        let mut compiler = LoopCompiler {
            profile: &profile,
            live: BTreeMap::from([(front, vec![0, 1])]),
            bridge_racks: BTreeSet::new(),
            ops: Vec::new(),
        };
        compiler.cross(front, back).unwrap();
        assert_eq!(compiler.live[&back], vec![1, 0]);
        compiler.cross(back, front).unwrap();
        assert_eq!(compiler.live[&front], vec![0, 1]);
        compiler.live.insert(back, vec![2]);
        compiler
            .transfer(front, Needle::new(Bed::Front, 11))
            .unwrap();
        assert_eq!(compiler.live[&Needle::new(Bed::Front, 11)], vec![0, 1]);
        assert_eq!(compiler.live[&back], vec![2]);
    }

    #[test]
    fn midpoint_transfer_preserves_stacks_and_passes_the_unchanged_strain_gate() {
        let profile = MachineProfile::hand_knit(4.0);
        let from = Needle::new(Bed::Front, 10);
        let target = Needle::new(Bed::Front, 14);
        let other = Needle::new(Bed::Back, 20);
        let aligned = Needle::new(Bed::Back, 10);
        let carrier = CarrierId("1".to_string());
        let program = |transfers: &[KnitoutOp]| {
            let mut ops = vec![KnitoutOp::In {
                carriers: vec![carrier.clone()],
            }];
            for needle in [from, from, target, other] {
                ops.push(KnitoutOp::Tuck {
                    dir: CourseDir::Plus,
                    needle,
                    carriers: vec![carrier.clone()],
                });
            }
            ops.extend_from_slice(transfers);
            ops.push(KnitoutOp::Out {
                carriers: vec![carrier.clone()],
            });
            KnitoutProgram {
                schema: KNITOUT_SCHEMA.to_string(),
                header: KnitoutHeader {
                    machine: profile.id.clone(),
                    gauge: format!("{:.1}", profile.gauge_e),
                    width: profile.needle_count,
                    position: "Center".to_string(),
                    carriers: vec![carrier.clone()],
                    extra: BTreeMap::new(),
                },
                ops,
            }
        };
        let context = KnitSafetyContext {
            wearer_age_class: WearerAgeClass::Adult,
            blocked_wale_pitch_mm: 5.0,
            source_doc: None,
            speed_fraction: None,
            takedown_fraction: None,
        };
        let old_route = [
            KnitoutOp::Rack { quarter_pitches: 0 },
            KnitoutOp::Xfer { from, to: aligned },
            KnitoutOp::Rack { quarter_pitches: 0 },
            KnitoutOp::Rack {
                quarter_pitches: 16,
            },
            KnitoutOp::Xfer {
                from: aligned,
                to: target,
            },
            KnitoutOp::Rack { quarter_pitches: 0 },
        ];
        let refusal = verify_knitout_program(&program(&old_route), &profile, &context).unwrap_err();
        assert_eq!(refusal.code, RefusalCode::MaterialProcessHazard);
        assert!(refusal.message.contains("strain"));

        let original = BTreeMap::from([(from, vec![0, 1]), (target, vec![2]), (other, vec![3])]);
        let mut compiler = LoopCompiler {
            profile: &profile,
            live: original,
            bridge_racks: BTreeSet::from([0]),
            ops: Vec::new(),
        };
        compiler.transfer(from, target).unwrap();
        assert_eq!(compiler.live[&target], vec![2, 0, 1]);
        assert_eq!(compiler.live[&other], vec![3]);
        assert_eq!(compiler.live.len(), 2);
        let racks: Vec<_> = compiler
            .ops
            .iter()
            .filter_map(|op| match op {
                KnitoutOp::Rack { quarter_pitches } => Some(*quarter_pitches),
                _ => None,
            })
            .collect();
        assert_eq!(racks, vec![-8, 0, 8, 0]);
        let verified = verify_knitout_program(&program(&compiler.ops), &profile, &context).unwrap();
        assert_eq!(verified.total_transfers, 2);

        // Eight pitches cannot be split into two strain-admitted crossings by
        // this local planner. Do not emit a failed partial route or weaken the VM.
        let mut impossible = LoopCompiler {
            profile: &profile,
            live: BTreeMap::from([(from, vec![0, 1]), (other, vec![3])]),
            bridge_racks: BTreeSet::from([0]),
            ops: Vec::new(),
        };
        let before = impossible.live.clone();
        let refusal = impossible
            .transfer(from, Needle::new(Bed::Front, 18))
            .unwrap_err();
        assert_eq!(refusal.code, RefusalCode::MachineStateInfeasible);
        assert_eq!(impossible.live, before);
        assert!(impossible.ops.is_empty());
    }

    #[test]
    fn transfer_route_retains_nonzero_bridge_birth_racks_until_one_bed_is_empty() {
        let profile = MachineProfile::hand_knit(4.0);
        let from = Needle::new(Bed::Front, 10);
        let mut compiler = LoopCompiler {
            profile: &profile,
            live: BTreeMap::from([(from, vec![0]), (Needle::new(Bed::Front, 20), vec![1])]),
            bridge_racks: BTreeSet::new(),
            ops: Vec::new(),
        };
        let back = Needle::new(Bed::Back, 12);
        compiler.cross(from, back).unwrap();
        assert_eq!(compiler.bridge_racks, BTreeSet::from([-8]));
        // Absolute rack +8 is ordinarily small, but it is four pitches away
        // from the still-live bridge born at rack -8.
        assert!(
            compiler
                .route_cost(&[(back, Needle::new(Bed::Front, 14))])
                .is_none()
        );
        compiler.cross(back, from).unwrap();
        assert!(compiler.bridge_racks.is_empty());
    }

    fn neckline_doc() -> KnitDoc {
        use Bed::Front as F;
        use LoopKind::{BindOff, CastOn, Knit};
        let mut stitches = Vec::new();
        for wale in 0..6 {
            stitches.push(topology_node(wale, 0, wale as i32, F, CastOn, &[]));
            stitches.push(topology_node(wale + 6, 1, wale as i32, F, Knit, &[wale]));
        }
        stitches.extend([
            topology_node(12, 2, 0, F, Knit, &[6]),
            topology_node(13, 2, 1, F, Knit, &[7]),
            topology_node(14, 2, 5, F, Knit, &[11]),
            topology_node(15, 2, 4, F, Knit, &[10]),
            topology_node(16, 3, 1, F, BindOff, &[13]),
            topology_node(17, 3, 0, F, BindOff, &[12, 16]),
            topology_node(18, 3, 4, F, BindOff, &[15]),
            topology_node(19, 3, 5, F, BindOff, &[14, 18]),
        ]);
        for node in &mut stitches {
            if [14, 15, 18, 19].contains(&node.id) {
                node.carrier_ids = vec![CarrierId("2".to_string())];
            }
        }
        let mut doc = topology_doc(stitches);
        doc.wearer_age_class = WearerAgeClass::Child3To14;
        for (key, value) in [
            ("shoulder_start_course", "2"),
            ("held_neck_stitch_ids", "8,9"),
            ("left_shoulder_carrier", "1"),
            ("right_shoulder_carrier", "2"),
            ("shoulder_initial_direction_1", "plus"),
            ("shoulder_initial_direction_2", "minus"),
        ] {
            doc.metadata.insert(key.to_string(), value.to_string());
        }
        doc
    }

    #[test]
    fn separate_shoulders_preserve_held_center_and_bind_off_without_a_neck_float() {
        let mut profile = MachineProfile::hand_knit(4.0);
        // Synthetic capability variant exercises delayed inserting hooks without claiming
        // a tested physical knitting machine or reusing an unrelated qualification.
        profile.has_yarn_inserting_hooks = true;
        let mut doc = neckline_doc();
        let bundle = compile_knit_doc(&doc, &profile, None, None, None).unwrap();
        let held = replay_parentage(
            &doc,
            &bundle.program,
            &[
                0, 1, 2, 3, 4, 5, 11, 10, 9, 8, 7, 6, 12, 13, 14, 15, 18, 19, 16, 17,
            ],
        );
        assert_eq!(
            held.values().flatten().copied().collect::<BTreeSet<_>>(),
            BTreeSet::from([8, 9])
        );
        assert_eq!(bundle.vm_report.max_float_mm, 0.0);
        assert_eq!(bundle.vm_report.estimated_courses, 6);
        let mut formed = 0;
        let mut second_in = None;
        let mut second_release = None;
        for op in &bundle.program.ops {
            match op {
                KnitoutOp::Knit { .. } | KnitoutOp::Tuck { .. } => formed += 1,
                KnitoutOp::InHook { carriers } if carriers[0].as_str() == "2" => {
                    second_in = Some(formed)
                }
                KnitoutOp::ReleaseHook { carriers } if carriers[0].as_str() == "2" => {
                    second_release = Some(formed)
                }
                _ => {}
            }
        }
        assert_eq!(
            second_in,
            Some(14),
            "second yarn starts only at the neckline"
        );
        assert_eq!(
            second_release,
            Some(20),
            "second inserting hook stays until its next course"
        );
        assert!(bundle.program.ops.iter().any(|op| matches!(op,
            KnitoutOp::Pause { note: Some(note) } if note.contains("2 occupied hooks"))));
        assert!(
            bundle
                .cert
                .limitations
                .iter()
                .any(|text| text.contains("held neckline"))
        );
        doc.stitches.reverse();
        assert_eq!(
            bundle.program,
            compile_knit_doc(&doc, &profile, None, None, None)
                .unwrap()
                .program
        );
    }

    #[test]
    fn neckline_metadata_cannot_hide_stranded_loops_or_bypass_holding_limits() {
        let profile = MachineProfile::hand_knit(4.0);
        let mut missing_held = neckline_doc();
        missing_held
            .metadata
            .insert("held_neck_stitch_ids".to_string(), "8".to_string());
        assert!(
            compile_knit_doc(&missing_held, &profile, None, None, None)
                .unwrap_err()
                .message
                .contains("strands")
        );
        for (key, value) in [
            ("held_neck_stitch_ids", "6,9"),
            ("held_neck_stitch_ids", "8,8,9"),
            ("left_shoulder_carrier", "2"),
            ("shoulder_initial_direction_2", "plus"),
        ] {
            let mut bad = neckline_doc();
            bad.metadata.insert(key.to_string(), value.to_string());
            assert_eq!(
                compile_knit_doc(&bad, &profile, None, None, None)
                    .unwrap_err()
                    .code,
                RefusalCode::MachineStateInfeasible
            );
        }
        let mut wrong_chain = neckline_doc();
        wrong_chain
            .stitches
            .iter_mut()
            .find(|node| node.id == 19)
            .unwrap()
            .parents[1]
            .stitch_id = 16;
        assert!(
            compile_knit_doc(&wrong_chain, &profile, None, None, None)
                .unwrap_err()
                .message
                .contains("preceding")
        );
        let mut tight_holding = profile;
        // Two source courses still require four independently carried shoulder passes.
        tight_holding.max_roller_imbalance_courses = 3;
        let refusal =
            compile_knit_doc(&neckline_doc(), &tight_holding, None, None, None).unwrap_err();
        assert_eq!(refusal.code, RefusalCode::MaterialProcessHazard);
        assert!(refusal.message.contains("roller imbalance"));
        let mut course_gap = neckline_doc();
        for node in &mut course_gap.stitches {
            if node.course == 3 {
                node.course = 100;
            }
        }
        let refusal = compile_knit_doc(
            &course_gap,
            &MachineProfile::hand_knit(4.0),
            None,
            None,
            None,
        )
        .unwrap_err();
        assert_eq!(refusal.code, RefusalCode::MaterialProcessHazard);
        assert!(refusal.message.contains("source courses"));
    }

    #[test]
    fn drafted_short_row_shoulders_rejoin_anchors_and_preserve_armhole_decreases() {
        use crate::knit::draft::draft_short_row_neckline_panel;
        let profile = MachineProfile::hand_knit(4.0);
        let gauge = topology_doc(Vec::new()).gauge;
        for length_mm in [80.0, 84.0] {
            let spec = ParametricPanelSpec {
                name: "synthetic anchored shoulders".to_string(),
                wearer_age_class: WearerAgeClass::Child3To14,
                hem_width_mm: 120.0,
                chest_width_mm: 120.0,
                length_mm,
                armhole_depth_mm: 48.0,
                armhole_inset_total_mm: 16.0,
                neck_width_mm: 40.0,
                neck_depth_mm: 40.0,
                rib_hem_courses: 2,
                cam_setting: 4.0,
                yarn_id: "synthetic".to_string(),
                carrier_id: CarrierId("1".to_string()),
                has_hood_or_neck_drawstring: false,
            };
            let doc = draft_short_row_neckline_panel(&spec, &gauge, &profile, 2).unwrap();
            assert_eq!(doc.short_row_turns.len(), 4);
            let held: BTreeSet<u32> = doc.metadata["held_neck_stitch_ids"]
                .split(',')
                .map(|value| value.parse().unwrap())
                .collect();
            let mut uses = BTreeMap::<u32, usize>::new();
            for node in &doc.stitches {
                if node.kind != LoopKind::Tuck {
                    for parent in &node.parents {
                        *uses.entry(parent.stitch_id).or_default() += 1;
                    }
                }
            }
            let terminals: BTreeSet<_> = doc
                .stitches
                .iter()
                .filter(|node| node.kind == LoopKind::BindOff && !uses.contains_key(&node.id))
                .map(|node| node.id)
                .collect();
            assert_eq!(terminals.len(), 2);
            for node in &doc.stitches {
                assert_eq!(
                    uses.get(&node.id).copied().unwrap_or(0),
                    usize::from(!held.contains(&node.id) && !terminals.contains(&node.id)),
                    "one actual consumption per non-held loop {}",
                    node.id
                );
            }
            let bundle = compile_knit_doc(&doc, &profile, None, None, None).unwrap();
            let order: Vec<_> = doc
                .passes
                .iter()
                .flat_map(|pass| pass.stitch_ids.iter().copied())
                .collect();
            let remaining = replay_parentage(&doc, &bundle.program, &order);
            assert_eq!(
                remaining
                    .values()
                    .flatten()
                    .copied()
                    .collect::<BTreeSet<_>>(),
                held
            );
            assert_eq!(bundle.vm_report.max_float_mm, 0.0);
            assert!(
                doc.stitches
                    .iter()
                    .any(|node| node.kind == LoopKind::Decrease)
            );
            assert!(draft_short_row_neckline_panel(&spec, &gauge, &profile, 5).is_err());
        }
    }

    #[test]
    fn drafted_necklines_compile_with_both_primary_carrier_sides() {
        let profile = MachineProfile::hand_knit(4.0);
        let gauge = topology_doc(Vec::new()).gauge;
        for length_mm in [48.0, 52.0] {
            let doc = draft_shaped_panel(
                &ParametricPanelSpec {
                    name: "synthetic separate shoulders".to_string(),
                    wearer_age_class: WearerAgeClass::Child3To14,
                    hem_width_mm: 80.0,
                    chest_width_mm: 80.0,
                    length_mm,
                    armhole_depth_mm: 0.0,
                    armhole_inset_total_mm: 0.0,
                    neck_width_mm: 24.0,
                    neck_depth_mm: 8.0,
                    rib_hem_courses: 2,
                    cam_setting: 4.0,
                    yarn_id: "synthetic".to_string(),
                    carrier_id: CarrierId("1".to_string()),
                    has_hood_or_neck_drawstring: false,
                },
                &gauge,
                &profile,
            )
            .unwrap();
            let bundle = compile_knit_doc(&doc, &profile, None, None, None).unwrap();
            assert_eq!(bundle.vm_report.max_float_mm, 0.0);
            assert_eq!(
                bundle
                    .program
                    .ops
                    .iter()
                    .filter(|op| matches!(op, KnitoutOp::Drop { .. }))
                    .count(),
                2
            );
            bundle.cert.verify_integrity().unwrap();
        }
    }

    #[test]
    fn calibration_swatch_emits_speed_number_only_for_motorized_carriages() {
        for manual in [
            MachineProfile::brother_kh930_ayab(),
            MachineProfile::brother_kh270_ayab(),
            MachineProfile::hand_knit(4.0),
        ] {
            let doc = draft_calibration_swatch(&manual, "synthetic-yarn", 8, 4, &[5.0]).unwrap();
            let bundle = compile_knit_doc(&doc, &manual, None, None, None).unwrap();
            assert!(
                !bundle
                    .program
                    .ops
                    .iter()
                    .any(|op| matches!(op, KnitoutOp::SpeedNumber { .. })),
                "{} must not emit motorized x-speed-number",
                manual.id
            );
        }
        for motorized in [
            MachineProfile::kniterate_7e(),
            MachineProfile::generic_vbed_12e(),
        ] {
            let doc = draft_calibration_swatch(&motorized, "synthetic-yarn", 8, 4, &[5.0]).unwrap();
            let bundle = compile_knit_doc(&doc, &motorized, None, None, None).unwrap();
            assert!(
                bundle
                    .program
                    .ops
                    .iter()
                    .any(|op| matches!(op, KnitoutOp::SpeedNumber { speed: 150 })),
                "{} should emit motorized calibration x-speed-number",
                motorized.id
            );
        }
    }
}

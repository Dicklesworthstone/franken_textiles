//! Transfer planner & `KnitDoc` -> lossless CMU Knitout v2 compiler (`FTX-K04`).
//!
//! Compiles validated `ftx-knit/1` documents into machine-profile-adapted `KnitoutProgram`s
//! (branching carrier insertion cleanly on `has_yarn_inserting_hooks`, planning sliderless
//! bed-to-bed decrease/rib transfers within `±max_racking_quarter_pitches`, and verifying every
//! emitted stream through the V-bed VM before sealing `ftx-machine-cert/1`).

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode};

use super::ir::{
    Bed, CarrierId, CourseDir, KNITOUT_SCHEMA, KnitDoc, KnitoutHeader, KnitoutOp, KnitoutProgram,
    LoopKind, Needle, StitchNode,
};
use super::profile::{CalibrationSwatchCaps, MachineProfile, WearerAgeClass};
use super::receipts::{FourClaimsVerdict, KnitMachineCert, KnitSwatchQualification};
use super::vm::{KnitSafetyContext, VBedVmReport, verify_knitout_program};

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

        // A same-bed move uses one empty opposite-bed hook. Prefer the source-aligned
        // hook, then nearby hooks inside both racking ranges. Searching occupied hooks
        // is bounded by the input's live-loop count rather than a fixed-size bed allocation.
        let reach = self.profile.max_racking_quarter_pitches / 4;
        let lo = (i64::from(from.index) - i64::from(reach))
            .max(i64::from(to.index) - i64::from(reach))
            .max(0);
        let hi = (i64::from(from.index) + i64::from(reach))
            .min(i64::from(to.index) + i64::from(reach))
            .min(i64::from(self.profile.needle_count) - 1);
        if lo <= hi {
            let pivot = i64::from(from.index).max(lo).min(hi);
            for index in (pivot..=hi).chain((lo..pivot).rev()) {
                let middle = Needle::new(from.bed.opposite(), index as i32);
                if !self.live.contains_key(&middle) {
                    self.cross(from, middle)?;
                    return self.cross(middle, to);
                }
            }

            // Full-gauge tubes occupy both beds. Temporarily park the opposite
            // loop on a free nearby hook, move the requested stack, then restore
            // that unrelated loop before the next stitch is formed.
            for index in (pivot..=hi).chain((lo..pivot).rev()) {
                let middle = Needle::new(from.bed.opposite(), index as i32);
                let spare_lo = (index - i64::from(reach)).max(0);
                let spare_hi =
                    (index + i64::from(reach)).min(i64::from(self.profile.needle_count) - 1);
                for spare_index in spare_lo..=spare_hi {
                    let spare = Needle::new(from.bed, spare_index as i32);
                    if spare == from || spare == to || self.live.contains_key(&spare) {
                        continue;
                    }
                    self.cross(middle, spare)?;
                    self.cross(from, middle)?;
                    self.cross(middle, to)?;
                    return self.cross(spare, middle);
                }
            }
        }
        Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            format!(
                "no parent-preserving transfer from {} to {} within the available hooks and racking range",
                from.knitout_token(),
                to.knitout_token()
            ),
        )
        .repair("leave a free transfer hook near the shaping edge or reduce the shaping step"))
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

    if !doc.short_row_turns.is_empty() || !doc.cable_groups.is_empty() {
        return Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            "short-row and cable schedules are not supported by this stitch-graph compiler",
        )
        .repair("use an explicitly verified low-level Knitout schedule for short rows or cables"));
    }
    for node in &doc.stitches {
        if node.bed.is_slider()
            || !matches!(
                node.kind,
                LoopKind::CastOn
                    | LoopKind::Knit
                    | LoopKind::Purl
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

    // Group stitches deterministically by course
    let mut by_course: BTreeMap<u32, Vec<&StitchNode>> = BTreeMap::new();
    let mut min_wale = i32::MAX;
    let mut max_wale = i32::MIN;
    let mut used_carriers: BTreeSet<CarrierId> = BTreeSet::new();
    for st in &doc.stitches {
        by_course.entry(st.course).or_default().push(st);
        min_wale = min_wale.min(st.wale);
        max_wale = max_wale.max(st.wale);
        for c in &st.carrier_ids {
            used_carriers.insert(c.clone());
        }
    }
    if used_carriers.is_empty() {
        used_carriers.insert(profile.carrier_ids[0].clone());
    }
    let carriers_vec: Vec<CarrierId> = used_carriers.into_iter().collect();

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
        ops.push(KnitoutOp::SpeedNumber { speed: 150 });
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
        ops,
    };
    let mut flat_dir = CourseDir::Plus;
    let mut last_cam_int: Option<i32> = None;
    let mut active_carriers = BTreeSet::new();
    let mut hooked_since = BTreeMap::new();
    let mut next_carrier_direction = BTreeMap::new();
    let mut last_pass_dir = None;
    let mut previous_layout: Option<bool> = None;

    for (&course_idx, nodes) in &by_course {
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
        let circular = nodes.len() > wales.len();
        if circular && nodes.len() != 2 * wales.len() {
            return Err(Refusal::new(
                RefusalCode::MachineStateInfeasible,
                format!("course {course_idx} mixes partial tubes and flat rows"),
            )
            .repair("supply complete paired front/back courses or separate flat panels"));
        }
        if previous_layout.is_some_and(|previous| previous != circular) {
            return Err(Refusal::new(
                RefusalCode::MachineStateInfeasible,
                "tube-to-panel joins and splits require an explicit supported schedule",
            )
            .repair("compile a complete tube or flat panel as one document"));
        }
        previous_layout = Some(circular);

        let mut sorted: Vec<&StitchNode> = nodes.clone();
        let shoulder = shoulders
            .as_ref()
            .filter(|schedule| course_idx >= schedule.start);
        let mut shoulder_directions = BTreeMap::new();
        if let Some(schedule) = shoulder {
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
                    shoulder_directions.insert(node.id, dir);
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

        let mut shoulder_bind_chains = BTreeMap::new();
        for (position, node) in sorted.iter().copied().enumerate() {
            if binding_off {
                let mut parents = node.parents.clone();
                parents.sort_by_key(|parent| parent.stack_pos);
                let preceding = if shoulder.is_some() {
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
            let dir = if let Some(dir) = shoulder_directions.get(&node.id) {
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
            for carrier in &node.carrier_ids {
                if active_carriers.insert(carrier.clone()) {
                    compiler.ops.push(if profile.has_yarn_inserting_hooks {
                        hooked_since.insert(carrier.clone(), course_idx);
                        KnitoutOp::InHook {
                            carriers: vec![carrier.clone()],
                        }
                    } else {
                        KnitoutOp::In {
                            carriers: vec![carrier.clone()],
                        }
                    });
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
            compiler.ops.push(if node.kind == LoopKind::CastOn {
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
            compiler.live.insert(needle, vec![node.id]);
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
        for c in &carriers_vec {
            ops.push(KnitoutOp::OutHook {
                carriers: vec![c.clone()],
            });
        }
    } else {
        for c in &carriers_vec {
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
        limitations.push("The center neckline loops remain on hold while separately carried shoulders are knitted and chain-bound off. Finish the held neckline manually; no anchored short-row or physical-fit claim is made.".to_string());
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
                        node.kind == LoopKind::CastOn
                    );
                    hooks.insert(*needle, vec![node.id]);
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
}

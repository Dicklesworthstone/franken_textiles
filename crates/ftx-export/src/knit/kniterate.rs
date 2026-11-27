//! Native/WASM Kniterate pass scheduling and independently decoded KCode export.
//!
//! Format and carrier geometry follow the MIT-licensed CMU backend pinned in
//! `tests/fixtures/kniterate/PROVENANCE.md`. Transfers preserve source order, passes
//! have one carrier, and calibrated cam values are encoded explicitly. Independent
//! decoding checks the needle raster, machine controls and operator instructions.

mod readback;

use std::collections::BTreeMap;
use std::fmt::Write as _;

use ftx_core::receipt::sha256_hex;
use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::knit::{
    Bed, CarrierId, CompiledKnitBundle, CourseDir, KnitMachineCert, KnitoutOp, KnitoutProgram,
    MachineBackendKind, MachineProfile, Needle,
};
use serde::{Deserialize, Serialize};

pub use readback::{
    KCodeReadback, KCodeTakedownReport, read_kniterate_kc, verify_kniterate_takedown,
};

/// Exact upstream format revision used by the backend and independent fixtures.
pub const KNITERATE_BACKEND_REVISION: &str = "b19ae3d25fe7b977513362731ed1ff9220dacf69";

// Upstream raster: 15 margin cells, 252 usable needles, one unselected sentinel,
// 15 margin cells. FTX keeps its certified zero-based physical needle addresses.
const RASTER_LEN: usize = 283;
const PARKING: [i32; 6] = [-23, -19, -19, -15, -15, -15]; // half-needle units

/// Source-bound result of independent pass decoding and semantic comparison.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KCodeVerification {
    /// Versioned export/read-back contract.
    pub schema: String,
    /// Upstream source revision, not a firmware qualification.
    pub upstream_revision: String,
    /// Digest of the input Knitout program, including its controls.
    pub program_sha256: String,
    /// Digest of the exact KCode bytes.
    pub kc_sha256: String,
    /// Physical carriage passes, including necessary empty moves.
    pub pass_count: usize,
    /// Actual needle operations decoded from the raster.
    pub needle_operation_count: usize,
    /// Executable operator pauses retained.
    pub operator_pause_count: usize,
    /// Independently replayed take-down controls and the existing nominal holding budget.
    pub takedown: KCodeTakedownReport,
    /// Always false: software verification does not authorize hardware execution.
    pub hardware_execution_authorized: bool,
}

/// Actual pass-oriented KCode and its source-bound software verification.
#[derive(Clone, Debug)]
pub struct KniterateExportBundle {
    /// Pass-oriented KCode text.
    pub kc_text: String,
    /// SHA-256 of the exact KCode bytes.
    pub kc_sha256: String,
    /// Existing software certificate; never hardware authorization.
    pub cert: KnitMachineCert,
    /// Independent read-back of this exact file.
    pub verification: KCodeVerification,
}

fn validate_target(bundle: &CompiledKnitBundle, profile: &MachineProfile) -> Result<(), Refusal> {
    profile.validate()?;
    bundle.cert.verify_integrity()?;
    if profile.backend_kind != MachineBackendKind::KniterateKc
        || profile.needle_count != 252
        || profile.has_yarn_inserting_hooks
        || profile.has_sliders
        || profile.min_carrier_spacing < 2
    {
        return Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            "Kniterate KCode requires 252 needles, carrier spacing >= 2, grippers and no sliders",
        ));
    }
    if bundle.cert.machine_sha256 != profile.canonical_sha256()?
        || bundle.cert.program_sha256 != bundle.program.canonical_text_sha256()?
    {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            "KCode target or Knitout program differs from the compiled certificate",
        ));
    }
    Ok(())
}

fn unsupported(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::UnqualifiedProcess, message)
        .repair("retain the verified Knitout file and use a backend qualified for this operation")
}

/// Emits real Kniterate KCode, then independently decodes and checks its semantics.
///
/// # Errors
/// Refuses unsupported controls, fractional dials/racks, plated carrier sets,
/// impossible travel, changed certificate identities or read-back discrepancies.
pub fn export_kniterate_kc(
    bundle: &CompiledKnitBundle,
    profile: &MachineProfile,
) -> Result<KniterateExportBundle, Refusal> {
    validate_target(bundle, profile)?;
    let schedule = schedule(&bundle.program, profile)?;
    let kc_text = render(&schedule)?;
    let (decoded, takedown) = check_readback(&kc_text, &bundle.program, &schedule, profile)?;
    let kc_sha256 = sha256_hex(kc_text.as_bytes());
    Ok(KniterateExportBundle {
        verification: KCodeVerification {
            schema: "ftx-kniterate-readback/1".into(),
            upstream_revision: KNITERATE_BACKEND_REVISION.into(),
            program_sha256: bundle.cert.program_sha256.clone(),
            kc_sha256: kc_sha256.clone(),
            pass_count: decoded.pass_count,
            needle_operation_count: decoded.needle_operation_count,
            operator_pause_count: decoded.operator_pause_count,
            takedown,
            hardware_execution_authorized: false,
        },
        kc_text,
        kc_sha256,
        cert: bundle.cert.clone(),
    })
}

/// Independently decodes raster selections, cams, motion and executable pauses.
/// Needle actions must match the original program in order; machine controls must
/// match the schedule, including empty carrier kicks and exact roller advances.
///
/// # Errors
/// `VerificationMismatch` for malformed/changed KCode, or the exact target,
/// identity, or unsupported-operation refusal.
pub fn verify_kniterate_kc_roundtrip(
    kc_text: &str,
    bundle: &CompiledKnitBundle,
    profile: &MachineProfile,
) -> Result<(), Refusal> {
    validate_target(bundle, profile)?;
    check_readback(
        kc_text,
        &bundle.program,
        &schedule(&bundle.program, profile)?,
        profile,
    )?;
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Knit,
    Tuck,
    Transfer(Bed),
    Split(Bed),
    Miss,
}

#[derive(Clone, Debug)]
struct Pass {
    kind: Kind,
    dir: CourseDir,
    carrier: u8,
    rack: i32,
    speed: u32,
    roller: i32,
    front: BTreeMap<i32, u8>,
    back: BTreeMap<i32, u8>,
    min: i32,
    max: i32,
    last: i32,
    out: bool,
    carriage: [i32; 2],
    yarn: Option<[i32; 2]>,
}

impl Pass {
    fn mode(&self) -> &'static str {
        match (self.kind, self.dir) {
            (Kind::Knit, _) => "Kn-Kn",
            (Kind::Tuck | Kind::Miss, _) => "Tu-Tu",
            (Kind::Transfer(Bed::Front) | Kind::Split(Bed::Front), CourseDir::Plus) => "Tr-Rr",
            (Kind::Transfer(Bed::Front) | Kind::Split(Bed::Front), CourseDir::Minus) => "Tr-Rl",
            (Kind::Transfer(_) | Kind::Split(_), CourseDir::Plus) => "Rr-Tr",
            (Kind::Transfer(_) | Kind::Split(_), CourseDir::Minus) => "Rl-Tr",
        }
    }
}

#[derive(Clone, Debug)]
enum Entry {
    Pass(Pass),
    Pause(Option<String>),
}

#[derive(Clone, Copy, Debug)]
struct Carrier {
    active: bool,
    at: i32,
    last: Option<(Needle, CourseDir)>,
}

struct Scheduler {
    entries: Vec<Entry>,
    pending: Option<Pass>,
    carriers: [Carrier; 6],
    next: CourseDir,
    stops: [i32; 2],
    rack: i32,
    speed: u32,
    cam: u8,
    spacing: i32,
    add_roller: i32,
}

fn carrier_id(cs: &[CarrierId]) -> Result<u8, Refusal> {
    if cs.len() != 1 || cs[0].as_str().len() != 1 {
        return Err(unsupported(
            "Kniterate passes require one carrier from 1 through 6",
        ));
    }
    let c = cs[0].as_str().as_bytes()[0];
    if !(b'1'..=b'6').contains(&c) {
        return Err(unsupported("Kniterate carrier must be 1 through 6"));
    }
    Ok(c - b'0')
}

fn dial(leading: i32, trailing: i32) -> Result<u8, Refusal> {
    // FTX compiler records dimensionless cam values in exact tenths in Stitch.
    if leading != trailing || !(0..=150).contains(&leading) || leading % 10 != 0 {
        return Err(unsupported(
            "Kniterate needs equal whole-number cam dials 0..15; fractional settings cannot be rounded",
        ));
    }
    Ok((leading / 10) as u8)
}

impl Scheduler {
    fn slot(&self, n: Needle) -> Result<i32, Refusal> {
        if n.bed.is_slider() || !(0..252).contains(&n.index) {
            return Err(unsupported(
                "KCode needle is outside the 252-needle main beds",
            ));
        }
        Ok(n.index + if n.bed == Bed::Back { self.rack } else { 0 })
    }

    fn blank(&self, dir: CourseDir, carrier: u8, slot: i32) -> Pass {
        Pass {
            kind: Kind::Miss,
            dir,
            carrier,
            rack: self.rack,
            speed: self.speed,
            roller: 0,
            front: BTreeMap::new(),
            back: BTreeMap::new(),
            min: slot,
            max: slot,
            last: slot,
            out: false,
            carriage: [0; 2],
            yarn: None,
        }
    }

    fn flush(&mut self) -> Result<(), Refusal> {
        if let Some(pass) = self.pending.take() {
            self.emit(pass)?;
        }
        Ok(())
    }

    fn emit(&mut self, mut pass: Pass) -> Result<(), Refusal> {
        if pass.dir != self.next {
            let mut empty = self.blank(self.next, 0, pass.min);
            empty.carriage = self.stops;
            self.entries.push(Entry::Pass(empty));
            self.next = self.next.reversed();
        }
        let mut left = 2 * pass.min - 5;
        let mut right = 2 * pass.max + 5;
        if pass.carrier != 0 {
            let c = usize::from(pass.carrier - 1);
            let start = self.carriers[c].at;
            let mut stop = match pass.dir {
                CourseDir::Plus => right,
                CourseDir::Minus => left,
            };
            if pass.out {
                stop = PARKING[c];
            } else {
                let delta = if pass.dir == CourseDir::Plus {
                    2 * self.spacing
                } else {
                    -2 * self.spacing
                };
                while self
                    .carriers
                    .iter()
                    .enumerate()
                    .any(|(i, other)| i != c && (other.at - stop).abs() < 2 * self.spacing)
                {
                    stop += delta;
                }
            }
            let yarn = if pass.dir == CourseDir::Plus {
                [start, stop]
            } else {
                [stop, start]
            };
            if yarn[0] >= yarn[1] {
                return Err(unsupported(
                    "carrier cannot traverse this pass from its current parking position",
                ));
            }
            pass.yarn = Some(yarn);
            left = left.min(yarn[0]);
            right = right.max(yarn[1]);
            self.carriers[c].at = stop;
        }
        let starting_side = if pass.dir == CourseDir::Plus { 0 } else { 1 };
        if starting_side == 0 {
            left = left.min(self.stops[0]);
        } else {
            right = right.max(self.stops[1]);
        }
        pass.carriage = [left, right];
        if left >= right || left < -31 || right > 535 {
            return Err(unsupported(
                "carrier spacing or carriage travel exceeds the KCode bed margins",
            ));
        }
        if let Some(previous) = self.entries.iter_mut().rev().find_map(|entry| match entry {
            Entry::Pass(p) => Some(p),
            Entry::Pause(_) => None,
        }) {
            previous.carriage[starting_side] = pass.carriage[starting_side];
        }
        self.stops = pass.carriage;
        self.next = pass.dir.reversed();
        self.entries.push(Entry::Pass(pass));
        Ok(())
    }

    fn motion(&mut self, carrier: u8, dir: CourseDir, slot: i32, out: bool) -> Result<(), Refusal> {
        self.flush()?;
        let mut pass = self.blank(dir, carrier, slot);
        pass.out = out;
        self.emit(pass)
    }

    fn prepare_carrier(&mut self, carrier: u8, dir: CourseDir, slot: i32) -> Result<(), Refusal> {
        if carrier == 0 {
            return Ok(());
        }
        let c = usize::from(carrier - 1);
        if !self.carriers[c].active {
            return Err(unsupported("KCode operation uses an inactive carrier"));
        }
        let wrong_side = match dir {
            CourseDir::Plus => self.carriers[c].at >= 2 * slot,
            CourseDir::Minus => self.carriers[c].at <= 2 * slot,
        };
        if wrong_side {
            self.motion(carrier, dir.reversed(), slot, false)?;
        }
        Ok(())
    }

    fn clear_other_carriers(&mut self, needle: Needle, selected: u8) -> Result<(), Refusal> {
        let slot = self.slot(needle)?;
        if self.pending.as_ref().is_some_and(|p| p.carrier != selected) {
            self.flush()?;
        }
        for c in 1..=6 {
            if c == selected {
                continue;
            }
            let state = self.carriers[usize::from(c - 1)];
            if !state.active {
                continue;
            }
            let Some((last, dir)) = state.last else {
                continue;
            };
            let attached = 2 * self.slot(last)? + if dir == CourseDir::Plus { 1 } else { -1 };
            let kick = if attached < 2 * slot {
                CourseDir::Minus
            } else {
                CourseDir::Plus
            };
            let wrong = if kick == CourseDir::Minus {
                state.at >= 2 * slot
            } else {
                state.at <= 2 * slot
            };
            if wrong {
                self.motion(c, kick, self.slot(last)?, false)?;
            }
        }
        Ok(())
    }

    fn action(
        &mut self,
        kind: Kind,
        dir: CourseDir,
        needle: Needle,
        to: Option<Needle>,
        carrier: u8,
    ) -> Result<(), Refusal> {
        let slot = self.slot(needle)?;
        if let Some(target) = to
            && (needle.bed == target.bed || self.slot(target)? != slot)
        {
            return Err(unsupported(
                "KCode transfer/split needles must align on opposite beds",
            ));
        }
        if kind != Kind::Miss {
            self.clear_other_carriers(needle, carrier)?;
        }
        let mergeable = self.pending.as_ref().is_some_and(|p| {
            p.kind == kind
                && p.dir == dir
                && p.carrier == carrier
                && p.rack == self.rack
                && p.speed == self.speed
                && self.add_roller == 0
                && !p.out
                && to.is_none()
                && if dir == CourseDir::Plus {
                    slot > p.last
                } else {
                    slot < p.last
                }
        });
        if !mergeable {
            self.flush()?;
            self.prepare_carrier(carrier, dir, slot)?;
            let mut pass = self.blank(dir, carrier, slot);
            pass.kind = kind;
            pass.roller = if matches!(kind, Kind::Transfer(_) | Kind::Split(_)) {
                0
            } else {
                100
            };
            pass.roller = pass
                .roller
                .checked_add(self.add_roller)
                .ok_or_else(|| unsupported("roller advance overflow"))?;
            self.add_roller = 0;
            if !(0..=2000).contains(&pass.roller) {
                return Err(unsupported(
                    "KCode roller advance must be within 0..2000 units",
                ));
            }
            self.pending = Some(pass);
        }
        let pass = self.pending.as_mut().expect("pass created above");
        pass.min = pass.min.min(slot);
        pass.max = pass.max.max(slot);
        pass.last = slot;
        let cam = if matches!(kind, Kind::Transfer(_)) {
            3
        } else {
            self.cam
        };
        if kind != Kind::Miss {
            if needle.bed == Bed::Front {
                pass.front.insert(needle.index, cam);
            } else {
                pass.back.insert(needle.index, cam);
            }
            if let Some(target) = to {
                if target.bed == Bed::Front {
                    pass.front.insert(target.index, cam);
                } else {
                    pass.back.insert(target.index, cam);
                }
            }
        }
        if let Some(target) = to {
            for state in &mut self.carriers {
                if let Some((last, _)) = &mut state.last
                    && *last == needle
                {
                    *last = target;
                }
            }
        }
        if carrier != 0 && kind != Kind::Miss {
            self.carriers[usize::from(carrier - 1)].last = Some((needle, dir));
        }
        Ok(())
    }
}

fn schedule(program: &KnitoutProgram, profile: &MachineProfile) -> Result<Vec<Entry>, Refusal> {
    if !(2..=12).contains(&profile.min_carrier_spacing) {
        return Err(unsupported(
            "KCode carrier spacing must be within 2..12 needles",
        ));
    }
    let mut s = Scheduler {
        entries: Vec::new(),
        pending: None,
        carriers: PARKING.map(|at| Carrier {
            active: false,
            at,
            last: None,
        }),
        next: CourseDir::Plus,
        stops: [-23, 525],
        rack: 0,
        speed: 100,
        cam: 5,
        spacing: profile.min_carrier_spacing as i32,
        add_roller: 0,
    };
    for op in &program.ops {
        match op {
            KnitoutOp::Comment { .. } => {}
            KnitoutOp::In { carriers } => {
                let c = usize::from(carrier_id(carriers)? - 1);
                if s.carriers[c].active {
                    return Err(unsupported("carrier is already active"));
                }
                s.carriers[c].active = true;
                s.carriers[c].last = None;
            }
            KnitoutOp::Out { carriers } => {
                s.flush()?;
                let carrier = carrier_id(carriers)?;
                let c = usize::from(carrier - 1);
                let (last, _) = s.carriers[c]
                    .last
                    .ok_or_else(|| unsupported("cannot park an unused carrier"))?;
                let slot = s.slot(last)?;
                s.prepare_carrier(carrier, CourseDir::Minus, slot)?;
                s.motion(carrier, CourseDir::Minus, slot, true)?;
                s.carriers[c].active = false;
            }
            KnitoutOp::Stitch { leading, trailing } => {
                s.cam = dial(*leading, *trailing)?;
            }
            KnitoutOp::Rack { quarter_pitches } => {
                s.flush()?;
                if quarter_pitches % 4 != 0 || !(-16..=16).contains(quarter_pitches) {
                    return Err(unsupported(
                        "Kniterate KCode supports whole-pitch racking within -4..4",
                    ));
                }
                s.rack = quarter_pitches / 4;
            }
            KnitoutOp::Knit {
                dir,
                needle,
                carriers,
            }
            | KnitoutOp::Tuck {
                dir,
                needle,
                carriers,
            }
            | KnitoutOp::Miss {
                dir,
                needle,
                carriers,
            } => {
                let carrier = if carriers.is_empty() {
                    0
                } else {
                    carrier_id(carriers)?
                };
                let kind = match op {
                    KnitoutOp::Knit { .. } => Kind::Knit,
                    KnitoutOp::Tuck { .. } => Kind::Tuck,
                    _ => Kind::Miss,
                };
                s.action(kind, *dir, *needle, None, carrier)?;
            }
            KnitoutOp::Xfer { from, to } => {
                s.flush()?;
                s.action(Kind::Transfer(from.bed), s.next, *from, Some(*to), 0)?;
                s.flush()?;
            }
            KnitoutOp::Split {
                dir,
                from,
                to,
                carriers,
            } => {
                s.action(
                    Kind::Split(from.bed),
                    *dir,
                    *from,
                    Some(*to),
                    carrier_id(carriers)?,
                )?;
                s.flush()?;
            }
            KnitoutOp::Drop { needle } | KnitoutOp::Amiss { needle } => {
                let kind = if matches!(op, KnitoutOp::Drop { .. }) {
                    Kind::Knit
                } else {
                    Kind::Tuck
                };
                s.flush()?;
                s.action(kind, s.next, *needle, None, 0)?;
                s.flush()?;
            }
            KnitoutOp::Pause { note } => {
                s.flush()?;
                s.entries.push(Entry::Pause(note.clone()));
            }
            KnitoutOp::SpeedNumber { speed } => {
                if *speed > 600 {
                    return Err(unsupported("Kniterate speed must be within 0..600"));
                }
                s.flush()?;
                s.speed = *speed;
            }
            KnitoutOp::AddRollerAdvance { units } => {
                s.flush()?;
                s.add_roller = s
                    .add_roller
                    .checked_add(*units)
                    .ok_or_else(|| unsupported("roller advance overflow"))?;
            }
            KnitoutOp::CarrierSpacing { needles } => {
                if !(2..=12).contains(needles) {
                    return Err(unsupported(
                        "KCode carrier spacing must be within 2..12 needles",
                    ));
                }
                s.flush()?;
                s.spacing = *needles as i32;
            }
            KnitoutOp::InHook { .. }
            | KnitoutOp::ReleaseHook { .. }
            | KnitoutOp::OutHook { .. }
            | KnitoutOp::PresserMode { .. } => {
                return Err(unsupported(
                    "Kniterate has grippers and no yarn-inserting hooks, sliders or presser control",
                ));
            }
        }
    }
    s.flush()?;
    if s.carriers.iter().any(|c| c.active) || s.add_roller != 0 {
        return Err(unsupported(
            "KCode requires parked carriers and no unapplied roller control at program end",
        ));
    }
    Ok(s.entries)
}

fn raster(pass: &Pass, selected: &BTreeMap<i32, u8>) -> String {
    let mut cells = vec![b'_'; RASTER_LEN];
    cells[..15].fill(b'.');
    cells[268..].fill(b'.');
    for &needle in selected.keys() {
        cells[15 + needle as usize] = b'-';
    }
    let mut markers = BTreeMap::<usize, String>::new();
    let index = |position: i32| ((position + 31) / 2) as usize;
    markers
        .entry(index(pass.carriage[0]))
        .or_default()
        .push('/');
    if let Some(yarn) = pass.yarn {
        for stop in yarn {
            markers
                .entry(index(stop))
                .or_default()
                .push(char::from(b'0' + pass.carrier));
        }
    }
    markers
        .entry(index(pass.carriage[1]))
        .or_default()
        .push('\\');
    let mut out = String::with_capacity(RASTER_LEN + 4);
    for (i, c) in cells.iter().enumerate() {
        if let Some(marker) = markers.get(&i) {
            out.push_str(marker);
        }
        out.push(char::from(*c));
    }
    if let Some(marker) = markers.get(&RASTER_LEN) {
        out.push_str(marker);
    }
    out
}

fn cams(selected: &BTreeMap<i32, u8>, dir: CourseDir) -> String {
    let mut values = vec![b'0'; RASTER_LEN];
    if let Some((&first, &value)) = selected.first_key_value() {
        values.fill(b"0123456789ABCDEF"[usize::from(value)]);
        let mut previous = first;
        for (&needle, &cam) in selected {
            let v = b"0123456789ABCDEF"[usize::from(cam)];
            if dir == CourseDir::Plus {
                values[16 + previous as usize..=15 + needle as usize].fill(v);
            } else {
                values[15 + needle as usize..].fill(v);
            }
            values[15 + needle as usize] = v;
            previous = needle;
        }
        for (&needle, &cam) in selected {
            values[15 + needle as usize] = b"0123456789ABCDEF"[usize::from(cam)];
        }
        let last = selected.last_key_value().expect("nonempty");
        values[16 + *last.0 as usize..].fill(b"0123456789ABCDEF"[usize::from(*last.1)]);
    }
    String::from_utf8(values).expect("ASCII cam alphabet")
}

fn render(entries: &[Entry]) -> Result<String, Refusal> {
    let mut out = String::from(
        "HOME\nRACK:0\n// FrankenTextiles: zero-based needles; software-verified, hardware unqualified\n",
    );
    let mut rack = 0;
    for entry in entries {
        match entry {
            Entry::Pause(note) => {
                // Official Kniterate support, 2026-04-07: an uncommented word pauses.
                if let Some(note) = note {
                    if note.contains(['\n', '\r']) || !note.is_ascii() {
                        return Err(unsupported("pause note must be a single ASCII line"));
                    }
                    let _ = writeln!(out, "// Operator: {note}");
                }
                out.push_str("Pause\n");
            }
            Entry::Pass(pass) => {
                if pass.rack != rack {
                    let _ = writeln!(out, "RACK:{}", pass.rack);
                    rack = pass.rack;
                }
                out.push_str("//\n//\n//\n//\n");
                let _ = writeln!(
                    out,
                    "FRNT:{}\nSTIF:{}\nREAR:{}\nSTIR:{}\n{} {} {} {} {}",
                    raster(pass, &pass.front),
                    cams(&pass.front, pass.dir),
                    raster(pass, &pass.back),
                    cams(&pass.back, pass.dir),
                    if pass.dir == CourseDir::Plus {
                        ">>"
                    } else {
                        "<<"
                    },
                    pass.mode(),
                    pass.carrier,
                    pass.speed,
                    pass.roller
                );
            }
        }
    }
    Ok(out)
}

fn check_readback(
    text: &str,
    program: &KnitoutProgram,
    entries: &[Entry],
    profile: &MachineProfile,
) -> Result<(KCodeReadback, KCodeTakedownReport), Refusal> {
    let decoded = readback::decode(text)?;
    readback::compare_program(&decoded, program)?;
    readback::compare_schedule(&decoded, entries)?;
    let takedown = decoded.verify_takedown(profile)?;
    Ok((decoded.summary(), takedown))
}

#[cfg(test)]
mod tests;

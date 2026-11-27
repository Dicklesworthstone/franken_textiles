//! Independent KCode reader. This module does not call the emitter, raster builder,
//! cam interpolator or pass scheduler. Its grammar is exercised against frozen
//! upstream files, including an upstream split fixture with a known stale opcode.

mod safety;

pub use safety::{KCodeTakedownReport, verify_kniterate_takedown};

use std::collections::BTreeMap;

use ftx_core::receipt::sha256_hex;
use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::knit::{Bed, CourseDir, KnitoutOp, KnitoutProgram, Needle};
use serde::{Deserialize, Serialize};

/// Independent structural and needle-operation report for a real KCode file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KCodeReadback {
    /// Exact input file digest.
    pub kc_sha256: String,
    /// Physical carriage passes, including empty moves.
    pub pass_count: usize,
    /// Needle operations, counting each transfer or split once.
    pub needle_operation_count: usize,
    /// Knit operations with yarn.
    pub knit_count: usize,
    /// Tuck operations with yarn.
    pub tuck_count: usize,
    /// Transfers without yarn.
    pub transfer_count: usize,
    /// Splits with yarn.
    pub split_count: usize,
    /// Drops without yarn.
    pub drop_count: usize,
    /// Empty-carrier tuck-release operations.
    pub amiss_count: usize,
    /// Executable pauses; commented instructions do not count.
    pub operator_pause_count: usize,
    /// Largest absolute whole-pitch rack value encountered.
    pub max_abs_rack: i32,
}

/// Reads the actual upstream KCode grammar, independently of the FTX emitter.
/// Validates bed widths, margin/sentinel cells, hexadecimal cams, aligned transfer
/// pairs, receiver direction, travel continuity and carrier pickup positions.
/// This is a software-format check, not firmware or physical qualification.
///
/// # Errors
/// `VerificationMismatch` for malformed/unsupported commands, impossible motion,
/// altered raster widths or unmatched transfer selections.
pub fn read_kniterate_kc(text: &str) -> Result<KCodeReadback, Refusal> {
    Ok(decode(text)?.summary())
}

fn bad(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::VerificationMismatch, message)
}

#[cfg(test)]
mod fixture_addresses {
    use super::*;

    #[test]
    fn frozen_keep_fixture_preserves_the_independently_authored_needle_addresses() {
        let decoded = decode(include_str!(
            "../../../tests/fixtures/kniterate/position-keep.kc"
        ))
        .unwrap();
        let actions: Vec<_> = decoded
            .entries
            .iter()
            .filter_map(|record| match record {
                Record::Pass(pass) => Some(pass.actions.as_slice()),
                Record::Pause(_) => None,
            })
            .flatten()
            .collect();
        // The frozen .k authors front needles 1..7 with Position: Keep; upstream
        // addresses those physical hooks as zero-based 0..6 in its KCode raster.
        let tucks: Vec<_> = actions
            .iter()
            .filter(|a| a.code == "tuck")
            .map(|a| (a.needle.index, a.dir.unwrap()))
            .collect();
        use CourseDir::{Minus, Plus};
        assert_eq!(
            tucks,
            [
                (0, Plus),
                (2, Plus),
                (4, Plus),
                (6, Plus),
                (5, Minus),
                (3, Minus),
                (1, Minus)
            ]
        );
        let knits: Vec<_> = actions
            .iter()
            .filter(|a| a.code == "knit")
            .map(|a| (a.needle.index, a.dir.unwrap()))
            .collect();
        let expected: Vec<_> = (0..7)
            .map(|i| (i, Plus))
            .chain((0..7).rev().map(|i| (i, Minus)))
            .collect();
        assert_eq!(knits, expected);
        let mut drops: Vec<_> = actions
            .iter()
            .filter(|a| a.code == "drop")
            .map(|a| a.needle.index)
            .collect();
        drops.sort_unstable();
        assert_eq!(drops, (0..7).collect::<Vec<_>>());
        assert!(
            actions
                .iter()
                .all(|a| a.needle.bed == Bed::Front && a.cam == 5 && a.rack == 0)
        );
        assert!(
            actions
                .iter()
                .filter(|a| matches!(a.code, "knit" | "tuck"))
                .all(|a| a.carrier == 6)
        );
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Action {
    code: &'static str,
    needle: Needle,
    to: Option<Needle>,
    dir: Option<CourseDir>,
    carrier: u8,
    cam: u8,
    rack: i32,
    speed: u32,
}

#[derive(Debug)]
pub(super) struct ReadPass {
    mode: String,
    dir: CourseDir,
    carrier: u8,
    rack: i32,
    speed: u32,
    roller: i32,
    front: BTreeMap<i32, u8>,
    back: BTreeMap<i32, u8>,
    carriage: [i32; 2],
    yarn: Option<[i32; 2]>,
    actions: Vec<Action>,
}

#[derive(Debug)]
enum Record {
    Pass(ReadPass),
    Pause(Option<String>),
}

#[derive(Debug)]
pub(super) struct Decoded {
    entries: Vec<Record>,
    sha256: String,
}

impl Decoded {
    pub(super) fn verify_takedown(
        &self,
        profile: &ftx_pattern::knit::MachineProfile,
    ) -> Result<KCodeTakedownReport, Refusal> {
        safety::check_decoded(self, profile)
    }

    pub(super) fn summary(&self) -> KCodeReadback {
        let mut r = KCodeReadback {
            kc_sha256: self.sha256.clone(),
            pass_count: 0,
            needle_operation_count: 0,
            knit_count: 0,
            tuck_count: 0,
            transfer_count: 0,
            split_count: 0,
            drop_count: 0,
            amiss_count: 0,
            operator_pause_count: 0,
            max_abs_rack: 0,
        };
        for record in &self.entries {
            match record {
                Record::Pause(_) => r.operator_pause_count += 1,
                Record::Pass(pass) => {
                    r.pass_count += 1;
                    r.max_abs_rack = r.max_abs_rack.max(pass.rack.abs());
                    for action in &pass.actions {
                        r.needle_operation_count += 1;
                        match action.code {
                            "knit" => r.knit_count += 1,
                            "tuck" => r.tuck_count += 1,
                            "xfer" => r.transfer_count += 1,
                            "split" => r.split_count += 1,
                            "drop" => r.drop_count += 1,
                            "amiss" => r.amiss_count += 1,
                            _ => unreachable!("closed action alphabet"),
                        }
                    }
                }
            }
        }
        r
    }
}

#[derive(Debug, PartialEq, Eq)]
struct BedRow {
    selected: Vec<i32>,
    width: usize,
    carriage: [i32; 2],
    yarn: Option<[i32; 2]>,
}

fn bed_row(text: &str, carrier: u8) -> Result<BedRow, Refusal> {
    let mut cells = Vec::new();
    let mut markers = Vec::new();
    for c in text.bytes() {
        match c {
            b'.' | b'_' | b'-' => cells.push(c),
            b'/' | b'\\' => markers.push((c, 2 * cells.len() as i32 - 31)),
            b'1'..=b'6' if c - b'0' == carrier => markers.push((c, 2 * cells.len() as i32 - 31)),
            _ => {
                return Err(bad(
                    "KCode bed row contains an unknown selection or carrier marker",
                ));
            }
        }
    }
    if !matches!(cells.len(), 282 | 283)
        || !cells[..15].iter().all(|c| *c == b'.')
        || !cells[cells.len() - 15..].iter().all(|c| *c == b'.')
        || !cells[15..cells.len() - 15]
            .iter()
            .all(|c| matches!(c, b'_' | b'-'))
        || (cells.len() == 283 && cells[267] != b'_')
    {
        return Err(bad(
            "KCode raster must contain 252 usable needles and intact margins/sentinel",
        ));
    }
    let expected = if carrier == 0 {
        vec![b'/', b'\\']
    } else {
        vec![b'/', b'0' + carrier, b'0' + carrier, b'\\']
    };
    if markers.iter().map(|m| m.0).collect::<Vec<_>>() != expected {
        return Err(bad(
            "KCode carriage/carrier markers are missing, duplicated or out of order",
        ));
    }
    let carriage = [markers[0].1, markers[markers.len() - 1].1];
    let yarn = (carrier != 0).then(|| [markers[1].1, markers[2].1]);
    if carriage[0] >= carriage[1] || yarn.is_some_and(|y| y[0] >= y[1]) {
        return Err(bad("KCode carriage or carrier endpoints are reversed"));
    }
    let selected = cells[15..267]
        .iter()
        .enumerate()
        .filter_map(|(i, c)| (*c == b'-').then_some(i as i32))
        .collect();
    Ok(BedRow {
        selected,
        width: cells.len(),
        carriage,
        yarn,
    })
}

fn cam_row(text: &str, row: &BedRow) -> Result<BTreeMap<i32, u8>, Refusal> {
    if text.len() != row.width
        || !text
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'A'..=b'F').contains(&c))
    {
        return Err(bad(
            "KCode cam raster must match the bed width and use hexadecimal dials 0..F",
        ));
    }
    Ok(row
        .selected
        .iter()
        .map(|&n| {
            let c = text.as_bytes()[15 + n as usize];
            (n, if c <= b'9' { c - b'0' } else { c - b'A' + 10 })
        })
        .collect())
}

fn field<'a>(line: &'a str, prefix: &str) -> Result<&'a str, Refusal> {
    line.strip_prefix(prefix)
        .ok_or_else(|| bad(format!("expected KCode {prefix} field")))
}

fn decode_pass(lines: &[&str], rack: i32) -> Result<ReadPass, Refusal> {
    let tokens: Vec<_> = lines[4].split_whitespace().collect();
    if tokens.len() != 5 {
        return Err(bad(
            "KCode pass needs direction, mode, carrier, speed and roller",
        ));
    }
    let dir = match tokens[0] {
        ">>" => CourseDir::Plus,
        "<<" => CourseDir::Minus,
        _ => return Err(bad("invalid KCode pass direction")),
    };
    let mode = tokens[1];
    if !matches!(
        mode,
        "Kn-Kn" | "Tu-Tu" | "Kn-Tu" | "Tu-Kn" | "Tr-Rr" | "Tr-Rl" | "Rr-Tr" | "Rl-Tr"
    ) {
        return Err(bad("unknown KCode pass cam mode"));
    }
    if (mode.contains("Rr") && dir != CourseDir::Plus)
        || (mode.contains("Rl") && dir != CourseDir::Minus)
    {
        return Err(bad(
            "KCode receiving cam direction contradicts carriage travel (stale upstream split opcode)",
        ));
    }
    let carrier: u8 = tokens[2]
        .parse()
        .map_err(|_| bad("invalid KCode carrier"))?;
    let speed: u32 = tokens[3].parse().map_err(|_| bad("invalid KCode speed"))?;
    let roller: i32 = tokens[4]
        .parse()
        .map_err(|_| bad("invalid KCode roller advance"))?;
    if carrier > 6 || speed > 600 || !(0..=2000).contains(&roller) {
        return Err(bad("KCode pass control outside supported machine range"));
    }
    let front_row = bed_row(field(lines[0], "FRNT:")?, carrier)?;
    let back_row = bed_row(field(lines[2], "REAR:")?, carrier)?;
    if front_row.width != back_row.width
        || front_row.carriage != back_row.carriage
        || front_row.yarn != back_row.yarn
    {
        return Err(bad(
            "front and rear KCode carriage/carrier geometry disagree",
        ));
    }
    let front = cam_row(field(lines[1], "STIF:")?, &front_row)?;
    let back = cam_row(field(lines[3], "STIR:")?, &back_row)?;
    for slot in front.keys().copied().chain(back.keys().map(|n| *n + rack)) {
        if 2 * slot <= front_row.carriage[0] || 2 * slot >= front_row.carriage[1] {
            return Err(bad("selected needle lies outside the carriage traversal"));
        }
        if let Some(yarn) = front_row.yarn
            && (2 * slot <= yarn[0] || 2 * slot >= yarn[1])
        {
            return Err(bad(
                "selected needle lies outside the yarn carrier traversal",
            ));
        }
    }
    let mut actions = Vec::new();
    let make = |code, needle, to, cam| Action {
        code,
        needle,
        to,
        dir: if carrier != 0 { Some(dir) } else { None },
        carrier,
        cam,
        rack,
        speed,
    };
    if mode.contains("Tr") {
        let source_bed = if mode.starts_with("Tr-") {
            Bed::Front
        } else {
            Bed::Back
        };
        let (source, target) = if source_bed == Bed::Front {
            (&front, &back)
        } else {
            (&back, &front)
        };
        let mut paired = 0;
        for (&n, &cam) in source {
            let dest = n + if source_bed == Bed::Front {
                -rack
            } else {
                rack
            };
            if let Some(&target_cam) = target.get(&dest) {
                if cam != target_cam {
                    return Err(bad("KCode transfer cam values disagree between beds"));
                }
                paired += 1;
                actions.push(make(
                    if carrier == 0 { "xfer" } else { "split" },
                    Needle::new(source_bed, n),
                    Some(Needle::new(source_bed.opposite(), dest)),
                    cam,
                ));
            } else if carrier != 0 {
                // Upstream combines knitting on the source bed with split passes.
                actions.push(make("knit", Needle::new(source_bed, n), None, cam));
            } else {
                return Err(bad("KCode transfer source has no aligned receiving needle"));
            }
        }
        if paired != target.len() {
            return Err(bad("KCode has an unmatched receiving needle"));
        }
    } else {
        for (bed, selected, tuck) in [
            (Bed::Front, &front, mode.starts_with("Tu-")),
            (Bed::Back, &back, mode.ends_with("-Tu")),
        ] {
            for (&n, &cam) in selected {
                let code = match (carrier == 0, tuck) {
                    (false, false) => "knit",
                    (false, true) => "tuck",
                    (true, false) => "drop",
                    (true, true) => "amiss",
                };
                actions.push(make(code, Needle::new(bed, n), None, cam));
            }
        }
    }
    actions.sort_by_key(|a| a.needle.index + if a.needle.bed == Bed::Back { rack } else { 0 });
    if dir == CourseDir::Minus {
        actions.reverse();
    }
    if actions.windows(2).any(|a| {
        let slot = |n: Needle| n.index + if n.bed == Bed::Back { rack } else { 0 };
        slot(a[0].needle) == slot(a[1].needle)
    }) {
        return Err(bad(
            "two needle actions occupy an aligned KCode slot in one pass",
        ));
    }
    Ok(ReadPass {
        mode: mode.into(),
        dir,
        carrier,
        rack,
        speed,
        roller,
        front,
        back,
        carriage: front_row.carriage,
        yarn: front_row.yarn,
        actions,
    })
}

pub(super) fn decode(text: &str) -> Result<Decoded, Refusal> {
    if text.len() > 128 * 1024 * 1024 || !text.is_ascii() || !text.starts_with("HOME\nRACK:0\n") {
        return Err(bad(
            "KCode must start with HOME and RACK:0 and be bounded ASCII text",
        ));
    }
    let lines: Vec<_> = text.lines().collect();
    let mut entries = Vec::new();
    let mut rack = 0;
    let mut i = 2;
    let mut next = CourseDir::Plus;
    let mut previous_stop = None;
    let mut carrier_at = [-23, -19, -19, -15, -15, -15];
    while i < lines.len() {
        let line = lines[i];
        if line.is_empty() || line.starts_with("//") {
            i += 1;
            continue;
        }
        if line == "Pause" {
            let note = lines[i - 1]
                .strip_prefix("// Operator: ")
                .map(str::to_owned);
            entries.push(Record::Pause(note));
            i += 1;
            continue;
        }
        if let Some(value) = line.strip_prefix("RACK:") {
            rack = value
                .parse::<i32>()
                .map_err(|_| bad("KCode racking must be an integer"))?;
            if !(-4..=4).contains(&rack) {
                return Err(bad("KCode racking exceeds four pitches"));
            }
            i += 1;
            continue;
        }
        if !line.starts_with("FRNT:") || i + 5 > lines.len() {
            return Err(bad(format!(
                "unknown or truncated KCode command at line {}",
                i + 1
            )));
        }
        let pass = decode_pass(&lines[i..i + 5], rack)?;
        if pass.dir != next {
            return Err(bad("KCode physical carriage passes must alternate"));
        }
        let (start, stop) = if pass.dir == CourseDir::Plus {
            (0, 1)
        } else {
            (1, 0)
        };
        if previous_stop.is_some_and(|position| position != pass.carriage[start]) {
            return Err(bad("KCode carriage teleports between passes"));
        }
        if let Some(yarn) = pass.yarn {
            let c = usize::from(pass.carrier - 1);
            if yarn[start] != carrier_at[c] {
                return Err(bad(
                    "KCode carrier pickup differs from its previous parking position",
                ));
            }
            carrier_at[c] = yarn[stop];
        }
        previous_stop = Some(pass.carriage[stop]);
        next = next.reversed();
        entries.push(Record::Pass(pass));
        i += 5;
    }
    if !entries.iter().any(|entry| matches!(entry, Record::Pass(_))) {
        return Err(bad("KCode contains no carriage passes"));
    }
    Ok(Decoded {
        entries,
        sha256: sha256_hex(text.as_bytes()),
    })
}

pub(super) fn compare_schedule(
    decoded: &Decoded,
    expected: &[super::Entry],
) -> Result<(), Refusal> {
    if decoded.entries.len() != expected.len() {
        return Err(bad(
            "KCode pass/pause count differs from the scheduled source",
        ));
    }
    for (index, (actual, wanted)) in decoded.entries.iter().zip(expected).enumerate() {
        let equal = match (actual, wanted) {
            (Record::Pause(actual), super::Entry::Pause(wanted)) => actual == wanted,
            (Record::Pass(a), super::Entry::Pass(b)) => {
                a.mode == b.mode()
                    && a.dir == b.dir
                    && a.carrier == b.carrier
                    && a.rack == b.rack
                    && a.speed == b.speed
                    && a.roller == b.roller
                    && a.front == b.front
                    && a.back == b.back
                    && a.carriage == b.carriage
                    && a.yarn == b.yarn
            }
            _ => false,
        };
        if !equal {
            return Err(bad(format!(
                "KCode pass/control differs from source schedule at entry {index}"
            )));
        }
    }
    Ok(())
}

pub(super) fn compare_program(decoded: &Decoded, program: &KnitoutProgram) -> Result<(), Refusal> {
    let mut expected = Vec::<Option<Action>>::new();
    let mut expected_notes = Vec::new();
    let mut rack = 0;
    let mut speed = 100;
    let mut cam = 5;
    for op in &program.ops {
        let cs = |carriers: &[ftx_pattern::knit::CarrierId]| -> Result<u8, Refusal> {
            if carriers.is_empty() {
                return Ok(0);
            }
            if carriers.len() != 1 {
                return Err(bad("KCode read-back cannot represent plated carriers"));
            }
            let c = carriers[0]
                .as_str()
                .parse::<u8>()
                .map_err(|_| bad("invalid source carrier"))?;
            if !(1..=6).contains(&c) {
                return Err(bad("source carrier outside 1..6"));
            }
            Ok(c)
        };
        let action = match op {
            KnitoutOp::Stitch { leading, trailing } => {
                if leading != trailing || leading % 10 != 0 || !(0..=150).contains(leading) {
                    return Err(bad("source cam is not an exact Kniterate dial"));
                }
                cam = (*leading / 10) as u8;
                None
            }
            KnitoutOp::Rack { quarter_pitches } => {
                rack = *quarter_pitches / 4;
                None
            }
            KnitoutOp::SpeedNumber { speed: value } => {
                speed = *value;
                None
            }
            KnitoutOp::Pause { note } => {
                expected_notes.push(note.as_ref());
                expected.push(None);
                None
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
            } => {
                let carrier = cs(carriers)?;
                let tuck = matches!(op, KnitoutOp::Tuck { .. });
                let code = match (carrier == 0, tuck) {
                    (false, false) => "knit",
                    (false, true) => "tuck",
                    (true, false) => "drop",
                    (true, true) => "amiss",
                };
                Some(Action {
                    code,
                    needle: *needle,
                    to: None,
                    dir: (carrier != 0).then_some(*dir),
                    carrier,
                    cam,
                    rack,
                    speed,
                })
            }
            KnitoutOp::Split {
                dir,
                from,
                to,
                carriers,
            } => Some(Action {
                code: "split",
                needle: *from,
                to: Some(*to),
                dir: Some(*dir),
                carrier: cs(carriers)?,
                cam,
                rack,
                speed,
            }),
            KnitoutOp::Xfer { from, to } => Some(Action {
                code: "xfer",
                needle: *from,
                to: Some(*to),
                dir: None,
                carrier: 0,
                cam: 3,
                rack,
                speed,
            }),
            KnitoutOp::Drop { needle } | KnitoutOp::Amiss { needle } => Some(Action {
                code: if matches!(op, KnitoutOp::Drop { .. }) {
                    "drop"
                } else {
                    "amiss"
                },
                needle: *needle,
                to: None,
                dir: None,
                carrier: 0,
                cam,
                rack,
                speed,
            }),
            _ => None,
        };
        if let Some(action) = action {
            expected.push(Some(action));
        }
    }
    let actual: Vec<_> = decoded
        .entries
        .iter()
        .flat_map(|entry| match entry {
            Record::Pause(_) => vec![None],
            Record::Pass(p) => p.actions.iter().cloned().map(Some).collect(),
        })
        .collect();
    let actual_notes: Vec<_> = decoded
        .entries
        .iter()
        .filter_map(|entry| match entry {
            Record::Pause(note) => Some(note.as_ref()),
            Record::Pass(_) => None,
        })
        .collect();
    if actual_notes != expected_notes {
        return Err(bad("KCode operator instructions differ from source pauses"));
    }
    if actual != expected {
        let first = actual
            .iter()
            .zip(&expected)
            .position(|(a, b)| a != b)
            .unwrap_or(actual.len().min(expected.len()));
        return Err(bad(format!(
            "decoded KCode needle semantics differ from Knitout at action {first}: actual {:?}, expected {:?}",
            actual.get(first),
            expected.get(first)
        )));
    }
    Ok(())
}

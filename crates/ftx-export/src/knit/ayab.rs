//! Brother KH-930 / KH-270 open AYAB backend (`FTX-K07`):
//! emits an un-antialiased 1-bit indexed PNG (`<= 200` px KH-930 / `<= 114` px KH-270) plus a
//! synchronized `ayab_session_checklist.json` (`automated_carriage_control: false`).

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::receipt::sha256_hex;
use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::knit::{
    Bed, KnitDoc, KnitMachineCert, LoopKind, MachineBackendKind, MachineProfile,
};
use serde::{Deserialize, Serialize};

/// Brother AYAB knitting mode corresponding to hardware bed setup.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AyabMode {
    /// Single main bed (KH-930 / KH-270 without ribber).
    Singlebed,
    /// Main bed + KR-850 ribber classic 2-color jacquard/rib.
    RibberClassic,
    /// Tubular circular knitting on main bed + ribber.
    Circular,
}

/// Synchronized manual operator instruction at a specific course row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AyabOperatorStep {
    /// 1-based row number matching bottom-to-top AYAB image progression.
    pub row_number: u32,
    /// Human operator action (e.g. `"manual_edge_decrease"`, `"bed_transfer_to_ribber"`, `"short_row_turn"`).
    pub action: String,
    /// Exact needle positions affected.
    pub detail: String,
}

/// Synchronized `ayab_session_checklist.json` emitted alongside the 1-bit AYAB PNG.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AyabSessionChecklist {
    /// Schema tag (`"ftx-ayab-checklist/1"`).
    pub schema: String,
    /// Machine profile ID (`brother-kh930-ayab` or `brother-kh270-ayab`).
    pub machine_id: String,
    /// AYAB operating mode.
    pub mode: AyabMode,
    /// Always `false` on manual-carriage Brother machines.
    pub automated_carriage_control: bool,
    /// Pattern width in needles (`<= 200` KH-930 / `<= 114` KH-270).
    pub width_needles: u32,
    /// Pattern height in courses.
    pub height_courses: u32,
    /// SHA-256 of the companion 1-bit indexed PNG.
    pub png_sha256: String,
    /// Ordered manual shaping/transfer operator stops.
    pub operator_steps: Vec<AyabOperatorStep>,
}

/// Complete Brother AYAB export bundle.
#[derive(Clone, Debug)]
pub struct AyabExportBundle {
    /// Un-antialiased 1-bit indexed PNG bytes (2-color black/white palette).
    pub png_bytes: Vec<u8>,
    /// SHA-256 of `png_bytes`.
    pub png_sha256: String,
    /// Synchronized manual-carriage checklist.
    pub checklist: AyabSessionChecklist,
    /// Pretty deterministic JSON of `checklist`.
    pub checklist_json: String,
    /// Sealed `ftx-machine-cert/1` (`automated_carriage_control: false`).
    pub cert: KnitMachineCert,
}

/// Exports a validated [`KnitDoc`] as an un-antialiased 1-bit indexed PNG + synchronized
/// `ayab_session_checklist.json` for Brother KH-930 (`<= 200` px) or KH-270 (`<= 114` px).
///
/// # Errors
/// [`RefusalCode::MachineStateInfeasible`] if width exceeds the machine needle count or profile is
/// not `MachineBackendKind::BrotherAyab`.
pub fn export_brother_ayab(
    doc: &KnitDoc,
    profile: &MachineProfile,
    cert: &KnitMachineCert,
) -> Result<AyabExportBundle, Refusal> {
    doc.validate()?;
    profile.validate()?;
    cert.verify_integrity()?;
    if profile.backend_kind != MachineBackendKind::BrotherAyab {
        return Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            format!(
                "Brother AYAB exporter requires BrotherAyab profile (got {:?})",
                profile.backend_kind
            ),
        ));
    }
    let machine_sha256 = profile.canonical_sha256()?;
    if cert.machine_sha256 != machine_sha256 {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            "Brother AYAB certificate machine_sha256 does not match target MachineProfile",
        ));
    }
    if cert.automated_carriage_control {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            "Brother AYAB certificate must report automated_carriage_control = false",
        ));
    }
    if cert.wearer_age_class != doc.wearer_age_class {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            "Brother AYAB certificate wearer_age_class does not match KnitDoc",
        ));
    }
    if doc.wearer_age_class.is_child() && doc.has_hood_or_neck_drawstring {
        return Err(Refusal::new(
            RefusalCode::MaterialProcessHazard,
            "EN 14682 child-safety refusal: Brother AYAB export refuses hood or neck drawstrings on child/infant knitwear",
        )
        .repair("remove hood/neck drawstrings and replace with ribbed shaping or concealed snaps"));
    }
    if doc.wearer_age_class.is_infant() && doc.short_row_turns.iter().any(|turn| !turn.anchored) {
        return Err(Refusal::new(
            RefusalCode::MaterialProcessHazard,
            "infant-safety refusal: Brother AYAB export refuses unanchored short-row eyelet holes on infant (<3 yr) knitwear",
        )
        .repair("anchor every short-row turn with a wrap-and-turn or German short-row double stitch"));
    }

    let primary_carrier = profile.carrier_ids.first().ok_or_else(|| {
        Refusal::new(
            RefusalCode::InvalidDocument,
            "Brother AYAB machine profile must declare at least one yarn carrier",
        )
    })?;

    let declared_carriers: BTreeSet<&str> = profile
        .carrier_ids
        .iter()
        .map(ftx_pattern::knit::CarrierId::as_str)
        .collect();
    let mut used_carriers: BTreeSet<&str> = BTreeSet::new();
    let mut min_w = i32::MAX;
    let mut max_w = i32::MIN;
    let mut max_c = 0u32;
    let mut has_back = false;
    let mut grid: BTreeMap<(u32, i32), bool> = BTreeMap::new();
    let mut operator_steps = Vec::new();

    for st in &doc.stitches {
        if st.bed.is_slider() {
            return Err(Refusal::new(
                RefusalCode::MachineStateInfeasible,
                format!(
                    "stitch {} uses slider bed {:?}, which Brother AYAB machines do not support",
                    st.id, st.bed
                ),
            ));
        }
        if st.carrier_ids.len() > 1 {
            return Err(Refusal::new(
                RefusalCode::MachineStateInfeasible,
                format!(
                    "stitch {} uses {} plated carriers; Brother AYAB 1-bit PNG can encode at most one yarn carrier per stitch",
                    st.id,
                    st.carrier_ids.len()
                ),
            )
            .repair("use single-carrier stitches or export via Knitout v2 for plated multi-feeder knitting"));
        }
        if let Some(cid) = st.carrier_ids.first() {
            if !declared_carriers.contains(cid.as_str()) {
                return Err(Refusal::new(
                    RefusalCode::MachineStateInfeasible,
                    format!(
                        "stitch {} uses carrier '{}' which is not declared on Brother AYAB profile '{}'",
                        st.id,
                        cid.as_str(),
                        profile.id
                    ),
                )
                .repair("map stitches to the machine profile's declared A/B yarn feeders"));
            }
            used_carriers.insert(cid.as_str());
            if used_carriers.len() > 2 {
                return Err(Refusal::new(
                    RefusalCode::MachineStateInfeasible,
                    format!(
                        "Brother AYAB 1-bit indexed PNG supports at most 2 yarn colors/carriers (pattern uses {})",
                        used_carriers.len()
                    ),
                )
                .repair("reduce the pattern to 2 carriers or export bed-separated Knitout v2 / hand-knit charts"));
            }
        }
        min_w = min_w.min(st.wale);
        max_w = max_w.max(st.wale);
        max_c = max_c.max(st.course);
        if st.bed == Bed::Back {
            has_back = true;
        }
        // Pixel bit = 1 for contrast/carrier-2/back-bed or active tuck; 0 for main color knit
        let bit = st.bed == Bed::Back
            || matches!(st.kind, LoopKind::Tuck | LoopKind::Purl | LoopKind::Cable)
            || st
                .carrier_ids
                .first()
                .is_some_and(|c| c.as_str() != primary_carrier.as_str());
        if grid.insert((st.course, st.wale), bit).is_some() {
            return Err(Refusal::new(
                RefusalCode::MachineStateInfeasible,
                format!(
                    "AYAB cannot encode multiple bed operations at course {}, wale {} in one image pixel",
                    st.course, st.wale
                ),
            )
            .repair("use the bed-separated charts and reviewed Knitout program; a separate AYAB carriage-pass schedule is required for this pattern"));
        }
        if matches!(
            st.kind,
            LoopKind::Decrease | LoopKind::Increase | LoopKind::Cable
        ) {
            operator_steps.push(AyabOperatorStep {
                row_number: st.course + 1,
                action: format!("manual_{:?}", st.kind).to_lowercase(),
                detail: format!("wale {} on {:?} bed", st.wale, st.bed),
            });
        }
    }
    for turn in &doc.short_row_turns {
        operator_steps.push(AyabOperatorStep {
            row_number: turn.course + 1,
            action: "short_row_turn".to_string(),
            detail: format!(
                "hold/turn at wale {} dir {:?} (anchored={})",
                turn.turn_wale, turn.dir_after, turn.anchored
            ),
        });
    }
    operator_steps.sort_by(|a, b| {
        a.row_number
            .cmp(&b.row_number)
            .then_with(|| a.action.cmp(&b.action))
            .then_with(|| a.detail.cmp(&b.detail))
    });

    let width_i64 = i64::from(max_w) - i64::from(min_w) + 1;
    let height_u64 = u64::from(max_c) + 1;
    if width_i64 <= 0 || width_i64 > i64::from(profile.needle_count) {
        return Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            format!(
                "AYAB image width {width_i64} needles exceeds machine '{}' limit {}",
                profile.id, profile.needle_count
            ),
        ));
    }
    if height_u64 > 10_000 {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            format!("AYAB image height {height_u64} courses exceeds 10000-row export budget"),
        ));
    }
    let width = width_i64 as u32;
    let height = height_u64 as u32;
    let mode = if has_back {
        if !profile.has_back_bed {
            return Err(Refusal::new(
                RefusalCode::MachineStateInfeasible,
                format!("pattern uses back bed but '{}' is single-bed", profile.id),
            ));
        }
        AyabMode::RibberClassic
    } else {
        AyabMode::Singlebed
    };

    // Encode exact 1-bit indexed PNG (color type 3, bit depth 1, 2-entry PLTE: black=[0,0,0], white=[255,255,255]).
    // Top row of PNG corresponds to final course `max_c` so AYAB knits bottom-up cleanly!
    let stride = (width as usize).div_ceil(8);
    let mut raw = Vec::with_capacity((height as usize) * (stride + 1));
    for y in 0..height {
        let course = max_c - y;
        raw.push(0u8); // filter type 0 (None)
        let mut row = vec![0u8; stride];
        for x in 0..width {
            let wale = min_w + x as i32;
            let bit = *grid.get(&(course, wale)).unwrap_or(&false);
            if bit {
                let byte_idx = (x as usize) / 8;
                let shift = 7 - ((x as usize) % 8);
                row[byte_idx] |= 1u8 << shift;
            }
        }
        raw.extend_from_slice(&row);
    }
    let png_bytes = encode_1bit_indexed_png(width, height, &raw);
    verify_ayab_1bit_png(&png_bytes, width, height)?;
    let png_sha256 = sha256_hex(&png_bytes);

    let checklist = AyabSessionChecklist {
        schema: "ftx-ayab-checklist/1".to_string(),
        machine_id: profile.id.clone(),
        mode,
        automated_carriage_control: false,
        width_needles: width,
        height_courses: height,
        png_sha256: png_sha256.clone(),
        operator_steps,
    };
    let checklist_json = serde_json::to_string_pretty(&checklist).map_err(|e| {
        Refusal::new(
            RefusalCode::InvalidDocument,
            format!("cannot serialize AYAB checklist: {e}"),
        )
    })?;

    Ok(AyabExportBundle {
        png_bytes,
        png_sha256,
        checklist,
        checklist_json,
        cert: cert.clone(),
    })
}

/// Independently parses and verifies a Brother AYAB 1-bit indexed PNG byte stream (`FTX-K07`).
///
/// Checks PNG signature, chunk CRC32 checksums, `IHDR` (`bit_depth == 1`, `color_type == 3`,
/// exact expected dimensions), exact 2-color white/black `PLTE`, zlib decompression of `IDAT`
/// to exact unfiltered scanline size, and terminating `IEND`.
///
/// # Errors
/// [`RefusalCode::VerificationMismatch`] if any PNG invariant or checksum fails.
pub fn verify_ayab_1bit_png(
    png_bytes: &[u8],
    expected_width: u32,
    expected_height: u32,
) -> Result<(), Refusal> {
    if expected_width == 0 || expected_height == 0 {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            "AYAB read-back failed: expected PNG dimensions must be strictly positive",
        ));
    }
    if png_bytes.len() < 8 || &png_bytes[..8] != b"\x89PNG\r\n\x1a\n" {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            "AYAB read-back failed: invalid PNG signature",
        ));
    }
    let mut pos = 8usize;
    let mut chunk_index = 0usize;
    let mut seen_ihdr = false;
    let mut seen_plte = false;
    let mut seen_iend = false;
    let mut idat = Vec::new();
    while pos < png_bytes.len() {
        if pos + 12 > png_bytes.len() {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                "AYAB read-back failed: truncated PNG chunk header",
            ));
        }
        let len = u32::from_be_bytes([
            png_bytes[pos],
            png_bytes[pos + 1],
            png_bytes[pos + 2],
            png_bytes[pos + 3],
        ]) as usize;
        let kind = &png_bytes[pos + 4..pos + 8];
        let data_start = pos + 8;
        let data_end = data_start
            .checked_add(len)
            .and_then(|end| end.checked_add(4))
            .ok_or_else(|| {
                Refusal::new(
                    RefusalCode::VerificationMismatch,
                    "AYAB read-back failed: chunk length overflow",
                )
            })?;
        if data_end > png_bytes.len() {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                "AYAB read-back failed: truncated PNG chunk payload",
            ));
        }
        let data = &png_bytes[data_start..data_start + len];
        let crc_stored = u32::from_be_bytes([
            png_bytes[data_start + len],
            png_bytes[data_start + len + 1],
            png_bytes[data_start + len + 2],
            png_bytes[data_start + len + 3],
        ]);
        if crc32(&[kind, data]) != crc_stored {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                "AYAB read-back failed: PNG chunk CRC32 mismatch",
            ));
        }
        if chunk_index == 0 && kind != b"IHDR" {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                "AYAB read-back failed: first PNG chunk must be IHDR",
            ));
        }
        match kind {
            b"IHDR" => {
                if seen_ihdr || data.len() != 13 {
                    return Err(Refusal::new(
                        RefusalCode::VerificationMismatch,
                        "AYAB read-back failed: malformed IHDR chunk",
                    ));
                }
                let w = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
                let h = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
                if w != expected_width
                    || h != expected_height
                    || data[8] != 1
                    || data[9] != 3
                    || data[10] != 0
                    || data[11] != 0
                    || data[12] != 0
                {
                    return Err(Refusal::new(
                        RefusalCode::VerificationMismatch,
                        format!(
                            "AYAB read-back failed: expected {expected_width}x{expected_height} 1-bit indexed PNG (got {w}x{h} depth={} color={})",
                            data[8], data[9]
                        ),
                    ));
                }
                seen_ihdr = true;
            }
            b"PLTE" => {
                if seen_plte || !idat.is_empty() {
                    return Err(Refusal::new(
                        RefusalCode::VerificationMismatch,
                        "AYAB read-back failed: PLTE must appear once before IDAT",
                    ));
                }
                if data != [255, 255, 255, 0, 0, 0] {
                    return Err(Refusal::new(
                        RefusalCode::VerificationMismatch,
                        "AYAB read-back failed: PLTE must be exact 2-color white/black palette",
                    ));
                }
                seen_plte = true;
            }
            b"IDAT" => {
                if !seen_plte {
                    return Err(Refusal::new(
                        RefusalCode::VerificationMismatch,
                        "AYAB read-back failed: IDAT appeared before PLTE",
                    ));
                }
                idat.extend_from_slice(data);
            }
            b"IEND" => {
                if !data.is_empty() || data_end != png_bytes.len() {
                    return Err(Refusal::new(
                        RefusalCode::VerificationMismatch,
                        "AYAB read-back failed: malformed or non-final IEND chunk",
                    ));
                }
                seen_iend = true;
            }
            _ => {}
        }
        chunk_index += 1;
        pos = data_end;
    }
    if !seen_ihdr || !seen_plte || !seen_iend || idat.is_empty() {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            "AYAB read-back failed: missing required IHDR/PLTE/IDAT/IEND chunk",
        ));
    }
    let stride = (expected_width as usize).div_ceil(8);
    let expected_raw_len = (expected_height as usize)
        .checked_mul(stride + 1)
        .ok_or_else(|| {
            Refusal::new(
                RefusalCode::VerificationMismatch,
                "AYAB read-back failed: scanline size overflow",
            )
        })?;
    let raw = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(&idat, expected_raw_len)
        .map_err(|e| {
            Refusal::new(
                RefusalCode::VerificationMismatch,
                format!("AYAB read-back failed: zlib decode error ({e:?})"),
            )
        })?;
    if raw.len() != expected_raw_len {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            format!(
                "AYAB read-back failed: decoded {} bytes != expected {expected_raw_len}",
                raw.len()
            ),
        ));
    }
    for row in 0..expected_height as usize {
        if raw[row * (stride + 1)] != 0 {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                format!("AYAB read-back failed: row {row} uses non-zero PNG filter byte"),
            ));
        }
    }
    Ok(())
}

fn encode_1bit_indexed_png(width: u32, height: u32, raw_scanlines: &[u8]) -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut push_chunk = |kind: &[u8; 4], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        out.extend_from_slice(&crc32(&[kind, data]).to_be_bytes());
    };
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[1, 3, 0, 0, 0]); // bit_depth=1, color_type=3 (indexed)
    push_chunk(b"IHDR", &ihdr);
    // Exact 2-entry palette: index 0 = white background (255,255,255), index 1 = black needle select (0,0,0)
    push_chunk(b"PLTE", &[255, 255, 255, 0, 0, 0]);
    let compressed = miniz_oxide::deflate::compress_to_vec_zlib(raw_scanlines, 6);
    push_chunk(b"IDAT", &compressed);
    push_chunk(b"IEND", &[]);
    out
}

fn crc32(parts: &[&[u8]]) -> u32 {
    let mut table = [0u32; 256];
    for (n, slot) in table.iter_mut().enumerate() {
        let mut c = n as u32;
        for _ in 0..8 {
            c = if c & 1 == 1 {
                0xedb8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *slot = c;
    }
    let mut c = 0xffff_ffff_u32;
    for part in parts {
        for &b in *part {
            c = table[((c ^ u32::from(b)) & 0xff) as usize] ^ (c >> 8);
        }
    }
    c ^ 0xffff_ffff
}

#[cfg(test)]
mod tests {
    use super::*;
    use ftx_pattern::knit::{
        CourseDir, ShortRowTurn, WearerAgeClass, compile_knit_doc, draft_calibration_swatch,
    };

    #[test]
    fn ayab_readback_refuses_corruption_and_unsupported_short_rows() {
        let kh930 = MachineProfile::brother_kh930_ayab();
        let doc = draft_calibration_swatch(&kh930, "wool-150tex", 16, 6, &[5.0]).unwrap();
        let compiled = compile_knit_doc(&doc, &kh930, None, None, None).unwrap();
        let bundle = export_brother_ayab(&doc, &kh930, &compiled.cert).unwrap();

        let mut short_rows = doc;
        short_rows.short_row_turns.push(ShortRowTurn {
            course: 3,
            turn_wale: 8,
            dir_after: CourseDir::Minus,
            anchored: true,
        });
        assert_eq!(
            compile_knit_doc(&short_rows, &kh930, None, None, None)
                .unwrap_err()
                .code,
            RefusalCode::MachineStateInfeasible
        );
        verify_ayab_1bit_png(
            &bundle.png_bytes,
            bundle.checklist.width_needles,
            bundle.checklist.height_courses,
        )
        .unwrap();

        // Corrupting a PNG byte triggers CRC32 verification failure.
        let mut corrupt = bundle.png_bytes.clone();
        corrupt[20] ^= 0xff;
        assert_eq!(
            verify_ayab_1bit_png(
                &corrupt,
                bundle.checklist.width_needles,
                bundle.checklist.height_courses
            )
            .unwrap_err()
            .code,
            RefusalCode::VerificationMismatch
        );

        // Infant garment with unanchored short-row turn refuses fail-closed.
        let mut infant_doc = short_rows;
        infant_doc.wearer_age_class = WearerAgeClass::InfantUnder3;
        infant_doc.short_row_turns[0].anchored = false;
        let mut infant_cert = compiled.cert.clone();
        infant_cert = KnitMachineCert::seal(
            infant_cert.program_sha256,
            infant_cert.machine_sha256,
            None,
            None,
            WearerAgeClass::InfantUnder3,
            false,
            infant_cert.checks_passed,
            infant_cert.limitations,
            infant_cert.four_claims,
        )
        .unwrap();
        assert_eq!(
            export_brother_ayab(&infant_doc, &kh930, &infant_cert)
                .unwrap_err()
                .code,
            RefusalCode::MaterialProcessHazard
        );
    }

    #[test]
    fn ayab_refuses_lossy_overlapping_bed_pixels_instead_of_overwriting_them() {
        let profile = MachineProfile::brother_kh930_ayab();
        let mut doc = draft_calibration_swatch(&profile, "synthetic-yarn", 8, 4, &[5.0]).unwrap();
        let compiled = compile_knit_doc(&doc, &profile, None, None, None).unwrap();
        let mut back = doc.stitches[0].clone();
        back.id = doc.stitches.iter().map(|st| st.id).max().unwrap() + 1;
        back.bed = Bed::Back;
        doc.stitches.push(back);
        for _ in 0..2 {
            let error = export_brother_ayab(&doc, &profile, &compiled.cert).unwrap_err();
            assert_eq!(error.code, RefusalCode::MachineStateInfeasible);
            assert!(error.message.contains("one image pixel"));
            // Preserve parent-before-child order while reversing which bed wins the old map insert.
            let back = doc.stitches.pop().unwrap();
            doc.stitches.insert(0, back);
        }
    }

    #[test]
    fn ayab_refuses_plated_undeclared_or_three_carriers_and_malformed_png_chunk_order() {
        use ftx_pattern::knit::CarrierId;

        let profile = MachineProfile::brother_kh930_ayab();
        let doc = draft_calibration_swatch(&profile, "synthetic-yarn", 8, 4, &[5.0]).unwrap();
        let compiled = compile_knit_doc(&doc, &profile, None, None, None).unwrap();

        // 1. Plated carriers (2 carriers on one stitch) refuse fail-closed.
        let mut plated = doc.clone();
        plated.stitches[0].carrier_ids = vec![CarrierId("A".into()), CarrierId("B".into())];
        let err = export_brother_ayab(&plated, &profile, &compiled.cert).unwrap_err();
        assert_eq!(err.code, RefusalCode::MachineStateInfeasible);
        assert!(!err.repairs.is_empty());

        // 2. Undeclared carrier refuses fail-closed.
        let mut undeclared = doc.clone();
        undeclared.stitches[0].carrier_ids = vec![CarrierId("Z".into())];
        let err = export_brother_ayab(&undeclared, &profile, &compiled.cert).unwrap_err();
        assert_eq!(err.code, RefusalCode::MachineStateInfeasible);

        // 3. 3 distinct carriers on an extended profile refuse rather than collapsing into 1-bit PNG.
        let mut three_carrier_profile = profile.clone();
        three_carrier_profile
            .carrier_ids
            .push(CarrierId("C".into()));
        let mut three_color_doc = doc.clone();
        three_color_doc.stitches[0].carrier_ids = vec![CarrierId("A".into())];
        three_color_doc.stitches[1].carrier_ids = vec![CarrierId("B".into())];
        three_color_doc.stitches[2].carrier_ids = vec![CarrierId("C".into())];
        let three_cert = KnitMachineCert::seal(
            compiled.cert.program_sha256.clone(),
            three_carrier_profile.canonical_sha256().unwrap(),
            None,
            None,
            three_color_doc.wearer_age_class,
            false,
            compiled.cert.checks_passed.clone(),
            compiled.cert.limitations.clone(),
            compiled.cert.four_claims.clone(),
        )
        .unwrap();
        let err =
            export_brother_ayab(&three_color_doc, &three_carrier_profile, &three_cert).unwrap_err();
        assert_eq!(err.code, RefusalCode::MachineStateInfeasible);
        assert!(err.message.contains("at most 2 yarn colors/carriers"));

        // 4. verify_ayab_1bit_png refuses 0x0 and out-of-order IDAT before PLTE.
        let bundle = export_brother_ayab(&doc, &profile, &compiled.cert).unwrap();
        assert_eq!(
            verify_ayab_1bit_png(&bundle.png_bytes, 0, 0)
                .unwrap_err()
                .code,
            RefusalCode::VerificationMismatch
        );
        // Build a syntactically valid CRC PNG with IDAT before PLTE -> must fail!
        let mut bad_order = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut push_chunk = |kind: &[u8; 4], data: &[u8]| {
            bad_order.extend_from_slice(&(data.len() as u32).to_be_bytes());
            bad_order.extend_from_slice(kind);
            bad_order.extend_from_slice(data);
            bad_order.extend_from_slice(&crc32(&[kind, data]).to_be_bytes());
        };
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&1u32.to_be_bytes());
        ihdr.extend_from_slice(&1u32.to_be_bytes());
        ihdr.extend_from_slice(&[1, 3, 0, 0, 0]);
        push_chunk(b"IHDR", &ihdr);
        let compressed = miniz_oxide::deflate::compress_to_vec_zlib(&[0, 0], 6);
        push_chunk(b"IDAT", &compressed);
        push_chunk(b"PLTE", &[255, 255, 255, 0, 0, 0]);
        push_chunk(b"IEND", &[]);
        assert_eq!(
            verify_ayab_1bit_png(&bad_order, 1, 1).unwrap_err().code,
            RefusalCode::VerificationMismatch
        );
    }
}

//! Brother KH-930 / KH-270 open AYAB backend (`FTX-K07`):
//! emits an un-antialiased 1-bit indexed PNG (`<= 200` px KH-930 / `<= 114` px KH-270) plus a
//! synchronized `ayab_session_checklist.json` (`automated_carriage_control: false`).

use std::collections::BTreeMap;

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
        ));
    }

    let mut min_w = i32::MAX;
    let mut max_w = i32::MIN;
    let mut max_c = 0u32;
    let mut has_back = false;
    let mut grid: BTreeMap<(u32, i32), bool> = BTreeMap::new();
    let mut operator_steps = Vec::new();

    for st in &doc.stitches {
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
                .is_some_and(|c| c.as_str() != profile.carrier_ids[0].as_str());
        grid.insert((st.course, st.wale), bit);
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

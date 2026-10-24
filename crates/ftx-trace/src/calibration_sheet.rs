//! File-free printable metric reference, spec 02 A.9. No image detector or physical accuracy claim.
//! Pixel-edge geometry is shared by the PNG, lossless one-bit PDF and JSON sidecar.

use crate::raster::{
    draw,
    image::{Gray, encode_png_gray},
};
use ftx_core::{Refusal, RefusalCode, py};
use ftx_export::text::{Anchor, stroke_text};
use serde_json::{Value, json};

mod dictionaries;

/// The reference used by the built-in ChArUco image detector.
pub const DEFAULT_DICTIONARY: &str = "DICT_4X4_50";

/// Supported predefined OpenCV names, including its exact case-sensitive aliases.
#[must_use]
pub fn dictionaries() -> &'static [&'static str] {
    dictionaries::NAMES
}

#[cfg(test)]
use DEFAULT_DICTIONARY as DICTIONARY;
#[cfg(test)]
use dictionaries::ARUCO4 as MARKERS;

fn bad_input(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

/// The three matching printable files, without filesystem paths or browser-owned state.
pub struct Sheet {
    /// Lossless 300-DPI grayscale PNG.
    pub png: Vec<u8>,
    /// Lossless one-bit PDF using the same pixel-edge geometry.
    pub pdf: Vec<u8>,
    /// `fabric-calib-sheet/1` geometry and print instructions, without `paths`.
    pub sidecar: Value,
}

impl Sheet {
    /// Stable native/browser sidecar bytes, including the final newline.
    #[must_use]
    pub fn sidecar_json(&self) -> Vec<u8> {
        let mut bytes =
            serde_json::to_vec_pretty(&self.sidecar).expect("finite calibration geometry");
        bytes.push(b'\n');
        bytes
    }
}

/// Render one supported reference entirely in memory. Print scale still needs a physical ruler.
///
/// # Errors
/// Unsupported paper/dictionary, noninteger dimensions or a layout that would clip marks/text.
pub fn render(
    paper: &str,
    square_mm: f64,
    marker_mm: f64,
    dictionary: &str,
) -> Result<Sheet, Refusal> {
    let (page, sidecar) = sheet(paper, square_mm, marker_mm, dictionary)?;
    Ok(Sheet {
        png: png(&page),
        pdf: pdf(&page),
        sidecar,
    })
}

const DPI: f64 = 300.0;
const PPM: f64 = DPI / 25.4;

fn px(mm: f64) -> usize {
    (mm * PPM).round_ties_even() as usize
}

fn mm(pixels: f64) -> f64 {
    py::round(pixels / PPM, 3)
}

// Half-open pixel-edge rectangle. Bounds are established by layout validation, never clamped.
fn rect(page: &mut Gray, x: usize, y: usize, w: usize, h: usize, value: u8) {
    for row in y..y + h {
        page.data[row * page.w + x..row * page.w + x + w].fill(value);
    }
}

fn outline(page: &mut Gray, x: usize, y: usize, w: usize, h: usize, thickness: usize) {
    rect(page, x, y, w, thickness, 0);
    rect(page, x, y + h - thickness, w, thickness, 0);
    rect(page, x, y, thickness, h, 0);
    rect(page, x + w - thickness, y, thickness, h, 0);
}

fn label(page: &mut Gray, text: &str, x: f64, y: f64, cap: f64, max_width: f64) {
    // The spec does not pin the font. Reuse the export stroke font with a compact advance;
    // fitting the text never changes any metric marks or the board geometry.
    let horizontal = (max_width / (text.len() as f64 * cap)).min(0.55);
    for stroke in stroke_text(text, 0.0, cap, cap, Anchor::Start, true) {
        for pair in stroke.windows(2) {
            let point = |p: ftx_core::Vec2| {
                (
                    ((x + p.x * horizontal) * PPM).round_ties_even() as i64,
                    ((y + p.y) * PPM).round_ties_even() as i64,
                )
            };
            draw::line(page, point(pair[0]), point(pair[1]), 0, px(0.17) as i64);
        }
    }
}

fn board(squares_y: usize, square: usize, marker: usize, bits: usize, codes: &[u64]) -> Gray {
    // Exactly 12 px/mm before nearest-neighbour page resampling, as in spec A.9.
    let side = 12 * square;
    let marker_pixels = 12 * marker;
    let cells = bits + 2;
    let inset = 6 * (square - marker);
    let mut board = Gray::filled(8 * side, squares_y * side, 255);
    let mut id = 0;
    for row in 0..squares_y {
        for col in 0..8 {
            let (x, y) = (col * side, row * side);
            if (row + col) % 2 == 0 {
                rect(&mut board, x, y, side, side, 0);
                continue;
            }
            rect(
                &mut board,
                x + inset,
                y + inset,
                marker_pixels,
                marker_pixels,
                0,
            );
            for bit in 0..bits * bits {
                if codes[id] & (1 << (bits * bits - 1 - bit)) != 0 {
                    // Inverse of OpenCV's floor(destination*cells/marker_pixels) lookup.
                    let col = bit % bits + 1;
                    let row = bit / bits + 1;
                    let x0 = (col * marker_pixels).div_ceil(cells);
                    let x1 = ((col + 1) * marker_pixels).div_ceil(cells);
                    let y0 = (row * marker_pixels).div_ceil(cells);
                    let y1 = ((row + 1) * marker_pixels).div_ceil(cells);
                    rect(
                        &mut board,
                        x + inset + x0,
                        y + inset + y0,
                        x1 - x0,
                        y1 - y0,
                        255,
                    );
                }
            }
            id += 1;
        }
    }
    board
}

fn nearest_indices(source: usize, destination: usize) -> Vec<usize> {
    // Pillow's nearest-neighbour affine sampler advances a floating pixel center. Using
    // rational arithmetic changes tie pixels (e.g. 2112 -> 2079 at x=31) and marker edges.
    let scale = source as f64 / destination as f64;
    let mut center = 0.5 * scale;
    (0..destination)
        .map(|_| {
            let index = center as usize;
            center += scale;
            index
        })
        .collect()
}

fn bar(page: &mut Gray, name: &str, x: f64, y: f64, length: f64, segment: f64) -> Value {
    let vertical = name.starts_with('y');
    let (x, y, length_px, thickness) = (px(x), px(y), px(length), px(3.5));
    let count = (length / segment).round_ties_even() as usize;
    for i in (0..count).step_by(2) {
        let begin = px(i as f64 * segment);
        let end = px((i + 1) as f64 * segment);
        if vertical {
            rect(page, x, y + begin, thickness, end - begin, 0);
        } else {
            rect(page, x + begin, y, end - begin, thickness, 0);
        }
    }
    let (w, h) = if vertical {
        (thickness, length_px)
    } else {
        (length_px, thickness)
    };
    outline(page, x, y, w, h, px(0.25));
    let tick = px(2.0);
    // Ticks extend transversely; their outer edges retain the exact measurement endpoints.
    if vertical {
        rect(page, x - tick, y, w + 2 * tick, px(0.25), 0);
        rect(page, x - tick, y + h - px(0.25), w + 2 * tick, px(0.25), 0);
    } else {
        rect(page, x, y - tick, px(0.25), h + 2 * tick, 0);
        rect(page, x + w - px(0.25), y - tick, px(0.25), h + 2 * tick, 0);
    }
    let (start, end) = if vertical {
        (
            [mm(x as f64 + thickness as f64 / 2.0), mm(y as f64)],
            [
                mm(x as f64 + thickness as f64 / 2.0),
                mm((y + length_px) as f64),
            ],
        )
    } else {
        (
            [mm(x as f64), mm(y as f64 + thickness as f64 / 2.0)],
            [
                mm((x + length_px) as f64),
                mm(y as f64 + thickness as f64 / 2.0),
            ],
        )
    };
    json!({"name":name,"axis":if vertical {"y"} else {"x"},
        "length_mm":mm(length_px as f64),"nominal_mm":length,
        "start_mm":start,"end_mm":end,"thickness_mm":3.5,"segment_mm":segment})
}

fn sheet(
    paper: &str,
    square: f64,
    marker: f64,
    dictionary: &str,
) -> Result<(Gray, Value), Refusal> {
    let (width, height, sy) = match paper {
        "letter" => (215.9, 279.4, 6),
        "a4" => (210.0, 297.0, 7),
        _ => return Err(bad_input("paper must be letter or a4")),
    };
    if !square.is_finite()
        || !marker.is_finite()
        || square.fract() != 0.0
        || marker.fract() != 0.0
        || marker <= 0.0
        || marker >= square
    {
        return Err(bad_input(
            "square_mm and marker_mm must be integers with marker < square",
        ));
    }
    let (bits, codes) = dictionaries::lookup(dictionary).ok_or_else(|| {
        bad_input("unknown predefined calibration dictionary").repair(format!(
            "supported dictionaries: {}",
            dictionaries().join(", ")
        ))
    })?;
    let (bw, bh) = (8.0 * square, sy as f64 * square);
    let (x0, y0) = ((width - bw - 17.0) / 2.0, 8.0);
    let band_y = y0 + bh + 26.0;
    if band_y + 100.0 > height - 6.0 || bw + 17.0 > width - 10.0 {
        return Err(bad_input(
            "board does not fit on this paper; use a smaller square_mm",
        ));
    }
    if bh < 108.0 {
        return Err(bad_input(
            "vertical scale bars overlap the lower marks; use a larger square_mm",
        ));
    }
    // Both the full 100 mm test square and the 1 inch square must actually fit.
    // The reference does not guard this for very small boards; refuse instead of clipping.
    if x0 + 106.0 + 25.4 > width - 4.0 {
        return Err(bad_input(
            "test squares do not fit on this paper; use a larger square_mm",
        ));
    }
    if band_y + 31.0 + 6.2 + 6.0 * 5.0 + 2.0 * 4.0 > height - 4.0 {
        return Err(bad_input("sheet text overflows the page"));
    }
    let mut page = Gray::filled(px(width), px(height), 255);
    let board = board(sy, square as usize, marker as usize, bits, codes);
    let (ox, oy, rw, rh) = (px(x0), px(y0), px(bw), px(bh));
    let xs = nearest_indices(board.w, rw);
    let ys = nearest_indices(board.h, rh);
    for (y, &source_y) in ys.iter().enumerate() {
        for (x, &source_x) in xs.iter().enumerate() {
            page.data[(oy + y) * page.w + ox + x] = board.at(source_x, source_y);
        }
    }
    let bars = vec![
        bar(&mut page, "y_100mm", x0 + bw + 4.0, y0 + 4.0, 100.0, 10.0),
        bar(&mut page, "y_4in", x0 + bw + 13.5, y0 + 4.0, 101.6, 12.7),
        bar(&mut page, "x_100mm", x0, y0 + bh + 6.0, 100.0, 10.0),
        bar(&mut page, "x_4in", x0, y0 + bh + 15.0, 101.6, 12.7),
    ];
    label(&mut page, "100 mm", x0 + bw + 2.0, y0 + 108.0, 2.4, 8.0);
    label(&mut page, "4 in", x0 + bw + 12.0, y0 + 108.0, 2.4, 6.5);
    label(
        &mut page,
        "100 mm  (10 mm segments)",
        x0 + 106.0,
        y0 + bh + 6.0,
        2.4,
        width - x0 - 110.0,
    );
    label(
        &mut page,
        "4 in  (1/2 in segments)",
        x0 + 106.0,
        y0 + bh + 15.0,
        2.4,
        width - x0 - 110.0,
    );
    let mut squares = Vec::new();
    for (name, nominal, x) in [("square_10cm", 100.0, x0), ("square_1in", 25.4, x0 + 106.0)] {
        let (x, y, side) = (px(x), px(band_y), px(nominal));
        outline(&mut page, x, y, side, side, px(0.5));
        squares.push(
            json!({"name":name,"size_mm":mm(side as f64),"nominal_mm":nominal,
            "origin_mm":[mm(x as f64),mm(y as f64)]}),
        );
    }
    label(&mut page, "10 cm square", x0 + 3.0, band_y + 4.0, 4.2, 92.0);
    label(
        &mut page,
        "measure outer edge to outer edge",
        x0 + 3.0,
        band_y + 12.0,
        3.2,
        92.0,
    );
    label(
        &mut page,
        "1 in square",
        x0 + 107.5,
        band_y + 4.0,
        3.2,
        22.0,
    );
    label(
        &mut page,
        "(outer edges)",
        x0 + 107.5,
        band_y + 11.0,
        2.4,
        22.0,
    );
    let tx = x0 + 106.0;
    let available = width - tx - 4.0;
    let mut ty = band_y + 31.0;
    label(
        &mut page,
        "PRINT AT 100% / ACTUAL SIZE.",
        tx,
        ty,
        4.2,
        available,
    );
    ty += 6.2;
    for text in [
        "Turn OFF 'Fit to page' / 'Scale to fit'.",
        "Check both bars with a ruler before use:",
        "100 mm and 4 in, along X and along Y.",
        "Tape the sheet flat to stiff card.",
        "Photos: sheet beside the child, facing",
        "the camera, centre over the floor line.",
    ] {
        label(&mut page, text, tx, ty, 3.2, available);
        ty += 5.0;
    }
    for text in [
        format!("ChArUco {dictionary}, 8x{sy} squares, square {square} mm, marker {marker} mm"),
        format!(
            "{} {width} x {height} mm, 300 dpi - fabric-pattern calib sheet v1",
            paper.to_uppercase()
        ),
    ] {
        label(&mut page, &text, tx, ty, 2.4, available);
        ty += 4.0;
    }
    let sidecar = json!({
        "schema":"fabric-calib-sheet/1", "paper":paper, "paper_mm":[width,height],
        "dpi":300, "image_px":[page.w,page.h],
        "board":{"type":"charuco","dictionary":dictionary,"squares_x":8,"squares_y":sy,
            "square_mm":square,"marker_mm":marker,"legacy_pattern":false,
            "origin_mm":[mm(ox as f64),mm(oy as f64)],"origin_px":[ox,oy],
            "size_mm":[bw,bh],"size_px":[rw,rh],"png_scale":py::round(rw as f64 / PPM / bw,6),
            "frame":"board mm: x right, y down from the board's top-left outer corner; chessboard corner id i at ((i % (sx-1) + 1)·square, (i // (sx-1) + 1)·square)"},
        "bars":bars,"test_squares":squares,
        "files":{"png":format!("calib-sheet-{paper}.png"),"pdf":format!("calib-sheet-{paper}.pdf")},
        "pdf_lossless":true,
        "print_instructions":"Print at 100% / Actual size. Check both bars with a ruler before use.",
        "pixel_convention":"page mm = px / (dpi/25.4) at pixel edges; OpenCV pixel centres sit at (px + 0.5)"
    });
    Ok((page, sidecar))
}

fn png(page: &Gray) -> Vec<u8> {
    // Insert physical pixel density after IHDR. The existing PNG encoder supplies lossless
    // grayscale IDAT/CRC; do not infer 72/96 DPI from the viewer's default.
    let encoded = encode_png_gray(page);
    let mut chunk = Vec::from(9_u32.to_be_bytes());
    chunk.extend(b"pHYs");
    let ppm = (DPI / 0.0254).round_ties_even() as u32;
    chunk.extend(ppm.to_be_bytes());
    chunk.extend(ppm.to_be_bytes());
    chunk.push(1);
    let mut crc = u32::MAX;
    for byte in &chunk[4..] {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0_u32.wrapping_sub(crc & 1));
        }
    }
    chunk.extend((!crc).to_be_bytes());
    [encoded[..33].to_vec(), chunk, encoded[33..].to_vec()].concat()
}

fn pdf(page: &Gray) -> Vec<u8> {
    // A bounded, single-image PDF; no JPEG or resampling. One bit per pixel, MSB first,
    // white padding at each row end. Physical size follows the spec's 300-DPI pixel grid.
    let stride = page.w.div_ceil(8);
    let mut bits = vec![255_u8; stride * page.h];
    for y in 0..page.h {
        for x in 0..page.w {
            if page.at(x, y) == 0 {
                bits[y * stride + x / 8] &= !(1 << (7 - x % 8));
            }
        }
    }
    let (w, h) = (page.w as f64 * 72.0 / DPI, page.h as f64 * 72.0 / DPI);
    let content = format!("q\n{w:.6} 0 0 {h:.6} 0 0 cm\n/Im0 Do\nQ\n");
    let objects = [
        b"<< /Type /Catalog /Pages 2 0 R /ViewerPreferences << /PrintScaling /None >> >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w:.6} {h:.6}] /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>").into_bytes(),
        format!("<< /Length {} >>\nstream\n{content}endstream", content.len()).into_bytes(),
        [format!("<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceGray /BitsPerComponent 1 /Interpolate false /Length {} >>\nstream\n",page.w,page.h,bits.len()).into_bytes(), bits, b"\nendstream".to_vec()].concat(),
    ];
    let mut out = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend(object);
        out.extend(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend(b"xref\n0 6\n0000000000 65535 f \n");
    for offset in offsets {
        out.extend(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend(
        format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_sampling_keeps_the_reference_tie_pixels() {
        let x = nearest_indices(2112, 2079);
        for (pixel, source) in [(0, 0), (31, 31), (94, 95), (1039, 1055), (2078, 2111)] {
            assert_eq!(x[pixel], source);
        }
        assert_eq!(nearest_indices(1584, 1559)[779], 791);
        assert_eq!(nearest_indices(1848, 1819)[909], 923);
    }

    #[test]
    fn letter_and_a4_match_spec_d7_metric_coordinates() {
        for (paper, origin, image, size, square_origin, y100, yinch, x100, xinch) in [
            (
                "letter",
                [135, 94],
                [2550, 3300],
                [2079, 1559],
                [11.43, 166.031],
                193.167,
                202.65,
                147.701,
                156.76,
            ),
            (
                "a4",
                [100, 94],
                [2480, 3508],
                [2079, 1819],
                [8.467, 187.96],
                190.204,
                199.771,
                169.714,
                178.774,
            ),
        ] {
            let (p, s) = sheet(paper, 22.0, 16.0, DICTIONARY).unwrap();
            assert_eq!(s["board"]["origin_px"], json!(origin));
            assert_eq!(s["image_px"], json!(image));
            assert_eq!(s["board"]["size_px"], json!(size));
            assert_eq!(s["board"]["png_scale"], json!(1.000125));
            assert_eq!(s["test_squares"][0]["origin_mm"], json!(square_origin));
            assert_eq!(s["test_squares"][0]["size_mm"], json!(99.991));
            assert_eq!(s["test_squares"][1]["size_mm"], json!(25.4));
            assert_eq!(s["bars"][0]["start_mm"][0], json!(y100));
            assert_eq!(s["bars"][1]["start_mm"][0], json!(yinch));
            assert_eq!(s["bars"][2]["start_mm"][1], json!(x100));
            assert_eq!(s["bars"][3]["start_mm"][1], json!(xinch));
            assert!(p.data.iter().all(|p| *p == 0 || *p == 255));
        }
    }

    #[test]
    fn dictionary_and_nonlegacy_checkerboard_are_pinned() {
        let bytes: Vec<_> = MARKERS
            .iter()
            .flat_map(|m| (*m as u16).to_be_bytes())
            .collect();
        assert_eq!(
            ftx_core::receipt::sha256_hex(&bytes),
            "372c6d504da619d970a5153562953a92c5a082d51cd48b5ca9ffe87bb78357b7"
        );
        let b = board(7, 22, 16, 4, &MARKERS);
        let mut id = 0;
        for row in 0..7 {
            for col in 0..8 {
                let (x, y) = (col * 264, row * 264);
                if (row + col) % 2 == 0 {
                    assert_eq!(b.at(x + 132, y + 132), 0);
                } else {
                    assert_eq!(b.at(x + 1, y + 1), 255);
                    assert_eq!(b.at(x + 36 + 16, y + 36 + 16), 0);
                    let mut bits = 0_u64;
                    for bit in 0..16 {
                        bits = (bits << 1)
                            | u64::from(
                                b.at(
                                    x + 36 + (bit % 4 + 1) * 32 + 16,
                                    y + 36 + (bit / 4 + 1) * 32 + 16,
                                ) == 255,
                            );
                    }
                    assert_eq!(bits, MARKERS[id]);
                    id += 1;
                }
            }
        }
        assert_eq!(id, 28);
    }

    #[test]
    fn png_is_lossless_with_explicit_300_dpi() {
        let (p, _) = sheet("letter", 22.0, 16.0, DICTIONARY).unwrap();
        let bytes = png(&p);
        assert_eq!(&bytes[37..41], b"pHYs");
        assert_eq!(&bytes[41..45], &11811_u32.to_be_bytes());
        assert_eq!(&bytes[45..49], &11811_u32.to_be_bytes());
        assert_eq!(bytes[49], 1);
        let decoded = crate::raster::image::load_png(&bytes).unwrap();
        assert_eq!((decoded.w, decoded.h), (p.w, p.h));
        for y in 0..p.h {
            for x in 0..p.w {
                assert_eq!(decoded.at(x, y), [p.at(x, y); 3]);
            }
        }
    }

    #[test]
    fn monochrome_pdf_has_exact_physical_size_and_valid_offsets() {
        for (paper, box_text) in [
            ("letter", "612.000000 792.000000"),
            ("a4", "595.200000 841.920000"),
        ] {
            let (p, _) = sheet(paper, 22.0, 16.0, DICTIONARY).unwrap();
            let bytes = pdf(&p);
            let text = String::from_utf8_lossy(&bytes);
            assert!(text.contains(&format!("/MediaBox [0 0 {box_text}]")));
            assert!(text.contains("/BitsPerComponent 1 /Interpolate false"));
            assert!(!text.contains("/DCTDecode"));
            let start: usize = text
                .rsplit_once("startxref\n")
                .unwrap()
                .1
                .lines()
                .next()
                .unwrap()
                .parse()
                .unwrap();
            assert_eq!(&bytes[start..start + 4], b"xref");
            let xref = std::str::from_utf8(&bytes[start..]).unwrap();
            for (i, entry) in xref.lines().skip(3).take(5).enumerate() {
                let offset: usize = entry[..10].parse().unwrap();
                assert!(bytes[offset..].starts_with(format!("{} 0 obj", i + 1).as_bytes()));
            }
        }
    }

    #[test]
    fn invalid_layouts_and_unknown_dictionaries_are_refused() {
        for (paper, square, marker, dict) in [
            ("legal", 22.0, 16.0, DICTIONARY),
            ("letter", 22.5, 16.0, DICTIONARY),
            ("letter", 22.0, 22.0, DICTIONARY),
            ("letter", 22.0, 0.0, DICTIONARY),
            ("letter", 1e100, 16.0, DICTIONARY),
            ("letter", f64::NAN, 16.0, DICTIONARY),
            ("letter", 24.0, 16.0, DICTIONARY),
            ("letter", 17.0, 12.0, DICTIONARY),
            ("letter", 2.0, 1.0, DICTIONARY),
            ("letter", 22.0, 16.0, "DICT_5X5_51"),
        ] {
            assert!(sheet(paper, square, marker, dict).is_err());
        }
    }

    #[test]
    fn named_dictionaries_and_all_board_pixels_match_the_independent_provider() {
        let oracle: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/oracle/measure/calibration_sheets.json"
        ))
        .unwrap();
        let names: Vec<_> = oracle["dictionaries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, dictionaries());
        assert_eq!(names.len(), 27);
        for entry in oracle["dictionaries"].as_array().unwrap() {
            let (bits, codes) = dictionaries::lookup(entry["name"].as_str().unwrap()).unwrap();
            assert_eq!(bits as u64, entry["marker_bits"].as_u64().unwrap());
            let expected: Vec<u64> = entry["codes_hex"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| u64::from_str_radix(s.as_str().unwrap(), 16).unwrap())
                .collect();
            assert_eq!(codes, expected);
        }
        assert_eq!(oracle["cases"].as_array().unwrap().len(), 108);
        for case in oracle["cases"].as_array().unwrap() {
            let name = case["dictionary"].as_str().unwrap();
            let (bits, codes) = dictionaries::lookup(name).unwrap();
            let rows = if case["paper"] == "a4" { 7 } else { 6 };
            let image = board(
                rows,
                case["square_mm"].as_u64().unwrap() as usize,
                case["marker_mm"].as_u64().unwrap() as usize,
                bits,
                codes,
            );
            assert_eq!(
                [image.w, image.h],
                [
                    case["width"].as_u64().unwrap() as usize,
                    case["height"].as_u64().unwrap() as usize
                ]
            );
            assert_eq!(
                ftx_core::receipt::sha256_hex(&image.data),
                case["board_pixels_sha256"],
                "{case}"
            );
        }
    }
}

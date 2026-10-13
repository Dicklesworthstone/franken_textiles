//! Text and number helpers of every export (`docs/spec/03` §2), byte-exact with the reference.

use std::sync::OnceLock;

use ftx_core::{Vec2, py};

/// `ascii_text` of each non-ASCII code point: NFKD, `×` → `x`, `— – ·` → `-`, code points 32..126 kept
/// (Unicode 15.0.0, the reference's Python 3.12; `tools/oracle/gen_ascii_fold.py`).
const ASCII_FOLD: &str = include_str!("ascii_fold.tsv");

fn ascii_fold() -> &'static [(u32, &'static str)] {
    static TABLE: OnceLock<Vec<(u32, &'static str)>> = OnceLock::new();
    TABLE.get_or_init(|| {
        ASCII_FOLD
            .lines()
            .filter(|l| !l.starts_with('#'))
            .filter_map(|l| {
                let (cp, ascii) = l.split_once('\t')?;
                Some((u32::from_str_radix(cp, 16).ok()?, ascii))
            })
            .collect()
    })
}

/// `fnum(v, nd)`: fixed-point with `nd` decimals (ties to even on the exact binary value), trailing zeros
/// and then a trailing `.` stripped, `-0` → `0`; never an exponent.
#[must_use]
pub fn fnum(v: f64, nd: usize) -> String {
    let s = py::fixed(v, nd);
    let s = if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.')
    } else {
        s.as_str()
    };
    if matches!(s, "-0" | "" | "-") {
        "0".to_string()
    } else {
        s.to_string()
    }
}

/// `ascii_text(s)`: NFKD, `×` → `x`, `— – ·` → `-`, only code points 32..126 kept.
#[must_use]
pub fn ascii_text(s: &str) -> String {
    let table = ascii_fold();
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let cp = u32::from(c);
        if (32..127).contains(&cp) {
            out.push(c);
        } else if cp >= 0x80
            && let Ok(i) = table.binary_search_by_key(&cp, |&(k, _)| k)
        {
            out.push_str(table[i].1);
        }
    }
    out
}

/// `safe_name(s, n)`: [`ascii_text`], every run outside `[A-Za-z0-9_-]` replaced by `_`, `_` stripped
/// from both ends, `X` if empty, then cut to `n` characters.
#[must_use]
pub fn safe_name(s: &str, n: usize) -> String {
    let mut out = String::new();
    let mut run = false;
    for c in ascii_text(s).chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            out.push(c);
            run = false;
        } else if !run {
            out.push('_');
            run = true;
        }
    }
    let out = out.trim_matches('_');
    let out = if out.is_empty() { "X" } else { out };
    out.chars().take(n).collect()
}

/// Helvetica advance widths (1/1000 em) of WinAnsi 32..126.
const HELVETICA: [u32; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556,
    556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667,
    611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667,
    667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500,
    222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];

/// Width of `s` (mm) in Helvetica at `size` mm, × 1.06 when bold; characters outside 32..126 count 556.
#[must_use]
pub fn text_width(s: &str, size: f64, bold: bool) -> f64 {
    let units: u32 = s
        .chars()
        .map(|c| {
            let cp = u32::from(c);
            if (32..127).contains(&cp) {
                HELVETICA[(cp - 32) as usize]
            } else {
                556
            }
        })
        .sum();
    let w = f64::from(units) / 1000.0 * size;
    if bold { w * 1.06 } else { w }
}

/// Python's `str.isspace` (Unicode White_Space plus the information separators U+001C..U+001F).
fn py_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// Greedy word wrap at `width` mm: a word starts a new line when the line with it would be wider and the
/// line is not empty.
#[must_use]
pub fn wrap(s: &str, size: f64, width: f64) -> Vec<String> {
    let mut out = Vec::new();
    let mut line = String::new();
    for word in s.split(py_space).filter(|w| !w.is_empty()) {
        let cand = if line.is_empty() {
            word.to_string()
        } else {
            format!("{line} {word}")
        };
        if !line.is_empty() && text_width(&cand, size, false) > width {
            out.push(std::mem::replace(&mut line, word.to_string()));
        } else {
            line = cand;
        }
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

/// Where stroke text is anchored on `x`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    /// Text starts at `x`.
    Start,
    /// Text is centred on `x`.
    Middle,
    /// Text ends at `x`.
    End,
}

/// The single-stroke font (grid 4 wide × 6 high, y up): strokes separated by `|`.
const FONT: &[(char, &str)] = &[
    ('A', "0,0 0,4 2,6 4,4 4,0|0,3 4,3"),
    ('B', "0,0 0,6 3,6 4,5 4,4 3,3 0,3|3,3 4,2 4,1 3,0 0,0"),
    ('C', "4,5 3,6 1,6 0,5 0,1 1,0 3,0 4,1"),
    ('D', "0,0 0,6 2,6 4,4 4,2 2,0 0,0"),
    ('E', "4,6 0,6 0,0 4,0|0,3 3,3"),
    ('F', "4,6 0,6 0,0|0,3 3,3"),
    ('G', "4,5 3,6 1,6 0,5 0,1 1,0 3,0 4,1 4,3 2,3"),
    ('H', "0,0 0,6|4,0 4,6|0,3 4,3"),
    ('I', "1,6 3,6|2,6 2,0|1,0 3,0"),
    ('J', "4,6 4,1 3,0 1,0 0,1"),
    ('K', "0,0 0,6|4,6 0,2|1,3 4,0"),
    ('L', "0,6 0,0 4,0"),
    ('M', "0,0 0,6 2,3 4,6 4,0"),
    ('N', "0,0 0,6 4,0 4,6"),
    ('O', "1,0 0,1 0,5 1,6 3,6 4,5 4,1 3,0 1,0"),
    ('P', "0,0 0,6 3,6 4,5 4,4 3,3 0,3"),
    ('Q', "1,0 0,1 0,5 1,6 3,6 4,5 4,1 3,0 1,0|2,2 4,0"),
    ('R', "0,0 0,6 3,6 4,5 4,4 3,3 0,3|2,3 4,0"),
    ('S', "4,5 3,6 1,6 0,5 0,4 1,3 3,3 4,2 4,1 3,0 1,0 0,1"),
    ('T', "0,6 4,6|2,6 2,0"),
    ('U', "0,6 0,1 1,0 3,0 4,1 4,6"),
    ('V', "0,6 2,0 4,6"),
    ('W', "0,6 1,0 2,3 3,0 4,6"),
    ('X', "0,0 4,6|0,6 4,0"),
    ('Y', "0,6 2,3 4,6|2,3 2,0"),
    ('Z', "0,6 4,6 0,0 4,0"),
    ('0', "1,0 0,1 0,5 1,6 3,6 4,5 4,1 3,0 1,0|0,1 4,5"),
    ('1', "1,5 2,6 2,0|1,0 3,0"),
    ('2', "0,5 1,6 3,6 4,5 4,4 0,0 4,0"),
    ('3', "0,5 1,6 3,6 4,5 4,4 3,3 1,3|3,3 4,2 4,1 3,0 1,0 0,1"),
    ('4', "3,0 3,6 0,2 4,2"),
    ('5', "4,6 0,6 0,3 3,3 4,2 4,1 3,0 0,0"),
    ('6', "3,6 1,6 0,5 0,1 1,0 3,0 4,1 4,2 3,3 0,3"),
    ('7', "0,6 4,6 1,0"),
    (
        '8',
        "1,3 0,4 0,5 1,6 3,6 4,5 4,4 3,3 1,3 0,2 0,1 1,0 3,0 4,1 4,2 3,3",
    ),
    ('9', "4,3 1,3 0,4 0,5 1,6 3,6 4,5 4,1 3,0 1,0"),
    ('-', "1,3 3,3"),
    ('.', "2,0 2,0.5"),
    (',', "2,0.5 1.5,-1"),
    (':', "2,1 2,1.5|2,4 2,4.5"),
    ('/', "0,0 4,6"),
    ('(', "3,6 2,4.5 2,1.5 3,0"),
    (')', "1,6 2,4.5 2,1.5 1,0"),
    ('+', "2,1 2,5|0,3 4,3"),
    ('=', "0,2 4,2|0,4 4,4"),
    ('_', "0,-0.5 4,-0.5"),
    ('\'', "2,6 2,5"),
    ('#', "1,0 1,6|3,0 3,6|0,2 4,2|0,4 4,4"),
    ('%', "0,0 4,6|0,6 1,6 1,5 0,5 0,6|3,1 4,1 4,0 3,0 3,1"),
    ('?', "0,5 1,6 3,6 4,5 4,4 2,3 2,2|2,0 2,0.5"),
    ('!', "2,6 2,2|2,0 2,0.5"),
    ('<', "4,6 0,3 4,0"),
    ('>', "0,6 4,3 0,0"),
    ('*', "2,1 2,5|0.5,2 3.5,4|0.5,4 3.5,2"),
    ('|', "2,-1 2,7"),
    ('&', "2,1 2,5|0,3 4,3"),
];

fn glyph(c: char) -> &'static str {
    FONT.iter()
        .find(|(k, _)| *k == c)
        .or_else(|| FONT.iter().find(|(k, _)| *k == '?'))
        .map_or("", |(_, g)| g)
}

/// Polylines of `s` in the single-stroke font with cap height `cap` mm, baseline at (`x`, `y`): upper-case
/// ASCII, 6 units of advance per character, spaces skipped but advanced, unknown characters drawn as `?`.
#[must_use]
pub fn stroke_text(
    s: &str,
    x: f64,
    y: f64,
    cap: f64,
    anchor: Anchor,
    y_down: bool,
) -> Vec<Vec<Vec2>> {
    let u = cap / 6.0;
    let adv = 6.0 * u;
    let s = ascii_text(s).to_ascii_uppercase();
    let total = s.len() as f64 * adv - 2.0 * u;
    let x0 = x - match anchor {
        Anchor::Middle => total / 2.0,
        Anchor::End => total,
        Anchor::Start => 0.0,
    };
    let sy = if y_down { -1.0 } else { 1.0 };
    let mut out = Vec::new();
    for (i, ch) in s.chars().enumerate() {
        if ch == ' ' {
            continue;
        }
        for stroke in glyph(ch).split('|') {
            out.push(
                stroke
                    .split_whitespace()
                    .filter_map(|pt| {
                        let (gx, gy) = pt.split_once(',')?;
                        let (gx, gy): (f64, f64) = (gx.parse().ok()?, gy.parse().ok()?);
                        Some(Vec2::new(x0 + i as f64 * adv + gx * u, y + sy * gy * u))
                    })
                    .collect(),
            );
        }
    }
    out
}

/// Tile column letters, bijective base 26: 0 → `A`, 25 → `Z`, 26 → `AA`.
#[must_use]
pub fn column_letters(c: usize) -> String {
    let mut s = Vec::new();
    let mut c = c + 1;
    while c > 0 {
        let r = (c - 1) % 26;
        c = (c - 1) / 26;
        s.push(b'A' + r as u8);
    }
    s.reverse();
    String::from_utf8(s).unwrap_or_default()
}

/// Consecutive runs as `a-b`, joined by `, `: `[2, 3, 4, 6, 8, 9]` → `2-4, 6, 8-9`.
#[must_use]
pub fn page_list(nums: &[i64]) -> String {
    let mut out = Vec::new();
    let mut i = 0;
    while i < nums.len() {
        let mut j = i;
        while j + 1 < nums.len() && nums[j + 1] == nums[j] + 1 {
            j += 1;
        }
        out.push(if i == j {
            nums[i].to_string()
        } else {
            format!("{}-{}", nums[i], nums[j])
        });
        i = j + 1;
    }
    out.join(", ")
}

/// A PDF string literal of [`ascii_text`]`(s)` with `\`, `(` and `)` escaped.
#[must_use]
pub fn pdf_string(s: &str) -> String {
    let t = ascii_text(s)
        .replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)");
    format!("({t})")
}

/// XML attribute and text escape: `&`, `<`, `>` and `"`.
#[must_use]
pub fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_goldens() {
        assert_eq!(fnum(12.0, 3), "12");
        assert_eq!(fnum(-0.0004, 3), "0");
        assert_eq!(fnum(1e-9, 3), "0");
        assert_eq!(fnum(0.1 + 0.2, 3), "0.3");
        assert_eq!(fnum(0.125, 2), "0.12");
        assert_eq!(fnum(2.5, 0), "2");
        assert_eq!(
            ascii_text("Café × — – · ñ ø ß ﬁ ½"),
            "Cafe x - - - n   fi 12"
        );
        assert_eq!(
            safe_name("Front bodice (left) ×2", 20),
            "Front_bodice_left_x2"
        );
        assert_eq!(safe_name("___", 20), "X");
        assert_eq!(safe_name("920 mm stature", 20), "920_mm_stature");
        assert_eq!(column_letters(701), "ZZ");
        assert_eq!(column_letters(702), "AAA");
        assert_eq!(page_list(&[2, 3, 4, 6, 8, 9]), "2-4, 6, 8-9");
        assert!((text_width("PLACE ON FOLD", 4.0, true) - 33.92848).abs() < 1e-12);
        assert!((text_width("Front bodice", 6.0, true) - 35.34888).abs() < 1e-12);
    }
}

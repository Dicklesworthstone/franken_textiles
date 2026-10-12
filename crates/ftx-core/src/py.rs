//! Python number and string formatting the reference's messages and files use (`docs/spec/01` §13).
//!
//! Rust's fixed-precision float formatting already rounds the exact binary value half-to-even and
//! keeps the sign of a negative value that rounds to zero, exactly like Python's `format(x, ".Nf")`
//! and `round(x, N)`; these helpers add Python's spellings of NaN and infinity, `repr(float)` and
//! `repr(str)`.

/// Python `round(x, ndigits)` for `ndigits ≥ 0`: correctly rounded on the exact binary value, ties
/// to even (`round(2.675, 2) = 2.67`, `round(-0.0004, 3) = -0.0`).
#[must_use]
pub fn round(x: f64, ndigits: usize) -> f64 {
    if !x.is_finite() {
        return x;
    }
    format!("{x:.ndigits$}").parse().unwrap_or(x)
}

/// Python `format(x, ".Nf")`: `fixed(-0.04, 1) = "-0.0"`, `fixed(2.5, 0) = "2"`.
#[must_use]
pub fn fixed(x: f64, prec: usize) -> String {
    if x.is_nan() {
        return "nan".to_string();
    }
    format!("{x:.prec$}")
}

/// Python `format(x, "+.Nf")`: `signed_fixed(1.25, 1) = "+1.2"`.
#[must_use]
pub fn signed_fixed(x: f64, prec: usize) -> String {
    if x.is_nan() {
        return "+nan".to_string();
    }
    format!("{x:+.prec$}")
}

/// Python `repr(float)` (also `str(float)` and how `json.dumps` writes floats): the shortest digits
/// that round-trip, positional for 1e-4 ≤ |x| < 1e16 with at least one fractional digit (`12.0`),
/// scientific otherwise with a signed, at least two-digit exponent (`1e-05`, `1e+16`).
#[must_use]
pub fn repr_f64(x: f64) -> String {
    if x.is_nan() {
        return "nan".to_string();
    }
    if x.is_infinite() {
        return if x > 0.0 { "inf" } else { "-inf" }.to_string();
    }
    if x == 0.0 {
        return if x.is_sign_negative() { "-0.0" } else { "0.0" }.to_string();
    }
    // `{:e}` gives the shortest round-trip digits as d.ddde±X.
    let sci = format!("{:e}", x.abs());
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let sign = if x < 0.0 { "-" } else { "" };
    if (-4..16).contains(&exp) {
        let n = digits.len();
        if exp >= 0 {
            let int_len = exp as usize + 1;
            if n <= int_len {
                format!("{sign}{digits}{}.0", "0".repeat(int_len - n))
            } else {
                format!("{sign}{}.{}", &digits[..int_len], &digits[int_len..])
            }
        } else {
            format!("{sign}0.{}{digits}", "0".repeat((-exp - 1) as usize))
        }
    } else {
        let frac = if digits.len() > 1 {
            format!(".{}", &digits[1..])
        } else {
            String::new()
        };
        let esign = if exp < 0 { '-' } else { '+' };
        format!(
            "{sign}{}{frac}e{esign}{:02}",
            &digits[..1],
            exp.unsigned_abs()
        )
    }
}

/// Python `repr(str)`: single quotes unless the text contains `'` and no `"`; backslash, the quote,
/// `\t`, `\n`, `\r` escaped; other non-printable characters as `\xNN`, `\uNNNN` or `\UNNNNNNNN`.
///
/// "Non-printable" covers control characters and the common separator, format and private-use
/// code points; unassigned code points (which Python also escapes) are printed as is, because that
/// needs the Unicode tables. Identifiers in pattern files are ASCII in practice.
#[must_use]
pub fn repr_str(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if is_unprintable(c) => {
                let v = u32::from(c);
                if v < 0x100 {
                    out.push_str(&format!("\\x{v:02x}"));
                } else if v < 0x1_0000 {
                    out.push_str(&format!("\\u{v:04x}"));
                } else {
                    out.push_str(&format!("\\U{v:08x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// Python `repr(list[str])`: `['a', 'b']`.
#[must_use]
pub fn repr_str_list<S: AsRef<str>>(items: &[S]) -> String {
    let parts: Vec<String> = items.iter().map(|s| repr_str(s.as_ref())).collect();
    format!("[{}]", parts.join(", "))
}

/// Python `str.isprintable() == false` for the code points we can classify without tables:
/// Cc, Zl, Zp, Zs other than the ASCII space, common Cf, and Co.
fn is_unprintable(c: char) -> bool {
    let v = u32::from(c);
    c.is_control()
        || matches!(
            v,
            0xA0 | 0xAD
                | 0x0600..=0x0605
                | 0x061C
                | 0x06DD
                | 0x070F
                | 0x1680
                | 0x180E
                | 0x2000..=0x200F
                | 0x2028..=0x202F
                | 0x205F..=0x2064
                | 0x2066..=0x206F
                | 0x3000
                | 0xFEFF
                | 0xFFF9..=0xFFFB
                | 0xE000..=0xF8FF
                | 0xF_0000..=0x10_FFFF
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_matches_python() {
        assert_eq!(round(0.125, 2), 0.12);
        assert_eq!(round(2.675, 2), 2.67);
        assert_eq!(round(1.0005, 3), 1.0);
        assert_eq!(round(-0.0004, 3).to_bits(), (-0.0f64).to_bits());
        assert_eq!(round(5e-05, 4), 0.0001);
        assert_eq!(round(0.375, 2), 0.38);
        assert_eq!(round(1.0005, 9), 1.0005);
        assert!(round(f64::NAN, 2).is_nan());
    }

    #[test]
    fn fixed_matches_python() {
        assert_eq!(format!("{:>6}", fixed(-0.04, 1)), "  -0.0");
        assert_eq!(signed_fixed(1.25, 1), "+1.2");
        assert_eq!(signed_fixed(-2.0, 1), "-2.0");
        assert_eq!(fixed(0.5, 0), "0");
        assert_eq!(fixed(2.5, 0), "2");
        assert_eq!(fixed(-0.4, 0), "-0");
        assert_eq!(fixed(f64::INFINITY, 1), "inf");
        assert_eq!(fixed(f64::NAN, 1), "nan");
    }

    #[test]
    fn repr_f64_matches_python() {
        for (x, s) in [
            (-87.6, "-87.6"),
            (12.0, "12.0"),
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (1e-05, "1e-05"),
            (1e16, "1e+16"),
            (0.0001, "0.0001"),
            (1234567890123456.0, "1234567890123456.0"),
            (123456789012345678.0, "1.2345678901234568e+17"),
            (1.5e-7, "1.5e-07"),
            (1e22, "1e+22"),
            (1e100, "1e+100"),
            (0.1, "0.1"),
            (0.3035143769968051, "0.3035143769968051"),
            (100.0, "100.0"),
            (-1e-300, "-1e-300"),
            (5e-324, "5e-324"),
            (f64::MAX, "1.7976931348623157e+308"),
        ] {
            assert_eq!(repr_f64(x), s, "{x:e}");
        }
        assert_eq!(repr_f64(f64::NEG_INFINITY), "-inf");
    }

    #[test]
    fn repr_str_matches_python() {
        assert_eq!(repr_str("missing"), "'missing'");
        assert_eq!(repr_str("it's"), "\"it's\"");
        assert_eq!(repr_str("a'b\"c"), "'a\\'b\"c'");
        assert_eq!(repr_str("tab\there\\"), "'tab\\there\\\\'");
        assert_eq!(repr_str("\u{7}\u{a0}é"), "'\\x07\\xa0é'");
        assert_eq!(repr_str("\u{2028}"), "'\\u2028'");
        assert_eq!(
            repr_str_list(&["bottom", "right", "top", "left"]),
            "['bottom', 'right', 'top', 'left']"
        );
        assert_eq!(repr_str_list::<&str>(&[]), "[]");
    }
}

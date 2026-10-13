//! `parse_length` (spec 04 §10.15): the first number in a text, an optional ` N/D` fraction and an optional
//! unit, in millimetres.
//!
//! The reference matches `(\d+(?:[.,]\d+)?)(?:\s+(\d+)/(\d+))?\s*(mm|cm|m\b|in\b|inch(?:es)?|"|”|″|'')?`
//! case-insensitively with `re.search`; this is that expression written out. Digits are ASCII here (Python's
//! `\d` also takes other Unicode decimal digits).

/// Python `\w` for the word boundaries: a Unicode letter or digit, or `_`.
fn is_word(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_alphanumeric() || c == '_')
}

fn digits(s: &[char], i: usize) -> usize {
    let mut j = i;
    while j < s.len() && s[j].is_ascii_digit() {
        j += 1;
    }
    j
}

/// The unit alternatives in the reference's order, at `i`: the matched length.
fn unit_at(s: &[char], i: usize) -> Option<usize> {
    let lower = |k: usize| s.get(i + k).map(|c| c.to_ascii_lowercase());
    let word_after = |k: usize| is_word(s.get(i + k).copied());
    match (lower(0), lower(1)) {
        (Some('m'), Some('m')) | (Some('c'), Some('m')) => return Some(2),
        _ => {}
    }
    if lower(0) == Some('m') && !word_after(1) {
        return Some(1);
    }
    if lower(0) == Some('i') && lower(1) == Some('n') {
        if !word_after(2) {
            return Some(2);
        }
        if lower(2) == Some('c') && lower(3) == Some('h') {
            return Some(if lower(4) == Some('e') && lower(5) == Some('s') {
                6
            } else {
                4
            });
        }
    }
    match s.get(i) {
        Some('"' | '\u{201d}' | '\u{2033}') => Some(1),
        Some('\'') if s.get(i + 1) == Some(&'\'') => Some(2),
        _ => None,
    }
}

/// Millimetres for a text (`'11 in'` → 279.4, `'10 1/2 in'` → 266.7, `'28cm'` → 280), or `None` when it
/// has no number, or no unit and no `default_unit` (`"mm"` for CLI targets, none for sheet labels).
#[must_use]
pub fn parse_length(text: &str, default_unit: Option<&str>) -> Option<f64> {
    let s: Vec<char> = text.chars().collect();
    let start = s.iter().position(char::is_ascii_digit)?;
    let mut k = digits(&s, start);
    if k + 1 < s.len() && matches!(s[k], '.' | ',') && s[k + 1].is_ascii_digit() {
        k = digits(&s, k + 1);
    }
    let whole: String = s[start..k]
        .iter()
        .map(|&c| if c == ',' { '.' } else { c })
        .collect();
    let mut v: f64 = whole.parse().ok()?;
    // (?:\s+(\d+)/(\d+))?
    let mut w = k;
    while w < s.len() && s[w].is_whitespace() {
        w += 1;
    }
    if w > k && w < s.len() && s[w].is_ascii_digit() {
        let n_end = digits(&s, w);
        if s.get(n_end) == Some(&'/') && s.get(n_end + 1).is_some_and(char::is_ascii_digit) {
            let d_end = digits(&s, n_end + 1);
            let num: f64 = s[w..n_end].iter().collect::<String>().parse().ok()?;
            let den: f64 = s[n_end + 1..d_end]
                .iter()
                .collect::<String>()
                .parse()
                .ok()?;
            v += num / den.max(1.0);
            k = d_end;
        }
    }
    while k < s.len() && s[k].is_whitespace() {
        k += 1;
    }
    let unit = match unit_at(&s, k) {
        Some(n) => s[k..k + n].iter().collect::<String>().to_lowercase(),
        None => default_unit?.to_string(),
    };
    match unit.as_str() {
        "" => None,
        "in" | "inch" | "inches" | "\"" | "\u{201d}" | "\u{2033}" | "''" => Some(v * 25.4),
        "mm" => Some(v),
        "cm" => Some(v * 10.0),
        "m" => Some(v * 1000.0),
        _ => None,
    }
}

/// A bare number as a length: itself when the default unit is millimetres, else `None`.
#[must_use]
pub fn parse_number(x: f64, default_unit: Option<&str>) -> Option<f64> {
    (default_unit == Some("mm")).then_some(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_spec_examples() {
        let mm = Some("mm");
        assert!((parse_length("11 in", mm).unwrap() - 279.4).abs() < 1e-12);
        assert!((parse_length("10.5\"", mm).unwrap() - 266.7).abs() < 1e-12);
        assert!((parse_length("10 1/2 in", mm).unwrap() - 266.7).abs() < 1e-12);
        assert_eq!(parse_length("28cm", mm), Some(280.0));
        assert_eq!(parse_length("12", None), None);
        assert_eq!(parse_length("Cut 2.6 fabric", None), None);
        assert_eq!(parse_length("3 inches", None), Some(3.0 * 25.4));
        assert_eq!(parse_length("1,5 m", None), Some(1500.0));
    }
}

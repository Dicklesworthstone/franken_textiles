//! JSON written the way the reference writes its manifest, cutter report and GarmentCode file: Python's
//! `json.dumps(obj, indent=1)` with keys in insertion order, ASCII only, floats as `repr`.

use ftx_core::py;
use serde_json::Value;

/// A JSON value whose objects keep their key order and whose numbers keep Python's int/float distinction.
#[derive(Clone, Debug, PartialEq)]
pub enum PyJson {
    /// `null`.
    Null,
    /// `true` / `false`.
    Bool(bool),
    /// An integer.
    Int(i64),
    /// An unsigned integer, including source identifiers and seeds above `i64::MAX`.
    UInt(u64),
    /// A float (`12.0` stays `12.0`).
    Float(f64),
    /// A string.
    Str(String),
    /// A list.
    List(Vec<PyJson>),
    /// An object in insertion order.
    Dict(Vec<(String, PyJson)>),
}

impl PyJson {
    /// An object from `(key, value)` pairs.
    #[must_use]
    pub fn dict(items: Vec<(&str, PyJson)>) -> Self {
        Self::Dict(items.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
    }

    /// A string.
    #[must_use]
    pub fn str(s: impl Into<String>) -> Self {
        Self::Str(s.into())
    }

    /// A list of floats.
    #[must_use]
    pub fn floats(v: &[f64]) -> Self {
        Self::List(v.iter().map(|&x| Self::Float(x)).collect())
    }

    /// The value under `key` of an object.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&PyJson> {
        match self {
            Self::Dict(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// Sets `key` (replacing in place, or appending).
    pub fn set(&mut self, key: &str, value: PyJson) {
        if let Self::Dict(items) = self {
            match items.iter_mut().find(|(k, _)| k == key) {
                Some(slot) => slot.1 = value,
                None => items.push((key.to_string(), value)),
            }
        }
    }

    /// A `serde_json` value (objects sorted by key, as `serde_json` keeps them).
    #[must_use]
    pub fn to_value(&self) -> Value {
        match self {
            Self::Null => Value::Null,
            Self::Bool(b) => Value::Bool(*b),
            Self::Int(i) => Value::from(*i),
            Self::UInt(i) => Value::from(*i),
            Self::Float(f) => serde_json::Number::from_f64(*f).map_or(Value::Null, Value::Number),
            Self::Str(s) => Value::String(s.clone()),
            Self::List(v) => Value::Array(v.iter().map(Self::to_value).collect()),
            Self::Dict(items) => Value::Object(
                items
                    .iter()
                    .map(|(k, v)| (k.clone(), v.to_value()))
                    .collect(),
            ),
        }
    }

    /// From a `serde_json` value (integers stay integers).
    #[must_use]
    pub fn from_value(v: &Value) -> Self {
        match v {
            Value::Null => Self::Null,
            Value::Bool(b) => Self::Bool(*b),
            Value::Number(n) => match (n.is_f64(), n.as_i64(), n.as_u64()) {
                (false, Some(i), _) => Self::Int(i),
                (false, _, Some(i)) => Self::UInt(i),
                _ => Self::Float(n.as_f64().unwrap_or(f64::NAN)),
            },
            Value::String(s) => Self::Str(s.clone()),
            Value::Array(a) => Self::List(a.iter().map(Self::from_value).collect()),
            Value::Object(o) => Self::Dict(
                o.iter()
                    .map(|(k, v)| (k.clone(), Self::from_value(v)))
                    .collect(),
            ),
        }
    }

    /// Parses JSON keeping object key order (integers without a fraction or exponent stay integers).
    ///
    /// # Errors
    /// Text that is not JSON, with the byte offset.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut p = Parser {
            s: text.as_bytes(),
            i: 0,
        };
        let v = p.value()?;
        p.ws();
        if p.i != p.s.len() {
            return Err(format!("trailing data at byte {}", p.i));
        }
        Ok(v)
    }

    /// `json.dumps(self, indent=indent)` (`None`: one line with `, ` and `: `).
    #[must_use]
    pub fn dumps(&self, indent: Option<usize>) -> String {
        let mut out = String::new();
        self.write(&mut out, indent, 0);
        out
    }

    fn write(&self, out: &mut String, indent: Option<usize>, level: usize) {
        let nl = |out: &mut String, level: usize| {
            if let Some(n) = indent {
                out.push('\n');
                out.push_str(&" ".repeat(n * level));
            }
        };
        let sep = if indent.is_some() { "," } else { ", " };
        match self {
            Self::Null => out.push_str("null"),
            Self::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Self::Int(i) => out.push_str(&i.to_string()),
            Self::UInt(i) => out.push_str(&i.to_string()),
            Self::Float(f) => out.push_str(&if f.is_nan() {
                "NaN".to_string()
            } else if f.is_infinite() {
                if *f > 0.0 { "Infinity" } else { "-Infinity" }.to_string()
            } else {
                py::repr_f64(*f)
            }),
            Self::Str(s) => write_str(out, s),
            Self::List(v) if v.is_empty() => out.push_str("[]"),
            Self::List(v) => {
                out.push('[');
                for (i, x) in v.iter().enumerate() {
                    if i > 0 {
                        out.push_str(sep);
                    }
                    nl(out, level + 1);
                    x.write(out, indent, level + 1);
                }
                nl(out, level);
                out.push(']');
            }
            Self::Dict(items) if items.is_empty() => out.push_str("{}"),
            Self::Dict(items) => {
                out.push('{');
                for (i, (k, v)) in items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(sep);
                    }
                    nl(out, level + 1);
                    write_str(out, k);
                    out.push_str(": ");
                    v.write(out, indent, level + 1);
                }
                nl(out, level);
                out.push('}');
            }
        }
    }
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }

    fn err<T>(&self, what: &str) -> Result<T, String> {
        Err(format!("{what} at byte {}", self.i))
    }

    fn eat(&mut self, lit: &str) -> bool {
        if self.s[self.i..].starts_with(lit.as_bytes()) {
            self.i += lit.len();
            true
        } else {
            false
        }
    }

    fn value(&mut self) -> Result<PyJson, String> {
        self.ws();
        match self.s.get(self.i) {
            None => self.err("unexpected end"),
            Some(b'{') => {
                self.i += 1;
                let mut items = Vec::new();
                self.ws();
                if self.eat("}") {
                    return Ok(PyJson::Dict(items));
                }
                loop {
                    self.ws();
                    let k = self.string()?;
                    self.ws();
                    if !self.eat(":") {
                        return self.err("expected ':'");
                    }
                    items.push((k, self.value()?));
                    self.ws();
                    if self.eat(",") {
                        continue;
                    }
                    if self.eat("}") {
                        return Ok(PyJson::Dict(items));
                    }
                    return self.err("expected ',' or '}'");
                }
            }
            Some(b'[') => {
                self.i += 1;
                let mut v = Vec::new();
                self.ws();
                if self.eat("]") {
                    return Ok(PyJson::List(v));
                }
                loop {
                    v.push(self.value()?);
                    self.ws();
                    if self.eat(",") {
                        continue;
                    }
                    if self.eat("]") {
                        return Ok(PyJson::List(v));
                    }
                    return self.err("expected ',' or ']'");
                }
            }
            Some(b'"') => Ok(PyJson::Str(self.string()?)),
            _ if self.eat("null") => Ok(PyJson::Null),
            _ if self.eat("true") => Ok(PyJson::Bool(true)),
            _ if self.eat("false") => Ok(PyJson::Bool(false)),
            _ if self.eat("NaN") => Ok(PyJson::Float(f64::NAN)),
            _ if self.eat("Infinity") => Ok(PyJson::Float(f64::INFINITY)),
            _ if self.eat("-Infinity") => Ok(PyJson::Float(f64::NEG_INFINITY)),
            _ => {
                let start = self.i;
                while self.i < self.s.len()
                    && matches!(
                        self.s[self.i],
                        b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9'
                    )
                {
                    self.i += 1;
                }
                let t = std::str::from_utf8(&self.s[start..self.i]).unwrap_or("");
                if t.is_empty() {
                    return self.err("unexpected character");
                }
                if !t.contains(['.', 'e', 'E']) {
                    if let Ok(n) = t.parse::<i64>() {
                        return Ok(PyJson::Int(n));
                    }
                    if let Ok(n) = t.parse::<u64>() {
                        return Ok(PyJson::UInt(n));
                    }
                    return Err(format!(
                        "integer outside signed/unsigned 64-bit range at byte {start}"
                    ));
                }
                t.parse::<f64>()
                    .map(PyJson::Float)
                    .map_err(|_| format!("bad number at byte {start}"))
            }
        }
    }

    fn string(&mut self) -> Result<String, String> {
        if !self.eat("\"") {
            return self.err("expected a string");
        }
        let mut out = String::new();
        let mut units: Vec<u16> = Vec::new();
        let flush = |out: &mut String, units: &mut Vec<u16>| {
            if !units.is_empty() {
                out.push_str(&String::from_utf16_lossy(units));
                units.clear();
            }
        };
        loop {
            let Some(&c) = self.s.get(self.i) else {
                return self.err("unterminated string");
            };
            self.i += 1;
            match c {
                b'"' => break,
                b'\\' => {
                    let Some(&e) = self.s.get(self.i) else {
                        return self.err("unterminated escape");
                    };
                    self.i += 1;
                    if e == b'u' {
                        let hex =
                            std::str::from_utf8(self.s.get(self.i..self.i + 4).unwrap_or(&[]))
                                .unwrap_or("");
                        let u = u16::from_str_radix(hex, 16)
                            .map_err(|_| format!("bad \\u escape at byte {}", self.i))?;
                        self.i += 4;
                        units.push(u);
                        continue;
                    }
                    flush(&mut out, &mut units);
                    out.push(match e {
                        b'n' => '\n',
                        b't' => '\t',
                        b'r' => '\r',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        other => char::from(other),
                    });
                }
                _ => {
                    flush(&mut out, &mut units);
                    // a run of raw UTF-8 bytes
                    let start = self.i - 1;
                    let mut end = self.i;
                    while end < self.s.len() && self.s[end] != b'"' && self.s[end] != b'\\' {
                        end += 1;
                    }
                    out.push_str(&String::from_utf8_lossy(&self.s[start..end]));
                    self.i = end;
                }
            }
        }
        flush(&mut out, &mut units);
        Ok(out)
    }
}

fn write_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || (c as u32) > 0x7e => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dumps_like_python() {
        let v = PyJson::dict(vec![
            ("b", PyJson::floats(&[841.0, 1e-5, 0.1])),
            ("a", PyJson::Int(3)),
            ("é", PyJson::str("x\u{1F600}\n")),
            ("e", PyJson::List(vec![])),
            ("d", PyJson::Dict(vec![])),
            ("n", PyJson::Null),
        ]);
        assert_eq!(
            v.dumps(Some(1)),
            "{\n \"b\": [\n  841.0,\n  1e-05,\n  0.1\n ],\n \"a\": 3,\n \"\\u00e9\": \"x\\ud83d\\ude00\\n\",\n \"e\": [],\n \"d\": {},\n \"n\": null\n}"
        );
        assert_eq!(PyJson::floats(&[1.5, 2.0]).dumps(None), "[1.5, 2.0]");
        // parsing keeps key order and round-trips
        let text = v.dumps(Some(1));
        assert_eq!(PyJson::parse(&text).unwrap().dumps(Some(1)), text);
    }

    #[test]
    fn full_width_unsigned_integers_keep_their_identity_through_every_conversion() {
        for value in [i64::MAX as u64, i64::MAX as u64 + 1, u64::MAX - 1, u64::MAX] {
            let text = value.to_string();
            let parsed = PyJson::parse(&text).unwrap();
            assert_eq!(parsed.dumps(None), text);
            assert_eq!(parsed.to_value().as_u64(), Some(value));
            let from_value = PyJson::from_value(&serde_json::json!(value));
            assert_eq!(from_value.dumps(None), text);
            assert_eq!(from_value.to_value().as_u64(), Some(value));
        }
        let text = "[-9223372036854775808, 9223372036854775808, 18446744073709551615, -0.0, 1.0]";
        assert_eq!(PyJson::parse(text).unwrap().dumps(None), text);
    }

    #[test]
    fn integer_overflow_refuses_instead_of_becoming_a_float() {
        for text in ["18446744073709551616", "-9223372036854775809"] {
            assert!(PyJson::parse(text).unwrap_err().contains("64-bit range"));
        }
        // An explicitly floating-point source retains floating-point semantics.
        assert!(matches!(PyJson::parse("1e20").unwrap(), PyJson::Float(_)));
    }
}

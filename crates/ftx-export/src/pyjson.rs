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
            Value::Number(n) => match n.as_i64() {
                Some(i) if !n.is_f64() => Self::Int(i),
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
    }
}

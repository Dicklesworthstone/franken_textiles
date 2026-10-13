//! A JSON value whose objects keep their key order. `names.json` is read in order (renames, splits and the
//! reference's error messages follow it), which a `serde_json::Value` would sort.

use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};

/// A JSON value with ordered objects.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum OValue {
    /// `null` (also absent).
    #[default]
    Null,
    /// `true` / `false`.
    Bool(bool),
    /// A number.
    Number(Number),
    /// A string.
    String(String),
    /// An array.
    Array(Vec<OValue>),
    /// An object in document order.
    Object(Vec<(String, OValue)>),
}

impl OValue {
    /// The member `key` of an object (the last one if repeated, as Python's `dict`).
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&OValue> {
        match self {
            Self::Object(m) => m.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// Whether an object has the member `key`.
    #[must_use]
    pub fn has(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// The members of an object in order (empty for anything else), duplicates resolved as Python's `dict`.
    #[must_use]
    pub fn entries(&self) -> Vec<(&str, &OValue)> {
        let mut out: Vec<(&str, &OValue)> = Vec::new();
        if let Self::Object(m) = self {
            for (k, v) in m {
                match out.iter_mut().find(|(q, _)| *q == k) {
                    Some(slot) => slot.1 = v,
                    None => out.push((k, v)),
                }
            }
        }
        out
    }

    /// The items of an array (empty for anything else).
    #[must_use]
    pub fn items(&self) -> &[OValue] {
        match self {
            Self::Array(a) => a,
            _ => &[],
        }
    }

    /// The text of a string.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s),
            _ => None,
        }
    }

    /// The value of a number.
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Number(n) => n.as_f64(),
            _ => None,
        }
    }

    /// Python truthiness: `null`, `false`, `0`, `""`, `[]` and `{}` are false.
    #[must_use]
    pub fn truthy(&self) -> bool {
        match self {
            Self::Null => false,
            Self::Bool(b) => *b,
            Self::Number(n) => n.as_f64().is_some_and(|x| x != 0.0),
            Self::String(s) => !s.is_empty(),
            Self::Array(a) => !a.is_empty(),
            Self::Object(m) => !m.is_empty(),
        }
    }

    /// As a `serde_json::Value` (object keys then sorted, as everywhere else in the workspace).
    #[must_use]
    pub fn to_value(&self) -> Value {
        match self {
            Self::Null => Value::Null,
            Self::Bool(b) => Value::Bool(*b),
            Self::Number(n) => Value::Number(n.clone()),
            Self::String(s) => Value::String(s.clone()),
            Self::Array(a) => Value::Array(a.iter().map(Self::to_value).collect()),
            Self::Object(_) => Value::Object(
                self.entries()
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), v.to_value()))
                    .collect::<Map<String, Value>>(),
            ),
        }
    }

    /// From a `serde_json::Value` (objects in its key order).
    #[must_use]
    pub fn from_value(v: &Value) -> Self {
        match v {
            Value::Null => Self::Null,
            Value::Bool(b) => Self::Bool(*b),
            Value::Number(n) => Self::Number(n.clone()),
            Value::String(s) => Self::String(s.clone()),
            Value::Array(a) => Self::Array(a.iter().map(Self::from_value).collect()),
            Value::Object(m) => Self::Object(
                m.iter()
                    .map(|(k, v)| (k.clone(), Self::from_value(v)))
                    .collect(),
            ),
        }
    }
}

impl serde::Serialize for OValue {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::{SerializeMap, SerializeSeq};
        match self {
            Self::Null => s.serialize_unit(),
            Self::Bool(b) => s.serialize_bool(*b),
            Self::Number(n) => n.serialize(s),
            Self::String(t) => s.serialize_str(t),
            Self::Array(a) => {
                let mut seq = s.serialize_seq(Some(a.len()))?;
                for v in a {
                    seq.serialize_element(v)?;
                }
                seq.end()
            }
            Self::Object(m) => {
                let mut map = s.serialize_map(Some(m.len()))?;
                for (k, v) in m {
                    map.serialize_entry(k, v)?;
                }
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for OValue {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = OValue;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("any JSON value")
            }
            fn visit_unit<E: de::Error>(self) -> Result<OValue, E> {
                Ok(OValue::Null)
            }
            fn visit_none<E: de::Error>(self) -> Result<OValue, E> {
                Ok(OValue::Null)
            }
            fn visit_some<D2: Deserializer<'de>>(self, d: D2) -> Result<OValue, D2::Error> {
                OValue::deserialize(d)
            }
            fn visit_bool<E: de::Error>(self, b: bool) -> Result<OValue, E> {
                Ok(OValue::Bool(b))
            }
            fn visit_i64<E: de::Error>(self, n: i64) -> Result<OValue, E> {
                Ok(OValue::Number(n.into()))
            }
            fn visit_u64<E: de::Error>(self, n: u64) -> Result<OValue, E> {
                Ok(OValue::Number(n.into()))
            }
            fn visit_f64<E: de::Error>(self, x: f64) -> Result<OValue, E> {
                Number::from_f64(x)
                    .map(OValue::Number)
                    .ok_or_else(|| E::custom("non-finite number"))
            }
            fn visit_str<E: de::Error>(self, s: &str) -> Result<OValue, E> {
                Ok(OValue::String(s.to_string()))
            }
            fn visit_string<E: de::Error>(self, s: String) -> Result<OValue, E> {
                Ok(OValue::String(s))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<OValue, A::Error> {
                let mut out = Vec::new();
                while let Some(v) = a.next_element::<OValue>()? {
                    out.push(v);
                }
                Ok(OValue::Array(out))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<OValue, A::Error> {
                let mut out = Vec::new();
                while let Some((k, v)) = a.next_entry::<String, OValue>()? {
                    out.push((k, v));
                }
                Ok(OValue::Object(out))
            }
        }
        d.deserialize_any(V)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn objects_keep_their_order_and_python_dict_semantics() {
        let v: OValue =
            serde_json::from_str(r#"{"z": 1, "a": [true, null], "z": "last"}"#).unwrap();
        let keys: Vec<&str> = v.entries().iter().map(|(k, _)| *k).collect();
        assert_eq!(keys, ["z", "a"]);
        assert_eq!(v.get("z").and_then(OValue::as_str), Some("last"));
        assert!(v.get("a").is_some_and(OValue::truthy) && !OValue::Null.truthy());
    }
}

//! Run receipts: what went in, with which parameters, and what came out, by SHA-256 (plan §6–7).
//! A receipt has no timestamps or host names, so the same run writes the same receipt everywhere,
//! and two runs agree exactly when their digests do.

use std::fmt;

use serde::ser::{
    self, Impossible, SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant,
    SerializeTuple, SerializeTupleStruct, SerializeTupleVariant, Serializer,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// A named byte stream and its SHA-256.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Digest {
    /// What the bytes are (a file name or a role such as `pattern`).
    pub name: String,
    /// Lowercase hex SHA-256.
    pub sha256: String,
    /// Length in bytes.
    pub bytes: u64,
}

impl Digest {
    /// Digest of `data` under `name`.
    #[must_use]
    pub fn of(name: impl Into<String>, data: &[u8]) -> Self {
        Self {
            name: name.into(),
            sha256: sha256_hex(data),
            bytes: data.len() as u64,
        }
    }
}

/// A run receipt.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Receipt {
    /// Always `"franken_textiles"`.
    pub tool: String,
    /// The `ftx-core` version that wrote it.
    pub version: String,
    /// The operation, e.g. `validate` or `export`.
    pub operation: String,
    /// Inputs in the order they were read.
    pub inputs: Vec<Digest>,
    /// Parameters that change the result.
    pub parameters: Map<String, Value>,
    /// Outputs in the order they were written.
    pub outputs: Vec<Digest>,
}

impl Receipt {
    /// An empty receipt for `operation`.
    #[must_use]
    pub fn new(operation: impl Into<String>) -> Self {
        Self {
            tool: "franken_textiles".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            operation: operation.into(),
            inputs: Vec::new(),
            parameters: Map::new(),
            outputs: Vec::new(),
        }
    }

    /// Records an input.
    #[must_use]
    pub fn input(mut self, name: impl Into<String>, data: &[u8]) -> Self {
        self.inputs.push(Digest::of(name, data));
        self
    }

    /// Records a parameter.
    #[must_use]
    pub fn param(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.parameters.insert(key.into(), value.into());
        self
    }

    /// Records an output.
    #[must_use]
    pub fn output(mut self, name: impl Into<String>, data: &[u8]) -> Self {
        self.outputs.push(Digest::of(name, data));
        self
    }
}

/// Incremental SHA-256 (FIPS 180-4).
#[derive(Clone, Debug)]
pub struct Sha256 {
    state: [u32; 8],
    buf: [u8; 64],
    buf_len: usize,
    total: u64,
}

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

impl Default for Sha256 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha256 {
    /// A fresh hasher.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            buf: [0; 64],
            buf_len: 0,
            total: 0,
        }
    }

    /// Feeds bytes.
    pub fn update(&mut self, mut data: &[u8]) {
        self.total = self.total.wrapping_add(data.len() as u64);
        if self.buf_len > 0 {
            let take = (64 - self.buf_len).min(data.len());
            self.buf[self.buf_len..self.buf_len + take].copy_from_slice(&data[..take]);
            self.buf_len += take;
            data = &data[take..];
            if self.buf_len < 64 {
                return;
            }
            let block = self.buf;
            self.compress(&block);
            self.buf_len = 0;
        }
        let (blocks, rest) = data.as_chunks::<64>();
        for block in blocks {
            self.compress(block);
        }
        self.buf[..rest.len()].copy_from_slice(rest);
        self.buf_len = rest.len();
    }

    /// The digest.
    #[must_use]
    pub fn finalize(mut self) -> [u8; 32] {
        let bit_len = self.total.wrapping_mul(8);
        let mut pad = vec![0x80u8];
        let used = (self.buf_len + 1) % 64;
        pad.resize(1 + if used <= 56 { 56 - used } else { 120 - used }, 0);
        pad.extend_from_slice(&bit_len.to_be_bytes());
        let total = self.total;
        self.update(&pad);
        self.total = total;
        let mut out = [0u8; 32];
        for (chunk, word) in out.as_chunks_mut::<4>().0.iter_mut().zip(self.state) {
            chunk.copy_from_slice(&word.to_be_bytes());
        }
        out
    }

    fn compress(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for (wi, word) in w.iter_mut().zip(block.as_chunks::<4>().0) {
            *wi = u32::from_be_bytes(*word);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (s, v) in self.state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *s = s.wrapping_add(v);
        }
    }
}

/// Lowercase hex SHA-256 of `data`.
#[must_use]
pub fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Canonical SHA-256 of any `Serialize` value for sealed cross-crate receipts.
///
/// Serializes via `serde_json::Value` (which stores object keys in deterministic `BTreeMap` order),
/// recursively normalizes `-0.0 -> 0.0`, refuses non-finite numbers fail-closed, and returns the
/// lowercase hex SHA-256 of the compact canonical UTF-8 JSON bytes.
///
/// # Errors
/// [`crate::RefusalCode::InvalidDocument`] if serialization fails, or
/// [`crate::RefusalCode::NonFinite`] if any floating-point value is NaN or infinite.
pub fn canonical_json_sha256<T: Serialize>(value: &T) -> Result<String, crate::Refusal> {
    let json = value
        .serialize(CanonicalJsonSerializer)
        .map_err(|e| match e {
            CanonicalJsonError::NonFinite => crate::Refusal::new(
                crate::RefusalCode::NonFinite,
                "canonical receipt JSON contains a non-finite number",
            ),
            CanonicalJsonError::Message(msg) => crate::Refusal::new(
                crate::RefusalCode::InvalidDocument,
                format!("cannot serialize canonical receipt payload: {msg}"),
            ),
        })?;
    let bytes = serde_json::to_vec(&json).map_err(|e| {
        crate::Refusal::new(
            crate::RefusalCode::InvalidDocument,
            format!("cannot encode canonical receipt JSON: {e}"),
        )
    })?;
    Ok(sha256_hex(&bytes))
}

#[derive(Debug)]
enum CanonicalJsonError {
    NonFinite,
    Message(String),
}

impl fmt::Display for CanonicalJsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinite => f.write_str("non-finite floating-point value"),
            Self::Message(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for CanonicalJsonError {}

impl ser::Error for CanonicalJsonError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Self::Message(msg.to_string())
    }
}

/// Serializer directly to canonical `serde_json::Value` (sorted `BTreeMap` keys, `-0.0 -> 0.0`,
/// and fail-closed rejection of `NaN` / `±Infinity` instead of `serde_json::to_value`'s silent `null`).
struct CanonicalJsonSerializer;

impl Serializer for CanonicalJsonSerializer {
    type Ok = Value;
    type Error = CanonicalJsonError;
    type SerializeSeq = CanonicalSeq;
    type SerializeTuple = CanonicalSeq;
    type SerializeTupleStruct = CanonicalSeq;
    type SerializeTupleVariant = CanonicalTupleVariant;
    type SerializeMap = CanonicalMap;
    type SerializeStruct = CanonicalStruct;
    type SerializeStructVariant = CanonicalStructVariant;

    fn serialize_bool(self, v: bool) -> Result<Value, CanonicalJsonError> {
        Ok(Value::Bool(v))
    }

    fn serialize_i8(self, v: i8) -> Result<Value, CanonicalJsonError> {
        self.serialize_i64(i64::from(v))
    }

    fn serialize_i16(self, v: i16) -> Result<Value, CanonicalJsonError> {
        self.serialize_i64(i64::from(v))
    }

    fn serialize_i32(self, v: i32) -> Result<Value, CanonicalJsonError> {
        self.serialize_i64(i64::from(v))
    }

    fn serialize_i64(self, v: i64) -> Result<Value, CanonicalJsonError> {
        Ok(Value::Number(v.into()))
    }

    fn serialize_i128(self, v: i128) -> Result<Value, CanonicalJsonError> {
        if let Ok(n) = i64::try_from(v) {
            Ok(Value::Number(n.into()))
        } else if let Ok(n) = u64::try_from(v) {
            Ok(Value::Number(n.into()))
        } else {
            Err(CanonicalJsonError::Message(
                "i128 value out of JSON integer range".to_string(),
            ))
        }
    }

    fn serialize_u8(self, v: u8) -> Result<Value, CanonicalJsonError> {
        self.serialize_u64(u64::from(v))
    }

    fn serialize_u16(self, v: u16) -> Result<Value, CanonicalJsonError> {
        self.serialize_u64(u64::from(v))
    }

    fn serialize_u32(self, v: u32) -> Result<Value, CanonicalJsonError> {
        self.serialize_u64(u64::from(v))
    }

    fn serialize_u64(self, v: u64) -> Result<Value, CanonicalJsonError> {
        Ok(Value::Number(v.into()))
    }

    fn serialize_u128(self, v: u128) -> Result<Value, CanonicalJsonError> {
        u64::try_from(v)
            .map(|n| Value::Number(n.into()))
            .map_err(|_| {
                CanonicalJsonError::Message("u128 value out of JSON integer range".to_string())
            })
    }

    fn serialize_f32(self, v: f32) -> Result<Value, CanonicalJsonError> {
        self.serialize_f64(f64::from(v))
    }

    fn serialize_f64(self, v: f64) -> Result<Value, CanonicalJsonError> {
        if !v.is_finite() {
            return Err(CanonicalJsonError::NonFinite);
        }
        let normalized = if v == 0.0 { 0.0_f64 } else { v };
        serde_json::Number::from_f64(normalized)
            .map(Value::Number)
            .ok_or(CanonicalJsonError::NonFinite)
    }

    fn serialize_char(self, v: char) -> Result<Value, CanonicalJsonError> {
        Ok(Value::String(v.to_string()))
    }

    fn serialize_str(self, v: &str) -> Result<Value, CanonicalJsonError> {
        Ok(Value::String(v.to_owned()))
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<Value, CanonicalJsonError> {
        Ok(Value::Array(
            v.iter().map(|&b| Value::Number(b.into())).collect(),
        ))
    }

    fn serialize_none(self) -> Result<Value, CanonicalJsonError> {
        Ok(Value::Null)
    }

    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<Value, CanonicalJsonError> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Value, CanonicalJsonError> {
        Ok(Value::Null)
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Value, CanonicalJsonError> {
        Ok(Value::Null)
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<Value, CanonicalJsonError> {
        Ok(Value::String(variant.to_owned()))
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Value, CanonicalJsonError> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Value, CanonicalJsonError> {
        let mut map = Map::new();
        map.insert(variant.to_owned(), value.serialize(self)?);
        Ok(Value::Object(map))
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq, CanonicalJsonError> {
        Ok(CanonicalSeq {
            items: Vec::with_capacity(len.unwrap_or(0)),
        })
    }

    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple, CanonicalJsonError> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleStruct, CanonicalJsonError> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleVariant, CanonicalJsonError> {
        Ok(CanonicalTupleVariant {
            name: variant.to_owned(),
            items: Vec::with_capacity(len),
        })
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, CanonicalJsonError> {
        Ok(CanonicalMap {
            entries: Map::new(),
            next_key: None,
        })
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStruct, CanonicalJsonError> {
        Ok(CanonicalStruct {
            entries: Map::new(),
        })
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, CanonicalJsonError> {
        Ok(CanonicalStructVariant {
            name: variant.to_owned(),
            entries: Map::new(),
        })
    }

    fn collect_str<T: ?Sized + fmt::Display>(self, value: &T) -> Result<Value, CanonicalJsonError> {
        Ok(Value::String(value.to_string()))
    }
}

struct CanonicalSeq {
    items: Vec<Value>,
}

impl SerializeSeq for CanonicalSeq {
    type Ok = Value;
    type Error = CanonicalJsonError;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        self.items.push(value.serialize(CanonicalJsonSerializer)?);
        Ok(())
    }

    fn end(self) -> Result<Value, Self::Error> {
        Ok(Value::Array(self.items))
    }
}

impl SerializeTuple for CanonicalSeq {
    type Ok = Value;
    type Error = CanonicalJsonError;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<Value, Self::Error> {
        SerializeSeq::end(self)
    }
}

impl SerializeTupleStruct for CanonicalSeq {
    type Ok = Value;
    type Error = CanonicalJsonError;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<Value, Self::Error> {
        SerializeSeq::end(self)
    }
}

struct CanonicalTupleVariant {
    name: String,
    items: Vec<Value>,
}

impl SerializeTupleVariant for CanonicalTupleVariant {
    type Ok = Value;
    type Error = CanonicalJsonError;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        self.items.push(value.serialize(CanonicalJsonSerializer)?);
        Ok(())
    }

    fn end(self) -> Result<Value, Self::Error> {
        let mut map = Map::new();
        map.insert(self.name, Value::Array(self.items));
        Ok(Value::Object(map))
    }
}

struct CanonicalMap {
    entries: Map<String, Value>,
    next_key: Option<String>,
}

impl SerializeMap for CanonicalMap {
    type Ok = Value;
    type Error = CanonicalJsonError;

    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<(), Self::Error> {
        self.next_key = Some(key.serialize(CanonicalKeySerializer)?);
        Ok(())
    }

    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        let key = self.next_key.take().ok_or_else(|| {
            CanonicalJsonError::Message("serialize_value called before serialize_key".to_string())
        })?;
        self.entries
            .insert(key, value.serialize(CanonicalJsonSerializer)?);
        Ok(())
    }

    fn serialize_entry<K: ?Sized + Serialize, V: ?Sized + Serialize>(
        &mut self,
        key: &K,
        value: &V,
    ) -> Result<(), Self::Error> {
        let key = key.serialize(CanonicalKeySerializer)?;
        let value = value.serialize(CanonicalJsonSerializer)?;
        self.entries.insert(key, value);
        Ok(())
    }

    fn end(self) -> Result<Value, Self::Error> {
        Ok(Value::Object(self.entries))
    }
}

struct CanonicalStruct {
    entries: Map<String, Value>,
}

impl SerializeStruct for CanonicalStruct {
    type Ok = Value;
    type Error = CanonicalJsonError;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        self.entries
            .insert(key.to_owned(), value.serialize(CanonicalJsonSerializer)?);
        Ok(())
    }

    fn end(self) -> Result<Value, Self::Error> {
        Ok(Value::Object(self.entries))
    }
}

struct CanonicalStructVariant {
    name: String,
    entries: Map<String, Value>,
}

impl SerializeStructVariant for CanonicalStructVariant {
    type Ok = Value;
    type Error = CanonicalJsonError;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        self.entries
            .insert(key.to_owned(), value.serialize(CanonicalJsonSerializer)?);
        Ok(())
    }

    fn end(self) -> Result<Value, Self::Error> {
        let mut map = Map::new();
        map.insert(self.name, Value::Object(self.entries));
        Ok(Value::Object(map))
    }
}

struct CanonicalKeySerializer;

impl CanonicalKeySerializer {
    fn invalid_key() -> CanonicalJsonError {
        CanonicalJsonError::Message("map key must be a string".to_string())
    }
}

impl Serializer for CanonicalKeySerializer {
    type Ok = String;
    type Error = CanonicalJsonError;
    type SerializeSeq = Impossible<String, CanonicalJsonError>;
    type SerializeTuple = Impossible<String, CanonicalJsonError>;
    type SerializeTupleStruct = Impossible<String, CanonicalJsonError>;
    type SerializeTupleVariant = Impossible<String, CanonicalJsonError>;
    type SerializeMap = Impossible<String, CanonicalJsonError>;
    type SerializeStruct = Impossible<String, CanonicalJsonError>;
    type SerializeStructVariant = Impossible<String, CanonicalJsonError>;

    fn serialize_bool(self, _v: bool) -> Result<String, CanonicalJsonError> {
        Err(Self::invalid_key())
    }

    fn serialize_i8(self, v: i8) -> Result<String, CanonicalJsonError> {
        Ok(v.to_string())
    }

    fn serialize_i16(self, v: i16) -> Result<String, CanonicalJsonError> {
        Ok(v.to_string())
    }

    fn serialize_i32(self, v: i32) -> Result<String, CanonicalJsonError> {
        Ok(v.to_string())
    }

    fn serialize_i64(self, v: i64) -> Result<String, CanonicalJsonError> {
        Ok(v.to_string())
    }

    fn serialize_i128(self, v: i128) -> Result<String, CanonicalJsonError> {
        Ok(v.to_string())
    }

    fn serialize_u8(self, v: u8) -> Result<String, CanonicalJsonError> {
        Ok(v.to_string())
    }

    fn serialize_u16(self, v: u16) -> Result<String, CanonicalJsonError> {
        Ok(v.to_string())
    }

    fn serialize_u32(self, v: u32) -> Result<String, CanonicalJsonError> {
        Ok(v.to_string())
    }

    fn serialize_u64(self, v: u64) -> Result<String, CanonicalJsonError> {
        Ok(v.to_string())
    }

    fn serialize_u128(self, v: u128) -> Result<String, CanonicalJsonError> {
        Ok(v.to_string())
    }

    fn serialize_f32(self, _v: f32) -> Result<String, CanonicalJsonError> {
        Err(Self::invalid_key())
    }

    fn serialize_f64(self, _v: f64) -> Result<String, CanonicalJsonError> {
        Err(Self::invalid_key())
    }

    fn serialize_char(self, v: char) -> Result<String, CanonicalJsonError> {
        Ok(v.to_string())
    }

    fn serialize_str(self, v: &str) -> Result<String, CanonicalJsonError> {
        Ok(v.to_owned())
    }

    fn serialize_bytes(self, _v: &[u8]) -> Result<String, CanonicalJsonError> {
        Err(Self::invalid_key())
    }

    fn serialize_none(self) -> Result<String, CanonicalJsonError> {
        Err(Self::invalid_key())
    }

    fn serialize_some<T: ?Sized + Serialize>(
        self,
        _value: &T,
    ) -> Result<String, CanonicalJsonError> {
        Err(Self::invalid_key())
    }

    fn serialize_unit(self) -> Result<String, CanonicalJsonError> {
        Err(Self::invalid_key())
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<String, CanonicalJsonError> {
        Err(Self::invalid_key())
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<String, CanonicalJsonError> {
        Ok(variant.to_owned())
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<String, CanonicalJsonError> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _value: &T,
    ) -> Result<String, CanonicalJsonError> {
        Err(Self::invalid_key())
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, CanonicalJsonError> {
        Err(Self::invalid_key())
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, CanonicalJsonError> {
        Err(Self::invalid_key())
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, CanonicalJsonError> {
        Err(Self::invalid_key())
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, CanonicalJsonError> {
        Err(Self::invalid_key())
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, CanonicalJsonError> {
        Err(Self::invalid_key())
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStruct, CanonicalJsonError> {
        Err(Self::invalid_key())
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, CanonicalJsonError> {
        Err(Self::invalid_key())
    }

    fn collect_str<T: ?Sized + fmt::Display>(
        self,
        value: &T,
    ) -> Result<String, CanonicalJsonError> {
        Ok(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FIPS 180-4 example vectors.
    #[test]
    fn sha256_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        let mut h = Sha256::new();
        let a = [b'a'; 1000];
        for _ in 0..1000 {
            h.update(&a);
        }
        let hex: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            hex,
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    #[test]
    fn split_updates_match_one_shot() {
        let data: Vec<u8> = (0..1000u32).map(|i| (i * 7 + 3) as u8).collect();
        for split in [0, 1, 55, 56, 63, 64, 65, 127, 128, 999] {
            let mut h = Sha256::new();
            h.update(&data[..split]);
            h.update(&data[split..]);
            let hex: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
            assert_eq!(hex, sha256_hex(&data), "split {split}");
        }
    }

    #[test]
    fn receipt_is_deterministic() {
        let make = || {
            Receipt::new("validate")
                .input("pattern.json", b"{}")
                .param("seam_tol_mm", 3.0)
                .output("validation.json", b"[]")
        };
        let (a, b) = (make(), make());
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            serde_json::to_string(&b).unwrap()
        );
        assert_eq!(a.inputs[0].bytes, 2);
        assert_eq!(a.outputs[0].sha256, sha256_hex(b"[]"));
        assert_eq!(a.parameters["seam_tol_mm"], 3.0);
    }

    #[test]
    fn canonical_json_sha256_normalizes_neg_zero_and_key_order() {
        use serde_json::json;
        #[derive(Serialize)]
        struct Nested {
            z: f32,
            a: f64,
        }
        let a = json!({"b": -0.0, "a": [1.0, -0.0]});
        let b = json!({"a": [1.0, 0.0], "b": 0.0});
        assert_eq!(
            canonical_json_sha256(&a).unwrap(),
            canonical_json_sha256(&b).unwrap()
        );
        assert_eq!(
            canonical_json_sha256(&Nested {
                z: -0.0_f32,
                a: -0.0_f64,
            })
            .unwrap(),
            canonical_json_sha256(&Nested {
                z: 0.0_f32,
                a: 0.0_f64,
            })
            .unwrap()
        );
        // Non-finite floats must fail closed with NonFinite (never silently serialize as null).
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let err = canonical_json_sha256(&[bad]).unwrap_err();
            assert_eq!(err.code, crate::RefusalCode::NonFinite);
        }
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let err = canonical_json_sha256(&Nested { z: bad, a: 1.0 }).unwrap_err();
            assert_eq!(err.code, crate::RefusalCode::NonFinite);
        }
    }

    #[test]
    fn process_refusal_codes_serialize_stably() {
        use crate::RefusalCode;
        for (code, want) in [
            (RefusalCode::UnqualifiedProcess, "unqualified_process"),
            (
                RefusalCode::OpaqueProprietaryTarget,
                "opaque_proprietary_target",
            ),
            (
                RefusalCode::MachineStateInfeasible,
                "machine_state_infeasible",
            ),
            (
                RefusalCode::MaterialProcessHazard,
                "material_process_hazard",
            ),
            (RefusalCode::VerificationMismatch, "verification_mismatch"),
        ] {
            assert_eq!(code.as_str(), want);
            assert_eq!(serde_json::to_string(&code).unwrap(), format!("\"{want}\""));
        }
    }
}

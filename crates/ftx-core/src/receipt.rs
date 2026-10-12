//! Run receipts: what went in, with which parameters, and what came out, by SHA-256 (plan §6–7).
//! A receipt has no timestamps or host names, so the same run writes the same receipt everywhere,
//! and two runs agree exactly when their digests do.

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
}

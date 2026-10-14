//! Counter-based Philox4x32-10 streams keyed by logical work identity (`StreamKey`, `Stream`).
//!
//! Provenance: lifted from frankensim `crates/fs-rand/src/philox.rs` (commit
//! `8254164a7dea53dc02343de01c9543a23d99dcf5`) and `crates/fs-rand/src/lib.rs` (`StreamKey`,
//! `Stream`; same author, MIT + OpenAI/Anthropic rider). Elementary transforms (`next_normal`,
//! `next_exponential`) route through [`crate::det`] so draws are bit-identical on native and
//! `wasm32-unknown-unknown`.

use crate::det;

/// Philox 32-bit multipliers (Random123).
const M0: u32 = 0xD251_1F53;
const M1: u32 = 0xCD9E_8D57;
/// Weyl key-schedule increments (golden-ratio / sqrt(3)−1 fractions).
const W0: u32 = 0x9E37_79B9;
const W1: u32 = 0xBB67_AE85;

#[inline]
fn mulhilo(a: u32, b: u32) -> (u32, u32) {
    let wide = u64::from(a) * u64::from(b);
    ((wide >> 32) as u32, wide as u32)
}

/// One Philox round.
#[inline]
fn round(ctr: [u32; 4], key: [u32; 2]) -> [u32; 4] {
    let (hi0, lo0) = mulhilo(M0, ctr[0]);
    let (hi1, lo1) = mulhilo(M1, ctr[2]);
    [hi1 ^ ctr[1] ^ key[0], lo1, hi0 ^ ctr[3] ^ key[1], lo0]
}

/// The full 10-round Philox4x32 block function: 128-bit counter + 64-bit key → 128 output bits.
#[must_use]
pub fn philox4x32_10(mut ctr: [u32; 4], mut key: [u32; 2]) -> [u32; 4] {
    for i in 0..10 {
        if i > 0 {
            key[0] = key[0].wrapping_add(W0);
            key[1] = key[1].wrapping_add(W1);
        }
        ctr = round(ctr, key);
    }
    ctr
}

/// Batched Philox4x32-10: `L` counter blocks under one shared key, computed lane-wise
/// (structure-of-arrays: `c[word][lane]`). Each lane is bitwise-identical to [`philox4x32_10`].
#[must_use]
#[allow(clippy::needless_range_loop)]
pub fn philox4x32_10_batch<const L: usize>(ctr: &[[u32; 4]; L], key: [u32; 2]) -> [[u32; 4]; L] {
    let mut c: [[u32; L]; 4] = [[0; L]; 4];
    for (lane, blk) in ctr.iter().enumerate() {
        for w in 0..4 {
            c[w][lane] = blk[w];
        }
    }
    let (mut k0, mut k1) = (key[0], key[1]);
    for r in 0..10 {
        if r > 0 {
            k0 = k0.wrapping_add(W0);
            k1 = k1.wrapping_add(W1);
        }
        for lane in 0..L {
            let (hi0, lo0) = mulhilo(M0, c[0][lane]);
            let (hi1, lo1) = mulhilo(M1, c[2][lane]);
            let n0 = hi1 ^ c[1][lane] ^ k0;
            let n2 = hi0 ^ c[3][lane] ^ k1;
            c[0][lane] = n0;
            c[1][lane] = lo1;
            c[2][lane] = n2;
            c[3][lane] = lo0;
        }
    }
    let mut out = [[0u32; 4]; L];
    for (lane, o) in out.iter_mut().enumerate() {
        for w in 0..4 {
            o[w] = c[w][lane];
        }
    }
    out
}

/// Logical identity of a deterministic counter-based stream: `(seed, kernel, tile)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StreamKey {
    /// 64-bit study seed (Philox key).
    pub seed: u64,
    /// 32-bit logical kernel identifier.
    pub kernel: u32,
    /// 32-bit logical tile / work-item identifier.
    pub tile: u32,
}

impl StreamKey {
    /// Open the stream at draw index 0.
    #[must_use]
    pub const fn stream(self) -> Stream {
        Stream {
            key: self,
            index: 0,
        }
    }
}

/// A sequential view over the counter-based Philox4x32-10 generator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stream {
    key: StreamKey,
    index: u64,
}

const fn philox_position_words(key: StreamKey, index: u64) -> ([u32; 4], [u32; 2]) {
    (
        [index as u32, (index >> 32) as u32, key.tile, key.kernel],
        [key.seed as u32, (key.seed >> 32) as u32],
    )
}

impl Stream {
    /// Random access: the 128 output bits at `index`, independent of sequential position.
    #[must_use]
    pub fn at(key: StreamKey, index: u64) -> [u32; 4] {
        let (counter, key_words) = philox_position_words(key, index);
        philox4x32_10(counter, key_words)
    }

    /// Current draw counter position.
    #[must_use]
    pub const fn index(&self) -> u64 {
        self.index
    }

    /// Logical stream key.
    #[must_use]
    pub const fn key(&self) -> StreamKey {
        self.key
    }

    /// Next 64 uniform bits.
    #[must_use]
    pub fn next_u64(&mut self) -> u64 {
        let block = Self::at(self.key, self.index);
        self.index = self.index.wrapping_add(1);
        (u64::from(block[1]) << 32) | u64::from(block[0])
    }

    /// Uniform in `[0, 1)` with 53 random bits (`2^-53` ladder).
    #[must_use]
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
    }

    /// Uniform integer in `[0, n)` via Lemire's widening-multiply method with deterministic
    /// rejection advancement.
    ///
    /// # Panics
    /// If `n == 0`.
    #[must_use]
    pub fn next_below(&mut self, n: u64) -> u64 {
        assert!(n > 0, "next_below(0) is meaningless");
        loop {
            let x = self.next_u64();
            let m = u128::from(x) * u128::from(n);
            let lo = m as u64;
            if lo >= n.wrapping_neg() % n {
                return (m >> 64) as u64;
            }
        }
    }

    /// Standard normal `N(0, 1)` via Box–Muller on [`crate::det`] functions (consumes 2 draws).
    #[must_use]
    pub fn next_normal(&mut self) -> f64 {
        let u = 1.0 - self.next_f64();
        let v = self.next_f64();
        let (_, c) = det::sin_cos(2.0 * std::f64::consts::PI * v);
        (-2.0 * det::ln(u)).sqrt() * c
    }

    /// Exponential(1) via inversion on [`crate::det::ln`] (consumes 1 draw).
    #[must_use]
    pub fn next_exponential(&mut self) -> f64 {
        -det::ln(1.0 - self.next_f64())
    }

    fn blocks_from<const L: usize>(&self, base: u64) -> [[u32; 4]; L] {
        let ctr: [[u32; 4]; L] = core::array::from_fn(|l| {
            let idx = base.wrapping_add(l as u64);
            philox_position_words(self.key, idx).0
        });
        let key_words = philox_position_words(self.key, base).1;
        philox4x32_10_batch::<L>(&ctr, key_words)
    }

    /// Bulk-fill `out` with uniform `[0, 1)` values via 8-lane batched generation, bitwise-identical
    /// to sequential [`Stream::next_f64`] calls.
    pub fn fill_f64(&mut self, out: &mut [f64]) {
        const L: usize = 8;
        let (chunks, tail) = out.as_chunks_mut::<L>();
        for chunk in chunks {
            let blocks = self.blocks_from::<L>(self.index);
            for (o, b) in chunk.iter_mut().zip(&blocks) {
                let u = (u64::from(b[1]) << 32) | u64::from(b[0]);
                *o = (u >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0);
            }
            self.index = self.index.wrapping_add(L as u64);
        }
        for o in tail {
            *o = self.next_f64();
        }
    }

    /// Bulk-fill `out` with uniform 64-bit words, bitwise-identical to sequential [`Stream::next_u64`].
    pub fn fill_u64(&mut self, out: &mut [u64]) {
        const L: usize = 8;
        let (chunks, tail) = out.as_chunks_mut::<L>();
        for chunk in chunks {
            let blocks = self.blocks_from::<L>(self.index);
            for (o, b) in chunk.iter_mut().zip(&blocks) {
                *o = (u64::from(b[1]) << 32) | u64::from(b[0]);
            }
            self.index = self.index.wrapping_add(L as u64);
        }
        for o in tail {
            *o = self.next_u64();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_matches_scalar() {
        let key = [0xA409_3822, 0x299F_31D0];
        let ctr: [[u32; 4]; 8] =
            core::array::from_fn(|l| [0x243F_6A88 ^ l as u32, 0x85A3_08D3, l as u32, 7]);
        let batched = philox4x32_10_batch::<8>(&ctr, key);
        for (l, blk) in ctr.iter().enumerate() {
            assert_eq!(batched[l], philox4x32_10(*blk, key), "lane {l}");
        }
    }

    #[test]
    fn random123_known_answers() {
        let cases: [([u32; 4], [u32; 2], [u32; 4]); 3] = [
            (
                [0, 0, 0, 0],
                [0, 0],
                [0x6627_e8d5, 0xe169_c58d, 0xbc57_ac4c, 0x9b00_dbd8],
            ),
            (
                [0xffff_ffff; 4],
                [0xffff_ffff; 2],
                [0x408f_276d, 0x41c8_3b0e, 0xa20b_c7c6, 0x6d54_51fd],
            ),
            (
                [0x243f_6a88, 0x85a3_08d3, 0x1319_8a2e, 0x0370_7344],
                [0xa409_3822, 0x299f_31d0],
                [0xd16c_fe09, 0x94fd_cceb, 0x5001_e420, 0x2412_6ea1],
            ),
        ];
        for (i, (ctr, key, want)) in cases.iter().enumerate() {
            let got = philox4x32_10(*ctr, *key);
            assert_eq!(got, *want, "KAT {i}");
        }
    }

    #[test]
    fn stream_random_access_and_bulk_fill_are_bitwise_identical() {
        let key = StreamKey {
            seed: 0x5EED_0001_DEAD_BEEF,
            kernel: 7,
            tile: 42,
        };
        let mut s1 = key.stream();
        let mut s2 = key.stream();
        let seq_f64: Vec<f64> = (0..19).map(|_| s1.next_f64()).collect();
        let mut bulk_f64 = vec![0.0; 19];
        s2.fill_f64(&mut bulk_f64);
        assert_eq!(seq_f64, bulk_f64);
        assert_eq!(s1.index(), s2.index());

        let mut s3 = key.stream();
        let seq_u64: Vec<u64> = (0..19).map(|_| s3.next_u64()).collect();
        let mut bulk_u64 = vec![0u64; 19];
        key.stream().fill_u64(&mut bulk_u64);
        assert_eq!(seq_u64, bulk_u64);

        for (i, &w) in seq_u64.iter().enumerate() {
            let b = Stream::at(key, i as u64);
            assert_eq!(w, (u64::from(b[1]) << 32) | u64::from(b[0]));
        }

        let mut s_norm = key.stream();
        let z = s_norm.next_normal();
        let e = s_norm.next_exponential();
        assert!(z.is_finite() && e > 0.0 && e.is_finite());
        assert_eq!(s_norm.index(), 3);
    }
}

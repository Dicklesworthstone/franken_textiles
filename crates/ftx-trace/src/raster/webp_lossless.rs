//! The lossless VP8L decoder of libwebp 1.6.0, for VP8L images and for VP8L-compressed alpha planes. The
//! port follows `src/dec/vp8l_dec.c`, `src/utils/huffman_utils.c`, `src/utils/bit_reader_utils.c` and
//! `src/dsp/lossless.c`. The pixels of a lossless stream are fixed by the format. The port keeps
//! libwebp's acceptance rules:
//! - incomplete Huffman codes are refused, a lone symbol is coded in zero bits;
//! - each transform appears at most once;
//! - backward references stay inside the image;
//! - the stream is padded with zeros to at least eight bytes, and reading past that is an error.
//!
//! The speed paths (packed tables, trivial codes, the 8-bit alpha path) are left out. They give the
//! same pixels.
//!
//! Provenance: libwebp 1.6.0 (BSD 3-clause; `LICENSE-libwebp.md` in this crate).

const BAD: &str = "WebP: bad lossless bitstream";
const NUM_LITERAL_CODES: usize = 256;
const NUM_LENGTH_CODES: usize = 24;
const NUM_DISTANCE_CODES: usize = 40;
const HUFFMAN_TABLE_BITS: u32 = 8;
const LENGTHS_TABLE_BITS: u32 = 7;
const MAX_ALLOWED_CODE_LENGTH: usize = 15;
const CODE_LENGTH_ORDER: [usize; 19] = [
    17, 18, 0, 1, 2, 3, 4, 5, 16, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
];
#[rustfmt::skip]
const CODE_TO_PLANE: [u8; 120] = [
    0x18, 0x07, 0x17, 0x19, 0x28, 0x06, 0x27, 0x29, 0x16, 0x1a, 0x26, 0x2a, 0x38, 0x05, 0x37, 0x39, 0x15, 0x1b,
    0x36, 0x3a, 0x25, 0x2b, 0x48, 0x04, 0x47, 0x49, 0x14, 0x1c, 0x35, 0x3b, 0x46, 0x4a, 0x24, 0x2c, 0x58, 0x45,
    0x4b, 0x34, 0x3c, 0x03, 0x57, 0x59, 0x13, 0x1d, 0x56, 0x5a, 0x23, 0x2d, 0x44, 0x4c, 0x55, 0x5b, 0x33, 0x3d,
    0x68, 0x02, 0x67, 0x69, 0x12, 0x1e, 0x66, 0x6a, 0x22, 0x2e, 0x54, 0x5c, 0x43, 0x4d, 0x65, 0x6b, 0x32, 0x3e,
    0x78, 0x01, 0x77, 0x79, 0x53, 0x5d, 0x11, 0x1f, 0x64, 0x6c, 0x42, 0x4e, 0x76, 0x7a, 0x21, 0x2f, 0x75, 0x7b,
    0x31, 0x3f, 0x63, 0x6d, 0x52, 0x5e, 0x00, 0x74, 0x7c, 0x41, 0x4f, 0x10, 0x20, 0x62, 0x6e, 0x30, 0x73, 0x7d,
    0x51, 0x5f, 0x40, 0x72, 0x7e, 0x61, 0x6f, 0x50, 0x71, 0x7f, 0x60, 0x70,
];

/// bit_reader_utils.c `VP8LBitReader`: a 64-bit window over the stream, least significant bit first.
struct BitReader<'a> {
    data: &'a [u8],
    val: u64,
    pos: usize,
    bit_pos: u32,
    eos: bool,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        let n = data.len().min(8);
        let val = data[..n]
            .iter()
            .rev()
            .fold(0u64, |v, &b| (v << 8) | u64::from(b));
        Self {
            data,
            val,
            pos: n,
            bit_pos: 0,
            eos: false,
        }
    }

    fn is_eos(&self) -> bool {
        self.eos || (self.pos == self.data.len() && self.bit_pos > 64)
    }

    fn shift_bytes(&mut self) {
        while self.bit_pos >= 8 && self.pos < self.data.len() {
            self.val = (self.val >> 8) | u64::from(self.data[self.pos]) << 56;
            self.pos += 1;
            self.bit_pos -= 8;
        }
        if self.is_eos() {
            self.eos = true;
            self.bit_pos = 0;
        }
    }

    fn prefetch(&self) -> u32 {
        (self.val >> (self.bit_pos & 63)) as u32
    }

    fn fill(&mut self) {
        if self.bit_pos >= 32 {
            self.shift_bytes();
        }
    }

    /// `VP8LReadBits`.
    fn bits(&mut self, n: u32) -> u32 {
        if !self.eos && n <= 24 {
            let v = self.prefetch() & ((1u32 << n) - 1);
            self.bit_pos += n;
            self.shift_bytes();
            v
        } else {
            self.eos = true;
            self.bit_pos = 0;
            0
        }
    }

    /// `ReadSymbol`: a two-level table lookup.
    fn symbol(&mut self, table: &[HCode]) -> usize {
        let mut val = self.prefetch();
        let mut i = (val & ((1 << HUFFMAN_TABLE_BITS) - 1)) as usize;
        let nbits = i32::from(table[i].bits) - HUFFMAN_TABLE_BITS as i32;
        if nbits > 0 {
            self.bit_pos += HUFFMAN_TABLE_BITS;
            val = self.prefetch();
            i += usize::from(table[i].value);
            i += (val & ((1 << nbits) - 1)) as usize;
        }
        self.bit_pos += u32::from(table[i].bits);
        usize::from(table[i].value)
    }
}

/// huffman_utils.h `HuffmanCode`.
#[derive(Clone, Copy, Default)]
struct HCode {
    bits: u8,
    value: u16,
}

/// huffman_utils.c `BuildHuffmanTable`: the canonical code of `lengths` as a root table of `root_bits`
/// plus second-level tables, or None for an invalid code (all zero, over-subscribed or incomplete).
fn build_table(root_bits: u32, lengths: &[usize]) -> Option<Vec<HCode>> {
    let mut count = [0usize; MAX_ALLOWED_CODE_LENGTH + 1];
    for &l in lengths {
        if l > MAX_ALLOWED_CODE_LENGTH {
            return None;
        }
        count[l] += 1;
    }
    if count[0] == lengths.len() {
        return None;
    }
    let mut offset = [0usize; MAX_ALLOWED_CODE_LENGTH + 1];
    for len in 1..MAX_ALLOWED_CODE_LENGTH {
        if count[len] > 1 << len {
            return None;
        }
        offset[len + 1] = offset[len] + count[len];
    }
    let mut sorted = vec![0u16; lengths.len()];
    for (symbol, &l) in lengths.iter().enumerate() {
        if l > 0 {
            if offset[l] >= lengths.len() {
                return None;
            }
            sorted[offset[l]] = symbol as u16;
            offset[l] += 1;
        }
    }
    let root_size = 1usize << root_bits;
    let mut table = vec![HCode::default(); root_size];
    if offset[MAX_ALLOWED_CODE_LENGTH] == 1 {
        table.fill(HCode {
            bits: 0,
            value: sorted[0],
        });
        return Some(table);
    }
    let replicate = |t: &mut [HCode], step: usize, mut end: usize, code: HCode| loop {
        end -= step;
        t[end] = code;
        if end == 0 {
            break;
        }
    };
    let next_key = |key: u32, len: usize| {
        let mut step = 1u32 << (len - 1);
        while key & step != 0 {
            step >>= 1;
        }
        if step == 0 {
            key
        } else {
            (key & (step - 1)) + step
        }
    };
    let (mut key, mut num_nodes, mut num_open) = (0u32, 1i64, 1i64);
    let mut symbol = 0usize;
    let mut step = 2usize;
    for (len, n) in count
        .iter_mut()
        .enumerate()
        .take(root_bits as usize + 1)
        .skip(1)
    {
        num_open <<= 1;
        num_nodes += num_open;
        num_open -= *n as i64;
        if num_open < 0 {
            return None;
        }
        for _ in 0..*n {
            let code = HCode {
                bits: len as u8,
                value: sorted[symbol],
            };
            symbol += 1;
            replicate(&mut table[key as usize..], step, root_size, code);
            key = next_key(key, len);
        }
        *n = 0;
        step <<= 1;
    }
    let mask = (root_size - 1) as u32;
    let mut low = u32::MAX;
    let (mut sub_start, mut sub_size) = (0usize, root_size);
    step = 2;
    for len in root_bits as usize + 1..=MAX_ALLOWED_CODE_LENGTH {
        num_open <<= 1;
        num_nodes += num_open;
        num_open -= count[len] as i64;
        if num_open < 0 {
            return None;
        }
        while count[len] > 0 {
            if key & mask != low {
                // NextTableBitSize
                let (mut l, mut left) = (len, 1i64 << (len - root_bits as usize));
                while l < MAX_ALLOWED_CODE_LENGTH {
                    left -= count[l] as i64;
                    if left <= 0 {
                        break;
                    }
                    l += 1;
                    left <<= 1;
                }
                let table_bits = l - root_bits as usize;
                sub_start = table.len();
                sub_size = 1 << table_bits;
                table.resize(sub_start + sub_size, HCode::default());
                low = key & mask;
                table[low as usize] = HCode {
                    bits: (table_bits + root_bits as usize) as u8,
                    value: (sub_start - low as usize) as u16,
                };
            }
            let code = HCode {
                bits: (len - root_bits as usize) as u8,
                value: sorted[symbol],
            };
            symbol += 1;
            // the key's high bits are below `step`, so the replicas stay inside the sub-table
            let at = sub_start + (key >> root_bits) as usize;
            replicate(&mut table[at..], step, sub_size, code);
            key = next_key(key, len);
            count[len] -= 1;
        }
        step <<= 1;
    }
    (num_nodes == 2 * offset[MAX_ALLOWED_CODE_LENGTH] as i64 - 1).then_some(table)
}

/// One meta-code's five Huffman codes: green (with lengths and cache), red, blue, alpha, distance.
struct Group {
    trees: [Vec<HCode>; 5],
}

/// vp8l_dec.c `ReadHuffmanCodeLengths` + `ReadHuffmanCode`: one code over `alphabet` symbols.
fn read_code(
    br: &mut BitReader<'_>,
    alphabet: usize,
    lengths: &mut [usize],
) -> Result<Vec<HCode>, String> {
    lengths[..alphabet].fill(0);
    if br.bits(1) == 1 {
        let num_symbols = br.bits(1) + 1;
        let first_len_code = br.bits(1);
        let symbol = br.bits(if first_len_code == 0 { 1 } else { 8 }) as usize;
        lengths[symbol] = 1;
        if num_symbols == 2 {
            lengths[br.bits(8) as usize] = 1;
        }
    } else {
        let mut cl_lengths = [0usize; 19];
        let num_codes = br.bits(4) as usize + 4;
        for &k in &CODE_LENGTH_ORDER[..num_codes] {
            cl_lengths[k] = br.bits(3) as usize;
        }
        let table = build_table(LENGTHS_TABLE_BITS, &cl_lengths).ok_or(BAD)?;
        let mut max_symbol = if br.bits(1) == 1 {
            let length_nbits = 2 + 2 * br.bits(3);
            let m = 2 + br.bits(length_nbits) as usize;
            if m > alphabet {
                return Err(BAD.into());
            }
            m
        } else {
            alphabet
        };
        let (mut symbol, mut prev) = (0usize, 8usize);
        while symbol < alphabet {
            if max_symbol == 0 {
                break;
            }
            max_symbol -= 1;
            br.fill();
            let p = table[(br.prefetch() & ((1 << LENGTHS_TABLE_BITS) - 1)) as usize];
            br.bit_pos += u32::from(p.bits);
            let code_len = usize::from(p.value);
            if code_len < 16 {
                lengths[symbol] = code_len;
                symbol += 1;
                if code_len != 0 {
                    prev = code_len;
                }
            } else {
                let slot = code_len - 16;
                let repeat = br.bits([2, 3, 7][slot]) as usize + [3, 3, 11][slot];
                if symbol + repeat > alphabet {
                    return Err(BAD.into());
                }
                let v = if code_len == 16 { prev } else { 0 };
                lengths[symbol..symbol + repeat].fill(v);
                symbol += repeat;
            }
        }
    }
    if br.eos {
        return Err(BAD.into());
    }
    build_table(HUFFMAN_TABLE_BITS, &lengths[..alphabet]).ok_or_else(|| BAD.into())
}

/// The entropy-coded image's codes: meta-code image (level 0 only), groups, colour cache size.
struct Codes {
    groups: Vec<Group>,
    meta: Option<(Vec<u32>, u32, usize)>, // (group per tile, subsample bits, tiles per row)
    cache_bits: u32,
}

/// vp8l_dec.c `ReadHuffmanCodes` + `ReadHuffmanCodesHelper`.
fn read_codes(
    br: &mut BitReader<'_>,
    xsize: usize,
    ysize: usize,
    cache_bits: u32,
    level0: bool,
) -> Result<Codes, String> {
    let mut meta = None;
    let (mut num_max, mut num_groups) = (1usize, 1usize);
    let mut mapping: Option<Vec<i64>> = None;
    if level0 && br.bits(1) == 1 {
        let bits = 2 + br.bits(3);
        let (hx, hy) = (sub_sample(xsize, bits), sub_sample(ysize, bits));
        let mut image = decode_stream(br, hx, hy, false)?;
        for p in &mut image {
            *p = (*p >> 8) & 0xffff;
            num_max = num_max.max(*p as usize + 1);
        }
        if num_max > 1000 || num_max > xsize * ysize {
            // compact the group indices in order of first use
            let mut map = vec![-1i64; num_max];
            num_groups = 0;
            for p in &mut image {
                let m = &mut map[*p as usize];
                if *m == -1 {
                    *m = num_groups as i64;
                    num_groups += 1;
                }
                *p = *m as u32;
            }
            mapping = Some(map);
        } else {
            num_groups = num_max;
        }
        meta = Some((image, bits, hx));
    }
    if br.eos {
        return Err(BAD.into());
    }
    let groups = read_groups(br, num_max, num_groups, mapping.as_deref(), cache_bits)?;
    Ok(Codes {
        groups,
        meta,
        cache_bits,
    })
}

fn read_groups(
    br: &mut BitReader<'_>,
    num_max: usize,
    num_groups: usize,
    mapping: Option<&[i64]>,
    cache_bits: u32,
) -> Result<Vec<Group>, String> {
    let cache = if cache_bits > 0 { 1 << cache_bits } else { 0 };
    let alphabets = [
        NUM_LITERAL_CODES + NUM_LENGTH_CODES + cache,
        256,
        256,
        256,
        NUM_DISTANCE_CODES,
    ];
    let mut lengths = vec![0usize; alphabets[0].max(256)];
    let mut groups: Vec<Option<Group>> = (0..num_groups).map(|_| None).collect();
    for i in 0..num_max {
        let slot = mapping.map_or(Some(i), |m| (m[i] >= 0).then(|| m[i] as usize));
        let mut trees: Vec<Vec<HCode>> = Vec::with_capacity(5);
        for &a in &alphabets {
            trees.push(read_code(br, a, &mut lengths)?);
        }
        if let Some(s) = slot {
            let trees: [Vec<HCode>; 5] = trees.try_into().map_err(|_| BAD)?;
            groups[s] = Some(Group { trees });
        }
    }
    groups
        .into_iter()
        .map(|g| g.ok_or_else(|| BAD.into()))
        .collect()
}

fn sub_sample(size: usize, bits: u32) -> usize {
    (size + (1 << bits) - 1) >> bits
}

/// vp8l_dec.c `GetCopyDistance` (and `GetCopyLength`).
fn copy_distance(symbol: usize, br: &mut BitReader<'_>) -> usize {
    if symbol < 4 {
        return symbol + 1;
    }
    let extra = (symbol as u32 - 2) >> 1;
    let offset = (2 + (symbol & 1)) << extra;
    offset + br.bits(extra) as usize + 1
}

/// vp8l_dec.c `PlaneCodeToDistance`.
fn plane_to_distance(xsize: usize, code: usize) -> usize {
    if code > 120 {
        code - 120
    } else {
        let d = usize::from(CODE_TO_PLANE[code - 1]);
        let dist = (d >> 4) as i64 * xsize as i64 + 8 - (d & 0xf) as i64;
        dist.max(1) as usize
    }
}

/// vp8l_dec.c `DecodeImageData`: the entropy-coded ARGB pixels of an `xsize`×`ysize` image.
fn decode_pixels(
    br: &mut BitReader<'_>,
    codes: &Codes,
    xsize: usize,
    ysize: usize,
) -> Result<Vec<u32>, String> {
    let total = xsize * ysize;
    let mut data = vec![0u32; total];
    let cache_size = if codes.cache_bits > 0 {
        1usize << codes.cache_bits
    } else {
        0
    };
    let mut cache = vec![0u32; cache_size];
    let insert = |cache: &mut [u32], argb: u32| {
        if !cache.is_empty() {
            let key = (0x1e35_a7bd_u32.wrapping_mul(argb) >> (32 - codes.cache_bits)) as usize;
            cache[key] = argb;
        }
    };
    let group_at = |col: usize, row: usize| -> usize {
        match &codes.meta {
            Some((image, bits, hx)) => image[(row >> bits) * hx + (col >> bits)] as usize,
            None => 0,
        }
    };
    let mask = codes
        .meta
        .as_ref()
        .map_or(usize::MAX, |m| (1usize << m.1) - 1);
    let (mut src, mut col, mut row) = (0usize, 0usize, 0usize);
    let mut group = &codes.groups[group_at(0, 0)];
    let len_limit = NUM_LITERAL_CODES + NUM_LENGTH_CODES;
    while src < total {
        if col & mask == 0 {
            group = &codes.groups[group_at(col, row)];
        }
        br.fill();
        let code = br.symbol(&group.trees[0]);
        if br.is_eos() {
            break;
        }
        if code < NUM_LITERAL_CODES {
            let red = br.symbol(&group.trees[1]);
            br.fill();
            let blue = br.symbol(&group.trees[2]);
            let alpha = br.symbol(&group.trees[3]);
            if br.is_eos() {
                break;
            }
            let argb = (alpha as u32) << 24 | (red as u32) << 16 | (code as u32) << 8 | blue as u32;
            data[src] = argb;
            insert(&mut cache, argb);
            src += 1;
            col += 1;
            if col >= xsize {
                col = 0;
                row += 1;
            }
        } else if code < len_limit {
            let length = copy_distance(code - NUM_LITERAL_CODES, br);
            let dist_symbol = br.symbol(&group.trees[4]);
            br.fill();
            let dist = plane_to_distance(xsize, copy_distance(dist_symbol, br));
            if br.is_eos() {
                break;
            }
            if src < dist || total - src < length {
                return Err(BAD.into());
            }
            for i in src..src + length {
                data[i] = data[i - dist];
                insert(&mut cache, data[i]);
            }
            src += length;
            col += length;
            while col >= xsize {
                col -= xsize;
                row += 1;
            }
            if col & mask != 0 {
                group = &codes.groups[group_at(col, row)];
            }
        } else if code < len_limit + cache_size {
            let argb = cache[code - len_limit];
            data[src] = argb;
            insert(&mut cache, argb);
            src += 1;
            col += 1;
            if col >= xsize {
                col = 0;
                row += 1;
            }
        } else {
            return Err(BAD.into());
        }
    }
    br.eos = br.is_eos();
    if br.eos {
        return Err("WebP: the lossless bitstream is truncated".into());
    }
    Ok(data)
}

/// One inverse transform (vp8l_dec.c `ReadTransform`, lossless.c `VP8LInverseTransform`).
enum Transform {
    Predictor {
        bits: u32,
        xsize: usize,
        data: Vec<u32>,
    },
    CrossColor {
        bits: u32,
        xsize: usize,
        data: Vec<u32>,
    },
    SubtractGreen,
    ColorIndexing {
        bits: u32,
        xsize: usize,
        palette: Vec<u32>,
    },
}

/// vp8l_dec.c `DecodeImageStream`: transforms (level 0 only), colour cache, codes and pixels; the result
/// after the inverse transforms.
fn decode_stream(
    br: &mut BitReader<'_>,
    xsize: usize,
    ysize: usize,
    level0: bool,
) -> Result<Vec<u32>, String> {
    let mut transforms = Vec::new();
    let mut seen = 0u32;
    let mut width = xsize;
    if level0 {
        while br.bits(1) == 1 {
            let kind = br.bits(2);
            if seen & (1 << kind) != 0 {
                return Err(BAD.into());
            }
            seen |= 1 << kind;
            transforms.push(match kind {
                0 | 1 => {
                    let bits = 2 + br.bits(3);
                    let data =
                        decode_stream(br, sub_sample(width, bits), sub_sample(ysize, bits), false)?;
                    if kind == 0 {
                        Transform::Predictor {
                            bits,
                            xsize: width,
                            data,
                        }
                    } else {
                        Transform::CrossColor {
                            bits,
                            xsize: width,
                            data,
                        }
                    }
                }
                2 => Transform::SubtractGreen,
                _ => {
                    let num_colors = br.bits(8) as usize + 1;
                    let bits = match num_colors {
                        17.. => 0,
                        5..=16 => 1,
                        3..=4 => 2,
                        _ => 3,
                    };
                    let raw = decode_stream(br, num_colors, 1, false)?;
                    // ExpandColorMap: byte-wise deltas, a black tail up to the indexable size
                    let mut palette = vec![0u32; 1 << (8 >> bits)];
                    palette[0] = raw[0];
                    for i in 1..num_colors {
                        let (p, d) = (palette[i - 1].to_le_bytes(), raw[i].to_le_bytes());
                        palette[i] =
                            u32::from_le_bytes(std::array::from_fn(|k| p[k].wrapping_add(d[k])));
                    }
                    let t = Transform::ColorIndexing {
                        bits,
                        xsize: width,
                        palette,
                    };
                    width = sub_sample(width, bits);
                    t
                }
            });
        }
    }
    let mut cache_bits = 0;
    if br.bits(1) == 1 {
        cache_bits = br.bits(4);
        if !(1..=11).contains(&cache_bits) {
            return Err(BAD.into());
        }
    }
    let codes = read_codes(br, width, ysize, cache_bits, level0)?;
    let mut data = decode_pixels(br, &codes, width, ysize)?;
    for t in transforms.iter().rev() {
        data = inverse_transform(t, &data, ysize);
    }
    Ok(data)
}

/// lossless_common.h `VP8LAddPixels`: per-channel sum mod 256.
fn add_pixels(a: u32, b: u32) -> u32 {
    let ag = (a & 0xff00_ff00).wrapping_add(b & 0xff00_ff00);
    let rb = (a & 0x00ff_00ff).wrapping_add(b & 0x00ff_00ff);
    (ag & 0xff00_ff00) | (rb & 0x00ff_00ff)
}

fn average2(a: u32, b: u32) -> u32 {
    (((a ^ b) & 0xfefe_fefe) >> 1) + (a & b)
}

/// lossless.c `Select`.
fn select(a: u32, b: u32, c: u32) -> u32 {
    let sub3 = |s: u32| {
        let (a, b, c) = (
            ((a >> s) & 0xff) as i32,
            ((b >> s) & 0xff) as i32,
            ((c >> s) & 0xff) as i32,
        );
        (b - c).abs() - (a - c).abs()
    };
    if sub3(24) + sub3(16) + sub3(8) + sub3(0) <= 0 {
        a
    } else {
        b
    }
}

fn clip255(a: i32) -> u32 {
    if (0..256).contains(&a) {
        a as u32
    } else {
        !(a as u32) >> 24
    }
}

/// lossless.c `ClampedAddSubtractFull` / `ClampedAddSubtractHalf`.
fn clamped_full(c0: u32, c1: u32, c2: u32) -> u32 {
    [24, 16, 8, 0].iter().fold(0, |acc, &s| {
        let ch = |c: u32| ((c >> s) & 0xff) as i32;
        acc | clip255(ch(c0) + ch(c1) - ch(c2)) << s
    })
}

fn clamped_half(c0: u32, c1: u32, c2: u32) -> u32 {
    let ave = average2(c0, c1);
    [24, 16, 8, 0].iter().fold(0, |acc, &s| {
        let (a, b) = (((ave >> s) & 0xff) as i32, ((c2 >> s) & 0xff) as i32);
        acc | clip255(a + (a - b) / 2) << s
    })
}

/// lossless.c's predictors 0–13 (14 and 15 are 0), with `out` holding the image so far: the pixel at `i`
/// sees its left neighbour at `i − 1` and the row above from `i − width` (the top-right of the last
/// column is the current row's first pixel, as in libwebp's contiguous buffer).
fn predict(mode: u32, out: &[u32], i: usize, width: usize) -> u32 {
    let left = out[i - 1];
    let top = |k: isize| out[(i as isize - width as isize + k) as usize];
    match mode {
        1 => left,
        2 => top(0),
        3 => top(1),
        4 => top(-1),
        5 => average2(average2(left, top(1)), top(0)),
        6 => average2(left, top(-1)),
        7 => average2(left, top(0)),
        8 => average2(top(-1), top(0)),
        9 => average2(top(0), top(1)),
        10 => average2(average2(left, top(-1)), average2(top(0), top(1))),
        11 => select(top(0), left, top(-1)),
        12 => clamped_full(left, top(0), top(-1)),
        13 => clamped_half(left, top(0), top(-1)),
        _ => 0xff00_0000,
    }
}

fn inverse_transform(t: &Transform, input: &[u32], ysize: usize) -> Vec<u32> {
    match t {
        Transform::SubtractGreen => input
            .iter()
            .map(|&argb| {
                let green = (argb >> 8) & 0xff;
                let rb = ((argb & 0x00ff_00ff) + ((green << 16) | green)) & 0x00ff_00ff;
                (argb & 0xff00_ff00) | rb
            })
            .collect(),
        Transform::Predictor { bits, xsize, data } => {
            let w = *xsize;
            let mut out = vec![0u32; w * ysize];
            let tiles = sub_sample(w, *bits);
            for y in 0..ysize {
                for x in 0..w {
                    let i = y * w + x;
                    let pred = if y == 0 {
                        if x == 0 { 0xff00_0000 } else { out[i - 1] }
                    } else if x == 0 {
                        out[i - w]
                    } else {
                        let mode = (data[(y >> bits) * tiles + (x >> bits)] >> 8) & 0xf;
                        predict(mode, &out, i, w)
                    };
                    out[i] = add_pixels(input[i], pred);
                }
            }
            out
        }
        Transform::CrossColor { bits, xsize, data } => {
            let w = *xsize;
            let tiles = sub_sample(w, *bits);
            let delta = |pred: u32, color: u32| {
                ((pred as u8 as i8 as i32) * (color as u8 as i8 as i32)) >> 5
            };
            input
                .iter()
                .enumerate()
                .map(|(i, &argb)| {
                    let (x, y) = (i % w, i / w);
                    let code = data[(y >> bits) * tiles + (x >> bits)];
                    let (g2r, g2b, r2b) = (code & 0xff, (code >> 8) & 0xff, (code >> 16) & 0xff);
                    let green = (argb >> 8) & 0xff;
                    let mut red = ((argb >> 16) & 0xff) as i32;
                    let mut blue = (argb & 0xff) as i32;
                    red = (red + delta(g2r, green)) & 0xff;
                    blue += delta(g2b, green);
                    blue += delta(r2b, red as u32);
                    blue &= 0xff;
                    (argb & 0xff00_ff00) | (red as u32) << 16 | blue as u32
                })
                .collect()
        }
        Transform::ColorIndexing {
            bits,
            xsize,
            palette,
        } => {
            let w = *xsize;
            let packed_w = sub_sample(w, *bits);
            let bits_per_pixel = 8 >> bits;
            let pixel_mask = (1u32 << bits_per_pixel) - 1;
            let mut out = Vec::with_capacity(w * ysize);
            for y in 0..ysize {
                let row = &input[y * packed_w..(y + 1) * packed_w];
                for x in 0..w {
                    let index = if *bits == 0 {
                        (row[x] >> 8) & 0xff
                    } else {
                        let packed = (row[x >> bits] >> 8) & 0xff;
                        (packed >> ((x & ((1 << bits) - 1)) as u32 * bits_per_pixel)) & pixel_mask
                    };
                    out.push(palette[index as usize]);
                }
            }
            out
        }
    }
}

/// A VP8L chunk payload (with its 5-byte header) decoded to ARGB; the header's size must be `w`×`h`.
pub(super) fn decode_image(data: &[u8], w: usize, h: usize) -> Result<Vec<u32>, String> {
    let mut br = BitReader::new(data);
    if br.bits(8) != 0x2f {
        return Err(BAD.into());
    }
    let (iw, ih) = (br.bits(14) as usize + 1, br.bits(14) as usize + 1);
    let _alpha = br.bits(1);
    if br.bits(3) != 0 || br.eos || (iw, ih) != (w, h) {
        return Err(BAD.into());
    }
    decode_stream(&mut br, w, h, true)
}

/// A VP8L-compressed alpha plane (an image stream without header, vp8l_dec.c `VP8LDecodeAlphaHeader`):
/// the green channel of the decoded image.
pub(super) fn decode_alpha(data: &[u8], w: usize, h: usize) -> Result<Vec<u8>, String> {
    let mut br = BitReader::new(data);
    let argb = decode_stream(&mut br, w, h, true)?;
    Ok(argb.iter().map(|&p| (p >> 8) as u8).collect())
}

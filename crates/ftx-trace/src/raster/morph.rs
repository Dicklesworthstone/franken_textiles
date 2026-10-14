//! OpenCV 5.0 morphology on 8-bit images (`morph.dispatch.cpp`): elliptical structuring elements, erosion and
//! dilation with OpenCV's default border (pixels outside the image never count), and `morphologyEx`'s open,
//! close and gradient. Minima and maxima round nothing, so any correct implementation gives OpenCV's bytes.

use super::image::Gray;

/// A structuring element: `w × h` flags, anchor at the centre.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Kernel {
    /// Width.
    pub w: usize,
    /// Height.
    pub h: usize,
    /// Row-major flags.
    pub on: Vec<bool>,
}

impl Kernel {
    /// `getStructuringElement(MORPH_ELLIPSE, (w, h))`.
    #[must_use]
    pub fn ellipse(w: usize, h: usize) -> Self {
        let (r, c) = ((h / 2) as i64, (w / 2) as i64);
        let inv_r2 = if r == 0 { 0.0 } else { 1.0 / (r * r) as f64 };
        let mut on = vec![false; w * h];
        for i in 0..h {
            let dy = i as i64 - r;
            if dy.abs() <= r {
                let dx = (c as f64 * (((r * r - dy * dy) as f64) * inv_r2).sqrt()).round_ties_even()
                    as i64;
                let (j1, j2) = (
                    (c - dx).max(0) as usize,
                    ((c + dx + 1).min(w as i64)) as usize,
                );
                on[i * w + j1..i * w + j2]
                    .iter_mut()
                    .for_each(|v| *v = true);
            }
        }
        Self { w, h, on }
    }

    /// `np.ones((w, h))`.
    #[must_use]
    pub fn rect(w: usize, h: usize) -> Self {
        Self {
            w,
            h,
            on: vec![true; w * h],
        }
    }

    /// The runs `(dy, dx0, dx1)` of set flags per row, relative to the anchor (`dx1` exclusive).
    fn runs(&self) -> Vec<(i64, i64, i64)> {
        let (ay, ax) = ((self.h / 2) as i64, (self.w / 2) as i64);
        let mut out = Vec::new();
        for i in 0..self.h {
            let mut j = 0;
            while j < self.w {
                if self.on[i * self.w + j] {
                    let start = j;
                    while j < self.w && self.on[i * self.w + j] {
                        j += 1;
                    }
                    out.push((i as i64 - ay, start as i64 - ax, j as i64 - ax));
                } else {
                    j += 1;
                }
            }
        }
        out
    }
}

/// Scratch rows for [`windows`].
#[derive(Default)]
struct Scratch {
    padded: Vec<u8>,
    forward: Vec<u8>,
    backward: Vec<u8>,
}

/// `out[x]` = the minimum (maximum when dilating) of `row[x + dx0 .. x + dx1]`, pixels outside the row not
/// counting (they read as the neutral value), by van Herk / Gil-Werman: running extremes forward and
/// backward within blocks of the window's length give every window in three passes, whatever its length.
fn windows(row: &[u8], dx0: i64, dx1: i64, dilate: bool, s: &mut Scratch, out: &mut [u8]) {
    let op = |a: u8, b: u8| if dilate { a.max(b) } else { a.min(b) };
    let neutral = if dilate { 0 } else { 255 };
    let (w, len) = (row.len() as i64, (dx1 - dx0) as usize);
    // padded[i] is row[dx0 + i]; the window of x is padded[x .. x + len]
    s.padded.clear();
    s.padded.extend((0..row.len() + len - 1).map(|i| {
        let j = dx0 + i as i64;
        if (0..w).contains(&j) {
            row[j as usize]
        } else {
            neutral
        }
    }));
    let n = s.padded.len();
    s.forward.resize(n, 0);
    s.backward.resize(n, 0);
    for i in 0..n {
        let v = s.padded[i];
        s.forward[i] = if i % len == 0 {
            v
        } else {
            op(s.forward[i - 1], v)
        };
    }
    for i in (0..n).rev() {
        let v = s.padded[i];
        s.backward[i] = if i % len == len - 1 || i == n - 1 {
            v
        } else {
            op(s.backward[i + 1], v)
        };
    }
    for (x, o) in out.iter_mut().enumerate() {
        *o = op(s.backward[x], s.forward[x + len - 1]);
    }
}

/// Erosion or dilation: per output row, the extreme over each kernel row's run of the input row it
/// reaches. Minima and maxima round nothing, so this is OpenCV's result bit for bit; outside the image
/// nothing counts, and a pixel whose kernel reaches no pixel takes the default border (255 for erosion,
/// 0 for dilation).
fn apply(img: &Gray, k: &Kernel, dilate: bool) -> Gray {
    if img.w > 0 && img.data.iter().all(|&v| v == 0 || v == 255) {
        return apply_binary(img, k, dilate);
    }
    let (w, h) = (img.w, img.h);
    let neutral = if dilate { 0 } else { 255 };
    let runs = k.runs();
    let mut data = vec![neutral; img.data.len()];
    let (mut scratch, mut win) = (Scratch::default(), vec![0u8; w]);
    for y in 0..h {
        let out = &mut data[y * w..(y + 1) * w];
        for &(dy, dx0, dx1) in &runs {
            let yy = y as i64 + dy;
            if !(0..h as i64).contains(&yy) {
                continue;
            }
            let yy = yy as usize;
            windows(
                &img.data[yy * w..(yy + 1) * w],
                dx0,
                dx1,
                dilate,
                &mut scratch,
                &mut win,
            );
            for (o, &v) in out.iter_mut().zip(&win) {
                *o = if dilate { (*o).max(v) } else { (*o).min(v) };
            }
        }
    }
    Gray {
        w: img.w,
        h: img.h,
        data,
    }
}

/// Positions in a row, as sorted, disjoint `[start, end)` runs.
type Runs = Vec<(i64, i64)>;

/// Beyond any row: a run reaching past the image on either side.
const FAR: i64 = 1 << 40;

/// [`apply`] on an image of 0s and 255s, by runs of 255: an erosion keeps the positions whose window lies
/// inside a run (pixels outside the image count as 255), a dilation those whose window meets one; the
/// kernel rows' results are then intersected (erosion) or united (dilation). Minima and maxima of 0s and
/// 255s are exactly these, so the bytes equal the general path's, in time proportional to the runs.
fn apply_binary(img: &Gray, k: &Kernel, dilate: bool) -> Gray {
    let (w, h) = (img.w as i64, img.h as i64);
    let kernel_runs = k.runs();
    let rows: Vec<Runs> = img.data.chunks_exact(img.w).map(ink_runs).collect();
    let mut data = vec![0u8; img.data.len()];
    for (y, out) in data.chunks_exact_mut(img.w).enumerate() {
        let mut acc: Option<Runs> = None;
        for &(dy, dx0, dx1) in &kernel_runs {
            let yy = y as i64 + dy;
            if !(0..h).contains(&yy) {
                continue;
            }
            let row = &rows[yy as usize];
            let moved = if dilate {
                grow(row, dx0, dx1, w)
            } else {
                shrink(row, dx0, dx1, w)
            };
            acc = Some(match acc {
                None => moved,
                Some(a) if dilate => unite(&a, &moved),
                Some(a) => intersect(&a, &moved),
            });
        }
        // no kernel row reaching the image: the default border
        let set = acc.unwrap_or_else(|| if dilate { Vec::new() } else { vec![(0, w)] });
        for (s, e) in set {
            out[s as usize..e as usize].fill(255);
        }
    }
    Gray {
        w: img.w,
        h: img.h,
        data,
    }
}

/// The runs of 255 in a row of 0s and 255s.
fn ink_runs(row: &[u8]) -> Runs {
    let mut runs = Vec::new();
    let mut x = 0;
    while x < row.len() {
        if row[x] == 255 {
            let s = x;
            while x < row.len() && row[x] == 255 {
                x += 1;
            }
            runs.push((s as i64, x as i64));
        } else {
            x += 1;
        }
    }
    runs
}

/// Erosion of one row by the window `[x + dx0, x + dx1)`: the outside of the image extends the runs
/// that touch it, then each run `[a, b)` keeps `[a - dx0, b - dx1 + 1)`; clipped to the row.
fn shrink(runs: &Runs, dx0: i64, dx1: i64, w: i64) -> Runs {
    let mut ext = runs.clone();
    match ext.first_mut() {
        Some(first) if first.0 == 0 => first.0 = -FAR,
        _ => ext.insert(0, (-FAR, 0)),
    }
    match ext.last_mut() {
        Some(last) if last.1 == w => last.1 = FAR,
        _ => ext.push((w, FAR)),
    }
    ext.iter()
        .map(|&(a, b)| ((a - dx0).max(0), (b - dx1 + 1).min(w)))
        .filter(|&(a, b)| a < b)
        .collect()
}

/// Dilation of one row by the window `[x + dx0, x + dx1)`: each run `[a, b)` reaches
/// `[a - dx1 + 1, b - dx0)`, merged and clipped to the row.
fn grow(runs: &Runs, dx0: i64, dx1: i64, w: i64) -> Runs {
    let mut out: Runs = Vec::with_capacity(runs.len());
    for &(a, b) in runs {
        let (s, e) = ((a - dx1 + 1).max(0), (b - dx0).min(w));
        if s >= e {
            continue;
        }
        match out.last_mut() {
            Some(last) if s <= last.1 => last.1 = last.1.max(e),
            _ => out.push((s, e)),
        }
    }
    out
}

fn intersect(a: &Runs, b: &Runs) -> Runs {
    let (mut i, mut j, mut out) = (0, 0, Vec::new());
    while i < a.len() && j < b.len() {
        let (s, e) = (a[i].0.max(b[j].0), a[i].1.min(b[j].1));
        if s < e {
            out.push((s, e));
        }
        if a[i].1 < b[j].1 {
            i += 1;
        } else {
            j += 1;
        }
    }
    out
}

fn unite(a: &Runs, b: &Runs) -> Runs {
    let mut all: Runs = a.iter().chain(b).copied().collect();
    all.sort_unstable();
    let mut out: Runs = Vec::with_capacity(all.len());
    for (s, e) in all {
        match out.last_mut() {
            Some(last) if s <= last.1 => last.1 = last.1.max(e),
            _ => out.push((s, e)),
        }
    }
    out
}

/// `erode(img, k)`.
#[must_use]
pub fn erode(img: &Gray, k: &Kernel) -> Gray {
    apply(img, k, false)
}

/// `dilate(img, k)`.
#[must_use]
pub fn dilate(img: &Gray, k: &Kernel) -> Gray {
    apply(img, k, true)
}

/// `morphologyEx(img, MORPH_CLOSE, k)`: dilate, then erode.
#[must_use]
pub fn close(img: &Gray, k: &Kernel) -> Gray {
    erode(&dilate(img, k), k)
}

/// `morphologyEx(img, MORPH_OPEN, k)`: erode, then dilate.
#[must_use]
pub fn open(img: &Gray, k: &Kernel) -> Gray {
    dilate(&erode(img, k), k)
}

/// `close(&open(img, k), k)`, computed over the bounding box of the image's nonzero pixels widened by twice
/// the kernel's reach (and a little): outside it every pass leaves zeros. Inside it the crop changes nothing:
/// a dilation counts missing pixels no more than zeros, and an erosion window that leaves the crop crosses its
/// border, which is still zero (the two dilations grow the shape by less than the margin).
#[must_use]
pub fn open_close(img: &Gray, k: &Kernel) -> Gray {
    let (w, h) = (img.w, img.h);
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for (y, row) in img.data.chunks_exact(w.max(1)).enumerate() {
        if let (Some(a), Some(b)) = (
            row.iter().position(|&v| v != 0),
            row.iter().rposition(|&v| v != 0),
        ) {
            (x0, x1, y0, y1) = (x0.min(a), x1.max(b + 1), y0.min(y), y1.max(y + 1));
        }
    }
    let mut data = vec![0u8; w * h];
    if x0 < x1 {
        let m = 2 * k.w.max(k.h) + 2;
        let (cx0, cy0, cx1, cy1) = (
            x0.saturating_sub(m),
            y0.saturating_sub(m),
            (x1 + m).min(w),
            (y1 + m).min(h),
        );
        let cw = cx1 - cx0;
        let crop = Gray {
            w: cw,
            h: cy1 - cy0,
            data: (cy0..cy1)
                .flat_map(|y| img.data[y * w + cx0..y * w + cx1].iter().copied())
                .collect(),
        };
        let done = close(&open(&crop, k), k);
        for (row, y) in done.data.chunks_exact(cw).zip(cy0..cy1) {
            data[y * w + cx0..y * w + cx1].copy_from_slice(row);
        }
    }
    Gray {
        w: img.w,
        h: img.h,
        data,
    }
}

/// `morphologyEx(img, MORPH_GRADIENT, k)`: dilation minus erosion.
#[must_use]
pub fn gradient(img: &Gray, k: &Kernel) -> Gray {
    let (d, e) = (dilate(img, k), erode(img, k));
    let data = d
        .data
        .iter()
        .zip(&e.data)
        .map(|(a, b)| a.saturating_sub(*b))
        .collect();
    Gray {
        w: img.w,
        h: img.h,
        data,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The definition, pixel by pixel: the extreme over every in-image pixel the kernel reaches.
    fn naive(img: &Gray, k: &Kernel, dilate: bool) -> Gray {
        let (w, h) = (img.w as i64, img.h as i64);
        let runs = k.runs();
        let mut data = vec![0u8; img.data.len()];
        for y in 0..h {
            for x in 0..w {
                let mut acc: Option<u8> = None;
                for &(dy, dx0, dx1) in &runs {
                    let yy = y + dy;
                    let (a, b) = ((x + dx0).max(0), (x + dx1).min(w));
                    if yy < 0 || yy >= h || a >= b {
                        continue;
                    }
                    for &v in &img.data[(yy * w + a) as usize..(yy * w + b) as usize] {
                        acc = Some(acc.map_or(v, |c| if dilate { c.max(v) } else { c.min(v) }));
                    }
                }
                data[(y * w + x) as usize] = acc.unwrap_or(if dilate { 0 } else { 255 });
            }
        }
        Gray {
            w: img.w,
            h: img.h,
            data,
        }
    }

    #[test]
    fn windowed_extremes_equal_the_definition() {
        let mut state = 0x9E37_79B9_7F4A_7C15_u64;
        let mut next = |m: u64| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state % m
        };
        for case in 0..400 {
            let (w, h) = (1 + next(40) as usize, 1 + next(30) as usize);
            // sparse ink, dense ink and noise
            let density = [8, 128, 256][case % 3];
            let data = (0..w * h)
                .map(|_| {
                    if next(256) < density {
                        next(256) as u8
                    } else {
                        255
                    }
                })
                .collect();
            let img = Gray { w, h, data };
            let (kw, kh) = (1 + next(45) as usize, 1 + next(45) as usize);
            let k = if case % 4 == 0 {
                Kernel::rect(kw, kh)
            } else {
                Kernel::ellipse(kw, kh)
            };
            for dilate in [false, true] {
                assert_eq!(
                    apply(&img, &k, dilate),
                    naive(&img, &k, dilate),
                    "case {case}"
                );
            }
        }
    }

    #[test]
    fn runs_of_ink_erode_and_dilate_as_the_definition() {
        let mut state = 0x1B87_3593_CC9E_2D51_u64;
        let mut next = |m: u64| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state % m
        };
        for case in 0..600 {
            let (w, h) = (1 + next(50) as usize, 1 + next(40) as usize);
            // pixel noise, filled blobs (touching the border too), all ink, no ink
            let mut data = vec![if case % 7 == 5 { 255 } else { 0u8 }; w * h];
            match case % 7 {
                0 | 1 => data
                    .iter_mut()
                    .for_each(|v| *v = if next(3) == 0 { 0 } else { 255 }),
                2..=4 => {
                    for _ in 0..1 + next(5) {
                        let (bx, by) = (next(w as u64) as usize, next(h as u64) as usize);
                        let (bw, bh) = (1 + next(25) as usize, 1 + next(25) as usize);
                        for y in by..(by + bh).min(h) {
                            for x in bx..(bx + bw).min(w) {
                                data[y * w + x] = 255;
                            }
                        }
                    }
                }
                _ => {}
            }
            let img = Gray { w, h, data };
            let (kw, kh) = (1 + next(35) as usize, 1 + next(35) as usize);
            let k = if case % 3 == 0 {
                Kernel::rect(kw, kh)
            } else {
                Kernel::ellipse(kw, kh)
            };
            for dilate in [false, true] {
                assert_eq!(
                    apply(&img, &k, dilate),
                    naive(&img, &k, dilate),
                    "case {case} dilate {dilate}"
                );
            }
        }
    }

    #[test]
    fn opening_then_closing_within_the_ink_equals_the_whole_image() {
        let mut state = 0x2545_F491_4F6C_DD1D_u64;
        let mut next = |m: u64| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state % m
        };
        for case in 0..300 {
            let (w, h) = (1 + next(90) as usize, 1 + next(70) as usize);
            let mut data = vec![0u8; w * h];
            // a few filled blobs anywhere (touching the border too), or nothing at all
            for _ in 0..next(4) {
                let (bx, by) = (next(w as u64) as usize, next(h as u64) as usize);
                let (bw, bh) = (1 + next(30) as usize, 1 + next(30) as usize);
                for y in by..(by + bh).min(h) {
                    for x in bx..(bx + bw).min(w) {
                        data[y * w + x] = if next(9) == 0 { 0 } else { 255 };
                    }
                }
            }
            let img = Gray { w, h, data };
            let s = 1 + next(15) as usize;
            let k = if case % 5 == 0 {
                Kernel::rect(s, 1 + next(15) as usize)
            } else {
                Kernel::ellipse(s, s)
            };
            assert_eq!(
                open_close(&img, &k),
                close(&open(&img, &k), &k),
                "case {case}"
            );
        }
    }
}

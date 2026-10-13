//! The numpy semantics the reference's numbers depend on (`docs/spec/01` §13), reproduced bit for
//! bit where numpy's own evaluation order is fixed (checked against numpy 2.5.4).

/// numpy `linspace(start, stop, n)`: `start + j·step` with `step = (stop − start)/(n − 1)`, the last
/// point forced to `stop`. Not `j/(n−1)`: the two differ by an ULP on many points.
#[must_use]
pub fn linspace(start: f64, stop: f64, n: usize) -> Vec<f64> {
    match n {
        0 => Vec::new(),
        1 => vec![start],
        _ => {
            let step = (stop - start) / (n - 1) as f64;
            let mut y: Vec<f64> = (0..n).map(|j| j as f64 * step + start).collect();
            y[n - 1] = stop;
            y
        }
    }
}

/// numpy `arange(start, stop, step)` for float64: `⌈(stop − start)/step⌉` values (none when that is not
/// positive); element 1 is `start + step` and element i ≥ 2 is `start + i·d` with
/// `d = (start + step) − start`, numpy's `DOUBLE_fill`, which can differ from `start + i·step` by an ULP.
#[must_use]
pub fn arange(start: f64, stop: f64, step: f64) -> Vec<f64> {
    let n = ((stop - start) / step).ceil();
    if n.is_nan() || n <= 0.0 {
        return Vec::new();
    }
    let n = n as usize;
    let mut y = Vec::with_capacity(n);
    y.push(start);
    if n > 1 {
        let second = start + step;
        y.push(second);
        let d = second - start;
        y.extend((2..n).map(|i| start + i as f64 * d));
    }
    y
}

/// numpy `percentile(values, p)` with the default linear method: virtual index
/// `(n·q + (1 − q)) − 1` for `q = p/100`, then numpy's two-sided lerp between the neighbouring order
/// statistics (`a + (b − a)·γ`, or `b − (b − a)·(1 − γ)` when γ ≥ 0.5). NaN for an empty slice; NaNs
/// in `values` sort last, as in numpy.
#[must_use]
pub fn percentile(values: &[f64], p: f64) -> f64 {
    let n = values.len();
    if n == 0 {
        return f64::NAN;
    }
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    let q = p / 100.0;
    // numpy's `linear` method has its own virtual index, (n − 1)·q (not the α = β = 1 general formula,
    // which can differ in the last bit)
    let virtual_index = (n - 1) as f64 * q;
    if virtual_index >= (n - 1) as f64 {
        return v[n - 1];
    }
    if virtual_index < 0.0 {
        return v[0];
    }
    let below = virtual_index.floor();
    let gamma = virtual_index - below;
    let (a, b) = (v[below as usize], v[below as usize + 1]);
    let diff = b - a;
    if gamma >= 0.5 {
        b - diff * (1.0 - gamma)
    } else {
        a + diff * gamma
    }
}

/// numpy's pairwise summation (`np.sum` / `add.reduce` on a contiguous float64 array): plain
/// accumulation below 8 elements, eight interleaved accumulators up to 128, recursive halving
/// (rounded down to a multiple of 8) above. Matches `np.sum` bit for bit.
#[must_use]
pub fn pairwise_sum(a: &[f64]) -> f64 {
    let n = a.len();
    if n < 8 {
        let mut res = 0.0;
        for &v in a {
            res += v;
        }
        res
    } else if n <= 128 {
        let mut r = [0.0; 8];
        r.copy_from_slice(&a[..8]);
        let mut i = 8;
        while i < n - (n % 8) {
            for (j, acc) in r.iter_mut().enumerate() {
                *acc += a[i + j];
            }
            i += 8;
        }
        let mut res = ((r[0] + r[1]) + (r[2] + r[3])) + ((r[4] + r[5]) + (r[6] + r[7]));
        while i < n {
            res += a[i];
            i += 1;
        }
        res
    } else {
        let mut n2 = n / 2;
        n2 -= n2 % 8;
        pairwise_sum(&a[..n2]) + pairwise_sum(&a[n2..])
    }
}

/// numpy `cumsum`: running sum in order.
#[must_use]
pub fn cumsum(a: &[f64]) -> Vec<f64> {
    let mut acc = 0.0;
    a.iter()
        .map(|&v| {
            acc += v;
            acc
        })
        .collect()
}

/// numpy `interp(x, xp, fp)` for one `x`, with the default `left = fp[0]`, `right = fp[last]`.
///
/// `xp` must be non-decreasing. The bracketing index is the last `j` with `xp[j] ≤ x`, so at a
/// repeated abscissa the value is the last of the repeated samples (`interp(1, [0,1,1,2],
/// [0,5,7,9]) = 7`). An exact hit returns `fp[j]`; otherwise `slope·(x − xp[j]) + fp[j]`, retried
/// from the right end if that is NaN, as numpy does. The multiply-add is fused: the reference's
/// numpy (arm64, clang's default `-ffp-contract=on`) compiles `slope*(x - dx[j]) + dy[j]` to an FMA,
/// which 3000 of 3000 random cases confirm (the unfused form matches 2539).
///
/// # Panics
/// If `xp` is empty or `xp` and `fp` differ in length.
#[must_use]
pub fn interp(x: f64, xp: &[f64], fp: &[f64]) -> f64 {
    assert!(
        !xp.is_empty() && xp.len() == fp.len(),
        "interp: xp and fp must be equal, non-empty"
    );
    let n = xp.len();
    if x.is_nan() {
        return x;
    }
    if x < xp[0] {
        return fp[0];
    }
    if x > xp[n - 1] {
        return fp[n - 1];
    }
    // Last j with xp[j] <= x (x >= xp[0] here, so j exists).
    let j = xp.partition_point(|&v| v <= x) - 1;
    if j == n - 1 || xp[j] == x {
        return fp[j];
    }
    let slope = (fp[j + 1] - fp[j]) / (xp[j + 1] - xp[j]);
    let r = slope.mul_add(x - xp[j], fp[j]);
    if !r.is_nan() {
        return r;
    }
    let r = slope.mul_add(x - xp[j + 1], fp[j + 1]);
    if r.is_nan() && fp[j] == fp[j + 1] {
        fp[j]
    } else {
        r
    }
}

/// numpy's float ordering for sorts: NaN sorts last (`a < b || (b != b && a == a)`).
fn sort_less<T: PartialOrd + Copy>(a: T, b: T) -> bool {
    #[allow(clippy::eq_op)]
    let (a_nan, b_nan) = (a != a, b != b);
    a < b || (b_nan && !a_nan)
}

/// numpy `argsort(v)` with the default kind: numpy 2.5's introsort for argsort (`aquicksort_`: median of
/// three, partitions down to 15 elements, then insertion sort; heapsort past 2·⌊log₂ n⌋ levels), which
/// arm64 builds use unchanged (the SIMD argsorts are x86 only). Ties come out in numpy's order, which is
/// neither stable nor reversed; 400 of 400 tie-heavy random arrays match numpy 2.5.4.
#[must_use]
pub fn argsort<T: PartialOrd + Copy>(v: &[T]) -> Vec<usize> {
    let num = v.len();
    let mut t: Vec<usize> = (0..num).collect();
    if num < 2 {
        return t;
    }
    let less = |a: usize, b: usize| sort_less(v[a], v[b]);
    let (mut pl, mut pr) = (0usize, num - 1);
    let mut stack: Vec<(usize, usize, i32)> = Vec::new();
    let mut cdepth = 2 * (usize::BITS - 1 - num.leading_zeros()) as i32;
    loop {
        if cdepth < 0 {
            aheapsort(v, &mut t[pl..=pr]);
        } else {
            while pr - pl > 15 {
                let pm = pl + ((pr - pl) >> 1);
                if less(t[pm], t[pl]) {
                    t.swap(pm, pl);
                }
                if less(t[pr], t[pm]) {
                    t.swap(pr, pm);
                }
                if less(t[pm], t[pl]) {
                    t.swap(pm, pl);
                }
                let vp = v[t[pm]];
                let (mut pi, mut pj) = (pl, pr - 1);
                t.swap(pm, pj);
                loop {
                    pi += 1;
                    while sort_less(v[t[pi]], vp) {
                        pi += 1;
                    }
                    pj -= 1;
                    while sort_less(vp, v[t[pj]]) {
                        pj -= 1;
                    }
                    if pi >= pj {
                        break;
                    }
                    t.swap(pi, pj);
                }
                t.swap(pi, pr - 1);
                cdepth -= 1;
                if pi - pl < pr - pi {
                    stack.push((pi + 1, pr, cdepth));
                    pr = pi - 1;
                } else {
                    stack.push((pl, pi - 1, cdepth));
                    pl = pi + 1;
                }
            }
            for pi in pl + 1..=pr {
                let vi = t[pi];
                let mut pj = pi;
                while pj > pl && sort_less(v[vi], v[t[pj - 1]]) {
                    t[pj] = t[pj - 1];
                    pj -= 1;
                }
                t[pj] = vi;
            }
        }
        match stack.pop() {
            Some((l, r, d)) => (pl, pr, cdepth) = (l, r, d),
            None => break,
        }
    }
    t
}

/// numpy's `aheapsort_` on a slice of indices (1-based heap).
fn aheapsort<T: PartialOrd + Copy>(v: &[T], a: &mut [usize]) {
    let mut n = a.len();
    let at = |a: &[usize], i: usize| v[a[i - 1]];
    for l in (1..=n >> 1).rev() {
        let tmp = a[l - 1];
        let (mut i, mut j) = (l, l << 1);
        while j <= n {
            if j < n && sort_less(at(a, j), at(a, j + 1)) {
                j += 1;
            }
            if sort_less(v[tmp], at(a, j)) {
                a[i - 1] = a[j - 1];
                i = j;
                j += j;
            } else {
                break;
            }
        }
        a[i - 1] = tmp;
    }
    while n > 1 {
        let tmp = a[n - 1];
        a[n - 1] = a[0];
        n -= 1;
        let (mut i, mut j) = (1, 2);
        while j <= n {
            if j < n && sort_less(at(a, j), at(a, j + 1)) {
                j += 1;
            }
            if sort_less(v[tmp], at(a, j)) {
                a[i - 1] = a[j - 1];
                i = j;
                j += j;
            } else {
                break;
            }
        }
        a[i - 1] = tmp;
    }
}

/// numpy `median` of float64 values: the middle order statistic, or the mean of the two middle ones
/// (`(a + b)/2`). NaN for an empty slice.
#[must_use]
pub fn median(values: &[f64]) -> f64 {
    let n = values.len();
    if n == 0 {
        return f64::NAN;
    }
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

/// numpy `median` of a float32 array, which numpy computes in float32: `(a + b)/2` rounded to float32.
#[must_use]
pub fn median_f32(values: &[f32]) -> f32 {
    let n = values.len();
    if n == 0 {
        return f32::NAN;
    }
    let mut v = values.to_vec();
    v.sort_by(f32::total_cmp);
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

/// numpy `percentile` of a float32 array (linear method), which numpy 2 computes in float32 throughout:
/// `q = p / float32(100)`, the virtual index `(n − 1)·q` and the two-sided lerp all in float32.
#[must_use]
pub fn percentile_f32(values: &[f32], p: f32) -> f32 {
    let n = values.len();
    if n == 0 {
        return f32::NAN;
    }
    let mut v = values.to_vec();
    v.sort_by(f32::total_cmp);
    let q = p / 100.0f32;
    // numpy's `linear` method: (n − 1)·q
    let virtual_index = (n - 1) as f32 * q;
    if virtual_index >= (n - 1) as f32 {
        return v[n - 1];
    }
    if virtual_index < 0.0 {
        return v[0];
    }
    let below = virtual_index.floor();
    let gamma = virtual_index - below;
    let (a, b) = (v[below as usize], v[below as usize + 1]);
    let diff = b - a;
    if gamma >= 0.5 {
        b - diff * (1.0 - gamma)
    } else {
        a + diff * gamma
    }
}

/// numpy `convolve(a, v, mode="valid")` for float64: the longer argument slides under the reversed
/// shorter one. Kernels of up to 11 taps go through numpy's `small_correlate`, a sequential sum that the
/// reference's build fuses into multiply-adds (reproduced exactly); longer ones go to BLAS `ddot`, whose
/// summation order this does not reproduce (the same sequential fused sum is used).
#[must_use]
pub fn convolve_valid(a: &[f64], v: &[f64]) -> Vec<f64> {
    let (a, v) = if v.len() > a.len() { (v, a) } else { (a, v) };
    if v.is_empty() {
        return Vec::new();
    }
    let k: Vec<f64> = v.iter().rev().copied().collect();
    (0..=a.len() - k.len())
        .map(|i| {
            k.iter()
                .zip(&a[i..])
                .fold(0.0f64, |s, (&kj, &aj)| aj.mul_add(kj, s))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// numpy 2.5.4 `np.argsort` on arrays with ties (the order is numpy's, not a stable sort's).
    #[test]
    fn argsort_matches_numpy_ties() {
        let v = [
            3.0, 1.0, 2.0, 1.0, 3.0, 0.0, 2.0, 1.0, 3.0, 0.0, 1.0, 2.0, 3.0, 1.0, 0.0, 2.0, 1.0,
            3.0, 2.0, 0.0,
        ];
        assert_eq!(
            argsort(&v),
            [
                9, 14, 5, 19, 7, 10, 13, 1, 16, 3, 2, 6, 18, 11, 15, 4, 8, 12, 17, 0
            ]
        );
        assert_eq!(argsort(&[2.0, f64::NAN, 1.0]), [2, 0, 1]);
        assert_eq!(argsort::<f64>(&[]), Vec::<usize>::new());
    }

    #[test]
    fn medians_and_f32_percentiles() {
        assert_eq!(median(&[3.0, 1.0, 2.0, 10.0]), 2.5);
        assert_eq!(median(&[3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median_f32(&[1.1, 2.3]), (1.1f32 + 2.3f32) / 2.0);
        let d = [1.1f32, 2.3, 3.7, 4.9, 5.2];
        assert_eq!(percentile_f32(&d, 75.0), 4.9);
        assert_eq!(percentile_f32(&d, 25.0), 2.3);
    }

    #[test]
    fn convolve_is_a_sequential_fused_sum() {
        let a = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(convolve_valid(&a, &[1.0, 0.0, -1.0]), [2.0, 2.0, 2.0]);
        assert_eq!(convolve_valid(&[1.0, 1.0], &a), [3.0, 5.0, 7.0, 9.0]);
    }

    /// Values from numpy 2.5.4 `np.percentile` (linear method).
    #[test]
    fn percentile_matches_numpy() {
        let a = [0.3, 0.1, 0.7, 0.2, 0.9, 0.05, 0.33, 0.41, 0.12, 0.6, 0.27];
        assert_eq!(percentile(&a, 95.0), 0.8);
        assert_eq!(percentile(&a, 50.0), 0.3);
        assert_eq!(percentile(&a, 5.0), 0.075_000_000_000_000_01);
        assert_eq!(percentile(&a, 100.0), 0.9);
        assert_eq!(percentile(&a, 0.0), 0.05);
        assert_eq!(percentile(&[1.0, 2.0], 75.0), 1.75);
        let x: Vec<f64> = (0..37).map(|i| f64::from(i) * 0.1).collect();
        assert_eq!(percentile(&x, 95.0), 3.42);
        assert!(percentile(&[], 50.0).is_nan());
        // where the general α = β = 1 index formula differs from `linear`'s (n − 1)·q in the last bit
        let b = [
            37.61117091846387,
            28.348893722645613,
            46.053983864146495,
            10.288752411450941,
            42.54505622958257,
            8.449365566354311,
            48.21788604471398,
            31.184636405307586,
        ];
        assert_eq!(percentile(&b, 10.0), 9.736936357921952);
        let c = [
            0.9800209275804772,
            26.387496660675165,
            10.261624622504396,
            37.06287615971844,
            19.434861520836836,
            19.02365820664012,
        ];
        assert_eq!(percentile(&c, 33.0), 15.956946452192618);
        assert_eq!(percentile_f32(&[15.930_443, 8.089_892], 95.0), 15.538_415);
        let d = [
            38.032_135f32,
            3.318_774,
            12.209_677,
            41.810_36,
            47.902_237,
            22.208_376,
            38.653_07,
            19.365_04,
        ];
        assert_eq!(percentile_f32(&d, 33.0), 20.246_475);
    }

    /// Values from numpy 2.5.4 `np.arange`; in the hex-row case 30 of 33 differ from `start + i·step`.
    #[test]
    fn arange_matches_numpy_fill() {
        let a = arange(0.1, 1.0, 0.1);
        assert_eq!(a.len(), 9);
        assert_eq!(a[6], 0.700_000_000_000_000_1);
        let h = arange(-125.300_000_000_000_01, 212.7, 10.392);
        assert_eq!(h.len(), 33);
        assert_eq!(
            h[30..],
            [
                186.459_999_999_999_87,
                196.851_999_999_999_86,
                207.243_999_999_999_86
            ]
        );
        assert!(arange(3.0, 3.0, 1.0).is_empty());
        assert_eq!(arange(0.0, 0.3, 0.1), vec![0.0, 0.1, 0.2]);
    }

    #[test]
    fn interp_matches_numpy_cases() {
        assert_eq!(
            interp(1.0, &[0.0, 1.0, 1.0, 2.0], &[0.0, 5.0, 7.0, 9.0]),
            7.0
        );
        assert_eq!(interp(-1.0, &[0.0, 1.0], &[3.0, 4.0]), 3.0);
        assert_eq!(interp(2.0, &[0.0, 1.0], &[3.0, 4.0]), 4.0);
        assert_eq!(interp(0.25, &[0.0, 1.0], &[0.0, 8.0]), 2.0);
        assert_eq!(interp(0.0, &[0.0, 0.0], &[3.0, 4.0]), 4.0);
        assert!(interp(f64::NAN, &[0.0, 1.0], &[3.0, 4.0]).is_nan());
    }

    #[test]
    fn linspace_is_j_times_step() {
        let t = linspace(0.0, 1.0, 200);
        let step = 1.0 / 199.0;
        assert_eq!(t[0], 0.0);
        assert_eq!(t[199], 1.0);
        for (j, v) in t.iter().enumerate().take(199) {
            assert_eq!(*v, j as f64 * step);
        }
        assert_eq!(linspace(2.0, 5.0, 1), vec![2.0]);
        assert!(linspace(0.0, 1.0, 0).is_empty());
    }

    /// Values captured from numpy 2.5.4 `np.sum` of `[1/(i+1) for i in range(n)]`.
    #[test]
    fn pairwise_sum_matches_numpy() {
        let h = |n: usize| (0..n).map(|i| 1.0 / (i as f64 + 1.0)).collect::<Vec<_>>();
        assert_eq!(pairwise_sum(&[]), 0.0);
        assert_eq!(
            pairwise_sum(&h(7)).to_bits(),
            2.5928571428571425_f64.to_bits()
        );
        assert_eq!(
            pairwise_sum(&h(100)).to_bits(),
            5.187377517639621_f64.to_bits()
        );
        assert_eq!(
            pairwise_sum(&h(1000)).to_bits(),
            7.485470860550345_f64.to_bits()
        );
        assert_eq!(cumsum(&[1.0, 2.0, 3.0]), vec![1.0, 3.0, 6.0]);
    }
}

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
/// from the right end if that is NaN, as numpy does.
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
    let r = slope * (x - xp[j]) + fp[j];
    if !r.is_nan() {
        return r;
    }
    let r = slope * (x - xp[j + 1]) + fp[j + 1];
    if r.is_nan() && fp[j] == fp[j + 1] {
        fp[j]
    } else {
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

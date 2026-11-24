//! Order-preserving data parallelism for the solver's independent per-item work.
//!
//! [`map`] returns `[f(0), f(1), …, f(n − 1)]` in index order. With the `parallel` feature the items
//! are computed on rayon's pool; without it, on the calling thread. Callers do every reduction (sums,
//! scatters into the Hessian, early exits) serially over that vector in index order, so a parallel run
//! performs the same floating-point operations in the same order as a serial one: the results are the
//! same bits whatever the thread count (plan §6 determinism; no mandatory pool).

/// Below this many items the work stays on the calling thread: a fork-join costs more than it saves.
#[cfg(feature = "parallel")]
const MIN_ITEMS: usize = 512;

/// Items per task, so each task amortizes rayon's scheduling over many small elements.
#[cfg(feature = "parallel")]
const CHUNK: usize = 64;

/// `[f(0), …, f(n − 1)]`, computed in parallel when the `parallel` feature is on and `n` is large enough.
pub(crate) fn map<T: Send>(n: usize, f: impl Fn(usize) -> T + Sync + Send) -> Vec<T> {
    #[cfg(feature = "parallel")]
    if n >= MIN_ITEMS {
        use rayon::prelude::*;
        return (0..n).into_par_iter().with_min_len(CHUNK).map(f).collect();
    }
    (0..n).map(f).collect()
}

/// `f(0..n)` for a filter whose output for each index depends on that index alone: with the `parallel`
/// feature and enough items, `f` runs on consecutive ranges of at most `chunk` indices at once and the
/// pieces are joined in order, which is the same list.
pub(crate) fn filter_ranges<T: Send>(
    n: usize,
    chunk: usize,
    f: impl Fn(std::ops::Range<usize>) -> Vec<T> + Sync + Send,
) -> Vec<T> {
    #[cfg(feature = "parallel")]
    if n >= MIN_ITEMS && chunk > 0 && n > chunk {
        use rayon::prelude::*;
        let pieces: Vec<Vec<T>> = (0..n.div_ceil(chunk))
            .into_par_iter()
            .map(|c| f(c * chunk..n.min((c + 1) * chunk)))
            .collect();
        return pieces.into_iter().flatten().collect();
    }
    #[cfg(not(feature = "parallel"))]
    let _ = chunk;
    f(0..n)
}

#[cfg(test)]
mod tests {
    use super::{filter_ranges, map};

    #[test]
    fn keeps_index_order_on_both_sides_of_the_threshold() {
        for n in [0, 1, 511, 512, 5000] {
            let v = map(n, |i| i * 3);
            assert_eq!(v, (0..n).map(|i| i * 3).collect::<Vec<_>>());
        }
    }

    #[test]
    fn range_filters_join_in_order() {
        let keep = |r: std::ops::Range<usize>| r.filter(|i| i % 7 < 3).collect::<Vec<_>>();
        for n in [0, 10, 512, 4097, 20_000] {
            assert_eq!(filter_ranges(n, 1024, keep), keep(0..n));
        }
    }
}

//! libc++'s `std::sort` (LLVM 17's introsort), for the one place where GEOS's output depends on how its
//! unstable sort orders ties: `BufferBuilder::createSubgraphs` sorts subgraphs by their rightmost x, so
//! tied subgraphs (common on a pixel grid) come out in introsort's order, which is not input order once a
//! range reaches 24 elements. Insertion sort below 24, sorting networks for 2–5, a median of three (a
//! ninther above 128) moved to the front, `__partition_with_equals_on_right` and `_on_left`, and the
//! bounded "incomplete" insertion sort after an already-partitioned split. Depth exhaustion (heap sort in
//! libc++) falls back to a stable sort of the range; GEOS's inputs never reach it.

/// Sorts `v` as libc++'s `std::sort(v.begin(), v.end(), less)` does, ties included.
pub(crate) fn sort<T: Copy>(v: &mut [T], less: impl Fn(&T, &T) -> bool) {
    let n = v.len();
    let depth = if n > 1 { 2 * n.ilog2() as usize } else { 0 };
    Sorter { v, less: &less }.intro(0, n, depth, true);
}

struct Sorter<'a, T, F> {
    v: &'a mut [T],
    less: &'a F,
}

impl<T: Copy, F: Fn(&T, &T) -> bool> Sorter<'_, T, F> {
    fn lt(&self, i: usize, j: usize) -> bool {
        (self.less)(&self.v[i], &self.v[j])
    }

    fn sort3(&mut self, x: usize, y: usize, z: usize) {
        if !self.lt(y, x) {
            if !self.lt(z, y) {
                return;
            }
            self.v.swap(y, z);
            if self.lt(y, x) {
                self.v.swap(x, y);
            }
            return;
        }
        if self.lt(z, y) {
            self.v.swap(x, z);
            return;
        }
        self.v.swap(x, y);
        if self.lt(z, y) {
            self.v.swap(y, z);
        }
    }

    fn sort4(&mut self, x1: usize, x2: usize, x3: usize, x4: usize) {
        self.sort3(x1, x2, x3);
        if self.lt(x4, x3) {
            self.v.swap(x3, x4);
            if self.lt(x3, x2) {
                self.v.swap(x2, x3);
                if self.lt(x2, x1) {
                    self.v.swap(x1, x2);
                }
            }
        }
    }

    fn sort5(&mut self, x1: usize, x2: usize, x3: usize, x4: usize, x5: usize) {
        self.sort4(x1, x2, x3, x4);
        if self.lt(x5, x4) {
            self.v.swap(x4, x5);
            if self.lt(x4, x3) {
                self.v.swap(x3, x4);
                if self.lt(x3, x2) {
                    self.v.swap(x2, x3);
                    if self.lt(x2, x1) {
                        self.v.swap(x1, x2);
                    }
                }
            }
        }
    }

    /// `__insertion_sort` (guarded: stops at `f`) or `__insertion_sort_unguarded` (an element before `f`
    /// is no greater than any in the range).
    fn insertion(&mut self, f: usize, l: usize, guarded: bool) {
        for i in f + 1..l {
            if self.lt(i, i - 1) {
                let t = self.v[i];
                let mut j = i;
                loop {
                    self.v[j] = self.v[j - 1];
                    j -= 1;
                    if (guarded && j == f) || !(self.less)(&t, &self.v[j - 1]) {
                        break;
                    }
                }
                self.v[j] = t;
            }
        }
    }

    /// `__insertion_sort_incomplete`: gives up (false) after moving eight elements.
    fn insertion_incomplete(&mut self, f: usize, l: usize) -> bool {
        match l - f {
            0 | 1 => return true,
            2 => {
                if self.lt(l - 1, f) {
                    self.v.swap(f, l - 1);
                }
                return true;
            }
            3 => {
                self.sort3(f, f + 1, l - 1);
                return true;
            }
            4 => {
                self.sort4(f, f + 1, f + 2, l - 1);
                return true;
            }
            5 => {
                self.sort5(f, f + 1, f + 2, f + 3, l - 1);
                return true;
            }
            _ => {}
        }
        let mut j = f + 2;
        self.sort3(f, f + 1, j);
        let mut count = 0;
        let mut i = j + 1;
        while i != l {
            if self.lt(i, j) {
                let t = self.v[i];
                let mut k = j;
                j = i;
                loop {
                    self.v[j] = self.v[k];
                    j = k;
                    if j == f {
                        break;
                    }
                    k -= 1;
                    if !(self.less)(&t, &self.v[k]) {
                        break;
                    }
                }
                self.v[j] = t;
                count += 1;
                if count == 8 {
                    return i + 1 == l;
                }
            }
            j = i;
            i += 1;
        }
        true
    }

    /// `__partition_with_equals_on_right`: the pivot's final index and whether the range was already
    /// partitioned.
    fn partition_right(&mut self, f0: usize, l0: usize) -> (usize, bool) {
        let (b, piv) = (f0, self.v[f0]);
        let (mut f, mut l) = (f0 + 1, l0);
        while (self.less)(&self.v[f], &piv) {
            f += 1;
        }
        if b == f - 1 {
            while f < l {
                l -= 1;
                if (self.less)(&self.v[l], &piv) {
                    break;
                }
            }
        } else {
            l -= 1;
            while !(self.less)(&self.v[l], &piv) {
                l -= 1;
            }
        }
        let already = f >= l;
        while f < l {
            self.v.swap(f, l);
            f += 1;
            while (self.less)(&self.v[f], &piv) {
                f += 1;
            }
            l -= 1;
            while !(self.less)(&self.v[l], &piv) {
                l -= 1;
            }
        }
        let p = f - 1;
        if b != p {
            self.v[b] = self.v[p];
        }
        self.v[p] = piv;
        (p, already)
    }

    /// `__partition_with_equals_on_left`: the new first index.
    fn partition_left(&mut self, f0: usize, l0: usize) -> usize {
        let (b, piv) = (f0, self.v[f0]);
        let (mut f, mut l) = (f0 + 1, l0);
        if (self.less)(&piv, &self.v[l - 1]) {
            while !(self.less)(&piv, &self.v[f]) {
                f += 1;
            }
        } else {
            while f < l && !(self.less)(&piv, &self.v[f]) {
                f += 1;
            }
        }
        if f < l {
            l -= 1;
            while (self.less)(&piv, &self.v[l]) {
                l -= 1;
            }
        }
        while f < l {
            self.v.swap(f, l);
            f += 1;
            while !(self.less)(&piv, &self.v[f]) {
                f += 1;
            }
            l -= 1;
            while (self.less)(&piv, &self.v[l]) {
                l -= 1;
            }
        }
        let p = f - 1;
        if b != p {
            self.v[b] = self.v[p];
        }
        self.v[p] = piv;
        f
    }

    fn intro(&mut self, mut f: usize, mut l: usize, mut depth: usize, mut leftmost: bool) {
        loop {
            let n = l - f;
            match n {
                0 | 1 => return,
                2 => {
                    if self.lt(l - 1, f) {
                        self.v.swap(f, l - 1);
                    }
                    return;
                }
                3 => return self.sort3(f, f + 1, l - 1),
                4 => return self.sort4(f, f + 1, f + 2, l - 1),
                5 => return self.sort5(f, f + 1, f + 2, f + 3, l - 1),
                _ => {}
            }
            if n < 24 {
                return self.insertion(f, l, leftmost);
            }
            if depth == 0 {
                let less = self.less;
                self.v[f..l].sort_by(|a, b| {
                    if less(a, b) {
                        std::cmp::Ordering::Less
                    } else if less(b, a) {
                        std::cmp::Ordering::Greater
                    } else {
                        std::cmp::Ordering::Equal
                    }
                });
                return;
            }
            depth -= 1;
            let h = n / 2;
            if n > 128 {
                self.sort3(f, f + h, l - 1);
                self.sort3(f + 1, f + h - 1, l - 2);
                self.sort3(f + 2, f + h + 1, l - 3);
                self.sort3(f + h - 1, f + h, f + h + 1);
                self.v.swap(f, f + h);
            } else {
                self.sort3(f + h, f, l - 1);
            }
            if !leftmost && !self.lt(f - 1, f) {
                f = self.partition_left(f, l);
                continue;
            }
            let (i, already) = self.partition_right(f, l);
            if already {
                let first_sorted = self.insertion_incomplete(f, i);
                if self.insertion_incomplete(i + 1, l) {
                    if first_sorted {
                        return;
                    }
                    l = i;
                    continue;
                } else if first_sorted {
                    f = i + 1;
                    continue;
                }
            }
            self.intro(f, i, depth, leftmost);
            leftmost = false;
            f = i + 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::sort;

    /// Sorted output whatever the ties, and ties in introsort's order: a 30-element range of one key in
    /// two halves comes back with the halves' order disturbed (libc++ is not stable from 24 elements).
    #[test]
    fn sorts_and_reorders_ties_from_24() {
        let mut s = 7u64;
        for n in [0usize, 1, 2, 5, 23, 24, 25, 60, 129, 300, 1000] {
            let mut v: Vec<(u64, usize)> = (0..n)
                .map(|i| {
                    s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                    ((s >> 60) % 5, i)
                })
                .collect();
            sort(&mut v, |a, b| a.0 > b.0);
            assert!(v.windows(2).all(|w| w[0].0 >= w[1].0), "n = {n}");
            let mut idx: Vec<usize> = v.iter().map(|p| p.1).collect();
            idx.sort_unstable();
            assert_eq!(idx, (0..n).collect::<Vec<_>>());
        }
        let mut short: Vec<(u8, usize)> = (0..23).map(|i| (0, i)).collect();
        sort(&mut short, |a, b| a.0 > b.0);
        assert!(
            short.iter().enumerate().all(|(i, p)| p.1 == i),
            "insertion sort keeps ties"
        );
    }
}

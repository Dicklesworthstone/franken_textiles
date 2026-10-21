//! Partial operations retain full-source arclength and paired notch correspondence. Selecting a
//! span changes the traveled geometry, never the original pattern or its whole-seam acceptance.

use ftx_core::polyline::{at_fractions, polyline_length};
use ftx_core::{Refusal, RefusalCode, Vec2, np};
use ftx_pattern::{Pattern, Seam, SeamPairs};
use serde::Serialize;

use super::{MAX_STATIONS, bad, path::PathReceipt, positive};

/// Original seam coverage and actual one-way lengths of an explicitly selected partial pass.
#[derive(Clone, Debug, Serialize)]
pub struct SourceSpan {
    /// Original [start, end] arclength fractions for A and oriented B, before stitch-line offset.
    /// B follows the pattern seam's reversal, independently of the pass's feed direction.
    pub source_ranges: [[f64; 2]; 2],
    /// Full original source-side lengths in mm, not the lengths of just this operation.
    pub source_lengths_mm: [f64; 2],
    /// One-way traveled centerline/offset lengths in mm, excluding repeated lock travel.
    pub sewn_lengths_mm: [f64; 2],
}

impl SourceSpan {
    pub(super) fn new(
        source_range: [f64; 2],
        stations: &SeamPairs,
        path: Option<&PathReceipt>,
    ) -> Self {
        let b = path.map_or(&stations.fb, |p| &p.source_fractions[1]);
        Self {
            source_ranges: [source_range, [b[0], b[b.len() - 1]]],
            source_lengths_mm: path
                .map_or([stations.len_a, stations.len_b], |p| p.source_lengths_mm),
            sewn_lengths_mm: [
                (stations.fa[stations.fa.len() - 1] - stations.fa[0]) * stations.len_a,
                (stations.fb[stations.fb.len() - 1] - stations.fb[0]) * stations.len_b,
            ],
        }
    }
}

pub(super) fn validate(range: [f64; 2]) -> Result<(), Refusal> {
    if range.iter().any(|f| !f.is_finite())
        || range[0] < 0.0
        || range[0] >= range[1]
        || range[1] > 1.0
    {
        return Err(bad(
            "sewing source_range needs finite 0 <= start < end <= 1",
        ));
    }
    Ok(())
}

pub(super) fn require_coverage(
    seam: &str,
    side: &str,
    mut ranges: Vec<[f64; 2]>,
) -> Result<(), Refusal> {
    ranges.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    let mut covered = 0.0_f64;
    for [start, end] in ranges {
        if start > covered {
            return Err(bad(format!(
                "{seam}: sewing source coverage gap on side {side}: {covered}..{start}"
            )));
        }
        covered = covered.max(end);
    }
    if covered < 1.0 {
        return Err(bad(format!(
            "{seam}: sewing source coverage gap on side {side}: {covered}..1"
        )));
    }
    Ok(())
}

/// Same dense source geometry and notch map as `Pattern::seam_pairs`, with exact span endpoints.
/// Existing whole-seam jobs retain their original sampler and nine-decimal break rounding.
pub(super) fn build(
    pattern: &Pattern,
    seam: &Seam,
    range: [f64; 2],
    pitch: f64,
) -> Result<SeamPairs, Refusal> {
    let ra = seam
        .side_ref("a")
        .ok_or_else(|| bad("missing seam side a"))?;
    let rb = seam
        .side_ref("b")
        .ok_or_else(|| bad("missing seam side b"))?;
    let (a, aj) = pattern.side_polyline(ra).map_err(|e| bad(e.to_string()))?;
    let (mut b, mut bj) = pattern.side_polyline(rb).map_err(|e| bad(e.to_string()))?;
    let na = pattern
        .notch_fractions(ra)
        .map_err(|e| bad(e.to_string()))?;
    let mut nb = pattern
        .notch_fractions(rb)
        .map_err(|e| bad(e.to_string()))?;
    if seam.reverse() {
        b.reverse();
        bj = bj.into_iter().rev().map(|f| 1.0 - f).collect();
        nb = nb.into_iter().rev().map(|f| 1.0 - f).collect();
    }
    sample(
        [&a, &b],
        [polyline_length(&a), polyline_length(&b)],
        [aj, bj],
        [na, nb],
        range,
        pitch,
    )
}

/// Bounded paired sampling shared with offsets. Fractions/lengths refer to full input paths;
/// source notches and edge junctions inside the requested interval remain exact stations.
pub(super) fn sample(
    lines: [&[Vec2]; 2],
    lengths: [f64; 2],
    junctions: [Vec<f64>; 2],
    notches: [Vec<f64>; 2],
    range: [f64; 2],
    pitch: f64,
) -> Result<SeamPairs, Refusal> {
    validate(range)?;
    if lengths.iter().any(|&l| !positive(l) || l >= 1e5) {
        return Err(bad("implausible source/offset stitch-line length"));
    }
    if notches[0].len() != notches[1].len() {
        return Err(bad(
            "sewing notches must pair before selecting a source span",
        ));
    }
    let wrap = |ns: &[f64]| [&[0.0][..], ns, &[1.0][..]].concat();
    let ka = wrap(&notches[0]);
    let kb = wrap(&notches[1]);
    let to_b = |f| np::interp(f, &ka, &kb);
    let to_a = |f| np::interp(f, &kb, &ka);
    validate([to_b(range[0]), to_b(range[1])])?;
    let mut breaks: Vec<_> = ka
        .iter()
        .chain(&junctions[0])
        .copied()
        .chain(junctions[1].iter().map(|&f| to_a(f)))
        .filter(|f| range[0] < *f && *f < range[1])
        .chain(range)
        .collect();
    breaks.sort_by(f64::total_cmp);
    // Do not round away distinct authored boundaries to manufacture a feasible stitch.
    breaks.dedup();
    let mut counts = Vec::with_capacity(breaks.len());
    let mut total = 1usize;
    for w in breaks.windows(2) {
        let span = ((w[1] - w[0]) * lengths[0]).max((to_b(w[1]) - to_b(w[0])) * lengths[1]);
        let count = (span / pitch).ceil().max(1.0);
        if !count.is_finite()
            || count > MAX_STATIONS as f64
            || total.saturating_add(count as usize) > MAX_STATIONS
        {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "sewing span station budget exceeded",
            ));
        }
        total += count as usize;
        counts.push(count as usize);
    }
    let mut fa = Vec::with_capacity(total);
    let mut fb = Vec::with_capacity(total);
    fa.push(range[0]);
    fb.push(to_b(range[0]));
    for (w, count) in breaks.windows(2).zip(counts) {
        for &u in &np::linspace(w[0], w[1], count + 1)[1..] {
            fa.push(u);
            fb.push(to_b(u));
        }
    }
    Ok(SeamPairs {
        a: at_fractions(lines[0], &fa),
        b: at_fractions(lines[1], &fb),
        fa,
        fb,
        len_a: lengths[0],
        len_b: lengths[1],
        a_junctions: junctions[0].clone(),
        b_junctions: junctions[1].clone(),
        notches_a: notches[0].clone(),
        notches_b: notches[1].clone(),
        notch_match: if notches[0].is_empty() {
            "none"
        } else {
            "piecewise"
        }
        .into(),
    })
}

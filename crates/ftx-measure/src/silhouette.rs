//! Sampled silhouette evidence for calibrated, rectified images (FTX-21).
//! The projection is the union of mesh triangles, not a convex hull or a list of visible vertices.
//! Fixed image scanlines preserve gaps and self-occlusion while giving a fixed-size fit residual.

use ftx_core::{Refusal, Vec2, Vec3};
use serde::{Deserialize, Serialize};

use crate::{camera::Camera, invalid, triangulate::Partition};

/// A posed surface in the same metre, z-up world frame as the camera.
/// Triangle winding is irrelevant to silhouette coverage; open surfaces are permitted.
#[derive(Clone, Debug)]
pub struct SurfaceMesh {
    /// Finite world-space positions, at most 100,000.
    pub vertices_m: Vec<Vec3>,
    /// At most 200,000 nondegenerate triangles with valid, distinct indices.
    pub triangles: Vec<[usize; 3]>,
}

/// Which image coordinate is held constant along a scanline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    /// y is fixed, intervals describe x coverage.
    Horizontal,
    /// x is fixed, intervals describe y coverage.
    Vertical,
}
impl Axis {
    fn coordinates(self, p: Vec2) -> (f64, f64) {
        match self {
            Self::Horizontal => (p.x, p.y),
            Self::Vertical => (p.y, p.x),
        }
    }
    fn dimensions(self, size: [u32; 2]) -> (u32, u32) {
        match self {
            Self::Horizontal => (size[0], size[1]),
            Self::Vertical => (size[1], size[0]),
        }
    }
}

/// Foreground runs on one fixed line in an already oriented, rectified image.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scanline {
    /// Horizontal or vertical image slice.
    pub axis: Axis,
    /// Fixed y or x coordinate, in full-resolution pixels.
    pub at_px: f64,
    /// Sorted disjoint [begin,end] foreground intervals, at most eight; empty means background.
    pub intervals_px: Vec<[f64; 2]>,
    /// Positive contour-location weight in pixels, not an independent full-mask noise model.
    pub sigma_px: f64,
    /// Held-out lines never influence the fit, derivatives or rank check.
    pub partition: Partition,
}

/// Explicit sampled silhouette evidence, not an automatically inferred anatomical contour.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Silhouette {
    /// Mask/source and contour-definition receipt; clothing and occluders must be addressed by caller.
    pub evidence: String,
    /// 1–64 unique slices; at least one must contain foreground.
    pub lines: Vec<Scanline>,
}

impl Silhouette {
    /// Reject malformed intervals, duplicate fit/held-out slices and image-coordinate mismatches.
    pub fn validate(&self, image_size_px: [u32; 2]) -> Result<(), Refusal> {
        if self.evidence.trim().is_empty()
            || self.evidence.len() > 4096
            || !(1..=64).contains(&self.lines.len())
            || image_size_px.iter().any(|n| !(2..=1_000_000).contains(n))
            || !self.lines.iter().any(|line| !line.intervals_px.is_empty())
        {
            return Err(invalid(
                "silhouette needs evidence, valid dimensions and 1..64 slices with foreground",
            ));
        }
        for (i, line) in self.lines.iter().enumerate() {
            let (along, across) = line.axis.dimensions(image_size_px);
            if !line.at_px.is_finite()
                || !(0.0..=f64::from(across - 1)).contains(&line.at_px)
                || !line.sigma_px.is_finite()
                || !(1e-6..=1e6).contains(&line.sigma_px)
                || line.intervals_px.len() > 8
                || self.lines[..i]
                    .iter()
                    .any(|p| p.axis == line.axis && p.at_px == line.at_px)
            {
                return Err(invalid(
                    "invalid or duplicate silhouette slice/sigma; do not reuse fit evidence as held out",
                ));
            }
            for (j, &[lo, hi]) in line.intervals_px.iter().enumerate() {
                if !lo.is_finite()
                    || !hi.is_finite()
                    || lo < 0.0
                    || hi > f64::from(along - 1)
                    || lo >= hi
                    || (j > 0 && line.intervals_px[j - 1][1] >= lo)
                {
                    return Err(invalid(
                        "silhouette intervals must be finite, ordered, disjoint and inside the image",
                    ));
                }
            }
        }
        Ok(())
    }

    /// Fix the reduction order without reattaching partitions to different image lines.
    pub fn canonicalize(&mut self) {
        self.lines
            .sort_by(|a, b| a.axis.cmp(&b.axis).then(a.at_px.total_cmp(&b.at_px)));
    }
}

/// Require rectification rather than approximate curved distorted triangle edges as straight lines.
pub fn require_rectified(camera: &Camera) -> Result<(), Refusal> {
    camera.validate()?;
    let d = &camera.intrinsics.distortion;
    if d.radial.iter().chain(&d.tangential).any(|v| *v != 0.0) {
        return Err(invalid(
            "silhouette slices require rectified pixels and zero lens distortion; rectify keypoints, mask and calibration together",
        ));
    }
    Ok(())
}

/// One sampled-line comparison, retaining the actual predicted coverage for diagnostics.
#[derive(Clone, Debug, Serialize)]
pub struct LineComparison {
    /// Fixed line orientation.
    pub axis: Axis,
    /// Fixed image coordinate.
    pub at_px: f64,
    /// Original fit/held-out partition.
    pub partition: Partition,
    /// Mesh coverage; interior triangle edges and overlapping surfaces have been merged.
    pub predicted_intervals_px: Vec<[f64; 2]>,
    /// Signed observed-edge distances, sixteen padded reverse-boundary distances, then signed
    /// coverage-length difference, in pixels. At most eight predicted intervals are permitted.
    /// Its length is fixed by the observation, not by the number of predicted components.
    /// Absent when observed foreground has no predicted coverage and no finite edge distance.
    pub residuals_px: Option<Vec<f64>>,
    /// Symmetric maximum boundary displacement; absent when exactly one side has no foreground.
    pub boundary_error_px: Option<f64>,
    /// Total missing plus extra foreground length on this line.
    pub symmetric_difference_px: f64,
    /// Fixed 3-pixel boundary gate; mismatched empty/nonempty lines always fail.
    pub passed: bool,
}

fn union(mut intervals: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    intervals.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    let mut out: Vec<[f64; 2]> = Vec::new();
    for [lo, hi] in intervals {
        if let Some(last) = out.last_mut() {
            // Shared-edge arithmetic can differ by a few ULPs. This joins roundoff-sized cracks,
            // never pixel-sized gaps; author-supplied evidence is not clamped or merged.
            let eps = 64.0 * f64::EPSILON * (1.0 + last[1].abs().max(lo.abs()));
            if lo <= last[1] + eps {
                last[1] = last[1].max(hi);
                continue;
            }
        }
        out.push([lo, hi]);
    }
    out
}

fn interval(tri: [Vec2; 3], axis: Axis, at: f64) -> Option<[f64; 2]> {
    let p = tri.map(|p| axis.coordinates(p));
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for i in 0..3 {
        let (mut a, mut b) = (p[i], p[(i + 1) % 3]);
        if a.1 > b.1 || (a.1 == b.1 && a.0 > b.0) {
            std::mem::swap(&mut a, &mut b);
        }
        if at < a.1 || at > b.1 {
            continue;
        }
        if a.1 == b.1 {
            lo = lo.min(a.0);
            hi = hi.max(b.0);
        } else {
            let x = a.0 + ((at - a.1) / (b.1 - a.1)) * (b.0 - a.0);
            lo = lo.min(x);
            hi = hi.max(x);
        }
    }
    (hi > lo).then_some([lo, hi])
}

fn boundary_distance(x: f64, intervals: &[[f64; 2]]) -> f64 {
    intervals
        .iter()
        .flatten()
        .map(|end| (x - end).abs())
        .fold(f64::INFINITY, f64::min)
}

fn line_comparison(line: &Scanline, predicted: Vec<[f64; 2]>) -> Result<LineComparison, Refusal> {
    if predicted.len() > 8 {
        return Err(invalid(
            "predicted silhouette exceeds eight components on a slice; no component was dropped",
        ));
    }
    let observed = &line.intervals_px;
    let residuals = observed.iter().flatten().map(|&x| {
        let distance = boundary_distance(x, &predicted);
        if predicted.iter().any(|&[lo, hi]| lo < x && x < hi) {
            -distance
        } else {
            distance
        }
    });
    let overlap: f64 = observed
        .iter()
        .map(|&[a, b]| {
            predicted
                .iter()
                .map(|&[c, d]| (b.min(d) - a.max(c)).max(0.0))
                .sum::<f64>()
        })
        .sum();
    // max(0) removes only roundoff in these mathematically nonnegative computed lengths.
    let missing = (observed.iter().map(|r| r[1] - r[0]).sum::<f64>() - overlap).max(0.0);
    let extra = (predicted.iter().map(|r| r[1] - r[0]).sum::<f64>() - overlap).max(0.0);
    let error = if predicted.is_empty() && observed.is_empty() {
        Some(0.0)
    } else if predicted.is_empty() || observed.is_empty() {
        None
    } else {
        Some(
            observed
                .iter()
                .flatten()
                .map(|&x| boundary_distance(x, &predicted))
                .chain(
                    predicted
                        .iter()
                        .flatten()
                        .map(|&x| boundary_distance(x, observed)),
                )
                .fold(0.0, f64::max),
        )
    };
    // Squaring aggregated missing/extra lengths couples absolute endpoint errors and creates
    // nonstationary kinks even when coverage topology is fixed. Symmetric squared boundary
    // distances plus signed total length remain smooth in that region. Missing/extra coverage
    // is still retained as a diagnostic, not confused with the optimization objective.
    // Keep individual signed errors rather than the norm of a group: a scalar norm has
    // ill-scaled finite differences near a noisy optimum. Zero padding bounds the number of
    // residuals independently of predicted topology; topology transitions can still be nonsmooth.
    let mut reverse = [0.0; 16];
    if !observed.is_empty() {
        for (slot, &x) in reverse.iter_mut().zip(predicted.iter().flatten()) {
            let distance = boundary_distance(x, observed);
            *slot = if observed.iter().any(|&[lo, hi]| lo < x && x < hi) {
                -distance
            } else {
                distance
            };
        }
    }
    let residuals = if !observed.is_empty() && predicted.is_empty() {
        None
    } else {
        Some(
            residuals
                .chain(reverse)
                .chain([extra - missing])
                .collect::<Vec<_>>(),
        )
    };
    if !missing.is_finite()
        || !extra.is_finite()
        || !(missing + extra).is_finite()
        || error.is_some_and(|e| !e.is_finite())
        || residuals
            .as_ref()
            .is_some_and(|r| r.iter().any(|v| !v.is_finite()))
    {
        return Err(invalid("silhouette comparison overflow"));
    }
    Ok(LineComparison {
        axis: line.axis,
        at_px: line.at_px,
        partition: line.partition,
        residuals_px: residuals,
        predicted_intervals_px: predicted,
        boundary_error_px: error,
        symmetric_difference_px: missing + extra,
        passed: error.is_some_and(|e| e <= 3.0),
    })
}

/// Compare selected fixed scanlines with projected mesh coverage. `partition: None` checks all.
/// No raster rounding, nearest-vertex substitution, hull approximation or outlier trimming is used.
/// Missing mesh coverage gives absent edge residuals (unusable for fitting) and a failed gate;
/// callers must not treat such a line as a successful zero residual.
/// At most eight million triangle/line tests are allowed per evaluation; excess is refused.
pub fn compare(
    mesh: &SurfaceMesh,
    camera: &Camera,
    image_size_px: [u32; 2],
    silhouette: &Silhouette,
    partition: Option<Partition>,
) -> Result<Vec<LineComparison>, Refusal> {
    silhouette.validate(image_size_px)?;
    require_rectified(camera)?;
    let mut lines: Vec<_> = silhouette
        .lines
        .iter()
        .filter(|line| partition.is_none_or(|p| p == line.partition))
        .collect();
    lines.sort_by(|a, b| a.axis.cmp(&b.axis).then(a.at_px.total_cmp(&b.at_px)));
    if mesh.vertices_m.is_empty()
        || mesh.vertices_m.len() > 100_000
        || mesh.triangles.is_empty()
        || mesh.triangles.len() > 200_000
        || mesh.triangles.len() * lines.len() > 8_000_000
    {
        return Err(invalid(
            "silhouette mesh is empty or exceeds the surface/scanline budget",
        ));
    }
    let pixels: Vec<_> = mesh
        .vertices_m
        .iter()
        .map(|&p| camera.project(p))
        .collect::<Result<_, _>>()?;
    let mut coverage: Vec<Vec<[f64; 2]>> = vec![Vec::new(); lines.len()];
    for &[a, b, c] in &mesh.triangles {
        if [a, b, c].iter().any(|&v| v >= pixels.len()) || a == b || b == c || c == a {
            return Err(invalid("invalid silhouette triangle indices"));
        }
        let area = (mesh.vertices_m[b] - mesh.vertices_m[a])
            .cross(mesh.vertices_m[c] - mesh.vertices_m[a])
            .norm2();
        if !area.is_finite() || area == 0.0 {
            return Err(invalid("degenerate silhouette surface triangle"));
        }
        let tri = [pixels[a], pixels[b], pixels[c]];
        let projected_area = (tri[1] - tri[0]).cross(tri[2] - tri[0]);
        if !projected_area.is_finite() {
            return Err(invalid("silhouette projection overflow"));
        }
        if projected_area == 0.0 {
            continue;
        } // edge-on surfaces have no foreground area
        for (line, segments) in lines.iter().zip(&mut coverage) {
            if let Some(segment) = interval(tri, line.axis, line.at_px) {
                if segment.iter().any(|v| !v.is_finite()) {
                    return Err(invalid("non-finite silhouette slice"));
                }
                segments.push(segment);
            }
        }
    }
    lines
        .into_iter()
        .zip(coverage)
        .map(|(line, intervals)| line_comparison(line, union(intervals)))
        .collect()
}

/// Explicit mask slice selection, established before fitting or selecting held-out checks.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaskSample {
    /// Horizontal row or vertical column.
    pub axis: Axis,
    /// Zero-based row/column in the already oriented, rectified mask.
    pub index: u32,
    /// Contour-location weight, in full-resolution pixels.
    pub sigma_px: f64,
    /// Fit/held-out assignment fixed before looking at the fit.
    pub partition: Partition,
}

/// Extract foreground intervals from a binary 0/255 mask without resizing or threshold guessing.
/// Pixel centers are integers; a run at centers i..j has boundaries i-0.5 and j+0.5.
/// Border foreground is refused: a cropped contour cannot establish the missing body outline.
/// This samples requested slices only, not a full-mask agreement or segmentation-quality claim.
pub fn sample_mask(
    image_size_px: [u32; 2],
    mask: &[u8],
    samples: &[MaskSample],
    evidence: &str,
) -> Result<Silhouette, Refusal> {
    let [w, h] = image_size_px.map(|v| v as usize);
    let size = w
        .checked_mul(h)
        .filter(|&n| n <= 40_000_000 && n == mask.len());
    if w < 2
        || h < 2
        || size.is_none()
        || !(1..=64).contains(&samples.len())
        || mask.iter().any(|&p| p != 0 && p != 255)
    {
        return Err(invalid(
            "silhouette mask needs matching dimensions, binary 0/255 pixels and 1..64 samples",
        ));
    }
    if mask[..w]
        .iter()
        .chain(&mask[(h - 1) * w..])
        .any(|&p| p != 0)
        || (0..h).any(|y| mask[y * w] != 0 || mask[y * w + w - 1] != 0)
    {
        return Err(invalid(
            "silhouette mask touches the image border; use an uncropped, unobstructed body contour",
        ));
    }
    let mut lines = Vec::with_capacity(samples.len());
    for sample in samples {
        let (along, across) = sample.axis.dimensions(image_size_px);
        if sample.index >= across {
            return Err(invalid("silhouette sample is outside the mask"));
        }
        let mut intervals = Vec::new();
        let mut begin = None;
        for i in 0..along {
            let p = match sample.axis {
                Axis::Horizontal => mask[sample.index as usize * w + i as usize],
                Axis::Vertical => mask[i as usize * w + sample.index as usize],
            };
            if p == 255 && begin.is_none() {
                begin = Some(f64::from(i) - 0.5);
            }
            if p == 0
                && let Some(lo) = begin.take()
            {
                intervals.push([lo, f64::from(i) - 0.5]);
                if intervals.len() > 8 {
                    return Err(invalid(
                        "mask slice exceeds eight foreground components; no component was dropped",
                    ));
                }
            }
        }
        lines.push(Scanline {
            axis: sample.axis,
            at_px: f64::from(sample.index),
            intervals_px: intervals,
            sigma_px: sample.sigma_px,
            partition: sample.partition,
        });
    }
    let mut silhouette = Silhouette {
        evidence: evidence.into(),
        lines,
    };
    silhouette.validate(image_size_px)?;
    silhouette.canonicalize();
    Ok(silhouette)
}

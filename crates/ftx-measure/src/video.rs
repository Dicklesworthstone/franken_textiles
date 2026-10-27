//! Decoded-frame selection for plan §10.2. Pixels and both masks are already oriented alike;
//! timestamps are decoded presentation times, not frame numbers. Sharpness is not stillness.

use ftx_core::{Refusal, Vec2};
use serde::{Deserialize, Serialize};

use super::{invalid, triangulate::FrameId};

const MAX_PIXELS: usize = 40_000_000;
const MAX_FRAMES: usize = 20_000;
const MAX_VARIANCE: f64 = 1_040_400.0; // The four-neighbour 8-bit Laplacian is in [-1020, 1020].

/// Identity and acquisition metadata for an already-oriented decoded frame.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameMetadata {
    /// Stream and decoded presentation timestamp, on the stream's declared clock.
    pub frame: FrameId,
    /// Clockwise rotation already applied to pixels, masks and any calibration.
    pub rotation_cw_deg: u16,
    /// Caller-declared lens/calibration epoch. Change it at a known physical lens switch.
    pub camera_epoch: String,
    /// Optional calibrated focal lengths in oriented-image pixels, not a guessed EXIF focal length.
    #[serde(default)]
    pub focal_px: Option<Vec2>,
}

impl FrameMetadata {
    /// Check acquisition identity and oriented calibration metadata before scoring or mask review.
    pub fn validate(&self) -> Result<(), Refusal> {
        if self.frame.stream.trim().is_empty()
            || self.frame.stream.len() > 256
            || self.camera_epoch.trim().is_empty()
            || self.camera_epoch.len() > 256
            || !matches!(self.rotation_cw_deg, 0 | 90 | 180 | 270)
            || self
                .focal_px
                .is_some_and(|p| !p.is_finite() || p.x <= 0.0 || p.y <= 0.0)
        {
            return Err(invalid(
                "frame needs a stream, camera epoch, quarter-turn rotation and valid optional focal lengths",
            ));
        }
        Ok(())
    }
}

/// Sharpness measured strictly inside one supplied mask. Nonzero mask values mean inside.
#[derive(Clone, Debug, Serialize)]
pub struct RegionSharpness {
    /// Count before excluding image and mask boundaries.
    pub mask_pixels: usize,
    /// Count whose complete 3 × 3 neighbourhood is inside the mask and image.
    pub interior_pixels: usize,
    /// Population variance of the four-neighbour Laplacian, in 8-bit intensity squared.
    /// Absent when the mask contains no usable interior; absence is never zero uncertainty.
    pub laplacian_variance: Option<f64>,
}

#[derive(Default)]
struct Moments {
    count: u64,
    sum: i64,
    squared: u64,
}

impl Moments {
    fn add(&mut self, value: i64) {
        self.count += 1;
        self.sum += value;
        self.squared += (value * value) as u64;
    }

    fn finish(self, mask_pixels: usize) -> RegionSharpness {
        // Bounded integer moments avoid cancellation and need no negative-variance clamp.
        // Even MAX_PIXELS samples of |L|=1020 fit in these intermediates on native and wasm.
        let laplacian_variance = (self.count != 0).then(|| {
            let n = u128::from(self.count);
            let sum = u128::from(self.sum.unsigned_abs());
            let numerator = n * u128::from(self.squared) - sum * sum;
            numerator as f64 / (n * n) as f64
        });
        RegionSharpness {
            mask_pixels,
            interior_pixels: self.count as usize,
            laplacian_variance,
        }
    }
}

/// Independently scored person and sheet, retaining the exact acquisition identity.
#[derive(Clone, Debug, Serialize)]
pub struct FrameQuality {
    /// The caller's decoded timestamp, orientation and calibration record.
    pub metadata: FrameMetadata,
    /// Width and height of the already-oriented image and masks.
    pub image_size_px: [usize; 2],
    /// No outside/background pixels contribute to this score.
    pub person: RegionSharpness,
    /// No person/background pixels outside the sheet mask contribute to this score.
    pub sheet: RegionSharpness,
}

/// Score row-major 8-bit grayscale pixels and equally-sized masks without I/O or image decoding.
/// Empty/thin masks remain diagnostic results, but cannot pass selection's sample-count gate.
/// The mask boundary itself is excluded: a sharp cutout must not make a blurred person look sharp.
pub fn score_frame(
    metadata: FrameMetadata,
    image_size_px: [usize; 2],
    gray: &[u8],
    person_mask: &[u8],
    sheet_mask: &[u8],
) -> Result<FrameQuality, Refusal> {
    metadata.validate()?;
    let [width, height] = image_size_px;
    let len = width
        .checked_mul(height)
        .filter(|&n| width >= 3 && height >= 3 && n <= MAX_PIXELS)
        .ok_or_else(|| {
            invalid("frame dimensions must be at least 3 × 3 and at most 40 million pixels")
        })?;
    if [gray.len(), person_mask.len(), sheet_mask.len()] != [len; 3] {
        return Err(invalid(
            "image and both masks must have exactly width × height samples",
        ));
    }
    let mut moments = [Moments::default(), Moments::default()];
    let masks = [person_mask, sheet_mask];
    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let i = y * width + x;
            let value = i64::from(gray[i - 1])
                + i64::from(gray[i + 1])
                + i64::from(gray[i - width])
                + i64::from(gray[i + width])
                - 4 * i64::from(gray[i]);
            for (mask, moment) in masks.iter().zip(&mut moments) {
                if [i - width, i, i + width]
                    .iter()
                    .all(|&j| mask[j - 1] != 0 && mask[j] != 0 && mask[j + 1] != 0)
                {
                    moment.add(value);
                }
            }
        }
    }
    let [person, sheet] = moments;
    Ok(FrameQuality {
        metadata,
        image_size_px,
        person: person.finish(person_mask.iter().filter(|&&v| v != 0).count()),
        sheet: sheet.finish(sheet_mask.iter().filter(|&&v| v != 0).count()),
    })
}

/// Explicit image-quality policy. Defaults are starting thresholds, not calibrated accuracy gates.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SelectionOptions {
    /// Non-overlapping selection windows: 100,000–250,000 microseconds (plan §10.2).
    pub window_us: u64,
    /// Required usable interior in each mask. At least two samples are needed for a variance.
    pub min_interior_pixels: usize,
    /// Minimum person Laplacian variance; texture, noise and exposure affect this statistic.
    pub min_person_variance: f64,
    /// Independent minimum sheet Laplacian variance.
    pub min_sheet_variance: f64,
    /// Restart windows when either supplied focal length drifts this fraction from the epoch anchor.
    pub max_focal_change_ratio: f64,
}

impl Default for SelectionOptions {
    fn default() -> Self {
        Self {
            window_us: 150_000,
            min_interior_pixels: 64,
            min_person_variance: 10.0,
            min_sheet_variance: 10.0,
            max_focal_change_ratio: 0.01,
        }
    }
}

impl SelectionOptions {
    /// Reject invalid policies instead of silently truncating or clamping them.
    pub fn validate(&self) -> Result<(), Refusal> {
        if !(100_000..=250_000).contains(&self.window_us)
            || !(2..=MAX_PIXELS).contains(&self.min_interior_pixels)
            || [self.min_person_variance, self.min_sheet_variance]
                .iter()
                .any(|&v| {
                    !v.is_finite()
                        || v <= 0.0
                        || v > MAX_VARIANCE
                        || !(MAX_VARIANCE / v).is_finite()
                })
            || !self.max_focal_change_ratio.is_finite()
            || self.max_focal_change_ratio <= 0.0
            || self.max_focal_change_ratio > 1.0
        {
            return Err(invalid(
                "invalid frame-selection window, sample count, variance threshold or focal-change ratio",
            ));
        }
        Ok(())
    }
}

/// A candidate remains in the report even when it cannot be selected.
#[derive(Clone, Debug, Serialize)]
pub struct FrameCandidate {
    /// Scores and acquisition record for this candidate.
    pub quality: FrameQuality,
    /// Reasons this candidate failed the image-quality policy.
    pub rejected: Vec<String>,
    /// Minimum of person/threshold and sheet/threshold. None for an ineligible candidate.
    pub limiting_score: Option<f64>,
}

/// A populated decoded-time bucket within one stable acquisition segment.
#[derive(Clone, Debug, Serialize)]
pub struct SelectionWindow {
    /// No bucket ever combines streams.
    pub stream: String,
    /// Anchor for this stream/lens/zoom/resolution/orientation segment.
    pub segment_start_us: u64,
    /// Why this segment starts independently of the preceding segment.
    pub segment_reasons: Vec<String>,
    /// Inclusive bucket boundary, on the decoded clock.
    pub start_us: u64,
    /// Exclusive bucket boundary, not the last observed timestamp.
    pub end_us: u64,
    /// The best eligible frame, or None when every candidate failed. No fallback to a bad frame.
    pub selected: Option<FrameId>,
    /// Every supplied candidate in timestamp order, including rejected ones.
    pub candidates: Vec<FrameCandidate>,
}

/// Deterministic frame choices, not permission to triangulate a moving subject.
#[derive(Clone, Debug, Serialize)]
pub struct FrameSelection {
    /// `ftx-frame-selection/1`.
    pub schema: &'static str,
    /// Exact thresholds used, including any defaults.
    pub options: SelectionOptions,
    /// Selected decoded identities in stream/time order.
    pub selected_frames: Vec<FrameId>,
    /// Populated windows; unobserved time gaps are not invented as frames or windows.
    pub windows: Vec<SelectionWindow>,
    /// Limitations of the supplied acquisition metadata.
    pub advisories: Vec<String>,
    /// Image sharpness cannot establish motion, metric scale, synchronization or body fit.
    pub no_claim: &'static str,
}

fn restart_reasons(
    anchor: &FrameQuality,
    frame: &FrameQuality,
    options: &SelectionOptions,
) -> Vec<String> {
    let mut reasons = Vec::new();
    if anchor.metadata.frame.stream != frame.metadata.frame.stream {
        return vec!["stream_start".into()];
    }
    if anchor.metadata.camera_epoch != frame.metadata.camera_epoch {
        reasons.push("camera_epoch_changed".into());
    }
    if anchor.image_size_px != frame.image_size_px {
        reasons.push("image_size_changed".into());
    }
    if anchor.metadata.rotation_cw_deg != frame.metadata.rotation_cw_deg {
        reasons.push("rotation_changed".into());
    }
    match (anchor.metadata.focal_px, frame.metadata.focal_px) {
        (Some(a), Some(b)) => {
            if (b.x - a.x).abs() / a.x > options.max_focal_change_ratio
                || (b.y - a.y).abs() / a.y > options.max_focal_change_ratio
            {
                reasons.push("focal_length_changed".into());
            }
        }
        (None, None) => {}
        _ => reasons.push("focal_calibration_availability_changed".into()),
    }
    reasons
}

fn candidate(
    quality: &FrameQuality,
    options: &SelectionOptions,
) -> Result<FrameCandidate, Refusal> {
    quality.metadata.validate()?;
    let [width, height] = quality.image_size_px;
    let pixels = width
        .checked_mul(height)
        .filter(|&n| width >= 3 && height >= 3 && n <= MAX_PIXELS)
        .ok_or_else(|| invalid("invalid scored-frame dimensions"))?;
    let mut rejected = Vec::new();
    let mut scores = [0.0; 2];
    for (index, (name, region, threshold)) in [
        ("person", &quality.person, options.min_person_variance),
        ("sheet", &quality.sheet, options.min_sheet_variance),
    ]
    .into_iter()
    .enumerate()
    {
        if region.mask_pixels > pixels
            || region.interior_pixels > region.mask_pixels
            || region.interior_pixels > (width - 2) * (height - 2)
            || (region.interior_pixels == 0) != region.laplacian_variance.is_none()
            || region
                .laplacian_variance
                .is_some_and(|v| !v.is_finite() || !(0.0..=MAX_VARIANCE).contains(&v))
        {
            return Err(invalid("invalid region sharpness statistics"));
        }
        if region.interior_pixels < options.min_interior_pixels {
            rejected.push(format!("{name}: insufficient mask interior"));
        }
        if let Some(variance) = region.laplacian_variance {
            scores[index] = variance / threshold;
            if variance < threshold {
                rejected.push(format!("{name}: sharpness below threshold"));
            }
        }
    }
    let limiting_score = rejected.is_empty().then(|| scores[0].min(scores[1]));
    Ok(FrameCandidate {
        quality: quality.clone(),
        rejected,
        limiting_score,
    })
}

/// Pick one eligible frame per populated time bucket. Maximize the weaker normalized mask score;
/// a sharp sheet cannot compensate for a blurred person. Ties select the earlier decoded timestamp.
/// Windows restart at declared lens/rotation/resolution changes and calibrated focal drift, measured
/// against the segment's first frame so gradual zoom cannot evade the threshold. Missing focal data
/// is reported; neither lens identity nor stillness is inferred from pixels.
pub fn select_frames(
    frames: &[FrameQuality],
    options: &SelectionOptions,
) -> Result<FrameSelection, Refusal> {
    options.validate()?;
    if frames.is_empty() || frames.len() > MAX_FRAMES {
        return Err(invalid("frame selection needs 1..20000 decoded frames"));
    }
    let mut ordered: Vec<_> = frames.iter().collect();
    ordered.sort_by(|a, b| a.metadata.frame.cmp(&b.metadata.frame));
    if ordered
        .windows(2)
        .any(|p| p[0].metadata.frame == p[1].metadata.frame)
    {
        return Err(invalid(
            "duplicate decoded frame identity; frame ordinals cannot disambiguate timestamps",
        ));
    }
    let mut report = FrameSelection {
        schema: "ftx-frame-selection/1",
        options: options.clone(),
        selected_frames: Vec::new(),
        windows: Vec::new(),
        advisories: Vec::new(),
        no_claim: "Sharpness selection only; selected windows are not still-window or synchronization evidence. Texture, noise and exposure affect scores; no metric accuracy or body-fit claim.",
    };
    if ordered.iter().any(|f| f.metadata.focal_px.is_none()) {
        report.advisories.push("Some frames lack calibrated focal lengths: undeclared zoom/lens changes cannot be detected there".into());
    }
    let mut anchor = ordered[0];
    let mut segment_reasons = vec!["stream_start".into()];
    let mut last_bucket = None;
    let mut best_score = None;
    for (index, quality) in ordered.into_iter().enumerate() {
        let candidate = candidate(quality, options)?;
        let reasons = restart_reasons(anchor, quality, options);
        if index > 0 && !reasons.is_empty() {
            anchor = quality;
            segment_reasons = reasons;
            last_bucket = None;
        }
        let start = anchor.metadata.frame.timestamp_us;
        let bucket = (quality.metadata.frame.timestamp_us - start) / options.window_us;
        if last_bucket != Some(bucket) {
            let start_us = start + bucket * options.window_us;
            let end_us = start_us
                .checked_add(options.window_us)
                .ok_or_else(|| invalid("decoded timestamp overflows the selection window"))?;
            report.windows.push(SelectionWindow {
                stream: quality.metadata.frame.stream.clone(),
                segment_start_us: start,
                segment_reasons: segment_reasons.clone(),
                start_us,
                end_us,
                selected: None,
                candidates: Vec::new(),
            });
            last_bucket = Some(bucket);
            best_score = None;
        }
        let window = report.windows.last_mut().expect("populated above");
        if let Some(score) = candidate.limiting_score
            && best_score.is_none_or(|best| score > best)
        {
            window.selected = Some(quality.metadata.frame.clone());
            best_score = Some(score);
        }
        window.candidates.push(candidate);
    }
    report.selected_frames = report
        .windows
        .iter()
        .filter_map(|w| w.selected.clone())
        .collect();
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata(timestamp_us: u64) -> FrameMetadata {
        FrameMetadata {
            frame: FrameId {
                stream: "synthetic".into(),
                timestamp_us,
            },
            rotation_cw_deg: 0,
            camera_epoch: "lens-a".into(),
            focal_px: Some(Vec2::new(800.0, 800.0)),
        }
    }

    fn quality(time: u64, person: f64, sheet: f64) -> FrameQuality {
        let region = |variance| RegionSharpness {
            mask_pixels: 100,
            interior_pixels: 64,
            laplacian_variance: Some(variance),
        };
        FrameQuality {
            metadata: metadata(time),
            image_size_px: [10, 10],
            person: region(person),
            sheet: region(sheet),
        }
    }

    #[test]
    fn exact_masked_laplacian_and_affine_image() {
        let checker: Vec<_> = (0..16)
            .map(|i| if (i / 4 + i % 4) % 2 == 0 { 0 } else { 255 })
            .collect();
        let scored = score_frame(metadata(0), [4, 4], &checker, &[1; 16], &[255; 16]).unwrap();
        assert_eq!(scored.person.interior_pixels, 4);
        assert_eq!(scored.person.laplacian_variance, Some(MAX_VARIANCE));
        let ramp: Vec<_> = (0..16).map(|i| (i / 4 + i % 4) as u8).collect();
        let scored = score_frame(metadata(0), [4, 4], &ramp, &[1; 16], &[1; 16]).unwrap();
        assert_eq!(scored.person.laplacian_variance, Some(0.0));
    }

    #[test]
    fn mask_edges_and_sharp_background_do_not_inflate_subject_score() {
        let mut pixels: Vec<_> = (0..100).map(|i| if i % 2 == 0 { 0 } else { 255 }).collect();
        let mut mask = vec![0; 100];
        for y in 3..7 {
            for x in 3..7 {
                pixels[y * 10 + x] = 80;
                mask[y * 10 + x] = 255;
            }
        }
        let q = score_frame(metadata(0), [10, 10], &pixels, &mask, &[0; 100]).unwrap();
        assert_eq!(q.person.mask_pixels, 16);
        assert_eq!(q.person.interior_pixels, 4);
        assert_eq!(q.person.laplacian_variance, Some(0.0));
        assert_eq!(q.sheet.laplacian_variance, None);
        assert!(
            select_frames(&[q], &SelectionOptions::default())
                .unwrap()
                .selected_frames
                .is_empty()
        );
    }

    #[test]
    fn both_regions_must_pass_and_weaker_region_controls_selection() {
        let input = [
            quality(0, 1.0, 5000.0),
            quality(1, 20.0, 80.0),
            quality(2, 50.0, 50.0),
            quality(3, 50.0, 50.0),
        ];
        let report = select_frames(&input, &SelectionOptions::default()).unwrap();
        assert_eq!(report.selected_frames, vec![metadata(2).frame]);
        assert_eq!(
            report.windows[0].candidates[0].rejected,
            ["person: sharpness below threshold"]
        );
        assert_eq!(report.windows[0].candidates[2].limiting_score, Some(5.0));
    }

    #[test]
    fn decoded_windows_reordering_and_missing_intervals_are_deterministic() {
        let mut input = vec![
            quality(0, 50.0, 50.0),
            quality(149_999, 100.0, 100.0),
            quality(150_000, 50.0, 50.0),
            quality(450_000, 50.0, 50.0),
        ];
        let first = select_frames(&input, &SelectionOptions::default()).unwrap();
        assert_eq!(
            first
                .selected_frames
                .iter()
                .map(|f| f.timestamp_us)
                .collect::<Vec<_>>(),
            [149_999, 150_000, 450_000]
        );
        assert_eq!(first.windows.len(), 3);
        input.reverse();
        assert_eq!(
            serde_json::to_value(first).unwrap(),
            serde_json::to_value(select_frames(&input, &SelectionOptions::default()).unwrap())
                .unwrap()
        );
    }

    #[test]
    fn gradual_zoom_lens_and_orientation_changes_start_new_segments() {
        let mut input: Vec<_> = (0..6).map(|t| quality(t, 50.0, 50.0)).collect();
        input[1].metadata.focal_px = Some(Vec2::new(807.0, 800.0));
        input[2].metadata.focal_px = Some(Vec2::new(814.0, 800.0));
        for q in &mut input[3..] {
            q.metadata.focal_px = Some(Vec2::new(814.0, 800.0));
            q.metadata.camera_epoch = "lens-b".into();
        }
        input[4].metadata.rotation_cw_deg = 90;
        input[5].metadata.rotation_cw_deg = 90;
        input[5].metadata.frame.stream = "synthetic-b".into();
        let report = select_frames(&input, &SelectionOptions::default()).unwrap();
        assert_eq!(report.windows.len(), 5);
        assert_eq!(report.windows[1].segment_reasons, ["focal_length_changed"]);
        assert_eq!(report.windows[2].segment_reasons, ["camera_epoch_changed"]);
        assert_eq!(report.windows[3].segment_reasons, ["rotation_changed"]);
        assert_eq!(report.windows[4].segment_reasons, ["stream_start"]);
    }

    #[test]
    fn poor_windows_are_retained_and_missing_calibration_is_reported() {
        let mut q = quality(0, 1.0, 2.0);
        q.metadata.focal_px = None;
        let report = select_frames(&[q], &SelectionOptions::default()).unwrap();
        assert!(report.selected_frames.is_empty());
        assert!(report.windows[0].selected.is_none());
        assert_eq!(report.windows[0].candidates[0].rejected.len(), 2);
        assert_eq!(report.advisories.len(), 1);
    }

    #[test]
    fn malformed_identity_pixels_policy_and_statistics_refuse() {
        assert!(score_frame(metadata(0), [usize::MAX, 4], &[], &[], &[]).is_err());
        assert!(score_frame(metadata(0), [4, 4], &[0; 16], &[1; 15], &[1; 16]).is_err());
        let mut m = metadata(0);
        m.rotation_cw_deg = 45;
        assert!(score_frame(m, [4, 4], &[0; 16], &[1; 16], &[1; 16]).is_err());
        let q = quality(0, 50.0, 50.0);
        assert!(select_frames(&[q.clone(), q.clone()], &SelectionOptions::default()).is_err());
        let bad = SelectionOptions {
            window_us: 99_999,
            ..SelectionOptions::default()
        };
        assert!(select_frames(std::slice::from_ref(&q), &bad).is_err());
        let mut bad = q;
        bad.person.laplacian_variance = Some(f64::NAN);
        assert!(select_frames(&[bad], &SelectionOptions::default()).is_err());
        assert!(
            select_frames(
                &[quality(u64::MAX, 50.0, 50.0)],
                &SelectionOptions::default()
            )
            .is_err()
        );
        assert!(
            serde_json::from_value::<SelectionOptions>(serde_json::json!({"window_ms": 150}))
                .is_err()
        );
    }
}

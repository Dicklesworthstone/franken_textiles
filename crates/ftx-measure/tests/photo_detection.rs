//! Pixel-to-evidence known answers. Images are synthetic pinhole quads rendered independently of
//! the detector; no real people, reference outputs, or candidate-generated oracle fixtures.
use ftx_core::{RefusalCode, det};
use ftx_measure::photo::{PhotoOptions, analyze_plain_sheet, measure_photo};
use serde_json::{Value, json};

type Point = [f64; 2];
struct Image {
    w: usize,
    h: usize,
    bgr: Vec<u8>,
}
impl Image {
    fn new(w: usize, h: usize, gray: u8) -> Self {
        Self {
            w,
            h,
            bgr: vec![gray; 3 * w * h],
        }
    }
    fn quad(&mut self, q: [Point; 4], color: [u8; 3]) {
        let xmin = q
            .iter()
            .map(|p| p[0])
            .fold(f64::INFINITY, f64::min)
            .floor()
            .max(0.0) as usize;
        let ymin = q
            .iter()
            .map(|p| p[1])
            .fold(f64::INFINITY, f64::min)
            .floor()
            .max(0.0) as usize;
        let xmax = (q.iter().map(|p| p[0]).fold(0.0, f64::max).ceil() as usize + 1).min(self.w);
        let ymax = (q.iter().map(|p| p[1]).fold(0.0, f64::max).ceil() as usize + 1).min(self.h);
        for y in ymin..ymax {
            for x in xmin..xmax {
                let mut covered = 0;
                for sy in 0..4 {
                    for sx in 0..4 {
                        let p = [
                            x as f64 + (sx as f64 + 0.5) / 4.0 - 0.5,
                            y as f64 + (sy as f64 + 0.5) / 4.0 - 0.5,
                        ];
                        if (0..4).all(|i| {
                            let a = q[i];
                            let b = q[(i + 1) % 4];
                            (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]) >= 0.0
                        }) {
                            covered += 1;
                        }
                    }
                }
                let i = 3 * (y * self.w + x);
                for (k, c) in color.iter().enumerate() {
                    self.bgr[i + k] = ((u32::from(self.bgr[i + k]) * (16 - covered)
                        + u32::from(*c) * covered
                        + 8)
                        / 16) as u8;
                }
            }
        }
    }
    fn blur(&mut self) {
        // Binomial 5-tap blur, not the detector's Gaussian implementation.
        for vertical in [false, true] {
            let old = self.bgr.clone();
            for y in 0..self.h {
                for x in 0..self.w {
                    for k in 0..3 {
                        let mut value = 0u32;
                        for (n, weight) in [1, 4, 6, 4, 1].into_iter().enumerate() {
                            let dx = if vertical { 0 } else { n as isize - 2 };
                            let dy = if vertical { n as isize - 2 } else { 0 };
                            let xx = (x as isize + dx).clamp(0, self.w as isize - 1) as usize;
                            let yy = (y as isize + dy).clamp(0, self.h as isize - 1) as usize;
                            value += weight * u32::from(old[3 * (yy * self.w + xx) + k]);
                        }
                        self.bgr[3 * (y * self.w + x) + k] = ((value + 8) / 16) as u8;
                    }
                }
            }
        }
    }
    fn scale(&self, paper: &str) -> ftx_measure::photo::PhotoScale {
        analyze_plain_sheet(&self.bgr, self.w, self.h, paper).unwrap()
    }
}
fn rectangle(x: f64, y: f64, w: f64, h: f64) -> [Point; 4] {
    [[x, y], [x + w, y], [x + w, y + h], [x, y + h]]
}
fn letter() -> (Image, [Point; 4]) {
    let mut im = Image::new(720, 960, 90);
    let q = rectangle(150.2, 170.4, 1.5 * 215.9, 1.5 * 279.4);
    im.quad(q, [242; 3]);
    (im, q)
}
fn assert_corners(report: &Value, expected: [Point; 4], tolerance: f64) {
    let actual = report["corners_px"].as_array().unwrap();
    for i in 0..4 {
        for k in 0..2 {
            assert!(
                (actual[i][k].as_f64().unwrap() - expected[i][k]).abs() < tolerance,
                "{report}"
            );
        }
    }
}

#[test]
fn square_on_pixels_calibrate_and_measure_with_edge_bootstrap() {
    let (mut im, corners) = letter();
    im.blur();
    let scale = im.scale("auto");
    let r = scale.report();
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["paper"], "letter");
    assert_eq!(r["coordinate_source"], "detected_edges");
    assert_eq!(r["candidates"], 1);
    assert_corners(r, corners, 0.5);
    assert!(r["edge_fit_rms_px"].as_f64().unwrap() < 0.2, "{r}");
    assert!(
        (r["blur_sigma_px_estimate"].as_f64().unwrap() - 1.0).abs() < 0.4,
        "{r}"
    );
    assert!(r["edge_points"].as_u64().unwrap() > 100);
    let result = measure_photo(
        &scale,
        &json!({"across_back":[[30,400],[630,400]]}),
        &PhotoOptions::default(),
    )
    .unwrap();
    let record = &result["records"][0];
    assert!(
        (record["value_mm"].as_f64().unwrap() - 400.0).abs() < 1.0,
        "{result}"
    );
    assert_eq!(record["provenance"], "PHOTO_ESTIMATED");
    assert!(record["sigma_mm"].as_f64().unwrap() > 0.0);
    assert!(
        result["photo"]["bootstrap"]["method"]
            .as_str()
            .unwrap()
            .contains("edge points")
    );
    assert_eq!(scale.report(), im.scale("auto").report());
    assert_eq!(
        result,
        measure_photo(
            &scale,
            &json!({"across_back":[[30,400],[630,400]]}),
            &PhotoOptions::default()
        )
        .unwrap()
    );
    let wrong = im.scale("a4");
    assert_eq!(wrong.report()["ok"], false);
    assert!(
        wrong.report()["warnings"][0]
            .as_str()
            .unwrap()
            .starts_with("ASPECT_MISMATCH")
    );
    assert!(
        measure_photo(
            &wrong,
            &json!({"x":[[1,1],[10,10]]}),
            &PhotoOptions::default()
        )
        .is_err()
    );
}

#[test]
fn a4_landscape_and_large_search_rescaling_keep_pixel_coordinates() {
    for (w, h, factor) in [(900, 720, 1.5), (1800, 1440, 3.0)] {
        let mut im = Image::new(w, h, 80);
        let q = rectangle(
            100.2 * factor,
            110.4 * factor,
            297.0 * factor,
            210.0 * factor,
        );
        im.quad(q, [240; 3]);
        im.blur();
        let scale = im.scale("auto");
        assert_eq!(scale.report()["ok"], true, "{}", scale.report());
        assert_eq!(scale.report()["paper"], "a4");
        assert_eq!(scale.report()["orientation"], "landscape");
        assert_corners(scale.report(), q, 0.5);
        assert!((scale.report()["mm_per_px"].as_f64().unwrap() - 1.0 / factor).abs() < 0.002);
    }
}

#[test]
fn a_larger_window_does_not_displace_the_sheet() {
    let mut im = Image::new(1500, 1100, 70);
    let q = rectangle(90.2, 120.4, 215.9 * 1.5, 279.4 * 1.5);
    im.quad(q, [235; 3]);
    im.quad(rectangle(650.0, 80.0, 500.0, 950.0), [245; 3]);
    let scale = im.scale("auto");
    assert_eq!(scale.report()["ok"], true, "{}", scale.report());
    assert_eq!(scale.report()["candidates"], 2);
    assert_corners(scale.report(), q, 0.6);
    let mut window = Image::new(900, 1100, 70);
    window.quad(rectangle(200.0, 80.0, 500.0, 950.0), [245; 3]);
    assert_eq!(window.scale("auto").report()["ok"], false);
}

#[test]
fn clipped_corner_and_partial_top_edge_recover_from_straight_support() {
    for top_clip in [false, true] {
        let (mut im, q) = letter();
        if top_clip {
            im.quad(rectangle(220.0, 155.0, 185.0, 45.0), [45; 3]);
        } else {
            im.quad(
                [
                    [120.0, 130.0],
                    [210.0, 130.0],
                    [210.0, 165.0],
                    [120.0, 250.0],
                ],
                [110, 155, 195],
            );
        }
        im.blur();
        let scale = im.scale("letter");
        assert_eq!(scale.report()["ok"], true, "{}", scale.report());
        assert_corners(scale.report(), q, 0.6);
    }
}

#[test]
fn insufficient_contrast_frame_clipping_and_heavy_occlusion_cannot_calibrate() {
    for mode in 0..4 {
        let (mut im, _) = letter();
        match mode {
            0 => im.bgr.fill(240),
            1 => {
                im.bgr.fill(235);
                im.quad(rectangle(150.0, 170.0, 324.0, 420.0), [242; 3]);
            }
            2 => {
                im.bgr.fill(90);
                im.quad(rectangle(-20.0, 170.0, 324.0, 420.0), [242; 3]);
            }
            _ => im.quad(rectangle(190.0, 150.0, 240.0, 60.0), [30; 3]),
        }
        let r = im.scale("letter");
        assert_eq!(r.report()["ok"], false, "mode {mode}: {}", r.report());
        assert!(r.report().get("mm_per_px").is_none());
    }
}

#[test]
fn physical_perspective_retains_paper_ambiguity() {
    for axis in [0.0_f64, 40.0] {
        let (s, c) = det::sin_cos(20.0_f64.to_radians());
        let (ay, ax) = det::sin_cos(axis.to_radians());
        let q = rectangle(-215.9 / 2.0, -279.4 / 2.0, 215.9, 279.4).map(|[x, y]| {
            let along = x * ax + y * ay;
            let z = 1200.0 + s * (ax * y - ay * x);
            [
                479.5 + 1800.0 * (c * x + (1.0 - c) * along * ax) / z,
                479.5 + 1800.0 * (c * y + (1.0 - c) * along * ay) / z,
            ]
        });
        let mut im = Image::new(960, 960, 70);
        im.quad(q, [242; 3]);
        im.blur();
        let named = im.scale("letter");
        assert_eq!(named.report()["ok"], true, "{}", named.report());
        assert_corners(named.report(), q, 0.6);
        let auto = im.scale("auto");
        if axis == 0.0 {
            assert_eq!(auto.report()["ok"], false, "{}", auto.report());
            assert!(
                auto.report()["warnings"][0]
                    .as_str()
                    .unwrap()
                    .starts_with("PAPER_AMBIGUOUS")
            );
        } else {
            assert_eq!(auto.report()["paper"], "letter");
        }
    }
}

#[test]
fn several_sheets_are_reported_and_malformed_or_over_budget_images_refuse() {
    let mut im = Image::new(1000, 720, 70);
    for x in [100.0, 520.0] {
        im.quad(rectangle(x, 100.0, 215.9 * 1.5, 279.4 * 1.5), [242; 3]);
    }
    let scale = im.scale("letter");
    assert_eq!(scale.report()["ok"], true, "{}", scale.report());
    assert_eq!(scale.report()["candidates"], 2);
    assert!(
        scale.report()["warnings"][0]
            .as_str()
            .unwrap()
            .starts_with("SEVERAL_SHEETS")
    );
    assert!(analyze_plain_sheet(&[0; 10], 100, 100, "letter").is_err());
    assert!(analyze_plain_sheet(&[], usize::MAX, 100, "letter").is_err());
    assert_eq!(
        analyze_plain_sheet(&[], 5000, 5000, "letter")
            .unwrap_err()
            .code,
        RefusalCode::BudgetExceeded
    );
    assert!(analyze_plain_sheet(&im.bgr, im.w, im.h, "typo").is_err());
    let mut crowded = Image::new(1400, 1800, 70);
    for y in 0..4 {
        for x in 0..4 {
            crowded.quad(
                rectangle(
                    20.0 + x as f64 * 340.0,
                    20.0 + y as f64 * 440.0,
                    215.9,
                    279.4,
                ),
                [242; 3],
            );
        }
    }
    assert_eq!(
        analyze_plain_sheet(&crowded.bgr, crowded.w, crowded.h, "letter")
            .unwrap_err()
            .code,
        RefusalCode::BudgetExceeded
    );
}

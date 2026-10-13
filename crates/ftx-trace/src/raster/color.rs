//! OpenCV 5.0 `cvtColor` for 8-bit BGR: to Lab (`RGB2Lab_b`, `color_lab.cpp`), gray and HSV saturation
//! (`color_rgb.simd.hpp`, `color_hsv.simd.hpp`), and the trace's colour-distance channel.
//!
//! Lab is OpenCV's own integer formula over two tables that OpenCV builds with its software float types: the
//! float32 steps are float32 here, the double steps double, the cube root is OpenCV's own (truncating) one,
//! and `pow` is evaluated in double and rounded to float32 (close enough that every table entry agrees). Gray
//! and HSV are OpenCV's fixed-point code; on arm64 the reference runs them through the KleidiCV/carotene HAL,
//! which rounds a few pixels differently (see `crates/ftx-trace/CONTRACT.md`).

use std::sync::OnceLock;

use ftx_core::det;

use super::image::{Bgr, Gray};

const XYZ_SHIFT: i32 = 12;
const GAMMA_SHIFT: i32 = 3;
const LAB_SHIFT2: i32 = XYZ_SHIFT + GAMMA_SHIFT;
const CBRT_TAB_SIZE: usize = 256 * 3 / 2 * (1 << GAMMA_SHIFT);

/// sRGB → XYZ (D65) and the D65 white, as OpenCV stores them (raw doubles).
const SRGB2XYZ_D65: [u64; 9] = [
    0x3fda_65a1_4488_c60d,
    0x3fd6_e297_396d_0918,
    0x3fc7_1819_d239_1d58,
    0x3fcb_38cd_a6e7_5ff6,
    0x3fe6_e297_396d_0918,
    0x3fb2_79aa_e6c8_f755,
    0x3f93_cc4a_c6cd_af4b,
    0x3fbe_836e_b4e9_8138,
    0x3fee_6842_7418_d691,
];
const D65: [u64; 3] = [
    0x3fee_6a22_b389_2ee8,
    0x3ff0_0000_0000_0000,
    0x3ff1_6b89_5076_3a19,
];

/// `cvRound` (to nearest, ties to even).
fn round_even(x: f64) -> i64 {
    x.round_ties_even() as i64
}

/// OpenCV's `softfloat` cube root (`f32_cbrt`, `core/src/softfloat.cpp`): the mantissa scaled into
/// [0.125, 1), a quartic rational approximation in double, and the result's mantissa TRUNCATED to float32 (so
/// it can sit one unit below the correctly rounded value, which moves a table entry).
fn cv_cbrt(x: f32) -> f32 {
    const A: [u64; 9] = [
        0x4046_a09e_6653_ba70,
        0x4068_08f4_6c61_16e0,
        0x405d_ca97_439c_ae14,
        0x402a_dd70_d282_7500,
        0x3fc4_f15f_83f5_5d2d,
        0x402d_9e20_660e_db21,
        0x4062_ff15_c028_5815,
        0x4065_10d0_6a81_12ce,
        0x4040_fecb_c9e2_c375,
    ];
    if x.is_nan() || x.is_infinite() {
        return x;
    }
    let v = x.to_bits();
    if v & 0x7fff_ffff == 0 {
        return 0.0;
    }
    let a = A.map(f64::from_bits);
    let s = v >> 31;
    let ex = ((v >> 23) & 0xff) as i32 - 127;
    let mut shx = ex % 3;
    if shx >= 0 {
        shx -= 3;
    }
    let ex = (ex - shx) / 3 - 1;
    let fr = f64::from_bits((((shx + 1023) as u64) << 52) | (u64::from(v & 0x007f_ffff) << 29));
    let num = (((a[0] * fr + a[1]) * fr + a[2]) * fr + a[3]) * fr + a[4];
    let den = (((a[5] * fr + a[6]) * fr + a[7]) * fr + a[8]) * fr + 1.0;
    let frac = ((num / den).to_bits() & 0x000f_ffff_ffff_ffff) >> 29;
    f32::from_bits((s << 31) + (((ex + 127) as u32) << 23) + frac as u32)
}

struct LabTables {
    gamma: [i32; 256],
    cbrt: Vec<i32>,
    coeffs: [i32; 9],
}

fn tables() -> &'static LabTables {
    static T: OnceLock<LabTables> = OnceLock::new();
    T.get_or_init(|| {
        // sRGBGammaTab_b
        let threshold = 809.0 / 20000.0;
        let (low_scale, power, xshift) = (323.0 / 25.0, 12.0 / 5.0, 11.0 / 200.0);
        let int_scale = 255.0f32 * 8.0;
        let mut gamma = [0i32; 256];
        for (i, g) in gamma.iter_mut().enumerate() {
            let x = f64::from(i as f32 / 255.0f32);
            let v = if x <= threshold {
                x / low_scale
            } else {
                det::exp(power * det::ln((x + xshift) / (1.0 + xshift)))
            };
            *g = round_even(f64::from(int_scale * (v as f32))) as i32;
        }
        // LabCbrtTab_b
        let lthresh = 216.0f32 / 24389.0f32;
        let lscale = 841.0f32 / 108.0f32;
        let lbias = 16.0f32 / 116.0f32;
        let tab_scale = 1.0f32 / (255.0f32 * 8.0f32);
        let lshift2 = (1i32 << LAB_SHIFT2) as f32;
        let cbrt_tab: Vec<i32> = (0..CBRT_TAB_SIZE)
            .map(|i| {
                let x = tab_scale * i as f32;
                let v = if x < lthresh {
                    x.mul_add(lscale, lbias)
                } else {
                    cv_cbrt(x)
                };
                round_even(f64::from(lshift2 * v)) as i32
            })
            .collect();
        // RGB2Lab_b coefficients for BGR input (blueIdx 0)
        let lshift = f64::from(1 << XYZ_SHIFT);
        let mut coeffs = [0i32; 9];
        for i in 0..3 {
            let c = |j: usize| f64::from_bits(SRGB2XYZ_D65[i * 3 + j]);
            let w = f64::from_bits(D65[i]);
            coeffs[i * 3 + 2] = round_even(lshift * c(0) / w) as i32;
            coeffs[i * 3 + 1] = round_even(lshift * c(1) / w) as i32;
            coeffs[i * 3] = round_even(lshift * c(2) / w) as i32;
        }
        LabTables {
            gamma,
            cbrt: cbrt_tab,
            coeffs,
        }
    })
}

fn descale(x: i32, n: i32) -> i32 {
    (x + (1 << (n - 1))) >> n
}

/// One BGR pixel in OpenCV's 8-bit Lab: `[L, a, b]`.
#[must_use]
pub fn lab(bgr: [u8; 3]) -> [u8; 3] {
    let t = tables();
    let c = &t.coeffs;
    let (r, g, b) = (
        t.gamma[usize::from(bgr[0])],
        t.gamma[usize::from(bgr[1])],
        t.gamma[usize::from(bgr[2])],
    );
    let f = |k: usize| t.cbrt[descale(r * c[k] + g * c[k + 1] + b * c[k + 2], XYZ_SHIFT) as usize];
    let (fx, fy, fz) = (f(0), f(3), f(6));
    let l_scale = (116 * 255 + 50) / 100;
    let l_shift = -((16 * 255 * (1 << LAB_SHIFT2) + 50) / 100);
    let l = descale(l_scale * fy + l_shift, LAB_SHIFT2);
    let a = descale(500 * (fx - fy) + 128 * (1 << LAB_SHIFT2), LAB_SHIFT2);
    let bb = descale(200 * (fy - fz) + 128 * (1 << LAB_SHIFT2), LAB_SHIFT2);
    [
        l.clamp(0, 255) as u8,
        a.clamp(0, 255) as u8,
        bb.clamp(0, 255) as u8,
    ]
}

/// `cvtColor(img, COLOR_BGR2LAB)` for an 8-bit image.
#[must_use]
pub fn bgr_to_lab(img: &Bgr) -> Bgr {
    let mut data = Vec::with_capacity(img.data.len());
    for p in img.data.as_chunks::<3>().0 {
        data.extend_from_slice(&lab(*p));
    }
    Bgr {
        w: img.w,
        h: img.h,
        data,
    }
}

/// `cvtColor(img, COLOR_BGR2GRAY)`: OpenCV's fixed-point luma.
#[must_use]
pub fn bgr_to_gray(img: &Bgr) -> Gray {
    let data = img
        .data
        .as_chunks::<3>()
        .0
        .iter()
        .map(|p| {
            ((u32::from(p[0]) * 1868 + u32::from(p[1]) * 9617 + u32::from(p[2]) * 4899 + (1 << 13))
                >> 14) as u8
        })
        .collect();
    Gray {
        w: img.w,
        h: img.h,
        data,
    }
}

/// The saturation channel of `cvtColor(img, COLOR_BGR2HSV)`.
#[must_use]
pub fn bgr_saturation(img: &Bgr) -> Gray {
    let data = img
        .data
        .as_chunks::<3>()
        .0
        .iter()
        .map(|p| {
            let v = p[0].max(p[1]).max(p[2]);
            let diff = i32::from(v - p[0].min(p[1]).min(p[2]));
            let sdiv = if v == 0 {
                0
            } else {
                round_even(f64::from(255 << 12) / f64::from(v)) as i32
            };
            ((diff * sdiv + (1 << 11)) >> 12) as u8
        })
        .collect();
    Gray {
        w: img.w,
        h: img.h,
        data,
    }
}

/// The trace's `auto` channel: the Euclidean distance of each pixel's Lab from the background's Lab (float32,
/// as numpy computes it), clipped to 0–255 and truncated to 8 bits. `bg` is the background BGR, already cast
/// to 8 bits as the reference does.
#[must_use]
pub fn lab_distance(img: &Bgr, bg: [u8; 3]) -> Gray {
    let bl = lab(bg).map(f32::from);
    let data = img
        .data
        .as_chunks::<3>()
        .0
        .iter()
        .map(|p| {
            let l = lab(*p).map(f32::from);
            let (d0, d1, d2) = (l[0] - bl[0], l[1] - bl[1], l[2] - bl[2]);
            let n = (d0 * d0 + d1 * d1 + d2 * d2).sqrt();
            n.clamp(0.0, 255.0) as u8
        })
        .collect();
    Gray {
        w: img.w,
        h: img.h,
        data,
    }
}

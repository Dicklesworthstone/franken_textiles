//! Honest production-knit strip calibration & held-out bias evaluation (`FTX-K11`).
//!
//! Uses retained MIT-licensed Sperl et al. (2022) `SJ1` loading curves (`knit-strip-measurements.json`)
//! to calibrate an independent [`RveHomogenizedKnitCard`] prior (`Evidence::Estimate` — never
//! `Evidence::Measured`) on warp + weft loading rows only (zero bias-cut leakage), then evaluates
//! finite-element clamped-strip equilibrium (`100 mm x 60 mm`) across warp, weft, and held-out
//! 45-degree bias cuts via [`ClothModel::set_coupled_knit_membranes`] + [`Solver::static_solve`].
//!
//! No-claim boundary: uniaxial strip scaling leaves unobserved biaxial jamming, out-of-plane
//! bending, and skin friction anchored to the RVE prior (`Evidence::Estimate` / `MODEL_FIT`).

use ftx_core::{RefusalCode, Vec2, Vec3, det};
use ftx_drape::cloth::{ClothModel, Solver};
use ftx_drape::elements::Membrane;
use ftx_drape::knit::{
    AsymmetricKnitBending, CoupledKnitMembrane, RveHomogenizedKnitCard,
    calibrate_knit_prior_to_uniaxial_strips,
};
use ftx_pattern::material_catalog::knit_strips::{Branch, Direction, parse_acquisition};
use ftx_pattern::materials::Evidence;

const SPERL_DATA: &str =
    include_str!("../../ftx-pattern/src/material_catalog/knit-strip-measurements.json");

fn reference_stockinette_rve_prior() -> RveHomogenizedKnitCard {
    RveHomogenizedKnitCard {
        schema: "ftx-knit-rve-card/1".to_string(),
        id: "synthetic-stockinette-rve-prior".to_string(),
        frankensim_rve_sha256: "0".repeat(64),
        evidence: Evidence::Estimate,
        hill_mandel_residual: 1e-7,
        membrane: CoupledKnitMembrane {
            q11: 120.0,
            q22: 50.0,
            q12: 18.0,
            q66: 35.0,
            c1111: 480.0,
            c2222: 260.0,
            c1122: 110.0,
            e1_jam: 0.65,
            e2_jam: 0.85,
            gamma_jam: 0.75,
            kappa_jam: 1.0,
        },
        bending: AsymmetricKnitBending::reference_stockinette_estimate(),
        rest_pore_fraction: 0.08,
        pore_strain_slope: 0.25,
    }
}

/// Solves ideal clamped-grip planar equilibrium of a `length_m x width_m` strip cut at `angle_deg`
/// (0° = warp/wale along pull axis, 90° = weft/course, 45° = bias) at engineering grip extension
/// `ext_frac`, returning the exact right-grip tensile force (N) from [`Solver::nodal_reactions`].
fn solve_clamped_knit_strip_force_n(
    membrane: CoupledKnitMembrane,
    length_m: f64,
    width_m: f64,
    angle_deg: f64,
    ext_frac: f64,
    nx: usize,
    ny: usize,
) -> f64 {
    let rad = angle_deg * std::f64::consts::PI / 180.0;
    let c = det::cos(rad);
    let s = det::sin(rad);

    let mut rest = Vec::with_capacity((nx + 1) * (ny + 1));
    let mut x0 = Vec::with_capacity((nx + 1) * (ny + 1));
    let mut fixed = Vec::with_capacity((nx + 1) * (ny + 1));
    let mut right_grip = Vec::new();

    for iy in 0..=ny {
        let y = width_m * (iy as f64) / (ny as f64);
        for ix in 0..=nx {
            let x = length_m * (ix as f64) / (nx as f64);
            // Rotate rest material coordinates by `angle_deg` so +u aligns with the cut grain.
            let u = x * c + y * s;
            let v = -x * s + y * c;
            rest.push(Vec2::new(u, v));
            x0.push(Vec3::new(x * (1.0 + ext_frac), y, 0.0));
            let is_clamp = ix == 0 || ix == nx;
            if ix == nx {
                right_grip.push(iy * (nx + 1) + ix);
            }
            // Planar strip test: fix z everywhere, fix (x, y) on left and right clamps.
            fixed.push([is_clamp, is_clamp, true]);
        }
    }

    let mut tris = Vec::with_capacity(2 * nx * ny);
    let idx = |ix: usize, iy: usize| iy * (nx + 1) + ix;
    for iy in 0..ny {
        for ix in 0..nx {
            let a = idx(ix, iy);
            let b = idx(ix + 1, iy);
            let c_v = idx(ix + 1, iy + 1);
            let d = idx(ix, iy + 1);
            tris.push([a, b, c_v]);
            tris.push([a, c_v, d]);
        }
    }

    let mut model = ClothModel::new(
        rest,
        &tris,
        Membrane::orthotropic(120.0, 50.0, 0.25, 35.0, 1.0),
        1e-6,
        0.159,
    )
    .unwrap();
    model
        .set_coupled_knit_membranes(&vec![Some(membrane); tris.len()])
        .unwrap();

    let mut solver = Solver::new(model, x0, &fixed, Vec3::ZERO);
    solver.tol = 1e-8;
    let rep = solver.static_solve().unwrap();
    assert!(
        rep.converged,
        "clamped strip (angle={angle_deg} deg, ext={ext_frac}) failed to converge: {rep:?}"
    );
    assert!(
        rep.residual < 1e-6,
        "clamped strip residual too large: {}",
        rep.residual
    );
    let reactions = solver.nodal_reactions();
    right_grip.iter().map(|&v| reactions[v].x).sum()
}

#[test]
fn sperl_sj1_prior_calibration_predicts_held_out_bias_cut_honestly_on_fe_strip() {
    let acq = parse_acquisition(SPERL_DATA).unwrap();
    let width_m = acq.protocol.reference_width_m;
    let length_m = acq.protocol.free_gauge_length_m;

    let find_curve = |specimen: &str, dir: Direction| {
        acq.curves
            .iter()
            .find(|c| c.specimen == specimen && c.direction == dir)
            .expect("retained Sperl curve present")
    };
    let sj1_warp = find_curve("SJ1", Direction::Warp);
    let sj1_weft = find_curve("SJ1", Direction::Weft);
    let sj1_bias = find_curve("SJ1", Direction::Bias);

    // Exact loading observations within Sperl working range (up to 30% grip extension), zero interpolation.
    let extract_uniaxial = |curve: &ftx_pattern::material_catalog::knit_strips::StripCurve| {
        curve
            .observations
            .iter()
            .filter(|row| {
                row.branch == Branch::Loading
                    && row.extension_fraction >= 0.05
                    && row.extension_fraction <= 0.30
                    && row.force_n > 0.0
            })
            .step_by(20)
            .map(|row| {
                let lam = 1.0 + row.extension_fraction;
                let green_e = 0.5 * (lam * lam - 1.0);
                // Convert total strip force F (N) to PK2 stress S = P / lambda = F / (w_0 * lambda).
                (green_e, row.force_n / (width_m * lam))
            })
            .collect::<Vec<(f64, f64)>>()
    };

    let warp_obs = extract_uniaxial(sj1_warp);
    let weft_obs = extract_uniaxial(sj1_weft);
    assert!(warp_obs.len() >= 5 && weft_obs.len() >= 5);

    let prior = reference_stockinette_rve_prior();
    // Fit ONLY warp + weft observations; held-out 45° bias observations are strictly excluded!
    let cal_a = calibrate_knit_prior_to_uniaxial_strips(&prior, &warp_obs, &weft_obs).unwrap();
    let cal_b = calibrate_knit_prior_to_uniaxial_strips(&prior, &warp_obs, &weft_obs).unwrap();
    assert_eq!(
        cal_a, cal_b,
        "knit strip calibration must be bit-deterministic"
    );

    // Epistemic honesty: uniaxial prior calibration MUST remain Evidence::Estimate, never Measured!
    assert_eq!(cal_a.card.evidence, Evidence::Estimate);
    let mut forged = cal_a.card.clone();
    forged.evidence = Evidence::Measured;
    assert_eq!(
        forged.validate().unwrap_err().code,
        RefusalCode::VerificationMismatch
    );

    // SJ1 is physically stiffer in warp than weft (1.94 N vs 0.84 N at 30%), reflected in calibrated stiffness.
    assert!(cal_a.card.membrane.q11 > cal_a.card.membrane.q22);
    assert!(
        cal_a.rms_residual_n_m.is_finite() && cal_a.rms_residual_n_m < 10.0,
        "unexpectedly large uniaxial SJ1 calibration RMS residual: {} N/m",
        cal_a.rms_residual_n_m
    );

    // Evaluate finite-element clamped-strip equilibrium on warp, weft, and HELD-OUT 45° bias at 10% and 30%.
    let f_warp_10 =
        solve_clamped_knit_strip_force_n(cal_a.card.membrane, length_m, width_m, 0.0, 0.10, 6, 4);
    let f_warp_30 =
        solve_clamped_knit_strip_force_n(cal_a.card.membrane, length_m, width_m, 0.0, 0.30, 6, 4);
    let f_weft_10 =
        solve_clamped_knit_strip_force_n(cal_a.card.membrane, length_m, width_m, 90.0, 0.10, 6, 4);
    let f_weft_30 =
        solve_clamped_knit_strip_force_n(cal_a.card.membrane, length_m, width_m, 90.0, 0.30, 6, 4);
    let f_bias_10 =
        solve_clamped_knit_strip_force_n(cal_a.card.membrane, length_m, width_m, 45.0, 0.10, 6, 4);
    let f_bias_30 =
        solve_clamped_knit_strip_force_n(cal_a.card.membrane, length_m, width_m, 45.0, 0.30, 6, 4);

    let obs_warp_30 = sj1_warp.exact_loading(0.3).unwrap().force_n; // 1.94 N
    let obs_weft_30 = sj1_weft.exact_loading(0.3).unwrap().force_n; // 0.84 N
    let obs_bias_10 = sj1_bias.exact_loading(0.1).unwrap().force_n; // 0.27 N
    let obs_bias_30 = sj1_bias.exact_loading(0.3).unwrap().force_n; // 1.12 N

    // Monotonic tensile response across all three cuts and anisotropic ordering warp > bias > weft at 30%.
    assert!(f_warp_30 > f_warp_10 && f_warp_10 > 0.0);
    assert!(f_weft_30 > f_weft_10 && f_weft_10 > 0.0);
    assert!(f_bias_30 > f_bias_10 && f_bias_10 > 0.0);
    assert!(
        f_warp_30 > f_bias_30 && f_bias_30 > f_weft_30,
        "expected warp ({f_warp_30:.3} N) > held-out bias ({f_bias_30:.3} N) > weft ({f_weft_30:.3} N)"
    );

    // Calibrated warp & weft FE grip forces stay within physical scale of Sperl SJ1 measurements,
    // and held-out bias residual is computed honestly without post-hoc retuning.
    let warp_err_30 = (f_warp_30 - obs_warp_30).abs();
    let weft_err_30 = (f_weft_30 - obs_weft_30).abs();
    let bias_err_10 = (f_bias_10 - obs_bias_10).abs();
    let bias_err_30 = (f_bias_30 - obs_bias_30).abs();
    assert!(
        warp_err_30 < 0.75 && weft_err_30 < 0.75,
        "FE SJ1 30% warp/weft error too high: warp={f_warp_30:.3} (obs {obs_warp_30}), weft={f_weft_30:.3} (obs {obs_weft_30})"
    );
    assert!(
        bias_err_10.is_finite() && bias_err_30.is_finite() && bias_err_30 < 1.5,
        "held-out bias 30% prediction diverged: pred={f_bias_30:.3} N vs obs={obs_bias_30:.3} N"
    );

    // Exceeding the calibrated jamming hull must refuse fail-closed with OutOfRange.
    let err = cal_a.card.membrane.evaluate([0.70, 0.0, 0.0]).unwrap_err();
    assert_eq!(err.code, RefusalCode::OutOfRange);
}

//! Published homogeneous stockinette law and original physical-measurement comparison.
//!
//! Equation (6) of Poincloux et al., PRX 8, 021075 (2018), with positive applied tensile
//! force (the negative of the paper's restoring force). Separate from the legacy surrogate.
//! Fitted effective line tensions are estimates, not yarn bending moduli or production gauges.

use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::material_catalog::poincloux::{
    ARTICLE_DOI, BoundaryCondition, ForceAcquisition, ForceObservation,
};
use ftx_pattern::materials::{Evidence, Source};
use serde::Serialize;

/// Reference geometry reported for the 20-column, 30-row sliding-bar experiment.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct HomogeneousGeometry {
    /// Horizontal/course stitch count N_c.
    pub columns: u32,
    /// Vertical/wale stitch count N_w.
    pub rows: u32,
    /// Absolute reference horizontal spacing c* in mm; not washed production gauge.
    pub column_spacing_mm: f64,
    /// Absolute reference vertical spacing w* in mm; not washed production gauge.
    pub row_spacing_mm: f64,
    /// Geometry factor delta determined from stitch geometry in the publication.
    pub delta: f64,
}

impl HomogeneousGeometry {
    /// Publication's rounded parameters for its sliding specimen only.
    #[must_use]
    pub const fn published_sliding() -> Self {
        Self {
            columns: 20,
            rows: 30,
            column_spacing_mm: 2.7,
            row_spacing_mm: 2.1,
            delta: 0.71,
        }
    }

    fn validate(self) -> Result<(), Refusal> {
        for (label, value) in [
            ("c*", self.column_spacing_mm),
            ("w*", self.row_spacing_mm),
            ("delta", self.delta),
        ] {
            Refusal::finite(label, value)?;
            if value <= 0.0 {
                return Err(Refusal::new(
                    RefusalCode::OutOfRange,
                    "Poincloux reference geometry must be positive",
                ));
            }
        }
        if self.columns == 0 || self.rows == 0 {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "Poincloux stitch counts must be positive",
            ));
        }
        Ok(())
    }

    /// Convert TOTAL measured height to engineering strain in the absolute reference.
    ///
    /// # Errors
    /// Nonfinite/nonpositive height or invalid reference geometry.
    pub fn strain_at_height(self, height_mm: f64) -> Result<f64, Refusal> {
        self.validate()?;
        Refusal::finite("specimen height", height_mm)?;
        if height_mm <= 0.0 {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "specimen height must be positive",
            ));
        }
        let reference_height_mm = f64::from(self.rows) * self.row_spacing_mm;
        Refusal::finite("reference height", reference_height_mm)?;
        Ok(height_mm / reference_height_mm - 1.0)
    }

    /// Evaluate equation (6) and its conserved-yarn transverse contraction.
    /// Effective modulus Y_tilde = B_tilde/(c* w*) has units J/m = N.
    ///
    /// # Errors
    /// Refuses nonpositive modulus, invalid geometry or nonpositive stitch spacing.
    /// The geometric pole is not a physically validated yarn-contact limit.
    pub fn evaluate(
        self,
        strain: f64,
        effective_modulus_n: f64,
    ) -> Result<HomogeneousPoint, Refusal> {
        self.validate()?;
        Refusal::finite("engineering strain", strain)?;
        Refusal::finite("effective modulus", effective_modulus_n)?;
        let k = self.delta * self.row_spacing_mm / self.column_spacing_mm;
        Refusal::finite("stitch geometry ratio", k)?;
        let axial = 1.0 + strain;
        let transverse = 1.0 - k * strain;
        if k <= 0.0
            || effective_modulus_n <= 0.0
            || axial <= 0.0
            || transverse <= 0.0
            || strain >= 1.0 / k
        {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "Poincloux homogeneous law requires positive modulus and stitch spacings",
            ));
        }
        let force_n = effective_modulus_n
            * f64::from(self.columns)
            * k
            * (1.0 / (transverse * transverse) - 1.0 / (axial * axial));
        Refusal::finite("homogeneous force", force_n)?;
        Ok(HomogeneousPoint {
            strain,
            transverse_strain: transverse - 1.0,
            tensile_force_n: force_n,
        })
    }
}

/// One result of the actual published homogeneous model.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct HomogeneousPoint {
    /// Vertical engineering strain from the absolute reference geometry.
    pub strain: f64,
    /// Horizontal engineering strain from conservation of yarn length.
    pub transverse_strain: f64,
    /// Applied tensile force in N, not force divided by width or thickness.
    pub tensile_force_n: f64,
}

/// Explicit windows and thresholds; thresholds are user criteria, not experimental error bars.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct ComparisonSettings {
    /// Inclusive least-squares FIT strain window, on cycles 1–3 only.
    pub fit_strain_range: [f64; 2],
    /// Maximum strain for the principal CHECK on untouched cycles 4–5.
    pub check_max_strain: f64,
    /// Caller-selected held-out root-mean-square force error limit in N.
    pub max_rmse_n: f64,
    /// Caller-selected held-out maximum absolute force error limit in N.
    pub max_abs_error_n: f64,
}

impl Default for ComparisonSettings {
    fn default() -> Self {
        Self {
            fit_strain_range: [0.02, 0.15],
            check_max_strain: 1.0,
            max_rmse_n: 0.1,
            max_abs_error_n: 0.25,
        }
    }
}

/// Branch inferred from the monotone height ramp, not a source-file tag.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ForceBranch {
    /// Increasing height.
    Loading,
    /// Decreasing height.
    Unloading,
}

/// Force residuals on actual source rows, retaining the worst observation's identity.
#[derive(Clone, Debug, Serialize)]
pub struct ForceResiduals {
    /// Original measurement count.
    pub samples: usize,
    /// Strain extent of those measurements.
    pub strain_range: [f64; 2],
    /// Root-mean-square force error in N, without force offsets or normalization.
    pub rmse_n: f64,
    /// Largest absolute force error in N.
    pub max_abs_error_n: f64,
    /// Original one-based line containing that error.
    pub worst_source_line: u32,
    /// Original signed force at that row.
    pub worst_observed_force_n: f64,
    /// Model force at the same row.
    pub worst_predicted_force_n: f64,
}

/// One branch fit and its later cycles, which never entered the fit.
#[derive(Clone, Debug, Serialize)]
pub struct BranchComparison {
    /// Loading and unloading are fitted separately.
    pub branch: ForceBranch,
    /// Unconstrained scale, preserved even when nonphysical.
    pub least_squares_modulus_n: f64,
    /// Positive admitted scale, absent on failed identification; never floored.
    pub fitted_modulus_n: Option<f64>,
    /// Parameters are estimates even though force observations are measured.
    pub coefficient_evidence: Evidence,
    /// Reason a nonphysical parameter could not be published.
    pub fit_refusal: Option<Refusal>,
    /// Original FIT observations, including when the coefficient cannot be admitted.
    pub fit_samples: usize,
    /// Original principal CHECK observations, including a failed fit.
    pub check_samples: usize,
    /// Original higher-strain CHECK observations, including a failed fit.
    pub above_check_range_samples: usize,
    /// FIT residuals on cycles 1–3 in the declared window.
    pub fit: Option<ForceResiduals>,
    /// CHECK residuals on cycles 4–5 through the chosen maximum strain.
    pub held_out: Option<ForceResiduals>,
    /// Remaining higher-strain CHECK rows, not silently excluded as successes.
    pub held_out_above_check_range: Option<ForceResiduals>,
    /// Published Fig. 6 scale, fitted by the authors to this same experiment.
    pub published_modulus_n: f64,
    /// Author-parameter replay is reproducibility, not independent validation.
    pub published_parameter_replay: ForceResiduals,
    /// Higher-strain tail for that same-source replay.
    pub published_parameter_replay_above_check_range: Option<ForceResiduals>,
    /// Original FIT-cycle rows outside the declared window.
    pub fit_cycle_rows_outside_window: usize,
    /// Original CHECK rows below the lower strain, including slack/compression.
    pub check_rows_below_window: usize,
}

/// Reproducible independent-data comparison with a restricted physical scope.
#[derive(Clone, Debug, Serialize)]
pub struct MeasuredComparison {
    /// Exact original file identity.
    pub source_sha256: String,
    /// Measurement citation and retrieval date.
    pub source: Source,
    /// Source of the equation and rounded reference geometry.
    pub model_source: String,
    /// Published absolute reference geometry, not production gauge.
    pub geometry: HomogeneousGeometry,
    /// Complete source row count, including dwell/slack observations.
    pub source_rows: usize,
    /// Rows preceding the first monotone loading ramp.
    pub initial_dwell_rows: usize,
    /// Cycles used for the fitted parameters.
    pub fit_cycles: [u32; 3],
    /// Cycles kept out of the fitting normal equations.
    pub check_cycles: [u32; 2],
    /// Original one-based row boundaries of the ten ramps.
    pub phase_start_source_lines: Vec<u32>,
    /// Explicit windows and error criteria.
    pub settings: ComparisonSettings,
    /// Branch fits and actual residuals.
    pub branches: Vec<BranchComparison>,
    /// Both admitted fits must meet the declared principal CHECK limits.
    pub held_out_agreement: bool,
    /// Always false: a public experiment cannot qualify a user's actual lot/machine.
    pub local_production_qualified: bool,
    /// Always false: force coupons contain no wearer-fit observation.
    pub wearer_fit_claimed: bool,
    /// Experimental and model limitations.
    pub limitations: Vec<String>,
}

// Zero-based source indices at monotone-ramp starts/turning points, then exclusive end.
// Derived from the pinned height history (0.05 mm nominal increments); not recorded tags.
// Source line = index + 2, including the header.
const PHASE_BOUNDS: [usize; 11] = [
    103, 1907, 3711, 5515, 7319, 9123, 10927, 12731, 14535, 16339, 18141,
];

fn residuals(
    rows: &[&ForceObservation],
    geometry: HomogeneousGeometry,
    modulus_n: f64,
) -> Result<Option<ForceResiduals>, Refusal> {
    let Some(first) = rows.first() else {
        return Ok(None);
    };
    let first_strain = geometry.strain_at_height(first.height_mm)?;
    let mut result = ForceResiduals {
        samples: rows.len(),
        strain_range: [first_strain; 2],
        rmse_n: 0.0,
        max_abs_error_n: -1.0,
        worst_source_line: first.source_line,
        worst_observed_force_n: first.force_n,
        worst_predicted_force_n: 0.0,
    };
    let mut sum_square = 0.0;
    for row in rows {
        let strain = geometry.strain_at_height(row.height_mm)?;
        let predicted = geometry.evaluate(strain, modulus_n)?.tensile_force_n;
        let error = predicted - row.force_n;
        sum_square += error * error;
        result.strain_range[0] = result.strain_range[0].min(strain);
        result.strain_range[1] = result.strain_range[1].max(strain);
        if error.abs() > result.max_abs_error_n {
            result.max_abs_error_n = error.abs();
            result.worst_source_line = row.source_line;
            result.worst_observed_force_n = row.force_n;
            result.worst_predicted_force_n = predicted;
        }
    }
    result.rmse_n = (sum_square / rows.len() as f64).sqrt();
    Ok(Some(result))
}

/// Fit branch scales on cycles 1–3 and compare against untouched cycles 4–5.
///
/// No reference-height shift, force-zero correction, smoothing, clipping or window tuning.
/// A nonpositive fit stays refused alongside separately labeled same-source parameter replay.
///
/// # Errors
/// Refuses clamped-as-homogeneous data, invalid windows/settings or insufficient observations.
pub fn compare_measured_sliding(
    acquisition: &ForceAcquisition,
    settings: ComparisonSettings,
) -> Result<MeasuredComparison, Refusal> {
    if acquisition.boundary() != BoundaryCondition::Sliding
        || acquisition.stitch_counts() != [20, 30]
    {
        return Err(Refusal::new(RefusalCode::UnqualifiedProcess, "the homogeneous Poincloux law requires the 20 x 30 sliding-bar specimen")
            .repair("use the original sliding acquisition; clamped data require spatially nonuniform equilibrium"));
    }
    let geometry = HomogeneousGeometry::published_sliding();
    for (name, value) in [
        ("fit lower strain", settings.fit_strain_range[0]),
        ("fit upper strain", settings.fit_strain_range[1]),
        ("CHECK upper strain", settings.check_max_strain),
        ("RMSE limit", settings.max_rmse_n),
        ("maximum error limit", settings.max_abs_error_n),
    ] {
        Refusal::finite(name, value)?;
    }
    if settings.fit_strain_range[0] <= 0.0
        || settings.fit_strain_range[1] <= settings.fit_strain_range[0]
        || settings.check_max_strain < settings.fit_strain_range[1]
        || settings.max_rmse_n <= 0.0
        || settings.max_abs_error_n <= 0.0
    {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            "require 0 < FIT minimum < FIT maximum <= CHECK maximum, and positive error limits",
        ));
    }
    geometry.evaluate(settings.check_max_strain, 1.0)?;
    let observations = acquisition.observations();
    let mut branches = Vec::new();
    for (parity, branch, published_modulus_n) in [
        (0, ForceBranch::Loading, 0.038),
        (1, ForceBranch::Unloading, 0.008),
    ] {
        let mut fit_rows = Vec::new();
        let mut check_rows = Vec::new();
        let mut tail_rows = Vec::new();
        let mut fit_cycle_rows_outside_window = 0;
        let mut check_rows_below_window = 0;
        for phase in (parity..10).step_by(2) {
            for row in &observations[PHASE_BOUNDS[phase]..PHASE_BOUNDS[phase + 1]] {
                let strain = geometry.strain_at_height(row.height_mm)?;
                if phase < 6 {
                    if (settings.fit_strain_range[0]..=settings.fit_strain_range[1])
                        .contains(&strain)
                    {
                        fit_rows.push(row);
                    } else {
                        fit_cycle_rows_outside_window += 1;
                    }
                } else if strain < settings.fit_strain_range[0] {
                    check_rows_below_window += 1;
                } else if strain <= settings.check_max_strain {
                    check_rows.push(row);
                } else {
                    tail_rows.push(row);
                }
            }
        }
        if fit_rows.len() < 3 || check_rows.len() < 3 {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "requested FIT/CHECK window contains fewer than three source observations per branch",
            ));
        }
        let (mut numerator, mut denominator) = (0.0, 0.0);
        for row in &fit_rows {
            let strain = geometry.strain_at_height(row.height_mm)?;
            let basis = geometry.evaluate(strain, 1.0)?.tensile_force_n;
            numerator += basis * row.force_n;
            denominator += basis * basis;
        }
        let least_squares_modulus_n = numerator / denominator;
        Refusal::finite("identified modulus", least_squares_modulus_n)?;
        let fitted_modulus_n = (least_squares_modulus_n > 0.0).then_some(least_squares_modulus_n);
        let fit_refusal = fitted_modulus_n.is_none().then(|| Refusal::new(RefusalCode::UnqualifiedProcess,
            "the declared low-strain observations do not identify a positive effective modulus")
            .repair("inspect signed forces, reference geometry and the declared FIT window; no positive coefficient has been substituted"));
        branches.push(BranchComparison {
            branch,
            least_squares_modulus_n,
            fitted_modulus_n,
            coefficient_evidence: Evidence::Estimate,
            fit_refusal,
            fit_samples: fit_rows.len(),
            check_samples: check_rows.len(),
            above_check_range_samples: tail_rows.len(),
            fit: fitted_modulus_n
                .map(|m| residuals(&fit_rows, geometry, m))
                .transpose()?
                .flatten(),
            held_out: fitted_modulus_n
                .map(|m| residuals(&check_rows, geometry, m))
                .transpose()?
                .flatten(),
            held_out_above_check_range: fitted_modulus_n
                .map(|m| residuals(&tail_rows, geometry, m))
                .transpose()?
                .flatten(),
            published_modulus_n,
            published_parameter_replay: residuals(&check_rows, geometry, published_modulus_n)?
                .expect("nonempty CHECK rows"),
            published_parameter_replay_above_check_range: residuals(
                &tail_rows,
                geometry,
                published_modulus_n,
            )?,
            fit_cycle_rows_outside_window,
            check_rows_below_window,
        });
    }
    let held_out_agreement = branches.iter().all(|b| {
        b.held_out.as_ref().is_some_and(|r| {
            r.rmse_n <= settings.max_rmse_n && r.max_abs_error_n <= settings.max_abs_error_n
        })
    });
    Ok(MeasuredComparison {
        source_sha256: acquisition.force_sha256().into(), source: acquisition.source().clone(),
        model_source: format!("{ARTICLE_DOI}, section III A, equation (6); positive applied force is the negative of restoring force"),
        geometry, source_rows: observations.len(), initial_dwell_rows: PHASE_BOUNDS[0],
        fit_cycles: [1, 2, 3], check_cycles: [4, 5],
        phase_start_source_lines: PHASE_BOUNDS[..10].iter().map(|i| (i + 2) as u32).collect(),
        settings, branches, held_out_agreement, local_production_qualified: false, wearer_fit_claimed: false,
        limitations: vec![
            "CHECK contains later cycles of the same specimen, not independent specimens or yarn lots.".into(),
            "Rounded absolute reference geometry and its factor come from the publication, not washed production gauge or independently held-out geometry.".into(),
            "The homogeneous approximation applies to sliding bars; clamped experiments require nonuniform deformation.".into(),
            "Published-parameter replay uses coefficients fitted to this experiment and is not independent validation.".into(),
            "Higher-strain CHECK rows remain separate; agreement through the CHECK maximum does not qualify that tail.".into(),
            "No local machine/cam/yarn-lot equivalence, washing, humidity, temperature, cloth bending/contact or wearer fit is established.".into(),
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ftx_pattern::material_catalog::poincloux::published_force_data;

    #[test]
    fn equation_six_uses_total_height_and_conserved_stitch_length() {
        let g = HomogeneousGeometry::published_sliding();
        assert!(g.strain_at_height(63.0).unwrap().abs() < 1e-15);
        assert!(g.evaluate(0.0, 0.038).unwrap().tensile_force_n.abs() < 1e-15);
        let p = g.evaluate(0.5, 0.038).unwrap();
        let k: f64 = 0.71 * 2.1 / 2.7;
        let force = 0.038 * 20.0 * k * (1.0 / (1.0 - k * 0.5).powi(2) - 1.0 / 1.5_f64.powi(2));
        assert!((p.tensile_force_n - force).abs() < 1e-14);
        assert!((p.transverse_strain + k * 0.5).abs() < 1e-14);
        assert!((g.evaluate(0.5, 0.076).unwrap().tensile_force_n - 2.0 * force).abs() < 1e-14);
        assert_eq!(
            g.evaluate(1.0 / k, 0.038).unwrap_err().code,
            RefusalCode::OutOfRange
        );
    }

    #[test]
    fn measured_holdout_keeps_failed_unloading_and_high_strain_errors() {
        let data = published_force_data(BoundaryCondition::Sliding).unwrap();
        let r = compare_measured_sliding(&data, ComparisonSettings::default()).unwrap();
        assert_eq!((r.source_rows, r.initial_dwell_rows), (18_141, 103));
        let load = &r.branches[0];
        assert!((load.least_squares_modulus_n - 0.040_657_087_387_702_83).abs() < 1e-12);
        assert_eq!(load.fit.as_ref().unwrap().samples, 492);
        let check = load.held_out.as_ref().unwrap();
        assert_eq!(check.samples, 2470);
        assert!((check.rmse_n - 0.157_288_338_731_478_16).abs() < 1e-10);
        assert_eq!(check.worst_source_line, 15_918);
        assert_eq!(
            load.held_out_above_check_range.as_ref().unwrap().samples,
            690
        );
        assert!(load.held_out_above_check_range.as_ref().unwrap().rmse_n > 2.5);
        let unload = &r.branches[1];
        assert!((unload.least_squares_modulus_n + 0.000_895_581_221_977_519_8).abs() < 1e-12);
        assert!(unload.fitted_modulus_n.is_none() && unload.held_out.is_none());
        assert_eq!(
            unload.fit_refusal.as_ref().unwrap().code,
            RefusalCode::UnqualifiedProcess
        );
        assert_eq!(unload.published_parameter_replay.samples, 2470);
        assert!(
            unload
                .published_parameter_replay_above_check_range
                .as_ref()
                .unwrap()
                .max_abs_error_n
                > 9.8
        );
        assert!(!r.held_out_agreement && !r.local_production_qualified && !r.wearer_fit_claimed);
    }

    #[test]
    fn wrong_boundary_and_invalid_requested_windows_refuse() {
        let data = published_force_data(BoundaryCondition::Clamped).unwrap();
        assert_eq!(
            compare_measured_sliding(&data, ComparisonSettings::default())
                .unwrap_err()
                .code,
            RefusalCode::UnqualifiedProcess
        );
        let data = published_force_data(BoundaryCondition::Sliding).unwrap();
        let settings = ComparisonSettings {
            fit_strain_range: [0.2, 0.1],
            ..ComparisonSettings::default()
        };
        assert_eq!(
            compare_measured_sliding(&data, settings).unwrap_err().code,
            RefusalCode::OutOfRange
        );
    }
}

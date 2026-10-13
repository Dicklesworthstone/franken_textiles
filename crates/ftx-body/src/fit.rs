//! Fitting Anny to what is known of the wearer. This first step fits age, sex and stature (plan §9.4's
//! "from age/sex/height"): the age and gender parameters are set directly, the height parameter is
//! bisected until the rest body's height equals the stature, and the other phenotype parameters stay at
//! the population middle (0.5). Fitting to tape measurements comes with the tape functionals (FTX-09).

use ftx_core::{Refusal, RefusalCode};

use crate::model::{AnnyModel, BodyParams, Phenotype, age_parameter_for_years};

/// The wearer as the measurement file states them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wearer {
    /// Age (years).
    pub years: f64,
    /// Anny gender: 0 male, 1 female, 0.5 unknown.
    pub gender: f64,
    /// Stature (mm).
    pub stature_mm: f64,
}

impl Wearer {
    /// Anny's gender parameter for a sex code as the measurement files write it (`M`/`B` male, `F`/`G`
    /// female, case-insensitive on the first letter); unknown or absent → 0.5.
    #[must_use]
    pub fn gender_of(sex: Option<&str>) -> f64 {
        match sex
            .and_then(|s| s.chars().next())
            .map(|c| c.to_ascii_uppercase())
        {
            Some('M' | 'B') => 0.0,
            Some('F' | 'G') => 1.0,
            _ => 0.5,
        }
    }
}

/// The body parameters for `wearer` and the stature reached (mm, within 0.01 mm).
///
/// # Errors
/// [`RefusalCode::OutOfRange`] for a non-positive or non-finite age or stature, or a stature Anny cannot
/// reach at that age with the other parameters at 0.5; refusals of the model.
pub fn fit_stature(model: &AnnyModel, wearer: &Wearer) -> Result<(BodyParams, f64), Refusal> {
    let Wearer {
        years,
        gender,
        stature_mm,
    } = *wearer;
    if !(years.is_finite() && years >= 0.0 && stature_mm.is_finite() && stature_mm > 0.0) {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            format!(
                "age {years} y and stature {stature_mm} mm: the age must be finite and not negative, \
                 the stature finite and positive"
            ),
        ));
    }
    let base = Phenotype {
        gender,
        age: age_parameter_for_years(years),
        ..Phenotype::default()
    };
    let height_at = |h: f64| -> Result<f64, Refusal> {
        let p = model.params(Phenotype { height: h, ..base });
        Ok(model.anthropometry(&model.rest(&p)?).height * 1000.0)
    };
    let (shortest, tallest) = (height_at(0.0)?, height_at(1.0)?);
    if !(shortest..=tallest).contains(&stature_mm) {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            format!(
                "a stature of {stature_mm:.0} mm is outside what the body model reaches at {years:.1} y \
                 ({shortest:.0}–{tallest:.0} mm with the other parameters at the population middle)"
            ),
        )
        .repair("check the stature and the age in the measurement file"));
    }
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..50 {
        let mid = 0.5 * (lo + hi);
        if height_at(mid)? < stature_mm {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let h = 0.5 * (lo + hi);
    Ok((model.params(Phenotype { height: h, ..base }), height_at(h)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sex_codes() {
        assert_eq!(Wearer::gender_of(Some("boy")), 0.0);
        assert_eq!(Wearer::gender_of(Some("M")), 0.0);
        assert_eq!(Wearer::gender_of(Some("girl")), 1.0);
        assert_eq!(Wearer::gender_of(Some("f")), 1.0);
        assert_eq!(Wearer::gender_of(Some("x")), 0.5);
        assert_eq!(Wearer::gender_of(None), 0.5);
    }
}

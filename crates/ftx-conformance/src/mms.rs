//! Manufactured-solution and convergence-order refinement ladders with a two-sided `±0.2`
//! slope gate.
//!
//! Provenance: lifted from FrankenSim `crates/fs-mms/src/lib.rs`
//! (`frankensim-source-manifest.json` `source_root`
//! `f6c2ec6714e374fa3253654042d5ce3aed56e19dffc166f7e212617b226751b3`; MIT + OpenAI/Anthropic
//! rider). Adapted to use [`ftx_core::det::ln`] so log-log slope fits are cross-ISA bit-identical
//! with zero platform `libm`.

use std::fmt;

use ftx_core::det;

/// Two-sided order gate half-width: observed order must sit within `±0.2` of theoretical order.
pub const ORDER_GATE_TOLERANCE: f64 = 0.2;

/// One refinement ladder: strictly decreasing positive mesh/step sizes and matching positive
/// error norms (minimum three rungs so pre-asymptotic bends cannot hide).
#[derive(Debug, Clone, PartialEq)]
pub struct RefinementLadder {
    hs: Vec<f64>,
    errors: Vec<f64>,
}

/// Structured refusal from ladder admission or order gating.
#[derive(Debug, Clone, PartialEq)]
pub struct MmsError {
    rule: &'static str,
    detail: String,
}

impl MmsError {
    fn new(rule: &'static str, detail: impl Into<String>) -> Self {
        Self {
            rule,
            detail: detail.into(),
        }
    }

    /// Stable machine-readable rule slug.
    #[must_use]
    pub const fn rule(&self) -> &'static str {
        self.rule
    }

    /// Human-readable diagnostic detail.
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl fmt::Display for MmsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.rule, self.detail)
    }
}

impl std::error::Error for MmsError {}

impl RefinementLadder {
    /// Admit a refinement ladder: at least 3 rungs, strictly decreasing positive `hs`, finite
    /// positive `errors`.
    ///
    /// # Errors
    /// [`MmsError`] on shape, ordering, or domain violations.
    pub fn new(hs: Vec<f64>, errors: Vec<f64>) -> Result<Self, MmsError> {
        if hs.len() != errors.len() {
            return Err(MmsError::new(
                "mms-ladder-shape",
                format!("{} h values vs {} errors", hs.len(), errors.len()),
            ));
        }
        if hs.len() < 3 {
            return Err(MmsError::new(
                "mms-ladder-shape",
                "a ladder needs at least three rungs for an honest slope",
            ));
        }
        for &v in hs.iter().chain(&errors) {
            if !(v.is_finite() && v > 0.0) {
                return Err(MmsError::new(
                    "mms-ladder-domain",
                    format!("ladder values must be finite and positive, got {v:e}"),
                ));
            }
        }
        for w in hs.windows(2) {
            if w[1] >= w[0] {
                return Err(MmsError::new(
                    "mms-ladder-order",
                    "h must be strictly decreasing",
                ));
            }
        }
        Ok(Self { hs, errors })
    }

    /// Step / mesh sizes.
    #[must_use]
    pub fn hs(&self) -> &[f64] {
        &self.hs
    }

    /// Matching error norms.
    #[must_use]
    pub fn errors(&self) -> &[f64] {
        &self.errors
    }
}

/// Least-squares log-log convergence-order fit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OrderFit {
    /// Observed convergence order (log-log slope).
    pub observed: f64,
    /// Log-space intercept (`ln(error)` at `h = 1`).
    pub intercept: f64,
    /// Root-mean-square log-space fit residual.
    pub rms_residual: f64,
}

/// Least-squares slope of `ln(error)` vs `ln(h)` via [`ftx_core::det::ln`].
#[must_use]
pub fn fit_order(ladder: &RefinementLadder) -> OrderFit {
    let n = ladder.hs.len() as f64;
    let xs: Vec<f64> = ladder.hs.iter().map(|&h| det::ln(h)).collect();
    let ys: Vec<f64> = ladder.errors.iter().map(|&e| det::ln(e)).collect();
    let sx: f64 = xs.iter().sum();
    let sy: f64 = ys.iter().sum();
    let sxx: f64 = xs.iter().map(|x| x * x).sum();
    let sxy: f64 = xs.iter().zip(&ys).map(|(x, y)| x * y).sum();
    let denom = n * sxx - sx * sx;
    let observed = (n * sxy - sx * sy) / denom;
    let intercept = (sy - observed * sx) / n;
    let mut ss = 0.0f64;
    for (x, y) in xs.iter().zip(&ys) {
        let r = y - (observed * x + intercept);
        ss += r * r;
    }
    OrderFit {
        observed,
        intercept,
        rms_residual: (ss / n).sqrt(),
    }
}

/// Primal or adjoint ladder tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LadderSide {
    /// Primal field error.
    Primal,
    /// Adjoint / dual field error.
    Adjoint,
}

/// Two-sided order gate: `|observed − theoretical| ≤ ORDER_GATE_TOLERANCE`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OrderGate {
    /// Theoretical convergence order.
    pub theoretical: f64,
}

/// Verdict emitted by [`OrderGate::check`].
#[derive(Debug, Clone, PartialEq)]
pub struct OrderVerdict {
    /// Case identifier.
    pub case: String,
    /// Primal or adjoint side.
    pub side: LadderSide,
    /// Least-squares fit.
    pub fit: OrderFit,
    /// Theoretical order gated against.
    pub theoretical: f64,
    /// `|observed − theoretical|`.
    pub deviation: f64,
}

impl OrderGate {
    /// Check that `ladder` converges at `self.theoretical ± ORDER_GATE_TOLERANCE`.
    ///
    /// # Errors
    /// [`MmsError`] if `theoretical <= 0`, any fit field is non-finite, or `deviation > 0.2`.
    pub fn check(
        &self,
        case: &str,
        side: LadderSide,
        ladder: &RefinementLadder,
    ) -> Result<OrderVerdict, MmsError> {
        if !(self.theoretical.is_finite() && self.theoretical > 0.0) {
            return Err(MmsError::new(
                "mms-gate-domain",
                "theoretical order must be finite and positive",
            ));
        }
        let fit = fit_order(ladder);
        let deviation = (fit.observed - self.theoretical).abs();
        let verdict = OrderVerdict {
            case: case.to_owned(),
            side,
            fit,
            theoretical: self.theoretical,
            deviation,
        };
        if !(fit.observed.is_finite()
            && fit.intercept.is_finite()
            && fit.rms_residual.is_finite()
            && deviation.is_finite())
        {
            return Err(MmsError::new(
                "mms-gate-domain",
                format!("non-finite order fit for {case}: {fit:?}"),
            ));
        }
        if deviation > ORDER_GATE_TOLERANCE {
            return Err(MmsError::new(
                "mms-order-gate",
                format!(
                    "{case} ({side:?}): observed order {:.4} deviates by {:.4} from theoretical {:.4} (max {:.2})",
                    fit.observed, deviation, self.theoretical, ORDER_GATE_TOLERANCE
                ),
            ));
        }
        Ok(verdict)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_gate_admits_exact_ladders_and_refuses_order_drop_or_too_few_rungs() {
        let hs = vec![0.1, 0.05, 0.025, 0.0125];
        let errs2: Vec<f64> = hs.iter().map(|h| 3.5 * h * h).collect();
        let ladder2 = RefinementLadder::new(hs.clone(), errs2).unwrap();
        let v = OrderGate { theoretical: 2.0 }
            .check("synthetic-o2", LadderSide::Primal, &ladder2)
            .unwrap();
        assert!(v.deviation < 1e-12);

        let errs1: Vec<f64> = hs.iter().map(|h| 2.0 * h).collect();
        let ladder1 = RefinementLadder::new(hs, errs1).unwrap();
        assert_eq!(
            OrderGate { theoretical: 2.0 }
                .check("dropped-order", LadderSide::Primal, &ladder1)
                .unwrap_err()
                .rule(),
            "mms-order-gate"
        );
        assert_eq!(
            RefinementLadder::new(vec![0.1, 0.05], vec![1e-2, 2.5e-3])
                .unwrap_err()
                .rule(),
            "mms-ladder-shape"
        );
    }
}

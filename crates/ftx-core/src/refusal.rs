//! Refusals: the typed "no" every subsystem returns instead of a silent clamp or a partial result
//! (plan §6). A refusal has a stable code, a message and repairs ranked most-likely first.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Stable refusal codes. The serialised spelling (`snake_case`) is part of the file formats.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefusalCode {
    /// A NaN or infinite number where a finite one is required.
    NonFinite,
    /// Geometry that cannot be measured or meshed (zero-length chord, collapsed loop, …).
    DegenerateGeometry,
    /// A document that does not follow its schema.
    InvalidDocument,
    /// A parameter name that does not exist (a misspelt style key, an unknown option).
    UnknownParameter,
    /// A value outside its allowed range.
    OutOfRange,
    /// True-size output asked of a pattern whose scale is unknown.
    Unscaled,
    /// A root search with no sign change in its bracket.
    NoRoot,
    /// An iteration that stopped without meeting its tolerance (a Newton stall).
    NotConverged,
    /// A work budget (iterations, candidates, time) ran out; nothing partial is published.
    BudgetExceeded,
    /// A body or garment surface wound inward.
    InwardWound,
    /// A drape start state that already intersects.
    IntersectingStart,
    /// A recorded physical garment pose crosses the visible wearer.
    IntersectingTrajectory,
    /// A file could not be read or written.
    Io,
    /// A physical process (knitting swatch, dye bleed/fastness, coupon) is unqualified or `UNCERTAIN`.
    UnqualifiedProcess,
    /// A proprietary binary machine target cannot be safely generated directly (handoff IR only).
    OpaqueProprietaryTarget,
    /// A discrete fabrication machine program violates carrier, bed, needle, or rail invariants.
    MachineStateInfeasible,
    /// A material or process exceeds physical safety bounds (strain, tension, dwell, scorch, child safety).
    MaterialProcessHazard,
    /// A sealed receipt, qualification digest, or hardware profile hash failed verification.
    VerificationMismatch,
}

impl RefusalCode {
    /// The serialised spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NonFinite => "non_finite",
            Self::DegenerateGeometry => "degenerate_geometry",
            Self::InvalidDocument => "invalid_document",
            Self::UnknownParameter => "unknown_parameter",
            Self::OutOfRange => "out_of_range",
            Self::Unscaled => "unscaled",
            Self::NoRoot => "no_root",
            Self::NotConverged => "not_converged",
            Self::BudgetExceeded => "budget_exceeded",
            Self::InwardWound => "inward_wound",
            Self::IntersectingStart => "intersecting_start",
            Self::IntersectingTrajectory => "intersecting_trajectory",
            Self::Io => "io",
            Self::UnqualifiedProcess => "unqualified_process",
            Self::OpaqueProprietaryTarget => "opaque_proprietary_target",
            Self::MachineStateInfeasible => "machine_state_infeasible",
            Self::MaterialProcessHazard => "material_process_hazard",
            Self::VerificationMismatch => "verification_mismatch",
        }
    }
}

impl fmt::Display for RefusalCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A refusal record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Refusal {
    /// Stable code.
    pub code: RefusalCode,
    /// What was refused and why, in the user's terms.
    pub message: String,
    /// What to do about it, most likely first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repairs: Vec<String>,
}

impl Refusal {
    /// A refusal with no repairs yet.
    #[must_use]
    pub fn new(code: RefusalCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            repairs: Vec::new(),
        }
    }

    /// Adds a repair (call in order, most likely first).
    #[must_use]
    pub fn repair(mut self, repair: impl Into<String>) -> Self {
        self.repairs.push(repair.into());
        self
    }

    /// Refuses a non-finite value named `what`; passes finite values through.
    ///
    /// # Errors
    /// [`RefusalCode::NonFinite`] when `value` is NaN or infinite.
    pub fn finite(what: &str, value: f64) -> Result<f64, Self> {
        if value.is_finite() {
            Ok(value)
        } else {
            Err(Self::new(
                RefusalCode::NonFinite,
                format!("{what} is {value}, not a finite number"),
            ))
        }
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "refused ({}): {}", self.code, self.message)?;
        for (i, r) in self.repairs.iter().enumerate() {
            write!(f, "\n  {}. {r}", i + 1)?;
        }
        Ok(())
    }
}

impl std::error::Error for Refusal {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_and_json() {
        let r = Refusal::new(RefusalCode::Unscaled, "the pattern has no known scale")
            .repair("run scale-fit against the body")
            .repair("photograph the sheet with a ruler");
        assert_eq!(
            r.to_string(),
            "refused (unscaled): the pattern has no known scale\n  1. run scale-fit against the body\n  2. photograph the sheet with a ruler"
        );
        let j = serde_json::to_value(&r).unwrap();
        assert_eq!(j["code"], "unscaled");
        assert_eq!(serde_json::from_value::<Refusal>(j).unwrap(), r);
        for code in [
            RefusalCode::NonFinite,
            RefusalCode::DegenerateGeometry,
            RefusalCode::InvalidDocument,
            RefusalCode::UnknownParameter,
            RefusalCode::OutOfRange,
            RefusalCode::Unscaled,
            RefusalCode::NoRoot,
            RefusalCode::NotConverged,
            RefusalCode::BudgetExceeded,
            RefusalCode::InwardWound,
            RefusalCode::IntersectingStart,
            RefusalCode::IntersectingTrajectory,
            RefusalCode::Io,
            RefusalCode::UnqualifiedProcess,
            RefusalCode::OpaqueProprietaryTarget,
            RefusalCode::MachineStateInfeasible,
            RefusalCode::MaterialProcessHazard,
            RefusalCode::VerificationMismatch,
        ] {
            let v = serde_json::to_value(code).unwrap();
            assert_eq!(v, code.as_str());
            assert_eq!(serde_json::from_value::<RefusalCode>(v).unwrap(), code);
        }
    }

    #[test]
    fn finite_gate() {
        assert_eq!(Refusal::finite("chest", 540.0), Ok(540.0));
        let e = Refusal::finite("chest", f64::NAN).unwrap_err();
        assert_eq!(e.code, RefusalCode::NonFinite);
        assert_eq!(e.message, "chest is NaN, not a finite number");
    }
}

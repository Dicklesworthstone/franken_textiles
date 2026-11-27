//! Original force histories for Poincloux, Adda-Bedia and Lechenault (2018).
//!
//! The author released these measurements under CC BY 4.0 in Zenodo record 17075127.
//! Import only the pinned original bytes: a caller cannot attach this experiment's geometry
//! or provenance to edited forces. The source column named `extension` is TOTAL specimen
//! height, as clarified by its README, not displacement or homogeneous membrane strain.

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use serde::Serialize;

use crate::materials::{Evidence, Source};

/// Versioned author dataset, including both boundary conditions.
pub const DATASET_DOI: &str = "https://doi.org/10.5281/zenodo.17075127";
/// Publication defining the experiments and the homogeneous force law.
pub const ARTICLE_DOI: &str = "https://doi.org/10.1103/PhysRevX.8.021075";
const README: &str = include_str!("poincloux/README.source.txt");
const README_SHA256: &str = "75e0b3e0916924c8ce578b347592ced53a8c39a8244232ff0841a51df90a92b9";
const SLIDING: &str = include_str!("poincloux/sliding_force_data.txt");
const CLAMPED: &str = include_str!("poincloux/clamped_force_data.txt");
const SLIDING_SHA256: &str = "8ff6d6980d5229558f0aea3df78b6c20b28e0828ff5677266960b42fa5f8d4d1";
const CLAMPED_SHA256: &str = "7cd7ac0b92a9f22b568c4bf90aa59386862c7cf4eac78e45aed96830592734bf";

/// Loading fixture, which determines whether a homogeneous model is appropriate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryCondition {
    /// Top and bottom stitch rows can slide laterally on almost frictionless bars.
    Sliding,
    /// Top and bottom rows are clamped at an imposed lateral stitch spacing.
    Clamped,
}

impl BoundaryCondition {
    /// Exact original filename within the versioned dataset.
    #[must_use]
    pub const fn filename(self) -> &'static str {
        match self {
            Self::Sliding => "sliding_force_data.txt",
            Self::Clamped => "clamped_force_data.txt",
        }
    }

    const fn pin(self) -> &'static str {
        match self {
            Self::Sliding => SLIDING_SHA256,
            Self::Clamped => CLAMPED_SHA256,
        }
    }
}

/// One unchanged acquisition row; signed forces and the original row order survive import.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct ForceObservation {
    /// One-based line number, including the original header at line one.
    pub source_line: u32,
    /// Experiment time in seconds; the clamped source has repeated recorded timestamps.
    pub time_s: f64,
    /// Total vertical specimen size in millimetres, NOT change in grip separation.
    pub height_mm: f64,
    /// Total force response in newtons; negative instrument readings are retained.
    pub force_n: f64,
}

/// Source-locked acquisition. Private fields prevent relabeling edited observations as this source.
#[derive(Clone, Debug, Serialize)]
pub struct ForceAcquisition {
    boundary: BoundaryCondition,
    evidence: Evidence,
    source: Source,
    protocol_source: Source,
    original_force_sha256: String,
    original_readme_sha256: String,
    columns: u32,
    rows: u32,
    observations: Vec<ForceObservation>,
}

impl ForceAcquisition {
    /// The actual source boundary condition, independent of a candidate model.
    #[must_use]
    pub const fn boundary(&self) -> BoundaryCondition {
        self.boundary
    }

    /// Stitches in the course/column direction, then wale/row direction.
    #[must_use]
    pub const fn stitch_counts(&self) -> [u32; 2] {
        [self.columns, self.rows]
    }

    /// Complete original history; no smoothing, clipping, averaging or resampling.
    #[must_use]
    pub fn observations(&self) -> &[ForceObservation] {
        &self.observations
    }

    /// SHA-256 of the original force file before parsing.
    #[must_use]
    pub fn force_sha256(&self) -> &str {
        &self.original_force_sha256
    }

    /// Primary dataset citation and retrieval date.
    #[must_use]
    pub const fn source(&self) -> &Source {
        &self.source
    }
}

/// Import an original author force file with an immutable source pin.
///
/// The public data do not provide local yarn-lot equivalence, washed/blocked gauge, cam setting,
/// humidity, temperature or wearer fit. This importer produces no machine qualification.
///
/// # Errors
/// [`RefusalCode::VerificationMismatch`] for altered or misidentified source bytes;
/// [`RefusalCode::InvalidDocument`] for malformed columns or nonphysical acquisition ordering.
pub fn parse_force_data(
    boundary: BoundaryCondition,
    original_text: &str,
) -> Result<ForceAcquisition, Refusal> {
    if original_text.len() > 2 * 1024 * 1024 {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "Poincloux force acquisition exceeds 2 MiB",
        ));
    }
    if sha256_hex(original_text.as_bytes()) != boundary.pin()
        || sha256_hex(README.as_bytes()) != README_SHA256
    {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            format!(
                "{} does not match the original Zenodo 17075127 source",
                boundary.filename()
            ),
        )
        .repair("supply the unmodified original file with the matching boundary condition"));
    }
    let invalid = || {
        Refusal::new(
            RefusalCode::InvalidDocument,
            "invalid Poincloux force acquisition",
        )
    };
    let mut lines = original_text.lines();
    if lines.next() != Some("time [s]\t extension [mm]\t force [N]") {
        return Err(invalid());
    }
    let mut observations: Vec<ForceObservation> = Vec::new();
    for (index, line) in lines.enumerate() {
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() != 3 {
            return Err(invalid());
        }
        let mut values = [0.0_f64; 3];
        for (out, field) in values.iter_mut().zip(fields) {
            *out = field.parse().map_err(|_| invalid())?;
            if !out.is_finite() {
                return Err(invalid());
            }
        }
        if values[0] < 0.0
            || values[1] <= 0.0
            || observations
                .last()
                .is_some_and(|last| values[0] < last.time_s)
        {
            return Err(invalid());
        }
        observations.push(ForceObservation {
            source_line: (index + 2) as u32,
            time_s: values[0],
            height_mm: values[1],
            force_n: values[2],
        });
    }
    let (columns, rows, expected_rows) = match boundary {
        BoundaryCondition::Sliding => (20, 30, 18_141),
        BoundaryCondition::Clamped => (51, 51, 60_115),
    };
    if observations.len() != expected_rows {
        return Err(invalid());
    }
    Ok(ForceAcquisition {
        boundary,
        evidence: Evidence::Measured,
        source: Source {
            reference: DATASET_DOI.into(),
            accessed_on: "2026-11-27".into(),
            note: format!("Samuel Poincloux, second version, 2025-09-08; {}; original signed time/height/force rows; CC BY 4.0", boundary.filename()),
        },
        protocol_source: Source {
            reference: ARTICLE_DOI.into(),
            accessed_on: "2026-11-27".into(),
            note: "Poincloux, Adda-Bedia and Lechenault, PRX 8, 021075 (2018), sections III A and V; Toyota KS858 single-bed knitting machine. Local lot, cam setting, washing, temperature and humidity are not supplied by this acquisition.".into(),
        },
        original_force_sha256: boundary.pin().into(),
        original_readme_sha256: README_SHA256.into(),
        columns,
        rows,
        observations,
    })
}

/// Load the bundled original source without network access.
///
/// # Errors
/// Same source-integrity checks as [`parse_force_data`].
pub fn published_force_data(boundary: BoundaryCondition) -> Result<ForceAcquisition, Refusal> {
    parse_force_data(
        boundary,
        match boundary {
            BoundaryCondition::Sliding => SLIDING,
            BoundaryCondition::Clamped => CLAMPED,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_sources_keep_total_heights_signed_forces_and_exact_rows() {
        let sliding = published_force_data(BoundaryCondition::Sliding).unwrap();
        assert_eq!(sliding.stitch_counts(), [20, 30]);
        assert_eq!(sliding.observations().len(), 18_141);
        let first = sliding.observations()[0];
        assert_eq!(
            (
                first.source_line,
                first.time_s,
                first.height_mm,
                first.force_n
            ),
            (2, 0.0, 53.13, 0.011)
        );
        let last = sliding.observations().last().unwrap();
        assert_eq!(
            (last.source_line, last.time_s, last.height_mm, last.force_n),
            (18_142, 905.963, 53.13, -0.017)
        );
        let clamped = published_force_data(BoundaryCondition::Clamped).unwrap();
        assert_eq!(clamped.stitch_counts(), [51, 51]);
        assert_eq!(clamped.observations().len(), 60_115);
        assert_eq!(clamped.observations()[0].height_mm, 117.27);
        assert_eq!(clamped.observations()[0].force_n, -0.01434);
        assert_eq!(
            clamped
                .observations()
                .windows(2)
                .filter(|pair| pair[0].time_s == pair[1].time_s)
                .count(),
            10,
        );
    }

    #[test]
    fn edited_rehashed_or_wrong_boundary_data_cannot_borrow_the_source() {
        assert_eq!(
            parse_force_data(BoundaryCondition::Clamped, SLIDING)
                .unwrap_err()
                .code,
            RefusalCode::VerificationMismatch
        );
        let changed = SLIDING.replacen("53.13", "53.14", 1);
        assert_eq!(
            parse_force_data(BoundaryCondition::Sliding, &changed)
                .unwrap_err()
                .code,
            RefusalCode::VerificationMismatch
        );
        assert_eq!(
            parse_force_data(
                BoundaryCondition::Sliding,
                "time [s]\textension [mm]\tforce [N]\n0\t0\t0\n"
            )
            .unwrap_err()
            .code,
            RefusalCode::VerificationMismatch
        );
    }
}

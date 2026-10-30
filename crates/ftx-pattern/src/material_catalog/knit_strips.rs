//! Measured grip-force screening from the MIT-licensed Sperl et al. (2022) production-knit data.
//! Clamped strips are spatially nonuniform: their force/extension curves are not homogeneous
//! membrane observations. Only exact author-reported loading rows become specimen-level attributes.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use serde::{Deserialize, Serialize};

use crate::materials::{Component, Evidence, Form, Material, Quantity, Source};

mod expanded;
mod transverse;
pub use transverse::{AcquisitionDocument, TransverseCurve, TransverseObservation};

#[cfg(test)]
const DATA: &str = include_str!("knit-strip-measurements.json");
const ARCHIVE: &str = "https://mslab.es/projects/YarnLevelFabrics/dataset/dataset.zip";
const ARTICLE: &str = "https://doi.org/10.1145/3528223.3530167";
const METADATA_SHA256: &str = "258e3cc2a40ba40d1e0152f80b1fb08c67d14c08db10efbc2ec385669887a6d4";
const RETRIEVED: &str = "2026-10-20";
const LICENSE_SHA256: &str = "a65ec8d5d26449679b482bc791c6d57abc17f5d14291d45886803027bf8a7480";

/// The source's three strip cuts. Its zero angle is the weft/course direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// Source 90-degree cut, loaded along warp/wale.
    Warp,
    /// Source zero-degree cut, loaded along weft/course.
    Weft,
    /// Source 45-degree cut.
    Bias,
}

impl Direction {
    /// Original source label, independent from any solver's coordinate convention.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Warp => "warp",
            Self::Weft => "weft",
            Self::Bias => "bias",
        }
    }

    /// Cut angle in the paper's weft-zero convention, in degrees.
    #[must_use]
    pub const fn source_angle_degrees(self) -> f64 {
        match self {
            Self::Warp => 90.0,
            Self::Weft => 0.0,
            Self::Bias => 45.0,
        }
    }
}

/// Original acquisition branch, retained rather than averaged into an elastic response.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Branch {
    /// Increasing grip separation.
    Loading,
    /// Decreasing grip separation after the loading branch.
    Unloading,
}

/// One unmodified numeric row from the author-supplied force CSV.
#[derive(Clone, Debug, Serialize)]
pub struct Observation {
    /// One-based original CSV line number, including the header at line one.
    pub csv_line: usize,
    /// Change in grip separation divided by its initial free gauge; not local membrane strain.
    pub extension_fraction: f64,
    /// Signed total measured grip force in newtons; negative noise is retained.
    pub force_n: f64,
    /// Original loading/unloading tag.
    pub branch: Branch,
}

/// A source-locked force history belonging to exactly one specimen and cut.
#[derive(Clone, Debug, Serialize)]
pub struct StripCurve {
    /// Exact source fabric codename, not a supplier SKU or another specimen's lot.
    pub specimen: String,
    /// Original source cut.
    pub direction: Direction,
    /// Entry within the retained source archive.
    pub entry: String,
    /// SHA-256 of the complete original CSV bytes before parsing.
    pub sha256: String,
    /// Citation, acquisition date and processing limitations.
    pub source: Source,
    /// Original row order; no interpolation, smoothing, truncation or resampling.
    pub observations: Vec<Observation>,
}

impl StripCurve {
    /// Find an exact loading-branch extension without inventing an interpolated measurement.
    ///
    /// # Errors
    /// Nonfinite/negative extension or an extension absent from the original loading rows.
    pub fn exact_loading(&self, extension_fraction: f64) -> Result<&Observation, Refusal> {
        if !extension_fraction.is_finite() || extension_fraction < 0.0 {
            return Err(invalid(
                "knit strip extension must be finite and nonnegative",
            ));
        }
        self.observations
            .iter()
            .find(|row| {
                row.branch == Branch::Loading && row.extension_fraction == extension_fraction
            })
            .ok_or_else(|| invalid("knit strip has no exact requested loading observation"))
    }
}

/// Geometry reported in section 4.3/Figure 4; solver idealizations remain separate.
#[derive(Clone, Debug, Serialize)]
pub struct StripProtocol {
    /// Initial free gauge length, excluding both gripped regions, in metres.
    pub free_gauge_length_m: f64,
    /// Initial strip width in metres.
    pub reference_width_m: f64,
    /// Paper's stated stretch-test working force in newtons; does not truncate retained rows.
    pub paper_working_force_limit_n: f64,
    /// Provenance of the geometry and protocol, distinct from the measured CSV values.
    pub source: Source,
    /// Reported facts and explicitly unreported acquisition conditions.
    pub conditions: BTreeMap<String, String>,
    /// Idealizations which an inverse consumer must declare, never measured facts.
    pub unverified_model_assumptions: Vec<String>,
}

/// Parsed retained Sperl acquisition, supplied by the caller rather than embedded in the binary.
#[derive(Clone, Debug, Serialize)]
pub struct StripAcquisition {
    /// SHA-256 of the exact supplied acquisition JSON bytes, including its source locks.
    pub acquisition_sha256: String,
    /// Original archive URL.
    pub archive_url: String,
    /// Retained retrieval date; not a new live observation.
    pub retrieved_on: String,
    /// Complete original MIT license and attribution notice.
    pub license: String,
    /// Original metadata archive entry.
    pub metadata_entry: String,
    /// SHA-256 of the complete original metadata bytes.
    pub metadata_sha256: String,
    /// Lossless original metadata including yarns, finishes and linked additional experiments.
    pub metadata_json: String,
    /// Source protocol shared by these pinned observations.
    pub protocol: StripProtocol,
    /// Complete histories in the caller's retained source order.
    pub curves: Vec<StripCurve>,
    /// Optional original transverse-contraction observations; not a solver width operator.
    pub transverse_curves: Vec<TransverseCurve>,
    /// Original dataset README establishing units and distribution terms, when supplied.
    pub transverse_readme: Option<AcquisitionDocument>,
}

// Only these original curve bytes have been acquired and linked to the paper's geometry.
// A caller cannot attach the source protocol to invented or merely rehashed observations.
const CURVE_PINS: [(&str, Direction, &str); 9] = [
    (
        "SJ1",
        Direction::Warp,
        "0f60fa0dbc3f99bcd0f0ba19c41f6988d7e8931bbfc6790003e12b326bddf0b6",
    ),
    (
        "SJ1",
        Direction::Weft,
        "0fe20ce39b5632ccf2437abc2646d70b5f65406fb5b19f8ea3fb804a26a26249",
    ),
    (
        "SJ1",
        Direction::Bias,
        "3a2abad579ebc91afbb2ae801365d69bc266b197ff5f11a9d737b63a38c55c1b",
    ),
    (
        "SJ3",
        Direction::Warp,
        "38332753d6bf437b5294f99e07c7cda87e1898c57d0be236891b17f662235688",
    ),
    (
        "SJ3",
        Direction::Weft,
        "ad9fa8d6a07a700348684813d5bf87212de2dba43a2577d2930922aec57780f9",
    ),
    (
        "SJ3",
        Direction::Bias,
        "fcb93afd8c919d3a6189f6622cb8fe8573a90afd09a04bb90b78b762dda327bf",
    ),
    (
        "DKIN8",
        Direction::Warp,
        "2e1e6c775fb40ac69df48eac521c6926463799753cf1d35a7e8bbf8999d34f59",
    ),
    (
        "DKIN8",
        Direction::Weft,
        "491b997ec5ce460c9f2018d7a791b63a3b656fa5654efb316212888cd3c3d10f",
    ),
    (
        "DKIN8",
        Direction::Bias,
        "d6b48f9f457932ffec430546f1660f4819fd316106b8bfc42aee3421e9f20365",
    ),
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Acquisition {
    archive_url: String,
    retrieved_on: String,
    license: String,
    metadata_entry: String,
    metadata_sha256: String,
    metadata_json: String,
    curves: Vec<Curve>,
    #[serde(default)]
    transverse_curves: Vec<transverse::RawCurve>,
    #[serde(default)]
    transverse_readme: Option<transverse::RawDocument>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Curve {
    specimen: String,
    direction: String,
    entry: String,
    sha256: String,
    csv: String,
}

#[derive(Deserialize)]
struct Fabric {
    composition: Vec<Yarn>,
    density_g_m2: f64,
    thickness_mm: f64,
    finishes: Vec<String>,
    #[serde(default)]
    stretch: BTreeMap<String, String>,
    #[serde(default)]
    poisson: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct Yarn {
    name: String,
    percentage: f64,
    count: u32,
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

fn observations(curve: &Curve) -> Result<Vec<Observation>, Refusal> {
    if sha256_hex(curve.csv.as_bytes()) != curve.sha256 {
        return Err(invalid(
            "knit strip source bytes differ from their retained hash",
        ));
    }
    let mut lines = curve.csv.lines();
    if lines.next() != Some("elongation,force,tag") {
        return Err(invalid(
            "knit strip CSV requires its original measured columns",
        ));
    }
    let mut observations = Vec::new();
    let mut previous_loading = None;
    let mut previous_unloading = None;
    let mut unloading = false;
    let mut count = 0;
    for line in lines {
        count += 1;
        let mut columns = line.split(',');
        let (Some(extension), Some(force), Some(branch), None) = (
            columns.next(),
            columns.next(),
            columns.next(),
            columns.next(),
        ) else {
            return Err(invalid("knit strip CSV needs exactly three columns"));
        };
        let extension: f64 = extension
            .parse()
            .map_err(|_| invalid("knit strip extension is not numeric"))?;
        let force: f64 = force
            .parse()
            .map_err(|_| invalid("knit strip force is not numeric"))?;
        if count > 4096 || !extension.is_finite() || extension < 0.0 || !force.is_finite() {
            return Err(invalid(
                "knit strip acquisition exceeds its finite observation domain",
            ));
        }
        let branch = match branch {
            "loading" if !unloading => {
                if previous_loading.is_some_and(|v| extension <= v) {
                    return Err(invalid(
                        "knit strip loading extensions must increase strictly",
                    ));
                }
                previous_loading = Some(extension);
                Branch::Loading
            }
            "unloading" => {
                if previous_loading.is_none()
                    || previous_unloading.is_some_and(|v| extension >= v)
                    || (!unloading && previous_loading.is_some_and(|v| extension > v))
                {
                    return Err(invalid("knit strip unloading branch is inconsistent"));
                }
                unloading = true;
                previous_unloading = Some(extension);
                Branch::Unloading
            }
            _ => return Err(invalid("knit strip branch is unknown or loading restarts")),
        };
        observations.push(Observation {
            csv_line: count + 1,
            extension_fraction: extension,
            force_n: force,
            branch,
        });
    }
    if !unloading {
        return Err(invalid(
            "knit strip source acquisition is missing its unloading branch",
        ));
    }
    Ok(observations)
}

/// Parse the pinned retained acquisition or an explicitly selected subset of its known curves.
///
/// Original metadata, license, archive entries and CSV hashes are checked before returning the
/// source's geometry. Altered rows with freshly computed hashes still refuse. This is an acquisition
/// adapter, not a constitutive fit: force remains total grip force and extension remains nonuniform
/// swatch extension. No thermal, contact, bending or plane-stress properties are inferred.
///
/// # Errors
/// Malformed/oversized input, unknown source revision or curve, changed original bytes, duplicate
/// specimen/cut identity, or invalid branch/row ordering. Input is limited to 4 MiB, 99 force
/// histories and nine optional transverse curves. Transverse observations have their own retrieval
/// date and retain unreported sampling details; they are never silently converted to solver widths.
pub fn parse_acquisition(data: &str) -> Result<StripAcquisition, Refusal> {
    if data.len() > 4 * 1024 * 1024 {
        return Err(invalid("knit strip acquisition exceeds 4 MiB"));
    }
    let acquisition: Acquisition = serde_json::from_str(data)
        .map_err(|e| invalid(format!("measured knit acquisition: {e}")))?;
    if acquisition.archive_url != ARCHIVE
        || ![RETRIEVED, expanded::RETRIEVED].contains(&acquisition.retrieved_on.as_str())
        || acquisition.metadata_entry != "dataset/data/fabrics.json"
        || acquisition.metadata_sha256 != METADATA_SHA256
        || sha256_hex(acquisition.metadata_json.as_bytes()) != METADATA_SHA256
        || sha256_hex(acquisition.license.as_bytes()) != LICENSE_SHA256
        || acquisition.curves.is_empty()
        || acquisition.curves.len() > 99
    {
        return Err(invalid(
            "knit strip acquisition does not match the retained primary source",
        ));
    }
    let metadata: BTreeMap<String, Fabric> = serde_json::from_str(&acquisition.metadata_json)
        .map_err(|e| invalid(format!("measured knit metadata: {e}")))?;
    let expanded_pins = if acquisition.retrieved_on == expanded::RETRIEVED {
        expanded::pins()?
    } else {
        BTreeMap::new()
    };
    let (transverse_curves, transverse_readme) = transverse::parse(
        &acquisition.transverse_curves,
        acquisition.transverse_readme.as_ref(),
        &metadata,
    )?;
    let mut identities = BTreeSet::new();
    let mut curves = Vec::with_capacity(acquisition.curves.len());
    for curve in &acquisition.curves {
        let direction = match curve.direction.as_str() {
            "warp" => Direction::Warp,
            "weft" => Direction::Weft,
            "bias" => Direction::Bias,
            _ => return Err(invalid("knit strip cut direction is unknown")),
        };
        if !identities.insert((&curve.specimen, direction)) {
            return Err(invalid("knit strip specimen/cut identity is duplicated"));
        }
        let expected_hash = CURVE_PINS
            .iter()
            .find(|(specimen, cut, _)| *specimen == curve.specimen && *cut == direction)
            .map(|(_, _, hash)| *hash)
            .or_else(|| {
                expanded_pins
                    .get(&curve.specimen)
                    .and_then(|cuts| cuts.get(&direction))
                    .map(String::as_str)
            })
            .ok_or_else(|| invalid("knit strip specimen/cut has no acquired source pin"))?;
        if curve.sha256 != expected_hash
            || metadata
                .get(&curve.specimen)
                .and_then(|f| f.stretch.get(direction.as_str()))
                .map(|path| format!("dataset/data/{path}"))
                != Some(curve.entry.clone())
        {
            return Err(invalid(
                "knit strip source hash or metadata entry does not match its specimen/cut",
            ));
        }
        let observations = observations(curve)?;
        curves.push(StripCurve {
            specimen: curve.specimen.clone(), direction, entry: curve.entry.clone(),
            sha256: curve.sha256.clone(), observations,
            source: Source {
                reference: format!("{ARCHIVE}#{}", curve.entry),
                accessed_on: acquisition.retrieved_on.clone(),
                note: format!("Sperl et al. (2022), original {} {} force CSV SHA-256 {}. Signed force and loading/unloading order retained without smoothing, interpolation, truncation or strain conversion. Upstream processing and instrument-level rawness unspecified.", curve.specimen, direction.as_str(), curve.sha256),
            },
        });
    }
    let retrieved_on = acquisition.retrieved_on.clone();
    Ok(StripAcquisition {
        acquisition_sha256: sha256_hex(data.as_bytes()), archive_url: acquisition.archive_url,
        retrieved_on: acquisition.retrieved_on, license: acquisition.license,
        metadata_entry: acquisition.metadata_entry, metadata_sha256: acquisition.metadata_sha256,
        metadata_json: acquisition.metadata_json, curves, transverse_curves, transverse_readme,
        protocol: StripProtocol {
            free_gauge_length_m: 0.100, reference_width_m: 0.060,
            paper_working_force_limit_n: 5.0,
            source: Source {
                reference: ARTICLE.into(), accessed_on: retrieved_on,
                note: "Sperl et al. (2022), section 4.3 and Figure 4; 100 mm free gauge excluding clamps, 60 mm width; horizontal displacement-controlled clamped stretch with measured force. Source angular origin is weft (0 degrees), bias (45 degrees), warp (90 degrees).".into(),
            },
            conditions: BTreeMap::from([
                ("boundary_condition".into(), "Two clamped ends; horizontal displacement control, total force measured; transverse deformation is nonuniform".into()),
                ("conditioning".into(), "Temperature, relative humidity and conditioning duration not supplied".into()),
                ("rate".into(), "Displacement rate and sample timestamps not supplied".into()),
                ("uncertainty".into(), "Replicate identities, instrument uncertainty and population bounds not supplied".into()),
                ("source_processing".into(), "Author CSV; preprocessing and instrument-level rawness unspecified".into()),
                ("additional_experiments".into(), "Original transverse-contraction curves may be supplied separately in transverse_curves; their spatial sampling and branch tags are not reported. Pear-loop observations are not supplied".into()),
            ]),
            unverified_model_assumptions: vec![
                "Planar deformation and exclusion of out-of-plane buckling must be explicitly chosen by the consumer".into(),
                "Ideal no-slip grip locking is a model approximation; clamp slip/compliance is not measured here".into(),
                "A membrane/plane-stress law and quasi-static equilibrium are modeling assumptions, not properties inferred by this acquisition".into(),
            ],
        },
    })
}

#[cfg(test)]
fn loading_observations(curve: &Curve) -> Result<[f64; 2], Refusal> {
    let rows = observations(curve)?;
    let exact = |extension| {
        rows.iter()
            .find(|row| row.branch == Branch::Loading && row.extension_fraction == extension)
            .map(|row| row.force_n)
            .ok_or_else(|| invalid("knit strip has no exact 10% or 30% loading observation"))
    };
    Ok([exact(0.1)?, exact(0.3)?])
}

fn record(specimen: &str, fabric: &Fabric, curves: &[(&str, &str, [Option<f64>; 2])]) -> Material {
    let conditions = BTreeMap::from([
        ("specimen".into(), format!("Sperl2022:{specimen}")),
        (
            "conditioning".into(),
            "Temperature, relative humidity and conditioning duration not supplied".into(),
        ),
        (
            "uncertainty".into(),
            "Replicates, measurement uncertainty and population bounds not supplied".into(),
        ),
    ]);
    let q = |value, unit: &str, method: &str| Quantity {
        value,
        unit: unit.into(),
        evidence: Evidence::Measured,
        source: "metadata".into(),
        method: method.into(),
        conditions: conditions.clone(),
        bounds: None,
    };
    let mut gauge = q(
        fabric.thickness_mm,
        "mm",
        "Original fabrics.json thickness_mm; measured gauge thickness with unreported pressure and instrument",
    );
    gauge.conditions.insert(
        "pressure".into(),
        "Not reported; does not establish unloaded clearance or a pressure state".into(),
    );
    let mut properties = BTreeMap::from([
        (
            "areal_density".into(),
            q(
                fabric.density_g_m2,
                "g/m2",
                "Original fabrics.json density_g_m2; measured fabric mass per area; balance, coupon and conditioning details not supplied",
            ),
        ),
        ("specimen_gauge_thickness".into(), gauge),
    ]);
    let mut sources = BTreeMap::from([
            ("metadata".into(), Source { reference: format!("{ARCHIVE}#dataset/data/fabrics.json"), accessed_on: RETRIEVED.into(), note: format!("Sperl et al. (2022), exact fabric {specimen}; original metadata SHA-256 {METADATA_SHA256}. Full original metadata bytes and MIT notice retained in knit-strip-measurements.json. No supplier SKU or lot equivalence is inferred.") }),
            ("paper".into(), Source { reference: ARTICLE.into(), accessed_on: RETRIEVED.into(), note: "Section 4.3 and Figure 4: clamped strip, 100 mm free gauge by 60 mm width, weft/bias/warp; displacement-controlled test with measured force and spatially nonuniform deformation. Full-swatch inversion is required before constitutive identification. Only exact reported loading force rows at 10% and 30% grip extension are used here, within the paper's stated 5 N working range. Instrument-level rawness and upstream processing are unspecified.".into() }),
        ]);
    for &(direction, hash, forces) in curves {
        let source = format!("strip_{direction}");
        sources.insert(source.clone(), Source {
                reference: format!("{ARCHIVE}#dataset/data/{specimen}/stretch-{direction}.csv"), accessed_on: RETRIEVED.into(),
                note: format!("Original source CSV SHA-256 {hash}; specimen {specimen}; retained losslessly with its loading and unloading tags under MIT. Direction means the paper's {direction} cut. Signed source values are preserved; this import adds no smoothing or interpolation. Author preprocessing and instrument rawness are unspecified."),
            });
        for (percent, force) in [10, 30].into_iter().zip(forces) {
            let Some(force) = force else {
                continue;
            };
            let mut force_q = q(
                force,
                "N",
                "Exact author-reported loading-branch row; total grip force at the declared grip extension on a clamped 100 mm by 60 mm strip; no additional interpolation or homogeneous-strain conversion",
            );
            force_q.source.clone_from(&source);
            force_q.conditions.extend([
                ("direction".into(), direction.into()),
                ("branch".into(), "loading".into()),
                (
                    "grip_extension_fraction".into(),
                    format!("{}", f64::from(percent) / 100.0),
                ),
                ("free_gauge_length_mm".into(), "100".into()),
                ("reference_strip_width_mm".into(), "60".into()),
                (
                    "strain_scope".into(),
                    "Grip-to-grip extension; spatially nonuniform clamped specimen".into(),
                ),
                (
                    "rate".into(),
                    "Displacement controlled; rate not reported".into(),
                ),
                (
                    "source_processing".into(),
                    "Author CSV; preprocessing and instrument-level rawness unspecified".into(),
                ),
            ]);
            properties.insert(format!("strip_force_{percent}pct_{direction}"), force_q);
        }
    }
    // Source yarn mass fractions preserve functional polyester distinctions. Full original
    // yarn labels/counts remain below; the Celliant additive composition is not inferred.
    let composition = fabric
        .composition
        .iter()
        .filter_map(|y| {
            let fiber = if y.name.contains("Spandex") {
                Some("elastane")
            } else if y.name.contains("Celliant") {
                Some("polyester (Celliant-labelled yarn)")
            } else if y.name.contains("Polyester") {
                Some("polyester")
            } else {
                None
            }?;
            Some(Component {
                fiber: fiber.into(),
                fraction: y.percentage,
                source: "metadata".into(),
            })
        })
        .collect::<Vec<_>>();
    let composition_complete = composition.len() == fabric.composition.len();
    Material {
            id: format!("sperl2022/{specimen}"),
            name: format!("Sperl 2022 {} {specimen}", if specimen.starts_with("SJ") { "single jersey" } else { "double-knit interlock" }),
            form: Form::Knit, composition, composition_complete,
            structure: BTreeMap::from([
                ("construction".into(), if specimen.starts_with("SJ") { "single jersey" } else { "double-knit interlock" }.into()),
                ("finish".into(), fabric.finishes.join("; ")),
                ("yarn_counts".into(), fabric.composition.iter().map(|y| format!("{}: {} yarn(s)", y.name, y.count)).collect::<Vec<_>>().join("; ")),
                ("composition_scope".into(), "Reported yarn mass fractions; Celliant additive composition and finish mass are not supplied".into()),
                ("identity_scope".into(), "Exact research fabric codename; supplier lot, purchasable SKU and general brand equivalence are not supplied".into()),
                ("gauge_scope".into(), "Reported thickness has unknown gauge pressure; canonical unloaded thickness remains unavailable".into()),
                ("mechanical_scope".into(), "Measured clamped-strip force screening; no homogeneous F, membrane tensor, shear modulus, bending law, recovery model or garment fit follows".into()),
                ("retention".into(), "Original nine force-curve CSVs, full metadata and MIT notice retained; no photos or geometry assets bundled".into()),
            ]),
            properties, sources, membrane_response: None, membrane_relaxation: None,
            thermal_response: None, optical_faces: None,
        }
}

pub(super) fn records() -> Result<Vec<Material>, Refusal> {
    // Compact transcriptions keep raw acquisitions out of normal catalog/browser construction.
    // The test below rebuilds these exact records independently from the retained original files.
    let fabric = |mass, gauge, finish: &str, yarns: &[(&str, f64, u32)]| Fabric {
        composition: yarns
            .iter()
            .map(|&(name, percentage, count)| Yarn {
                name: name.into(),
                percentage,
                count,
            })
            .collect(),
        density_g_m2: mass,
        thickness_mm: gauge,
        finishes: vec![finish.into()],
        stretch: BTreeMap::new(),
        poisson: BTreeMap::new(),
    };
    let mut records = vec![
        record(
            "SJ1",
            &fabric(
                159.,
                0.43,
                "Anti-microbial",
                &[("75D/72F Polyester", 0.9, 1), ("20D Spandex", 0.1, 1)],
            ),
            &[
                (
                    "warp",
                    "0f60fa0dbc3f99bcd0f0ba19c41f6988d7e8931bbfc6790003e12b326bddf0b6",
                    [Some(0.40), Some(1.94)],
                ),
                (
                    "weft",
                    "0fe20ce39b5632ccf2437abc2646d70b5f65406fb5b19f8ea3fb804a26a26249",
                    [Some(0.04), Some(0.84)],
                ),
                (
                    "bias",
                    "3a2abad579ebc91afbb2ae801365d69bc266b197ff5f11a9d737b63a38c55c1b",
                    [Some(0.27), Some(1.12)],
                ),
            ],
        ),
        record(
            "SJ3",
            &fabric(
                238.00000000000003,
                0.63,
                "Water Repellent",
                &[("55D Spandex", 0.13, 1), ("50D/72F Polyester", 0.87, 3)],
            ),
            &[
                (
                    "warp",
                    "38332753d6bf437b5294f99e07c7cda87e1898c57d0be236891b17f662235688",
                    [Some(0.71), Some(2.11)],
                ),
                (
                    "weft",
                    "ad9fa8d6a07a700348684813d5bf87212de2dba43a2577d2930922aec57780f9",
                    [Some(0.80), Some(2.48)],
                ),
                (
                    "bias",
                    "fcb93afd8c919d3a6189f6622cb8fe8573a90afd09a04bb90b78b762dda327bf",
                    [Some(0.53), Some(1.78)],
                ),
            ],
        ),
        record(
            "DKIN8",
            &fabric(
                267.,
                0.62,
                "Anti-microbial",
                &[
                    ("50D/72F Polyester", 0.39, 1),
                    ("50D/36F Polyester Celliant", 0.38, 1),
                    ("40D Spandex", 0.23, 1),
                ],
            ),
            &[
                (
                    "warp",
                    "2e1e6c775fb40ac69df48eac521c6926463799753cf1d35a7e8bbf8999d34f59",
                    [Some(1.21), Some(3.39)],
                ),
                (
                    "weft",
                    "491b997ec5ce460c9f2018d7a791b63a3b656fa5654efb316212888cd3c3d10f",
                    [Some(1.56), Some(5.00)],
                ),
                (
                    "bias",
                    "d6b48f9f457932ffec430546f1660f4819fd316106b8bfc42aee3421e9f20365",
                    [Some(1.15), Some(3.39)],
                ),
            ],
        ),
    ];
    records.extend(expanded::records()?);
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn records_from(data: &str) -> Result<Vec<Material>, Refusal> {
        let acquisition: Acquisition = serde_json::from_str(data)
            .map_err(|e| invalid(format!("measured knit acquisition: {e}")))?;
        if acquisition.archive_url != ARCHIVE
            || acquisition.retrieved_on != RETRIEVED
            || acquisition.metadata_entry != "dataset/data/fabrics.json"
            || acquisition.metadata_sha256 != METADATA_SHA256
            || sha256_hex(acquisition.metadata_json.as_bytes()) != acquisition.metadata_sha256
            || !acquisition.license.starts_with("MIT License")
            || acquisition.curves.len() != 9
        {
            return Err(invalid(
                "measured knit metadata identity, source hash or retained license differs",
            ));
        }
        let metadata: BTreeMap<String, Fabric> =
            serde_json::from_str(&acquisition.metadata_json)
                .map_err(|e| invalid(format!("measured knit metadata: {e}")))?;
        let mut records = Vec::new();
        for specimen in ["SJ1", "SJ3", "DKIN8"] {
            let fabric = metadata
                .get(specimen)
                .ok_or_else(|| invalid("measured knit specimen is missing"))?;
            let mut observations = Vec::new();
            for direction in ["warp", "weft", "bias"] {
                let mut matches = acquisition
                    .curves
                    .iter()
                    .filter(|c| c.specimen == specimen && c.direction == direction);
                let curve = matches
                    .next()
                    .ok_or_else(|| invalid("measured knit direction missing"))?;
                if matches.next().is_some()
                    || fabric
                        .stretch
                        .get(direction)
                        .map(|p| format!("dataset/data/{p}"))
                        != Some(curve.entry.clone())
                {
                    return Err(invalid(
                        "measured knit direction has duplicate or mismatched source identity",
                    ));
                }
                observations.push((
                    direction,
                    curve.sha256.as_str(),
                    loading_observations(curve)?.map(Some),
                ));
            }
            records.push(record(specimen, fabric, &observations));
        }
        Ok(records)
    }

    #[test]
    fn compact_catalog_replays_exactly_from_original_source_acquisitions() {
        assert_eq!(
            serde_json::to_value(&records().unwrap()[..3]).unwrap(),
            serde_json::to_value(records_from(DATA).unwrap()).unwrap()
        );
    }

    #[test]
    fn retained_source_hashes_and_exact_loading_rows_are_required() {
        let mut acquisition: serde_json::Value = serde_json::from_str(DATA).unwrap();
        acquisition["curves"][0]["csv"] = "elongation,force,tag\n0.1,99,loading\n".into();
        assert!(records_from(&acquisition.to_string()).is_err());
        let mut acquisition: serde_json::Value = serde_json::from_str(DATA).unwrap();
        acquisition["metadata_json"] = "{}".into();
        assert!(records_from(&acquisition.to_string()).is_err());
        let curve = |csv: &str| Curve {
            specimen: "test".into(),
            direction: "warp".into(),
            entry: "test".into(),
            sha256: sha256_hex(csv.as_bytes()),
            csv: csv.into(),
        };
        assert_eq!(loading_observations(&curve("elongation,force,tag\n0.1,1,loading\n0.3,2,loading\n0.3,9,unloading\n0.1,-0.02,unloading\n")).unwrap(), [1., 2.]);
        for csv in [
            "elongation,force,tag\n0.099,1,loading\n0.101,1.1,loading\n0.3,2,loading\n0.3,2,unloading\n",
            "elongation,force,tag\n0.1,1,loading\n0.1,2,loading\n0.3,3,loading\n0.3,3,unloading\n",
            "elongation,force,tag\n0.1,1,loading\n0.3,NaN,loading\n0.3,3,unloading\n",
            "elongation,force,tag\n0.1,1,loading\n0.3,3,loading\n0.3,3,unloading\n0.4,4,loading\n",
        ] {
            assert!(loading_observations(&curve(csv)).is_err());
        }
    }
}

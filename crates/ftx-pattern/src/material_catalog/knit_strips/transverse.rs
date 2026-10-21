//! Original transverse-contraction curves, retained without guessing an image measurement operator.

use super::{ARCHIVE, ARTICLE, invalid, sha256_hex};
use super::{BTreeMap, BTreeSet, Deserialize, Direction, Fabric, Refusal, Serialize, Source};

const RETRIEVED: &str = "2026-10-21";
const README_SHA256: &str = "0cc62a11d420260f77981a7f86db4b790a67b6743c75b921474a40bda4cbd13d";
const PINS: [(&str, Direction, &str); 9] = [
    (
        "SJ1",
        Direction::Warp,
        "887ae1bb00819c05800abccdd902b405cfa5947e1d3d3507a9da68df5c3fc1dd",
    ),
    (
        "SJ1",
        Direction::Weft,
        "797a6984f76b3612f4a49e1a1303dbeeea641d4a8a6e1a2be27bb784ded9db8e",
    ),
    (
        "SJ1",
        Direction::Bias,
        "ae85e96aff015c40611d32c6dcb57eba82299d5883cb65e29a7f9b22ad2234a6",
    ),
    (
        "SJ3",
        Direction::Warp,
        "4a25e364f4de1b884cc69895e18cf6ce63227035461a54f574122fa0fc9c93df",
    ),
    (
        "SJ3",
        Direction::Weft,
        "d923220eb422125f4293a2cd3cf0205f1079daace4c74c3c9b25c00cd01b0ac7",
    ),
    (
        "SJ3",
        Direction::Bias,
        "f0866579581ddd29674f359e822af192a145c321e5ff646b885cfcd86d4a3761",
    ),
    (
        "DKIN8",
        Direction::Warp,
        "c0502da6db31c57144b4944ecdcd260a6cc56fdf6a3d51cd3b8fc6e4908488f2",
    ),
    (
        "DKIN8",
        Direction::Weft,
        "a194fd8e4f8d7d09ec6d2270f128bd7b435d05f4945ab2e1cb9e67ee65d9ab7b",
    ),
    (
        "DKIN8",
        Direction::Bias,
        "ddbf9f1347a68afda337d78ba5718cd702aefbd5a4d2daa5fbcfe0eb098924f7",
    ),
];

/// One original transverse observation. The source CSV does not supply a loading/unloading tag.
#[derive(Clone, Debug, Serialize)]
pub struct TransverseObservation {
    /// One-based source CSV line, including the header at line one.
    pub csv_line: usize,
    /// Change in grip separation divided by the reference free gauge, dimensionless.
    pub extension_fraction: f64,
    /// Signed fractional width reduction, c = 1 - w / reference_width; not a Poisson ratio.
    pub compression_fraction: f64,
}

/// Source transverse observations, with no assumed equivalence to material-midpoint node distance.
#[derive(Clone, Debug, Serialize)]
pub struct TransverseCurve {
    /// Exact source fabric codename.
    pub specimen: String,
    /// Original cut direction; source zero angle is weft.
    pub direction: Direction,
    /// Source ZIP archive entry.
    pub entry: String,
    /// SHA-256 of all original CSV bytes, before parsing.
    pub sha256: String,
    /// Source identity, units and observed retrieval date.
    pub source: Source,
    /// Unmodified source rows in original order, without interpolation or smoothing.
    pub observations: Vec<TransverseObservation>,
    /// Unreported spatial/branch details that a solver consumer must resolve explicitly.
    pub measurement_scope: String,
}

impl TransverseCurve {
    /// Find an exact supplied extension; never interpolate or assume a force-curve row match.
    ///
    /// # Errors
    /// Nonfinite/negative extension or no exact original transverse observation.
    pub fn exact_observation(
        &self,
        extension_fraction: f64,
    ) -> Result<&TransverseObservation, Refusal> {
        if !extension_fraction.is_finite() || extension_fraction < 0.0 {
            return Err(invalid(
                "transverse extension must be finite and nonnegative",
            ));
        }
        self.observations
            .iter()
            .find(|row| row.extension_fraction == extension_fraction)
            .ok_or_else(|| invalid("knit strip has no exact requested transverse observation"))
    }
}

/// Lossless retained source documentation supporting acquisition units and license.
#[derive(Clone, Debug, Serialize)]
pub struct AcquisitionDocument {
    /// Source ZIP archive entry.
    pub entry: String,
    /// SHA-256 of all original text bytes, including its line endings.
    pub sha256: String,
    /// Complete original text, not a reworded protocol.
    pub text: String,
    /// Citation and actual retrieval date.
    pub source: Source,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawCurve {
    specimen: String,
    direction: Direction,
    entry: String,
    sha256: String,
    retrieved_on: String,
    csv: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawDocument {
    entry: String,
    sha256: String,
    retrieved_on: String,
    text: String,
}

pub(super) fn parse(
    curves: &[RawCurve],
    readme: Option<&RawDocument>,
    metadata: &BTreeMap<String, Fabric>,
) -> Result<(Vec<TransverseCurve>, Option<AcquisitionDocument>), Refusal> {
    if curves.len() > PINS.len() || (!curves.is_empty() && readme.is_none()) {
        return Err(invalid(
            "transverse acquisition needs at most nine curves and its retained units README",
        ));
    }
    let document = readme.map(|text| {
        if text.entry != "dataset/README.md" || text.sha256 != README_SHA256
            || sha256_hex(text.text.as_bytes()) != README_SHA256
            || text.retrieved_on != RETRIEVED {
            return Err(invalid("transverse acquisition units README differs from its retained source"));
        }
        Ok(AcquisitionDocument {
            entry: text.entry.clone(), sha256: text.sha256.clone(), text: text.text.clone(),
            source: Source {
                reference: format!("{ARCHIVE}#{}", text.entry), accessed_on: RETRIEVED.into(),
                note: "Original MIT dataset README; mechanical-test elongation/compression values are dimensionless fractions. No spatial width-sampling coordinates or branch tags are supplied for transverse CSVs.".into(),
            },
        })
    }).transpose()?;
    let mut identities = BTreeSet::new();
    let mut result = Vec::with_capacity(curves.len());
    for curve in curves {
        let pin = PINS
            .iter()
            .find(|(name, cut, _)| *name == curve.specimen && *cut == curve.direction);
        if !identities.insert((&curve.specimen, curve.direction))
            || !pin.is_some_and(|(_, _, hash)| curve.sha256 == *hash)
            || sha256_hex(curve.csv.as_bytes()) != curve.sha256
            || curve.retrieved_on != RETRIEVED
            || metadata
                .get(&curve.specimen)
                .and_then(|fabric| fabric.poisson.get(curve.direction.as_str()))
                .map(|path| format!("dataset/data/{path}"))
                != Some(curve.entry.clone())
        {
            return Err(invalid(
                "transverse specimen/cut, source entry, bytes or retrieval identity differ",
            ));
        }
        let observations = observations(&curve.csv)?;
        result.push(TransverseCurve {
            specimen: curve.specimen.clone(), direction: curve.direction, entry: curve.entry.clone(),
            sha256: curve.sha256.clone(), observations,
            source: Source {
                reference: format!("{ARCHIVE}#{}", curve.entry), accessed_on: RETRIEVED.into(),
                note: format!("Sperl et al. (2022), exact original {} {} transverse CSV SHA-256 {}. Dataset README establishes fractional units. Paper section 4.3/Figure 4 ({ARTICLE}) defines c=1-w/reference_width, with reference width 60 mm. No additional interpolation, clamping or conversion to Poisson ratio.", curve.specimen, curve.direction.as_str(), curve.sha256),
            },
            measurement_scope: "Source CSV has no loading/unloading tag, timestamps, uncertainty or spatial measurement coordinates. Figure 4 depicts transverse width near the swatch center; the exact image-sampling line/material points are not reported. Equivalence to a solver's material-midpoint distance is unverified, particularly for shearing bias cuts.".into(),
        });
    }
    Ok((result, document))
}

fn observations(csv: &str) -> Result<Vec<TransverseObservation>, Refusal> {
    let mut lines = csv.lines();
    if lines.next() != Some("elongation,compression") {
        return Err(invalid(
            "transverse CSV requires its original elongation/compression columns",
        ));
    }
    let mut observations = Vec::new();
    let mut previous = None;
    for (i, line) in lines.enumerate() {
        let mut columns = line.split(',');
        let (Some(extension), Some(compression), None) =
            (columns.next(), columns.next(), columns.next())
        else {
            return Err(invalid("transverse CSV needs exactly two columns"));
        };
        let extension: f64 = extension
            .parse()
            .map_err(|_| invalid("transverse extension is not numeric"))?;
        let compression: f64 = compression
            .parse()
            .map_err(|_| invalid("transverse compression is not numeric"))?;
        if i >= 4096
            || !extension.is_finite()
            || extension < 0.0
            || !compression.is_finite()
            || previous.is_some_and(|value| extension <= value)
        {
            return Err(invalid(
                "transverse rows require finite observations and increasing nonnegative extensions",
            ));
        }
        previous = Some(extension);
        observations.push(TransverseObservation {
            csv_line: i + 2,
            extension_fraction: extension,
            compression_fraction: compression,
        });
    }
    if observations.is_empty() {
        return Err(invalid("transverse CSV has no observations"));
    }
    Ok(observations)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transverse_parser_retains_signed_values_and_refuses_reordered_rows() {
        let rows = observations("elongation,compression\n0,-0.001\n0.2,0.1\n").unwrap();
        assert_eq!(rows[0].compression_fraction, -0.001);
        for csv in [
            "elongation,compression\n0.2,0.1\n0.1,0.2\n",
            "elongation,compression\n0.2,0.1\n0.2,0.2\n",
            "elongation,compression\n0.2,NaN\n",
            "elongation,compression,tag\n0.2,0.1,loading\n",
            "elongation,compression\n",
        ] {
            assert!(observations(csv).is_err());
        }
    }
}

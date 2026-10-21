//! Measured grip-force screening from the MIT-licensed Sperl et al. (2022) production-knit data.
//! Clamped strips are spatially nonuniform: their force/extension curves are not homogeneous
//! membrane observations. Only exact author-reported loading rows become specimen-level attributes.

use std::collections::BTreeMap;

#[cfg(test)]
use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use serde::Deserialize;

use crate::materials::{Component, Evidence, Form, Material, Quantity, Source};

#[cfg(test)]
const DATA: &str = include_str!("knit-strip-measurements.json");
const ARCHIVE: &str = "https://mslab.es/projects/YarnLevelFabrics/dataset/dataset.zip";
const ARTICLE: &str = "https://doi.org/10.1145/3528223.3530167";
const METADATA_SHA256: &str = "258e3cc2a40ba40d1e0152f80b1fb08c67d14c08db10efbc2ec385669887a6d4";
const RETRIEVED: &str = "2026-10-20";

#[cfg(test)]
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
}

#[cfg(test)]
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
    #[cfg(test)]
    #[serde(default)]
    stretch: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct Yarn {
    name: String,
    percentage: f64,
    count: u32,
}

#[cfg(test)]
fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

/// Preserve signed source values and both branches. This importer applies no smoothing, resampling,
/// force inversion or strain conversion. Missing exact loading rows are never interpolated.
#[cfg(test)]
fn loading_observations(curve: &Curve) -> Result<[f64; 2], Refusal> {
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
    let mut observations = [None; 2];
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
        match branch {
            "loading" if !unloading => {
                if previous_loading.is_some_and(|v| extension <= v) {
                    return Err(invalid(
                        "knit strip loading extensions must increase strictly",
                    ));
                }
                previous_loading = Some(extension);
                for (i, target) in [0.1, 0.3].into_iter().enumerate() {
                    if extension == target {
                        observations[i] = Some(force);
                    }
                }
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
            }
            _ => return Err(invalid("knit strip branch is unknown or loading restarts")),
        }
    }
    let [Some(low), Some(high)] = observations else {
        return Err(invalid(
            "knit strip has no exact 10% or 30% loading observation",
        ));
    };
    if !unloading {
        return Err(invalid(
            "knit strip source acquisition is missing its unloading branch",
        ));
    }
    Ok([low, high])
}

fn record(specimen: &str, fabric: &Fabric, curves: &[(&str, &str, [f64; 2])]) -> Material {
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
        .map(|y| Component {
            fiber: if y.name.contains("Spandex") {
                "elastane"
            } else if y.name.contains("Celliant") {
                "polyester (Celliant-labelled yarn)"
            } else {
                "polyester"
            }
            .into(),
            fraction: y.percentage,
            source: "metadata".into(),
        })
        .collect();
    Material {
            id: format!("sperl2022/{specimen}"),
            name: format!("Sperl 2022 {} {specimen}", if specimen.starts_with("SJ") { "single jersey" } else { "double-knit interlock" }),
            form: Form::Knit, composition, composition_complete: true,
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

pub(super) fn records() -> Vec<Material> {
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
        #[cfg(test)]
        stretch: BTreeMap::new(),
    };
    vec![
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
                    [0.40, 1.94],
                ),
                (
                    "weft",
                    "0fe20ce39b5632ccf2437abc2646d70b5f65406fb5b19f8ea3fb804a26a26249",
                    [0.04, 0.84],
                ),
                (
                    "bias",
                    "3a2abad579ebc91afbb2ae801365d69bc266b197ff5f11a9d737b63a38c55c1b",
                    [0.27, 1.12],
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
                    [0.71, 2.11],
                ),
                (
                    "weft",
                    "ad9fa8d6a07a700348684813d5bf87212de2dba43a2577d2930922aec57780f9",
                    [0.80, 2.48],
                ),
                (
                    "bias",
                    "fcb93afd8c919d3a6189f6622cb8fe8573a90afd09a04bb90b78b762dda327bf",
                    [0.53, 1.78],
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
                    [1.21, 3.39],
                ),
                (
                    "weft",
                    "491b997ec5ce460c9f2018d7a791b63a3b656fa5654efb316212888cd3c3d10f",
                    [1.56, 5.00],
                ),
                (
                    "bias",
                    "d6b48f9f457932ffec430546f1660f4819fd316106b8bfc42aee3421e9f20365",
                    [1.15, 3.39],
                ),
            ],
        ),
    ]
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
                    loading_observations(curve)?,
                ));
            }
            records.push(record(specimen, fabric, &observations));
        }
        Ok(records)
    }

    #[test]
    fn compact_catalog_replays_exactly_from_original_source_acquisitions() {
        assert_eq!(
            serde_json::to_value(records()).unwrap(),
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

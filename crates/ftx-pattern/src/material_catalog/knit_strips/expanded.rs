//! Remaining released Sperl specimens, using the existing catalog and acquisition contracts.
//! The compact table contains exact source rows; original CSVs are test/acquisition data only.

use std::collections::BTreeMap;

use ftx_core::Refusal;
use serde::Deserialize;

use super::{Direction, Fabric, invalid, record};
use crate::materials::{Material, Source};

pub(super) const RETRIEVED: &str = "2026-10-30";
const SUMMARY: &str = include_str!("expanded.json");
const SUPPLEMENT: &str =
    "https://mslab.es/projects/YarnLevelFabrics/contents/sperl2022eylsmpf_supplementary.pdf";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Row {
    specimen: String,
    construction: String,
    fabric: Fabric,
    curves: Vec<Curve>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Curve {
    direction: Direction,
    sha256: String,
    // Exact loading observations at 10% and 30%; absent rows are never extrapolated.
    force_n: [Option<f64>; 2],
}

fn rows() -> Result<Vec<Row>, Refusal> {
    serde_json::from_str(SUMMARY).map_err(|e| invalid(format!("expanded knit source summary: {e}")))
}

pub(super) fn pins() -> Result<BTreeMap<String, BTreeMap<Direction, String>>, Refusal> {
    Ok(rows()?
        .into_iter()
        .map(|row| {
            (
                row.specimen,
                row.curves
                    .into_iter()
                    .map(|curve| (curve.direction, curve.sha256))
                    .collect(),
            )
        })
        .collect())
}

fn expanded_record(row: &Row) -> Material {
    let curves: Vec<_> = row
        .curves
        .iter()
        .map(|curve| {
            (
                curve.direction.as_str(),
                curve.sha256.as_str(),
                curve.force_n,
            )
        })
        .collect();
    let mut material = record(&row.specimen, &row.fabric, &curves);
    material.name = format!("Sperl 2022 {} {}", row.construction, row.specimen);
    material
        .structure
        .insert("construction".into(), row.construction.clone());
    material.structure.extend([
        ("construction_source".into(), "Author supplementary document, named specimen section; original construction names, not an inference from fabric weight or composition".into()),
        ("yarn_mass_fractions".into(), row.fabric.composition.iter().map(|y| format!("{}: {}", y.name, y.percentage)).collect::<Vec<_>>().join("; ")),
        ("composition_scope".into(), "Reported yarn mass fractions; source-only labels CD and Lycra T400 do not establish a constituent fiber identity. Unresolved fractions remain outside the fiber composition. Celliant additive composition and finish mass are not supplied".into()),
        ("retention".into(), "Original 90 additional force-curve CSVs, full metadata and MIT notice retained in tests/fixtures/materials/knit-strip-expanded.json; no photos, geometry assets or supplementary pages bundled".into()),
        ("force_scope".into(), "Exact source loading observations at 10% and 30% grip extension where present, including separately marked observations outside the paper's stated 5 N test/working range. The publication/data-range discrepancy is retained. No curve resampling, truncation or modeled-range extension".into()),
    ]);
    for source in material.sources.values_mut() {
        source.accessed_on = RETRIEVED.into();
    }
    material.sources.get_mut("metadata").unwrap().note = format!(
        "Sperl et al. (2022), exact fabric {}; original metadata SHA-256 {}. Fresh archive SHA-256 4ef059e1a33844fbd082c3662ac2df4444ab9b5280b0f1be49f66b11c44d7b8d, 142552558 bytes. Original metadata bytes and MIT notice retained in tests/fixtures/materials/knit-strip-expanded.json. No supplier SKU, lot or general brand equivalence is inferred.",
        row.specimen,
        super::METADATA_SHA256,
    );
    material.sources.get_mut("paper").unwrap().note =
        "Section 4.3 and Figure 4: clamped strip, 100 mm free gauge by 60 mm width, weft/bias/warp; displacement-controlled total force with spatially nonuniform deformation. The paper describes tests/working range up to 5 N, whereas released CSVs contain higher forces. Exact 10% and 30% source loading rows are retained when available, even if their reported force exceeds that range; each retained force records whether it lies in the working range. The discrepancy does not extend the published model's range. No missing row is interpolated or extrapolated. Instrument-level rawness and upstream processing are unspecified.".into();
    material.sources.insert("supplement".into(), Source {
        reference: SUPPLEMENT.into(),
        accessed_on: RETRIEVED.into(),
        note: format!("Sperl et al. (2022), author supplementary document, {} specimen section. Only factual construction labels are transcribed. PDF SHA-256 b12a6771a72120111c092273ecfddfa293cd427ec53ea07255ec91bd328edf6d, 10226857 bytes. Copyright ACM; source pages, photos and illustrations are not redistributed. Numeric fabric observations and original CSVs come from the separately MIT-licensed dataset.", row.specimen),
    });
    for (name, quantity) in &mut material.properties {
        if name.starts_with("strip_force_") {
            quantity.conditions.extend([
                ("paper_working_force_limit_n".into(), "5".into()),
                (
                    "within_paper_working_force_range".into(),
                    (0.0..=5.0).contains(&quantity.value).to_string(),
                ),
            ]);
        }
    }
    material
}

pub(super) fn records() -> Result<Vec<Material>, Refusal> {
    Ok(rows()?.iter().map(expanded_record).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::material_catalog::knit_strips::parse_acquisition;

    const ACQUISITION: &str =
        include_str!("../../../../../tests/fixtures/materials/knit-strip-expanded.json");

    #[test]
    fn every_compact_observation_replays_from_its_exact_released_curve() {
        let acquisition = parse_acquisition(ACQUISITION).unwrap();
        assert_eq!(acquisition.curves.len(), 90);
        assert_eq!(
            acquisition
                .curves
                .iter()
                .map(|curve| curve.observations.len())
                .sum::<usize>(),
            103_242
        );
        let metadata: BTreeMap<String, Fabric> =
            serde_json::from_str(&acquisition.metadata_json).unwrap();
        let rows = rows().unwrap();
        assert_eq!(rows.len(), 30);
        let mut missing = Vec::new();
        for row in &rows {
            let original = &metadata[&row.specimen];
            assert_eq!(row.fabric.density_g_m2, original.density_g_m2);
            assert_eq!(row.fabric.thickness_mm, original.thickness_mm);
            assert_eq!(row.fabric.finishes, original.finishes);
            assert_eq!(row.fabric.composition.len(), original.composition.len());
            for (compact, source) in row.fabric.composition.iter().zip(&original.composition) {
                assert_eq!(compact.name, source.name);
                assert_eq!(compact.percentage, source.percentage);
                assert_eq!(compact.count, source.count);
            }
            assert_eq!(row.curves.len(), 3);
            for compact in &row.curves {
                let curve = acquisition
                    .curves
                    .iter()
                    .find(|curve| {
                        curve.specimen == row.specimen && curve.direction == compact.direction
                    })
                    .unwrap();
                assert_eq!(curve.sha256, compact.sha256);
                for (extension, value) in [0.1, 0.3].into_iter().zip(compact.force_n) {
                    assert_eq!(
                        curve.exact_loading(extension).ok().map(|o| o.force_n),
                        value,
                        "{} {:?} at {extension}",
                        row.specimen,
                        compact.direction,
                    );
                    if value.is_none() {
                        missing.push((row.specimen.as_str(), compact.direction, extension));
                    }
                }
            }
            expanded_record(row).validate().unwrap();
        }
        assert_eq!(
            missing,
            [
                ("DKIN4", Direction::Warp, 0.3),
                ("DKIN5", Direction::Warp, 0.3)
            ]
        );
    }

    #[test]
    fn original_source_identity_and_retrieval_date_cannot_be_relabelled() {
        let mut acquisition: serde_json::Value = serde_json::from_str(ACQUISITION).unwrap();
        acquisition["retrieved_on"] = super::super::RETRIEVED.into();
        assert!(parse_acquisition(&acquisition.to_string()).is_err());
        acquisition["retrieved_on"] = RETRIEVED.into();
        let original = acquisition["curves"][0]["csv"].as_str().unwrap();
        let changed = original.replace("0.1,0.09,loading", "0.1,99,loading");
        assert_ne!(original, changed);
        acquisition["curves"][0]["csv"] = changed.clone().into();
        acquisition["curves"][0]["sha256"] =
            ftx_core::receipt::sha256_hex(changed.as_bytes()).into();
        assert!(parse_acquisition(&acquisition.to_string()).is_err());
    }
}

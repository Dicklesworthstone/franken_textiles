//! Numerical facts from the MIT Fabric Properties Dataset's summary and identified workbook rows.
//! No photos/video or source archive are redistributed. Width, composition fractions, conditioning,
//! directional moduli and sheet bending are NOT supplied by this table and remain absent.

use std::collections::BTreeMap;

use crate::materials::{Evidence, Form, Material, Quantity, Source};

const TABLE: &str = "https://people.csail.mit.edu/klbouman/pw/projects/materialproperties/images/materialproptable.png";
const DATASET: &str =
    "https://people.csail.mit.edu/klbouman/pw/projects/materialproperties/dataset.html";

// ID order from the primary table: description, lbf*in^2, oz/yd^2, single-measurement asterisk.
// Beam/test-specimen EI (N*m^2) must never be mistaken for sheet D (N*m).
const ROWS: [(&str, f64, f64, bool); 30] = [
    ("Lycra", 0.0250, 7.0600, false),
    ("Faux Fur", 0.0512, 13.5400, false),
    ("Silk", 0.0075, 1.9750, false),
    ("Silk", 0.0110, 1.9750, false),
    ("Cotton", 0.0100, 2.9400, false),
    ("Wool", 0.0375, 11.3400, false),
    ("Taffeta", 0.0165, 3.3750, false),
    ("Linen", 0.0225, 4.4750, false),
    ("Corduroy", 0.0185, 4.5150, false),
    ("Cotton", 0.0225, 4.1045, false),
    ("Velvet", 0.0200, 3.9250, false),
    ("Fleece", 0.0205, 5.6300, false),
    ("Denim", 0.0250, 8.7500, true),
    ("Upholstery", 0.0400, 9.8100, false),
    ("Upholstery", 0.0750, 14.1600, false),
    ("Pleather", 0.1000, 16.6600, true),
    ("Minky", 0.0600, 7.5100, true),
    ("Damask Upholstery", 0.0800, 14.6650, false),
    ("Flannel Backed Vinyl", 0.0350, 5.1400, true),
    ("Upholstery", 0.0675, 10.2570, false),
    ("Outdoor Polyester", 0.0550, 6.1100, true),
    ("Silk", 0.0300, 4.0935, false),
    ("Wool", 0.0500, 9.4600, true),
    ("Canvas", 0.1000, 11.5900, true),
    ("Nylon Rip Stop", 0.0165, 1.7261, false),
    ("Terry Knit", 0.0330, 4.9850, true),
    ("Lycra", 0.0550, 6.6400, true),
    ("Laminated Cotton", 0.0500, 5.3200, true),
    ("Lycra", 0.0120, 5.2100, true),
    ("Upholstery", 0.0875, 11.3900, false),
];

const ARCHIVE: &str =
    "https://people.csail.mit.edu/klbouman/pw/data/materialproperties/material_properties.zip";
const ARCHIVE_SHA: &str = "99021ff230ac8f08b87e111ff5d30fc4fc23303c10a1ffa1f1ac05eb934bc01d";
const WORKBOOK_SHA: &str = "94f71fbd0dbd67b108f0472f73a5b1c048b2917d3403f7863c981faaa84575c1";

// Sheet1 rows 2..53, columns A..G: fabric, sample, EI, thickness, density, area weight,
// ordinal bending-confidence rating. These are source rows, not 52 independent fabrics/replicates.
// Area weights include source formula results; they are not independent of density/thickness.
const RAW_ROWS: [(u8, &str, f64, f64, f64, f64, u8); 52] = [
    (
        1,
        "a",
        0.025,
        0.0145,
        0.02345179188522615,
        7.051297169714717,
        3,
    ),
    (
        1,
        "b",
        0.025,
        0.0155,
        0.0219985784710582,
        7.0705191092258755,
        2,
    ),
    (
        2,
        "a",
        0.025,
        0.0505,
        0.013471417432942277,
        14.106837250419298,
        1,
    ),
    (
        2,
        "b",
        0.09,
        0.0505,
        0.013471417432942277,
        14.106837250419298,
        3,
    ),
    (
        2,
        "c",
        0.03,
        0.0491,
        0.012739580587970246,
        12.970646004842614,
        3,
    ),
    (
        2,
        "d",
        0.06,
        0.0491,
        0.012739580587970246,
        12.970646004842614,
        3,
    ),
    (
        3,
        "a",
        0.007,
        0.005,
        0.018_879_498_364_231_19,
        1.9574263904034896,
        3,
    ),
    (
        3,
        "b",
        0.008,
        0.005,
        0.019220928457926726,
        1.992825862517843,
        3,
    ),
    (
        4,
        "a",
        0.01,
        0.006,
        0.02076741568325232,
        2.583798789647521,
        2,
    ),
    (
        4,
        "b",
        0.012,
        0.0065,
        0.019_994_328_245_061_18,
        2.6949155381823258,
        2,
    ),
    (
        5,
        "a",
        0.01,
        0.0075,
        0.020058180896831018,
        3.1194482930751595,
        1,
    ),
    (
        5,
        "b",
        0.01,
        0.0075,
        0.017737269158635,
        2.7585000995509157,
        1,
    ),
    (
        6,
        "a",
        0.04,
        0.0444,
        0.01262296458994884,
        11.621690841930754,
        1,
    ),
    (
        6,
        "b",
        0.035,
        0.0438,
        0.012181333316924687,
        11.063535191497063,
        1,
    ),
    (
        7,
        "a",
        0.013,
        0.008,
        0.020470378469240064,
        3.395790143505296,
        1,
    ),
    (
        7,
        "b",
        0.02,
        0.008,
        0.020224300173652485,
        3.3549687072068637,
        2,
    ),
    (
        7,
        "c",
        0.033,
        0.01,
        0.01760003387064493,
        3.649_543_023_416_933,
        2,
    ),
    (
        8,
        "a",
        0.03,
        0.0085,
        0.025650878311679278,
        4.521_121_207_703_343,
        2,
    ),
    (
        8,
        "b",
        0.015,
        0.0095,
        0.022500259743258217,
        4.432_371_167_343_923,
        2,
    ),
    (
        9,
        "a",
        0.017,
        0.012,
        0.0178399784731835,
        4.439_157_523_439_196,
        2,
    ),
    (
        9,
        "b",
        0.02,
        0.012,
        0.01844788142347508,
        4.590_423_230_366_151,
        3,
    ),
    (
        10,
        "a",
        0.02,
        0.0085,
        0.023255205981936707,
        4.098_869_585_552_237,
        2,
    ),
    (
        10,
        "b",
        0.025,
        0.0085,
        0.02331811640037034,
        4.109957924263675,
        2,
    ),
    (
        11,
        "a",
        0.02,
        0.0095,
        0.020457508583711512,
        4.029_965_530_922_498,
        2,
    ),
    (
        11,
        "b",
        0.02,
        0.0095,
        0.01937172270209356,
        3.8160743985308145,
        2,
    ),
    (
        12,
        "a",
        0.011,
        0.0228,
        0.011745175445527693,
        5.552_893_443_276_94,
        1,
    ),
    (
        12,
        "b",
        0.03,
        0.023,
        0.011964493044950055,
        5.706_201_738_941_94,
        2,
    ),
    (13, "a", 0.025, 0.024, 0.018, 8.75, 2),
    (
        14,
        "a",
        0.035,
        0.0257,
        0.01871526777738244,
        9.973_650_670_637_32,
        2,
    ),
    (
        14,
        "b",
        0.045,
        0.0254,
        0.01832112439397892,
        9.649_633_620_012_091,
        2,
    ),
    (15, "a", 0.08, 0.032, 0.021, 13.69, 1),
    (15, "b", 0.07, 0.035, 0.02, 14.63, 2),
    (16, "a", 0.1, 0.032, 0.025, 16.66, 1),
    (
        17,
        "a",
        0.06,
        0.0187,
        0.019368888356013466,
        7.5105421293705215,
        2,
    ),
    (
        18,
        "a",
        0.075,
        0.0433,
        0.016_493_651_389_168_58,
        14.809134980411127,
        3,
    ),
    (
        18,
        "b",
        0.085,
        0.045,
        0.01556344611390002,
        14.522562837802386,
        1,
    ),
    (
        19,
        "a",
        0.035,
        0.0136,
        0.018_212_529_771_324_88,
        5.136108235799421,
        1,
    ),
    (
        20,
        "a",
        0.07,
        0.0328,
        0.01516188144164906,
        10.312214173228346,
        2,
    ),
    (
        20,
        "b",
        0.065,
        0.0322,
        0.015282676391823274,
        10.204230800679287,
        2,
    ),
    (
        21,
        "a",
        0.055,
        0.0135,
        0.021840416914800376,
        6.113_918_949_461_558,
        1,
    ),
    (
        22,
        "a",
        0.03,
        0.0125,
        0.015768549571738955,
        4.0872080489947376,
        1,
    ),
    (
        22,
        "b",
        0.03,
        0.013,
        0.015226721760409638,
        4.104_636_931_510_106,
        2,
    ),
    (
        23,
        "a",
        0.05,
        0.0318,
        0.014340221388507367,
        9.456_010_816_644_424,
        2,
    ),
    (
        24,
        "a",
        0.1,
        0.0195,
        0.028662868734303234,
        11.589888298452982,
        1,
    ),
    (
        25,
        "a",
        0.018,
        0.003,
        0.027681721382021962,
        1.7220245237328222,
        1,
    ),
    (
        25,
        "b",
        0.015,
        0.003,
        0.02780460993562333,
        1.7296691748752562,
        2,
    ),
    (
        26,
        "a",
        0.033,
        0.015,
        0.016025939800992116,
        4.984_708_315_700_588,
        1,
    ),
    (
        27,
        "a",
        0.055,
        0.015,
        0.021359728039033663,
        6.643_729_809_261_029,
        3,
    ),
    (
        28,
        "a",
        0.05,
        0.011,
        0.023318257143694527,
        5.318801181448146,
        2,
    ),
    (
        29,
        "a",
        0.05,
        0.012,
        0.020941390334673352,
        5.210_888_039_757_44,
        2,
    ),
    (
        30,
        "a",
        0.055,
        0.0339,
        0.016117190966408304,
        11.329585836713104,
        3,
    ),
    (
        30,
        "b",
        0.12,
        0.0322,
        0.017147623188734057,
        11.449_454_285_019_18,
        3,
    ),
];

pub(super) fn raw_records() -> Vec<Material> {
    RAW_ROWS.iter().enumerate().map(|(i, &(fabric, sample, bending, thickness, density, weight, confidence))| {
        let name = ROWS[usize::from(fabric) - 1].0;
        let source_row = format!("matprop.xlsx Sheet1 row {}; fabric {fabric}, sample {sample}", i + 2);
        let conditions = BTreeMap::from([
            ("specimen".into(), format!("MIT-ICCV2013-fabric-{fabric}")),
            ("source_row".into(), source_row.clone()),
            ("direction".into(), "not identified for sample letters".into()),
            ("conditioning".into(), "not reported; humidity, temperature and gauge pressure unknown".into()),
            ("aggregation".into(), "one workbook row; sample letters do not establish independent replicates".into()),
        ]);
        let q = |value, unit: &str, method: &str| Quantity {
            value, unit: unit.into(), evidence: Evidence::Published, source: "workbook".into(),
            method: method.into(), conditions: conditions.clone(), bounds: None,
        };
        Material {
            id: format!("mit.raw-2013.{fabric:02}{sample}"),
            name: format!("MIT fabric {fabric} sample {sample}: {name}"),
            form: match name { "Terry Knit" => Form::Knit, "Laminated Cotton" => Form::Laminate, _ => Form::Sheet },
            composition: Vec::new(), composition_complete: false,
            structure: BTreeMap::from([
                ("source_row".into(), source_row.clone()),
                ("summary_record".into(), format!("mit.fabric-2013.{fabric:02}")),
                ("bending_confidence_ordinal".into(), confidence.to_string()),
                ("confidence_scope".into(), "source ordinal 1..4, 1 highest confidence, bending only; not probability, sigma or a weight".into()),
                ("identity_scope".into(), "source sample within named research fabric; lot, direction and replicate independence unknown".into()),
                ("retention".into(), "numerical facts only; no source workbook/media redistribution license asserted".into()),
            ]),
            properties: BTreeMap::from([
                ("reported_thickness".into(), q(thickness, "in", "Source column D; measurement pressure and conditioning not reported; not qualified uncompressed/contact thickness")),
                ("bulk_density".into(), q(density, "lb/in3", "Source column E; bulk specimen density, not solid fiber density")),
                ("areal_density".into(), q(weight, "oz/yd2", "Source column F as reported, including cached density*thickness*16*36*36 formula results; independent measurement status unknown; source rounding preserved")),
                ("specimen_flexural_rigidity".into(), q(bending, "lbf*in2", "Source column C; specimen EI, unknown coupon width/geometry; no sheet N*m conversion")),
            ]),
            membrane_response: None, membrane_relaxation: None, thermal_response: None, optical_faces: None,
            sources: BTreeMap::from([
                ("workbook".into(), Source { reference: ARCHIVE.into(), accessed_on: "2026-10-21".into(),
                    note: format!("Bouman, Xiao, Battaglia and Freeman, ICCV 2013; Lowell ACMTRL measurements. {source_row}. ZIP SHA256 {ARCHIVE_SHA}; member material_properties (2) 2/matprop.xlsx SHA256 {WORKBOOK_SHA}. Numeric facts transcribed, source archive retained externally. No direction inferred or summary replaced.") }),
                ("dataset".into(), Source { reference: DATASET.into(), accessed_on: "2026-10-21".into(),
                    note: "Primary page and archive README identify fabric/sample numbers and ordinal bending confidence. No quantitative uncertainty, coupon width, conditioning or dataset redistribution license supplied.".into() }),
            ]),
        }
    }).collect()
}

pub(super) fn records() -> Vec<Material> {
    ROWS.iter().enumerate().map(|(i, &(name, bending, weight, single))| {
        let specimen = format!("MIT-ICCV2013-fabric-{}", i + 1);
        let aggregation = if single {
            "one measurement (asterisk in source); direction unspecified"
        } else {
            "reported average; most specimens averaged over two orthogonal directions; exact replicate count unspecified"
        };
        let conditions = BTreeMap::from([
            ("specimen".into(), specimen.clone()),
            ("aggregation".into(), aggregation.into()),
            ("conditioning".into(), "not specified in summary table".into()),
        ]);
        let q = |value, unit: &str, method: &str| Quantity {
            value, unit: unit.into(), evidence: Evidence::Measured, source: "table".into(),
            method: method.into(), conditions: conditions.clone(), bounds: None,
        };
        Material {
            id: format!("mit.fabric-2013.{:02}", i + 1),
            name: format!("MIT fabric {}: {name}", i + 1),
            form: match name {
                "Terry Knit" => Form::Knit,
                "Laminated Cotton" => Form::Laminate,
                _ => Form::Sheet,
            },
            composition: Vec::new(), composition_complete: false,
            structure: BTreeMap::from([
                ("specimen".into(), specimen),
                ("source_description".into(), name.into()),
                ("measurement_aggregation".into(), aggregation.into()),
                ("identity_scope".into(), "specific research specimen; supplier, lot and blend fractions not reported in summary".into()),
                ("retention".into(), "transcribed numerical facts only; no source media; no dataset redistribution license asserted".into()),
            ]),
            properties: BTreeMap::from([
                ("areal_density".into(), q(weight, "oz/yd2", "Lowell ACMTRL measurement reported in MIT summary table; test standard not given")),
                ("specimen_flexural_rigidity".into(), q(bending, "lbf*in2", "Reported bending stiffness of test specimen; coupon width/geometry absent, so no sheet-rigidity conversion")),
            ]),
            membrane_response: None, membrane_relaxation: None, thermal_response: None, optical_faces: None,
            sources: BTreeMap::from([
                ("table".into(), Source { reference: TABLE.into(), accessed_on: "2026-10-14".into(),
                    note: format!("Bouman, Xiao, Battaglia and Freeman, Estimating the Material Properties of Fabric from Video, ICCV 2013. Summary table specimen {}. Original numeric units retained; asterisks identify single measurements. No image copied.", i + 1) }),
                ("dataset".into(), Source { reference: DATASET.into(), accessed_on: "2026-10-14".into(),
                    note: "Primary authors' dataset page identifies Lowell ACMTRL Laboratory and aggregation conventions. Ground-truth archive not ingested; these are summary-table facts, not raw replicate replay.".into() }),
            ]),
        }
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::material_catalog::{Selection, normalize};
    use crate::materials::Catalog;

    #[test]
    fn all_thirty_specimens_are_unique_source_bearing_and_still_partial() {
        let c = Catalog::builtin().unwrap();
        let rows: Vec<_> = c
            .materials
            .iter()
            .filter(|m| m.id.starts_with("mit.fabric-2013."))
            .collect();
        assert_eq!(rows.len(), 30);
        assert_eq!(rows.iter().map(|m| m.properties.len()).sum::<usize>(), 60);
        for record in rows {
            let resolved = c
                .resolve(&Selection {
                    id: record.id.clone(),
                    ..Selection::default()
                })
                .unwrap();
            assert!(!record.composition_complete && record.composition.is_empty());
            assert!(
                resolved
                    .material()
                    .properties
                    .values()
                    .all(|q| q.evidence == Evidence::Measured)
            );
            assert!(
                !resolved
                    .material()
                    .properties
                    .contains_key("bending_rigidity")
            );
            assert!(resolved.mechanics().is_err());
            assert!(
                resolved
                    .material()
                    .require("thickness", "m", false)
                    .is_err()
            );
            let replay = c.resolve(&resolved.frozen_selection()).unwrap();
            assert_eq!(resolved.sha256(), replay.sha256());
        }
    }

    #[test]
    fn original_units_normalize_without_conflating_sheet_and_specimen_bending() {
        let rows = records();
        let linen = normalize(&rows[7]).unwrap();
        // Independent conversion through grams and square centimetres.
        let expected_mass = 4.475 * 28.349_523_125 / (91.44 * 91.44) * 10.0;
        assert!((linen.properties["areal_density"].value - expected_mass).abs() < 1e-14);
        let bending = &linen.properties["specimen_flexural_rigidity"];
        assert_eq!(bending.unit, "N*m2");
        assert!(
            (bending.value - 0.0225 * 0.453_592_37 * 9.80665 * (2.54_f64 / 100.).powi(2)).abs()
                < 1e-18
        );
        assert_eq!(rows[7].properties["areal_density"].unit, "oz/yd2");
        assert_eq!(rows[15].properties["areal_density"].value, 16.6600);
        assert_eq!(rows[24].properties["areal_density"].value, 1.7261);
        assert_eq!(
            rows[29].properties["specimen_flexural_rigidity"].value,
            0.0875
        );
        assert_ne!(
            rows[2].properties["specimen_flexural_rigidity"].value,
            rows[3].properties["specimen_flexural_rigidity"].value
        );
    }

    #[test]
    fn source_aggregation_and_material_identity_are_not_silently_upgraded() {
        let rows = records();
        assert!(rows[12].structure["measurement_aggregation"].starts_with("one measurement"));
        assert!(
            rows[0].structure["measurement_aggregation"]
                .contains("exact replicate count unspecified")
        );
        assert_eq!(rows[15].form, Form::Sheet); // Pleather is not authenticated animal hide.
        assert_eq!(rows[25].form, Form::Knit);
        assert_eq!(rows[27].form, Form::Laminate);
        let mut bad = rows[7].clone();
        let specimen = bad.properties.remove("specimen_flexural_rigidity").unwrap();
        bad.properties.insert("bending_rigidity".into(), specimen);
        assert!(
            normalize(&bad).is_err(),
            "N*m^2 must not pass a sheet N*m gate"
        );
    }

    #[test]
    fn raw_rows_retain_identity_provenance_and_missing_consumer_requirements() {
        let catalog = Catalog::builtin().unwrap();
        let rows = raw_records();
        assert_eq!(rows.len(), 52);
        assert_eq!(rows.iter().map(|m| m.properties.len()).sum::<usize>(), 208);
        let mut identities = std::collections::BTreeSet::new();
        for row in rows {
            assert!(identities.insert(row.id.clone()));
            let resolved = catalog
                .resolve(&Selection {
                    id: row.id.clone(),
                    ..Selection::default()
                })
                .unwrap();
            let material = resolved.material();
            assert!(material.require("reported_thickness", "m", false).unwrap() > 0.0);
            assert!(material.require("thickness", "m", false).is_err());
            assert!(material.require("areal_density", "kg/m2", false).unwrap() > 0.0);
            assert!(material.require("bulk_density", "kg/m3", false).unwrap() > 0.0);
            assert!(resolved.mechanics().is_err());
            assert!(resolved.thermal_resistance().is_err());
            for q in row.properties.values() {
                assert_eq!(q.evidence, Evidence::Published);
                assert!(q.bounds.is_none());
                assert!(q.conditions["direction"].contains("not identified"));
                assert!(q.conditions["conditioning"].contains("not reported"));
            }
            assert!(!row.properties.contains_key("bending_rigidity"));
            assert!(row.sources["workbook"].note.contains(WORKBOOK_SHA));
            assert!(row.sources["workbook"].note.contains(ARCHIVE_SHA));
            let replay = catalog.resolve(&resolved.frozen_selection()).unwrap();
            assert_eq!(resolved.sha256(), replay.sha256());
            assert_eq!(
                serde_json::to_value(resolved.authored()).unwrap(),
                serde_json::to_value(row).unwrap()
            );
        }
    }

    #[test]
    fn raw_linen_normalizes_and_disagreements_do_not_replace_summary_or_source_values() {
        let catalog = Catalog::builtin().unwrap();
        let get = |id: &str| {
            catalog
                .resolve(&Selection {
                    id: id.into(),
                    ..Selection::default()
                })
                .unwrap()
        };
        let linen = get("mit.raw-2013.08a");
        assert!(
            (linen
                .material()
                .require("reported_thickness", "m", false)
                .unwrap()
                - 0.000_215_9)
                .abs()
                < 1e-18
        );
        let expected_density = 0.025650878311679278 * 453.59237 / 2.54_f64.powi(3) * 1000.0;
        assert!(
            (linen
                .material()
                .require("bulk_density", "kg/m3", false)
                .unwrap()
                - expected_density)
                .abs()
                < 1e-12
        );
        assert!(
            (get("mit.raw-2013.08b").material().properties["reported_thickness"].value
                - 0.000_241_3)
                .abs()
                < 1e-18
        );
        assert_eq!(
            get("mit.fabric-2013.04").authored().properties["areal_density"].value,
            1.975
        );
        assert_eq!(
            get("mit.raw-2013.04a").authored().properties["areal_density"].value,
            2.583798789647521
        );
        assert_eq!(
            get("mit.raw-2013.04b").authored().properties["areal_density"].value,
            2.6949155381823258
        );
        assert_eq!(
            get("mit.fabric-2013.29").authored().properties["specimen_flexural_rigidity"].value,
            0.012
        );
        assert_eq!(
            get("mit.raw-2013.29a").authored().properties["specimen_flexural_rigidity"].value,
            0.05
        );
        assert!(catalog.get("mit.raw-2013.07c").is_ok());
        let denim = get("mit.raw-2013.13a");
        assert_eq!(denim.authored().properties["areal_density"].value, 8.75);
        assert_ne!(8.75, 0.018 * 0.024 * 16.0 * 36.0 * 36.0);
        let mut stale = linen.frozen_selection();
        stale
            .record
            .as_mut()
            .unwrap()
            .properties
            .get_mut("reported_thickness")
            .unwrap()
            .conditions
            .insert("conditioning".into(), "invented 20 C / 65% RH".into());
        assert!(catalog.resolve(&stale).is_err());
        let mut wrong = linen.authored().clone();
        wrong.properties.get_mut("bulk_density").unwrap().unit = "lb/in2".into();
        assert!(normalize(&wrong).is_err());
        let screen: crate::material_screen::Request = serde_json::from_value(serde_json::json!({
            "schema":"ftx-material-screen/1", "basis":"nominal",
            "candidates":[{"key":"a","selection":{"id":"mit.raw-2013.08a"}},{"key":"b","selection":{"id":"mit.raw-2013.08b"}}],
            "constraints":[{"attribute":{"key":"reported_thickness","unit":"m", "conditions":{"conditioning":"20 C / 65% RH"}},"maximum":0.001}],
            "objectives":[{"attribute":{"key":"areal_density","unit":"kg/m2"},"direction":"minimize"}]
        })).unwrap();
        let report = serde_json::to_value(screen.evaluate(&catalog).unwrap()).unwrap();
        assert!(report["frontier"].as_array().unwrap().is_empty());
        assert_eq!(report["alternatives"].as_array().unwrap().len(), 2);
        assert!(
            report["alternatives"]
                .as_array()
                .unwrap()
                .iter()
                .all(|r| r["eligible"] == false)
        );
    }
}

//! Direct bovine specimen observations from Schneider's 2026 CC BY 4.0 release.
//! Workbook geometry and calculated stress/strain conflict with its landing-page protocol.
//! Only independent instrument table fields are retained; no constitutive conversion follows.

use std::collections::BTreeMap;

use crate::materials::{Evidence, Form, Material, Quantity, Source};

const DATASET: &str = "https://data.mendeley.com/datasets/2pvf4x9crc/1";
const WORKBOOK: &str =
    "https://data.mendeley.com/datasets/2pvf4x9crc/1/files/d1702584-64ca-4593-852a-f557c4349c2c";
const WORKBOOK_SHA256: &str = "56794ef58b20d7da55c2f9155561ee0a38784dbe424b2581266ac034919f387d";

// Bovine D1 / Bovine D2, Results Table 1, rows 4..8. Columns C, G, D, E:
// caliper thickness [mm], maximum force [kgf], force at break [kgf], displacement at break [mm].
// These are source-rounded instrument results, not peaks recomputed from every acquired node.
const ROWS: [(&str, u8, [f64; 4]); 10] = [
    ("D1", 1, [1.90, 19.24, 18.44, 39.87]),
    ("D1", 2, [1.96, 17.68, 16.70, 38.37]),
    ("D1", 3, [1.94, 18.55, 17.15, 38.51]),
    ("D1", 4, [1.97, 19.21, 17.83, 37.47]),
    ("D1", 5, [1.92, 19.39, 17.47, 38.21]),
    ("D2", 1, [1.93, 22.31, 21.55, 42.17]),
    ("D2", 2, [1.99, 19.42, 18.56, 43.64]),
    ("D2", 3, [1.92, 18.52, 17.69, 37.11]),
    ("D2", 4, [1.98, 17.71, 16.80, 35.64]),
    ("D2", 5, [1.91, 17.83, 16.40, 36.44]),
];

pub(super) fn records() -> Vec<Material> {
    ROWS.iter().map(|&(direction, trial, values)| {
        let sheet = format!("Bovine {direction}");
        let row = trial + 3;
        let specimen = format!("Schneider2026:{sheet}:specimen-{trial}");
        let conditions = BTreeMap::from([
            ("specimen".into(), specimen.clone()),
            ("source_sheet".into(), sheet.clone()),
            ("source_direction".into(), direction.into()),
            ("direction_scope".into(), "Two perpendicular source directions; relation to hide backbone, neck/back region or fabric warp/weft not reported".into()),
            ("conditioning".into(), "Temperature, relative humidity, preconditioning duration and specimen moisture content not reported in the release".into()),
            ("test_protocol".into(), "Landing page: Instron 5944, ISO 3376 tensile testing, nominal ASTM D412-C dogbone, crosshead speed 100 mm/min until fracture".into()),
            ("geometry_discrepancy".into(), "Landing page gives 33 mm gauge and 6 mm waist; workbook labels 65 mm gauge and divides displacement by 65. Instrument stress and workbook area formulas are not consistent. No gauge length, stress, local strain or constitutive law is selected from these conflicting values".into()),
            ("uncertainty".into(), "One source-rounded instrument result for this specimen; no uncertainty bounds or statistical independence inferred from the five direction-specific trials".into()),
        ]);
        let quantity = |value, unit: &str, column: &str, method: &str| {
            let mut conditions = conditions.clone();
            conditions.insert("workbook_cell".into(), format!("'{sheet}'!{column}{row}"));
            Quantity {
                value, unit: unit.into(), evidence: Evidence::Measured,
                source: "workbook".into(), method: method.into(), conditions, bounds: None,
            }
        };
        let mut thickness = quantity(values[0], "mm", "C",
            "Results Table 1: Thickness; landing page describes pre-test digital caliper with +/-0.01 mm instrument precision, not a specimen uncertainty bound");
        thickness.conditions.insert("measurement_pressure".into(),
            "Caliper pressure and unloaded reference state not reported; unavailable as contact or sewing clearance".into());
        let mut displacement = quantity(values[3], "mm", "E",
            "Results Table 1: Displacement at Break (Standard); reported instrument travel, not a local extensometer observation or specimen strain");
        displacement.conditions.insert("displacement_scope".into(),
            "Instrument displacement under the crosshead-controlled protocol; no local extensometer, grip-slip or machine-compliance correction established by the release".into());
        Material {
            id: format!("schneider/2026/bovine-{}-{trial}", direction.to_ascii_lowercase()),
            name: format!("Schneider bovine leather {direction}, tensile specimen {trial}"),
            form: Form::Hide,
            composition: Vec::new(), composition_complete: false,
            structure: BTreeMap::from([
                ("specimen".into(), specimen),
                ("animal".into(), "bovine".into()),
                ("source_sheet".into(), sheet.clone()),
                ("source_direction".into(), direction.into()),
                ("identity_scope".into(), "Source tensile trial; supplier, product, lot, hide location, tanning and finish are not identified in the released bovine sheet or landing-page protocol".into()),
                ("mechanical_scope".into(), "Direct specimen force/displacement observations for screening. Conflicting geometry and calculated stress/strain prevent inverse-calibration admission. No membrane, bending, friction, failure law or garment-strength prediction is supplied".into()),
                ("retention".into(), "Forty attributed numerical observations transcribed from ten bovine trials; source workbook and unrelated materials are not redistributed".into()),
            ]),
            properties: BTreeMap::from([
                ("reported_thickness".into(), thickness),
                ("tensile_maximum_force".into(), quantity(values[1], "kgf", "G",
                    "Results Table 1: Maximum Force; original instrument total force, not stress or force per unit width")),
                ("tensile_force_at_break".into(), quantity(values[2], "kgf", "D",
                    "Results Table 1: Force at Break (Standard); original instrument-reported break criterion, distinct from maximum force")),
                ("tensile_displacement_at_break".into(), displacement),
            ]),
            membrane_response: None, membrane_relaxation: None, thermal_response: None,
            optical_faces: None,
            sources: BTreeMap::from([
                ("workbook".into(), Source {
                    reference: WORKBOOK.into(), accessed_on: "2026-10-27".into(),
                    note: format!("Elizabeth Schneider (Virginia Tech), MBL Sustainability and Characterization Testing Data, version 1, published 2026-08-24, DOI 10.17632/2pvf4x9crc.1. Tensile Test Data.xlsx, 4385263 bytes, published and independently verified SHA-256 {WORKBOOK_SHA256}; sheets Bovine D1 and Bovine D2, Results Table 1 rows 4..8, columns C/G/D/E. CC BY 4.0 (https://creativecommons.org/licenses/by/4.0/); numerical transcription and explicitly documented SI normalization only. Original author does not endorse this application."),
                }),
                ("protocol".into(), Source {
                    reference: DATASET.into(), accessed_on: "2026-10-27".into(),
                    note: "Same version's Steps to reproduce. Nominal dogbone dimensions: total length 115 mm, gauge length 33 mm, waist width 6 mm, end width 25 mm; conflicts with workbook gauge and calculated stress are retained, not reconciled by assumption. Source force columns explicitly use kgf; calculated stress columns labelled MPa omit the kgf-to-N factor. No calculated stress, strain or modulus is imported.".into(),
                }),
            ]),
        }
    }).collect()
}

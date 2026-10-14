//! Numerical facts transcribed from the MIT Fabric Properties Dataset's published 30-specimen table.
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
            membrane_response: None, membrane_relaxation: None, thermal_response: None,
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
            .filter(|m| m.id.starts_with("mit."))
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
}

//! Observed wet-denim states from Mangat's 2012 TUL dissertation, Table 4-9.
//! All rows use the Alambeta 1000 Pa setting. They do not define a pressure or moisture law.

use std::collections::BTreeMap;

use crate::materials::{Component, Evidence, Form, Material, Quantity, Source};

const THESIS: &str =
    "https://dspace.tul.cz/bitstreams/57667674-5146-46c9-a46a-6aaeabbbedd0/download";
const MOISTURE_BASIS: &str = "Water mass divided by total wet fabric mass: (Fw - Fd) / Fw; Equation 2-39, pages 47-48. This is not dry-mass moisture regain.";

struct Row {
    weft: &'static str,
    slug: &'static str,
    water_fraction: f64,
    thickness_mm: f64,
    resistance: f64,
    conductivity: f64,
    absorptivity: f64,
    page: u8,
}

// First and last observations of three Table 4-9 series, not interpolated endpoints.
const ROWS: [Row; 6] = [
    Row {
        weft: "Cotton",
        slug: "cotton",
        water_fraction: 0.012,
        thickness_mm: 0.86,
        resistance: 0.0128,
        conductivity: 0.067,
        absorptivity: 197.0,
        page: 91,
    },
    Row {
        weft: "Cotton",
        slug: "cotton",
        water_fraction: 0.447,
        thickness_mm: 0.67,
        resistance: 0.0032,
        conductivity: 0.209,
        absorptivity: 854.0,
        page: 92,
    },
    Row {
        weft: "Spun polypropylene",
        slug: "spun-pp",
        water_fraction: 0.017,
        thickness_mm: 0.91,
        resistance: 0.0149,
        conductivity: 0.0609,
        absorptivity: 146.0,
        page: 90,
    },
    Row {
        weft: "Spun polypropylene",
        slug: "spun-pp",
        water_fraction: 0.557,
        thickness_mm: 0.79,
        resistance: 0.0048,
        conductivity: 0.187,
        absorptivity: 992.0,
        page: 90,
    },
    Row {
        weft: "Polyester (PET)",
        slug: "pet",
        water_fraction: 0.045,
        thickness_mm: 0.78,
        resistance: 0.0108,
        conductivity: 0.072,
        absorptivity: 193.0,
        page: 92,
    },
    Row {
        weft: "Polyester (PET)",
        slug: "pet",
        water_fraction: 0.400,
        thickness_mm: 0.67,
        resistance: 0.0040,
        conductivity: 0.168,
        absorptivity: 751.0,
        page: 93,
    },
];

fn record(row: &Row) -> Material {
    let moisture = format!("{:.3}", row.water_fraction);
    let specimen = format!("Mangat2012:Table4-9:{}:mu-{moisture}", row.slug);
    let conditions = BTreeMap::from([
        ("specimen".into(), specimen.clone()),
        ("conditioning".into(), format!("Wet denim at reported water wet-mass fraction {moisture}; moisture preparation, dwell and measurement sequence not specified for this row")),
        ("pressure_pa".into(), "1000".into()),
        ("water_mass_fraction_wet_basis".into(), moisture.clone()),
        ("moisture_definition".into(), MOISTURE_BASIS.into()),
        ("laboratory_temperature".into(), "20-22; unit not printed in section 3.5, page 71".into()),
        ("laboratory_relative_humidity_percent".into(), "20-25; section 3.5, page 71".into()),
        ("plate_temperatures".into(), "Not reported for Table 4-9; the laboratory range is not a measured plate-temperature domain".into()),
        ("replicates_and_uncertainty".into(), "Reported mean observations; individual coupon identifiers, replicate counts and numerical uncertainty not supplied for Table 4-9".into()),
        ("washing_and_lot".into(), "Specific washing treatment and lot not identified in Table 4-9".into()),
        ("evaporation_scope".into(), "Two-plate measurement limits surface evaporation; page 89. No free-air drying or evaporative heat-transfer law.".into()),
    ]);
    let q = |value, unit: &str, column: &str| Quantity {
        value,
        unit: unit.into(),
        evidence: Evidence::Measured,
        source: "thesis".into(),
        method: format!(
            "Table 4-9, page {}, {column}; Alambeta 1000 Pa setting",
            row.page
        ),
        conditions: conditions.clone(),
        bounds: None,
    };
    let properties = BTreeMap::from([
        (
            "thickness".into(),
            q(row.thickness_mm, "mm", "denim thickness"),
        ),
        (
            "thermal_resistance".into(),
            q(row.resistance, "m2*K/W", "thermal resistance"),
        ),
        // Preserve separately rounded observations without creating a second transport authority.
        (
            "reported_thermal_conductivity".into(),
            q(row.conductivity, "W/(m*K)", "thermal conductivity"),
        ),
        (
            "reported_thermal_absorptivity".into(),
            q(row.absorptivity, "W*s^0.5/(m2*K)", "thermal absorbtivity"),
        ),
        (
            "water_mass_fraction_wet_basis".into(),
            q(
                row.water_fraction,
                "1",
                "moisture in denim, mu; mass-ratio definition in Equation 2-39",
            ),
        ),
    ]);
    Material {
        id: format!("tul/mangat-2012/denim-{}/mu-{moisture}", row.slug),
        name: format!("Mangat2012 cotton-warp/{}-weft denim, water wet-mass fraction {moisture} at 1000 Pa", row.weft),
        form: Form::Woven,
        composition: if row.slug == "cotton" {
            vec![Component { fiber: "cotton".into(), fraction: 1.0, source: "thesis".into() }]
        } else {
            Vec::new()
        },
        composition_complete: row.slug == "cotton",
        structure: BTreeMap::from([
            ("source_sample_mark".into(), specimen),
            ("warp_yarn".into(), "Cotton".into()),
            ("weft_yarn".into(), row.weft.into()),
            ("weave".into(), "3/1 Z twill; subsequent analysis restricted in section 4.5, page 83".into()),
            ("fiber_composition_scope".into(), "Dry textile fibers only; absorbed water is recorded separately. Cotton/synthetic mass fractions are not assigned from yarn counts.".into()),
            ("identity_scope".into(), "Table 4-9 series mean, not an individually identified coupon or purchasable product; washing treatment and lot not reported".into()),
            // The common sewing gate must not use wet, loaded thickness as uncompressed clearance.
            ("compression_model".into(), "fixed_pressure_observation/1".into()),
            ("selected_pressure_pa".into(), "1000".into()),
            ("water_mass_fraction_wet_basis".into(), moisture),
            ("thermal_transport_authority".into(), "Reported thermal resistance; reported conductivity and absorptivity remain observations, not alternative transport or heat-storage laws".into()),
            ("state_scope".into(), "Single observed moisture and pressure state; no pressure interpolation, moisture interpolation, mass invariance, recovery, transient storage, drape or whole-garment validation".into()),
        ]),
        properties,
        membrane_response: None,
        membrane_relaxation: None,
        thermal_response: None,
        optical_faces: None,
        sources: BTreeMap::from([("thesis".into(), Source {
            reference: THESIS.into(),
            accessed_on: "2026-10-19".into(),
            note: "Muhammad Mushtaq Ahmed Mangat (2012), The Effect of Moisture and Finishing on Thermal Comfort and Selected Mechanical Properties of Denims with a Portion of Synthetic Fibres, PhD dissertation, Faculty of Textile Engineering, Technical University of Liberec. Declaration dated 2012-02-12. Table 4-9, pages 90-93; Equation 2-39, pages 47-48; pressure protocol pages 55-57; laboratory conditions page 71; weave restriction page 83; evaporation limitation page 89. Primary university PDF SHA-256 385938cb5baa6dc1044196ac057b5427d09f8c6ea12abe5e97846ce78a9da994. Numerical observations and attribution only; the dissertation and its images are not redistributed.".into(),
        })]),
    }
}

pub(super) fn records() -> Vec<Material> {
    ROWS.iter().map(record).collect()
}

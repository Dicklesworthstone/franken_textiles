//! Fixed-force observations from Rogale et al. (2023), DOI 10.3390/ma16103842.
//! Table 3 reports forces, not pressures: the loaded contact area is unreported.
//! The two states are ordinary materials, not an interpolated compression law.

use std::collections::BTreeMap;

use crate::materials::{Component, Evidence, Form, Material, Quantity, Source};

const ARTICLE: &str = "https://pmc.ncbi.nlm.nih.gov/articles/PMC10224179/";
const CONTACT_AREA: &str = "Not reported; measurement force cannot be converted to pressure";

struct Row {
    mark: &'static str,
    slug: &'static str,
    name: &'static str,
    areal_g_m2: f64,
    initial_mm: f64,
    displacement_mm: f64,
    resistance_m2_k_w: [f64; 2],
    polyester: bool,
}

// Table 1 mass; Table 3 initial thickness, compression displacement and the two Rct values.
const ROWS: [Row; 3] = [
    Row {
        mark: "M1",
        slug: "m1",
        name: "Polyester fleece",
        areal_g_m2: 298.8,
        initial_mm: 2.92,
        displacement_mm: 0.67,
        resistance_m2_k_w: [0.0172, 0.0164],
        polyester: true,
    },
    Row {
        mark: "M2",
        slug: "m2",
        name: "Polyester spacer material",
        areal_g_m2: 315.9,
        initial_mm: 3.28,
        displacement_mm: 0.69,
        resistance_m2_k_w: [0.1045, 0.0853],
        polyester: true,
    },
    Row {
        mark: "M4",
        slug: "m4",
        name: "Double-faced diamond-quilted lining",
        areal_g_m2: 278.3,
        initial_mm: 5.36,
        displacement_mm: 3.11,
        resistance_m2_k_w: [0.2229, 0.1095],
        polyester: false,
    },
];

fn record(row: &Row, compressed: bool) -> Material {
    let force = if compressed { "9.5" } else { "0.95" };
    let q = |value, unit: &str, evidence, method: &str, measured_force: Option<&str>| {
        let mut conditions = BTreeMap::from([
            ("specimen_group".into(), format!("Rogale2023:{}", row.mark)),
            ("conditioning".into(), "Preconditioning and specimen moisture content not reported".into()),
            ("replicates_and_uncertainty".into(), "Individual specimen identifiers, replicate count and numerical uncertainty not reported".into()),
        ]);
        if let Some(measured_force) = measured_force {
            conditions.extend([
                ("measurement_force_n".into(), measured_force.into()),
                ("loaded_contact_area".into(), CONTACT_AREA.into()),
                ("test_temperature".into(), "Numerical plate temperatures not reported for Table 3".into()),
                ("loading_history".into(), "0.95 N initial reference followed by 9.5 N loading; loading rate, dwell time and recovery not reported".into()),
            ]);
        }
        Quantity {
            value,
            unit: unit.into(),
            evidence,
            source: "paper".into(),
            method: method.into(),
            conditions,
            bounds: None,
        }
    };
    let mut mass = q(
        row.areal_g_m2,
        "g/m2",
        Evidence::Published,
        "Table 1 mass per unit area; test method not specified",
        None,
    );
    mass.conditions.insert(
        "state_scope".into(),
        "Source material-group mass, not a separate measurement at either applied force".into(),
    );
    let initial = q(
        row.initial_mm,
        "mm",
        Evidence::Measured,
        "Table 3 initial thickness x2 at 0.95 N; EN ISO 5084:1996 cited in section 3",
        Some("0.95"),
    );
    let mut thickness = initial.clone();
    if compressed {
        thickness = q(
            row.initial_mm - row.displacement_mm,
            "mm",
            Evidence::Estimate,
            "Derived loaded thickness = Table 3 initial x2 minus compression displacement x1; Figure 7 and Equation (8); not an independently tabulated thickness",
            Some("9.5"),
        );
        thickness.conditions.extend([
            (
                "derivation".into(),
                format!("{} mm - {} mm", row.initial_mm, row.displacement_mm),
            ),
            ("reference_force_n".into(), "0.95".into()),
            (
                "rounding".into(),
                "Difference of rounded source values; uncertainty not propagated".into(),
            ),
        ]);
    }
    let mut resistance = q(
        row.resistance_m2_k_w[usize::from(compressed)],
        "m2*K/W",
        Evidence::Measured,
        "Table 3 Rct from multi-purpose differential conductometer at steady state",
        Some(force),
    );
    resistance.conditions.insert("applicability".into(),
        "Effective thermal contact resistance under the stated apparatus force; no temperature-dependent law or characterized temperature interval".into());
    let mut properties = BTreeMap::from([
        ("areal_density".into(), mass),
        ("thickness".into(), thickness),
        ("thermal_resistance".into(), resistance),
    ]);
    if compressed {
        // Original observations remain in their own force contexts and reported units.
        properties.insert("reference_thickness_0p95n".into(), initial);
        properties.insert("compression_displacement".into(), q(row.displacement_mm, "mm", Evidence::Measured,
            "Table 3 compression displacement x1 relative to the 0.95 N reference; Figure 7 and Equation (8)", Some("9.5")));
    }
    let mut structure = BTreeMap::from([
        ("source_sample_mark".into(), row.mark.into()),
        ("construction".into(), row.name.into()),
        ("selected_force_n".into(), force.into()),
        ("loaded_contact_area".into(), CONTACT_AREA.into()),
        ("state".into(), if compressed { "Compressed at 9.5 N" } else { "Initial reference at 0.95 N; not unloaded loft" }.into()),
        ("identity_scope".into(), "Study specimen group; supplier, product, lot and individual coupon identifiers not reported".into()),
        ("form_scope".into(), "Effective sheet or quilted assembly; weave/knit/bonding construction not established by the source text".into()),
        ("thermal_model_scope".into(), "Two separately selected fixed-force observations; no force/pressure interpolation, thermal storage, garment comfort or drape validation".into()),
    ]);
    if compressed {
        // The common sewing gate must not consume loaded thickness as uncompressed clearance.
        // This identifies the observation state without asserting a pressure or pressure law.
        structure.insert(
            "compression_model".into(),
            "fixed_force_observation/1".into(),
        );
    }
    if !row.polyester {
        structure.insert("layer_composition_and_mass".into(),
            "Table 1: two 100% polyester covers, 68.3 g/m2 each; 100% polypropylene lining, 12.4 g/m2; 100% polyester padding, 129.3 g/m2. Whole-assembly fiber mass fractions not explicitly reported.".into());
    }
    Material {
        id: format!("zagreb/rogale-2023/{}/force-{force}n", row.slug),
        name: format!("Rogale2023 {}: {} at {force} N", row.mark, row.name),
        form: Form::Sheet,
        composition: if row.polyester {
            vec![Component { fiber: "polyester".into(), fraction: 1.0, source: "paper".into() }]
        } else {
            Vec::new()
        },
        composition_complete: row.polyester,
        structure,
        properties,
        membrane_response: None,
        membrane_relaxation: None,
        thermal_response: None,
        sources: BTreeMap::from([("paper".into(), Source {
            reference: ARTICLE.into(),
            accessed_on: "2026-10-18".into(),
            note: "Rogale, First Rogale, Knezic, Jukl and Majstorovic (2023), Measurement Methods of the Thermal Resistance of Materials Used in Clothing, Materials 16(10):3842, DOI 10.3390/ma16103842. Tables 1 and 3; section 3, Figure 7 and Equation (8) define the initial reference and compression displacement. CC BY 4.0, copyright 2023 the authors. Numerical facts and attributed derived thickness only; no source article or images redistributed. Reviewed PMC HTML SHA-256 f0534fa9e4ff434cd2b21e731f021880552133a0d70e4dc16ac0f3604ba04cf0. Loaded contact area and Table 3 plate temperatures are not reported.".into(),
        })]),
    }
}

pub(super) fn records() -> Vec<Material> {
    ROWS.iter()
        .flat_map(|row| [record(row, false), record(row, true)])
        .collect()
}

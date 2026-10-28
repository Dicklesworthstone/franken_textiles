//! Source observations from Lupescu et al. (2025), DOI 10.3390/textiles5010005, Tables 2–6.
//! Baseline transport, gauge thickness, compression and humid-state tests retain separate scopes.
//! The tabulated resistance is already source-derived; no new thickness or mixture law is fitted.

use std::collections::BTreeMap;

use crate::materials::{Component, Evidence, Form, Material, Quantity, Source};

const ARTICLE: &str = "https://doi.org/10.3390/textiles5010005";
const PDF_SHA256: &str = "21d5a283bb6ccc65b737d38a9e1f03ad8efcc8c471270bb45e1143a9893a3644";
const BASELINE: &str = "Before the separate 90% RH saturation experiment; baseline humidity, preconditioning duration and moisture content not numerically reported";

struct Row {
    slug: &'static str,
    mark: &'static str,
    product: &'static str,
    // Table 2 feed recipe, in mass percent: milkweed, LM 2.2 bicomponent fiber, PLA.
    recipe: Option<[u8; 3]>,
    mass: [f64; 2],
    thickness: [f64; 2],
    density: f64,
    conductivity: [f64; 2],
    resistance: f64,
    specific_resistance: f64,
    clo: f64,
    compression: [f64; 2],
    recovery: [f64; 2],
    moisture_regain: [f64; 2],
    porosity: [f64; 2],
    humid_conductivity: [f64; 2],
}

// Paired entries are the source central value and reported spread. Tables 3/4 and conductivity
// columns in Table 6 explicitly identify SD; Table 5 and other Table 6 columns print only ±.
const ROWS: [Row; 9] = [
    Row {
        slug: "sample-1",
        mark: "1",
        product: "air-laid web",
        recipe: Some([30, 10, 60]),
        mass: [55.34, 1.47],
        thickness: [5.05, 0.16],
        density: 10.96,
        conductivity: [35.32, 0.04],
        resistance: 0.143,
        specific_resistance: 0.013,
        clo: 0.922,
        compression: [54.6, 2.1],
        recovery: [84.6, 4.2],
        moisture_regain: [8.41, 0.51],
        porosity: [98.95, 0.10],
        humid_conductivity: [35.69, 0.06],
    },
    Row {
        slug: "sample-2",
        mark: "2",
        product: "air-laid web",
        recipe: Some([70, 10, 20]),
        mass: [60.75, 1.61],
        thickness: [4.80, 0.35],
        density: 12.66,
        conductivity: [33.13, 0.03],
        resistance: 0.145,
        specific_resistance: 0.011,
        clo: 0.935,
        compression: [47.5, 2.6],
        recovery: [93.1, 2.1],
        moisture_regain: [33.35, 0.18],
        porosity: [98.16, 0.12],
        humid_conductivity: [33.38, 0.04],
    },
    Row {
        slug: "sample-3",
        mark: "3",
        product: "air-laid web",
        recipe: Some([50, 20, 30]),
        mass: [60.51, 1.56],
        thickness: [5.12, 0.24],
        density: 11.82,
        conductivity: [33.96, 0.09],
        resistance: 0.151,
        specific_resistance: 0.013,
        clo: 0.972,
        compression: [52.6, 2.9],
        recovery: [84.0, 3.4],
        moisture_regain: [19.80, 0.47],
        porosity: [98.63, 0.18],
        humid_conductivity: [34.02, 0.03],
    },
    Row {
        slug: "sample-4",
        mark: "4",
        product: "thick air-laid nonwoven",
        recipe: Some([50, 10, 40]),
        mass: [161.45, 1.54],
        thickness: [8.40, 0.46],
        density: 19.22,
        conductivity: [32.81, 0.02],
        resistance: 0.256,
        specific_resistance: 0.014,
        clo: 1.651,
        compression: [49.0, 4.1],
        recovery: [88.4, 3.6],
        moisture_regain: [18.94, 1.59],
        porosity: [97.76, 0.21],
        humid_conductivity: [32.83, 0.05],
    },
    Row {
        slug: "sample-5",
        mark: "5",
        product: "thick air-laid nonwoven",
        recipe: Some([50, 20, 30]),
        mass: [113.20, 1.98],
        thickness: [6.65, 0.24],
        density: 17.02,
        conductivity: [33.44, 0.01],
        resistance: 0.199,
        specific_resistance: 0.012,
        clo: 1.283,
        compression: [48.4, 3.7],
        recovery: [84.6, 3.1],
        moisture_regain: [21.21, 2.33],
        porosity: [98.05, 0.11],
        humid_conductivity: [33.57, 0.03],
    },
    Row {
        slug: "sample-6",
        mark: "6",
        product: "thick air-laid nonwoven",
        recipe: Some([50, 40, 10]),
        mass: [170.15, 2.22],
        thickness: [10.45, 0.60],
        density: 16.28,
        conductivity: [33.65, 0.02],
        resistance: 0.311,
        specific_resistance: 0.019,
        clo: 2.003,
        compression: [58.7, 3.6],
        recovery: [87.3, 2.8],
        moisture_regain: [23.43, 0.36],
        porosity: [98.19, 0.10],
        humid_conductivity: [33.69, 0.04],
    },
    Row {
        slug: "sample-7",
        mark: "7",
        product: "air-laid web",
        recipe: Some([50, 20, 30]),
        mass: [56.90, 1.07],
        thickness: [4.55, 0.60],
        density: 12.51,
        conductivity: [32.55, 0.07],
        resistance: 0.140,
        specific_resistance: 0.011,
        clo: 0.902,
        compression: [48.8, 4.1],
        recovery: [90.6, 2.5],
        moisture_regain: [20.29, 0.36],
        porosity: [98.59, 0.12],
        humid_conductivity: [32.59, 0.05],
    },
    Row {
        slug: "thinsulate",
        mark: "Thinsulate",
        product: "commercial polyester nonwoven",
        recipe: None,
        mass: [166.84, 0.60],
        thickness: [5.02, 0.39],
        density: 33.24,
        conductivity: [33.31, 0.09],
        resistance: 0.151,
        specific_resistance: 0.005,
        clo: 0.972,
        compression: [59.5, 2.1],
        recovery: [95.1, 2.4],
        moisture_regain: [0.99, 0.10],
        porosity: [97.58, 0.13],
        humid_conductivity: [33.67, 0.07],
    },
    Row {
        slug: "primaloft",
        mark: "Primaloft",
        product: "commercial polyester nonwoven",
        recipe: None,
        mass: [86.86, 0.30],
        thickness: [5.08, 0.12],
        density: 17.10,
        conductivity: [34.14, 0.02],
        resistance: 0.149,
        specific_resistance: 0.009,
        clo: 0.960,
        compression: [79.9, 1.9],
        recovery: [99.8, 1.8],
        moisture_regain: [3.32, 0.20],
        porosity: [98.84, 0.10],
        humid_conductivity: [34.17, 0.04],
    },
];

fn spread(quantity: &mut Quantity, value: f64, is_sd: bool) {
    quantity.conditions.insert(
        if is_sd {
            "reported_standard_deviation"
        } else {
            "reported_plus_minus"
        }
        .into(),
        format!("{value} {}", quantity.unit),
    );
    quantity.conditions.insert("variability_scope".into(), if is_sd {
        "Source-labelled SD in original units, not uncertainty of the mean, hard bounds or a propagated prediction interval"
    } else {
        "Source prints a plus/minus value without identifying its statistic; retained in original units, not interpreted as SD, confidence interval or hard bounds"
    }.into());
}

fn record(row: &Row) -> Material {
    let specimen = format!("Lupescu2025:{}", row.mark);
    let q = |value, unit: &str, evidence, table: &str, method: &str| Quantity {
        value,
        unit: unit.into(),
        evidence,
        source: "paper".into(),
        method: method.into(),
        bounds: None,
        conditions: BTreeMap::from([
            ("specimen".into(), specimen.clone()),
            ("source_table".into(), table.into()),
            ("source_row".into(), row.mark.into()),
        ]),
    };
    let mut mass = q(
        row.mass[0],
        "g/m2",
        Evidence::Measured,
        "3",
        "ISO 9073-1; Table 3 mean mass per unit area; section 2.4.1",
    );
    spread(&mut mass, row.mass[1], true);
    mass.conditions.extend([
        (
            "sample_count".into(),
            "at least 10 specimens per condition".into(),
        ),
        ("conditioning".into(), BASELINE.into()),
        (
            "scope".into(),
            "Reference material-group mass, not a post-compression or RH90 mass measurement".into(),
        ),
    ]);
    let mut thickness = q(
        row.thickness[0],
        "mm",
        Evidence::Measured,
        "3",
        "ISO 9073-2; Table 3 mean thickness; section 2.4.1",
    );
    spread(&mut thickness, row.thickness[1], true);
    thickness.conditions.extend([
        ("pressure_kpa".into(), "0.1".into()),
        ("measurement_count".into(), "at least 10 measurements per specimen".into()),
        ("conditioning".into(), BASELINE.into()),
        ("scope".into(), "Gauge thickness at 100 Pa; not unloaded loft, hotplate contact pressure or a compressed/wet thickness law".into()),
    ]);
    let density = q(
        row.density,
        "kg/m3",
        Evidence::Published,
        "3",
        "Table 3 source-calculated bulk density, Equation (2): mass per area divided by gauge thickness; not constituent-fiber density",
    );
    let mut conductivity = q(
        row.conductivity[0],
        "mW/(m*K)",
        Evidence::Measured,
        "4",
        "ASTM C518; FOX 314 two-plate heat-flow meter; section 2.4.2; Table 4 mean, repeated before-saturation value in Table 6",
    );
    spread(&mut conductivity, row.conductivity[1], true);
    conductivity.conditions.extend([
        ("direction".into(), "through thickness".into()),
        ("warm_plate_temperature_c".into(), "37".into()),
        ("cold_plate_temperature_c".into(), "21".into()),
        ("measurement_count".into(), "at least 5 measurements per value".into()),
        ("conditioning".into(), BASELINE.into()),
        ("contact_pressure".into(), "Not reported for heat-flow meter; gauge pressure does not establish thermal contact pressure".into()),
        ("temperature_scope".into(), "One 37/21 C plate protocol, not a measured temperature-response domain".into()),
    ]);
    let mut resistance = q(
        row.resistance,
        "m2*K/W",
        Evidence::Published,
        "4",
        "Directly tabulated Table 4 thermal resistance; source Equation (4) derives R from thickness/conductivity; source rounding retained, not an independent resistance acquisition",
    );
    resistance.conditions.extend(
        conductivity
            .conditions
            .iter()
            .filter(|(key, _)| {
                !matches!(
                    key.as_str(),
                    "reported_standard_deviation" | "variability_scope"
                )
            })
            .map(|(k, v)| (k.clone(), v.clone())),
    );
    let specific = q(
        row.specific_resistance,
        "(m2*K)/(W*kg/m3)",
        Evidence::Published,
        "4",
        "Table 4 source-reported specific thermal resistance; retain the source's resistance-per-bulk-density definition and rounding",
    );
    let clo = q(
        row.clo,
        "clo",
        Evidence::Published,
        "4",
        "Table 4 source-reported insulation, derived in Equation (3); separately rounded from R, not a garment comfort rating",
    );
    let mut compression = q(
        row.compression[0],
        "%",
        Evidence::Measured,
        "5",
        "ASTM D6571-22; Table 5 C; Equation (5): (A-B)/A*100 after 10 min under 7.26 kg",
    );
    spread(&mut compression, row.compression[1], false);
    compression.conditions.extend([
        ("measurement_count".into(), "at least 8 measurements per value".into()),
        ("initial_state".into(), "Initial A measured without weight; not Table 3 thickness at 100 Pa".into()),
        ("applied_mass_kg".into(), "7.26".into()),
        ("loading_duration_min".into(), "10".into()),
        ("loaded_area".into(), "Not numerically reported; no pressure or local compression law inferred".into()),
        ("conditioning".into(), "Compression-test temperature, humidity and preconditioning duration not numerically reported".into()),
    ]);
    let mut recovery = q(
        row.recovery[0],
        "%",
        Evidence::Measured,
        "5",
        "ASTM D6571-22; Table 5 N; Equation (6): J/E*100, recovered height after second load divided by recovered height after first load; not recovered/initial height",
    );
    spread(&mut recovery, row.recovery[1], false);
    recovery.conditions.extend([
        ("measurement_count".into(), "at least 8 measurements per value".into()),
        ("loading_history".into(), "7.26 kg for 10 min; unload/recover 10 min to E; second compression for 24 h; unload/recover 1 h to J".into()),
        ("loaded_area".into(), "Not numerically reported; no pressure or local recovery-time law inferred".into()),
    ]);
    let mut moisture = q(
        row.moisture_regain[0],
        "%",
        Evidence::Measured,
        "6",
        "ASTM D2654-22 and D1776/D1776M-20; Equation (9): (Mwet-Mdry)/Mdry*100; Table 6",
    );
    spread(&mut moisture, row.moisture_regain[1], false);
    moisture.conditions.extend([
        ("conditioning_temperature_c".into(), "21".into()),
        ("relative_humidity_percent".into(), "90".into()),
        ("equilibration".into(), "Until mass variation between successive weighings was less than 0.01%".into()),
        ("mass_basis".into(), "Water gain divided by dry specimen mass; not wet-mass fraction or available free liquid".into()),
    ]);
    let mut porosity = q(
        row.porosity[0],
        "%",
        Evidence::Published,
        "6",
        "Table 6 source-calculated porosity, section 2.4.4 Equations (7)-(8); fiber density and source geometry model, not an independent pore-volume acquisition",
    );
    spread(&mut porosity, row.porosity[1], false);
    let mut humid = q(
        row.humid_conductivity[0],
        "mW/(m*K)",
        Evidence::Measured,
        "6",
        "Table 6 measured conductivity after 90% RH saturation; section 2.4.2 heat-flow meter protocol; separate observation, not the baseline material law",
    );
    spread(&mut humid, row.humid_conductivity[1], true);
    humid.conditions.extend([
        ("conditioning_temperature_c".into(), "21".into()),
        ("relative_humidity_percent".into(), "90".into()),
        ("warm_plate_temperature_c".into(), "37".into()),
        ("cold_plate_temperature_c".into(), "21".into()),
        ("equilibration".into(), "Until mass variation between successive weighings was less than 0.01%".into()),
        ("scope".into(), "No post-saturation mass per area, thickness, resistance or heat capacity reported; do not pair with baseline geometry to construct a wet law".into()),
    ]);
    let mut structure = BTreeMap::from([
        ("source_sample_mark".into(), row.mark.into()),
        ("construction".into(), row.product.into()),
        ("identity_scope".into(), "Exact study specimen group; lot, individual coupon identities and commercial grade are not supplied".into()),
        ("source_pdf_sha256".into(), PDF_SHA256.into()),
        ("source_pdf_locator".into(), "https://pdfs.semanticscholar.org/d257/e9122f20f0ab3afbb374b161347b4dec5206.pdf".into()),
        ("source_tables".into(), "Tables 2/3/4/5/6, PDF pages 5/7/8/10/11; visually verified 2026-10-27".into()),
        ("scope".into(), "Baseline mass and steady resistance support existing consumers. Humid conductivity, compression and recovery are separate observations, not parameterized moisture/pressure/history laws. No heat capacity, membrane, bending, friction, garment warmth rating or machine qualification supplied".into()),
        ("rights".into(), "Lupescu, Robert, Sanchez-Diaz and Elkoun (2025), CC BY 4.0. Attributed numerical transcription; no article pages or images redistributed".into()),
    ]);
    if let Some([mw, lm, pla]) = row.recipe {
        structure.extend([
            ("nominal_feed_recipe_mass_percent".into(), format!("milkweed {mw}; LM 2.2 {lm}; polylactic acid {pla}")),
            ("bicomponent_fiber".into(), "LM 2.2: PET core and polyolefin sheath, source-reported 50:50 ratio; polyolefin species not identified".into()),
            ("processing".into(), "SPIKE air-laid at CENT, Tourcoing; thermal bonding at 150 C; recipe is process metadata, not machine authorization".into()),
            ("composition_scope".into(), "Table 2 input recipe; section 3.5 reports preferential milkweed loss. Final sheet composition was not assayed, so feed fractions are not installed as final fiber mass fractions".into()),
        ]);
    } else {
        structure.insert("commercial_identity_scope".into(),
            "Study's Thinsulate/Primaloft comparison sample, reported 100% PET; grade, supplier SKU, lot and current availability unreported. Never join to another same-brand catalog record".into());
    }
    Material {
        id: format!("lupescu/2025/{}", row.slug),
        name: match row.recipe {
            Some([mw, lm, pla]) => format!(
                "Lupescu2025 {}: milkweed {mw}% / LM 2.2 {lm}% / PLA {pla}% feed, {}",
                row.mark, row.product
            ),
            None => format!("Lupescu2025 {}: {}", row.mark, row.product),
        },
        form: Form::Nonwoven,
        composition: if row.recipe.is_none() {
            vec![Component { fiber: "polyester".into(), fraction: 1.0, source: "paper".into() }]
        } else { Vec::new() },
        composition_complete: row.recipe.is_none(),
        structure,
        properties: BTreeMap::from([
            ("areal_density".into(), mass), ("reported_thickness".into(), thickness),
            ("bulk_density".into(), density), ("thermal_conductivity".into(), conductivity),
            ("thermal_resistance".into(), resistance), ("reported_specific_thermal_resistance".into(), specific),
            ("reported_thermal_insulation".into(), clo), ("compression_ratio".into(), compression),
            ("short_term_compression_recovery".into(), recovery), ("moisture_regain".into(), moisture),
            ("porosity".into(), porosity), ("thermal_conductivity_rh90".into(), humid),
        ]),
        membrane_response: None, membrane_relaxation: None, thermal_response: None, optical_faces: None,
        sources: BTreeMap::from([("paper".into(), Source {
            reference: ARTICLE.into(), accessed_on: "2026-10-27".into(),
            note: "Lupescu, Robert, Sanchez-Diaz and Elkoun (2025), Development of Thermally Insulating Nonwovens from Milkweed Fibers Using an Air-Laid Spike Process, Textiles 5(1):5. Tables 2-6 and sections 2.1, 2.2, 2.4, 3.5; published 2025-01-22, CC BY 4.0. Version-of-record PDF mirrored by Semantic Scholar, SHA-256 retained in structure; source table facsimiles checked. Numerical observations and source-derived values only, not new empirical validation.".into(),
        })]),
    }
}

pub(super) fn records() -> Vec<Material> {
    ROWS.iter().map(record).collect()
}

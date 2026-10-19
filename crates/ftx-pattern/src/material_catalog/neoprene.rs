//! Matched specimen-group observations from Potočić Matković et al. (2025).
//! Tables 1 and 2, DOI 10.3390/polym17212820. Pressure entries are displacement,
//! not loaded thickness; the article's extrapolated loaded resistance is not imported.

use std::collections::BTreeMap;

use crate::materials::{Evidence, Form, Material, Quantity, Source};

const ARTICLE: &str = "https://www.mdpi.com/2073-4360/17/21/2820";
const DISPLACEMENT_REFERENCE: &str = "Instron preload and displacement zero not specified; the separate Table 1 thickness measurement uses 1 kPa. Figure 1 relates initial thickness and displacement, but cross-instrument reference compatibility is not established.";

// Source mark, thickness at 1 kPa (mm), mass (g/m2), dry Rct (m2*K/W),
// displacement (mm) at the four Table 2 pressures: 50, 100, 150, 200 kPa.
struct Row(&'static str, f64, f64, f64, [f64; 4]);

const ROWS: [Row; 33] = [
    Row(
        "M1OSa3",
        3.5900,
        826.61,
        0.0575,
        [0.9480, 1.4424, 1.7454, 2.2409],
    ),
    Row(
        "M1OSa5",
        5.7567,
        1380.69,
        0.0955,
        [1.8563, 2.6931, 3.1880, 3.5361],
    ),
    Row(
        "M1OSa7",
        7.6233,
        1668.22,
        0.1365,
        [2.6134, 3.8141, 4.5001, 4.8921],
    ),
    Row(
        "M1OSb3",
        3.7400,
        945.70,
        0.0665,
        [1.2812, 1.8560, 2.1887, 2.5285],
    ),
    Row(
        "M1OSb5",
        5.5033,
        1277.17,
        0.1005,
        [1.8770, 2.7348, 3.2186, 3.5175],
    ),
    Row(
        "M1OSb7",
        7.3967,
        1835.69,
        0.1315,
        [2.5327, 3.7126, 4.3689, 4.7489],
    ),
    Row(
        "M1NO3",
        3.7667,
        1104.54,
        0.0765,
        [1.0289, 1.5539, 1.8567, 1.9871],
    ),
    Row(
        "M1NO5",
        5.1467,
        1283.13,
        0.0915,
        [1.3917, 2.1887, 2.6430, 2.8679],
    ),
    Row(
        "M1NO7",
        6.9400,
        1755.99,
        0.1205,
        [1.8575, 2.9068, 3.5125, 4.0899],
    ),
    Row(
        "M2BS3",
        3.8067,
        968.12,
        0.0635,
        [1.3424, 1.8770, 2.1803, 2.3915],
    ),
    Row(
        "M2BS5",
        6.9833,
        1277.00,
        0.1175,
        [2.2996, 3.2784, 3.8224, 4.0158],
    ),
    Row(
        "M2BS7",
        8.0000,
        1966.54,
        0.1545,
        [1.8867, 3.0666, 3.7524, 4.5651],
    ),
    Row(
        "M2NO3",
        3.7633,
        1148.23,
        0.0715,
        [1.4429, 2.0482, 2.3912, 2.6147],
    ),
    Row(
        "M2NO5",
        5.1000,
        1458.62,
        0.1035,
        [1.9373, 2.7539, 3.2173, 3.4899],
    ),
    Row(
        "M2NO7",
        7.2167,
        1808.25,
        0.1265,
        [2.2498, 3.4194, 4.0652, 4.3669],
    ),
    Row(
        "M2OS3",
        3.6733,
        895.70,
        0.0695,
        [1.1504, 1.7260, 2.0488, 2.4266],
    ),
    Row(
        "M2OS5",
        5.2367,
        1277.00,
        0.0825,
        [1.8154, 2.6728, 3.1571, 3.3950],
    ),
    Row(
        "M2OS7",
        8.2833,
        1978.56,
        0.0985,
        [2.3291, 3.6905, 4.4471, 5.0344],
    ),
    Row(
        "M3NOa3",
        3.2933,
        874.61,
        0.0535,
        [1.3931, 1.9274, 2.2296, 2.4007],
    ),
    Row(
        "M3NOa5",
        4.6567,
        1382.11,
        0.0815,
        [1.7240, 2.4711, 2.8945, 3.0578],
    ),
    Row(
        "M3NOa7",
        6.7433,
        1820.76,
        0.1195,
        [2.3106, 3.4406, 4.0765, 4.4477],
    ),
    Row(
        "M3NOb3",
        3.4500,
        988.29,
        0.1340,
        [0.9581, 1.4729, 1.7758, 2.1753],
    ),
    Row(
        "M3NOb5",
        5.5167,
        1288.65,
        0.1720,
        [2.1198, 3.0483, 3.5628, 3.7701],
    ),
    Row(
        "M3NOb7",
        7.0667,
        1659.18,
        0.2010,
        [2.5008, 3.6812, 4.3166, 4.6160],
    ),
    Row(
        "M3NOc3",
        3.1267,
        858.12,
        0.0355,
        [1.2918, 1.7765, 2.0594, 2.2801],
    ),
    Row(
        "M3NOc5",
        5.2467,
        1323.01,
        0.0985,
        [1.9673, 2.8044, 3.2690, 3.6280],
    ),
    Row(
        "M3NOc7",
        7.6267,
        1933.70,
        0.1395,
        [2.5720, 3.8026, 4.4792, 4.9447],
    ),
    Row(
        "M4BS3",
        3.6967,
        759.59,
        0.1305,
        [1.4129, 1.9376, 2.2498, 2.3798],
    ),
    Row(
        "M4BS5",
        5.7167,
        1202.27,
        0.1590,
        [1.9768, 2.8548, 3.3490, 3.6564],
    ),
    Row(
        "M4BS7",
        7.7367,
        1538.36,
        0.2055,
        [2.9671, 4.2378, 4.9239, 5.2902],
    ),
    Row(
        "M4OS3",
        3.3167,
        687.29,
        0.1240,
        [1.2209, 1.7256, 2.0181, 2.2109],
    ),
    Row(
        "M4OS5",
        5.5733,
        1093.46,
        0.1770,
        [1.8857, 2.7433, 3.2482, 3.6741],
    ),
    Row(
        "M4OS7",
        7.1967,
        1404.61,
        0.1980,
        [2.7534, 3.9029, 4.5375, 4.7555],
    ),
];

fn record(row: &Row) -> Material {
    let Row(mark, thickness_mm, areal_g_m2, resistance, displacement_mm) = row;
    let quantity = |value, unit: &str, method: &str, observations: &[(&str, &str)]| {
        let mut conditions = BTreeMap::from([
            (
                "specimen_group".into(),
                format!("PotocicMatkovic2025:{mark}"),
            ),
            (
                "conditioning".into(),
                "Preconditioning duration and specimen moisture content not reported".into(),
            ),
            (
                "uncertainty".into(),
                "Numerical uncertainty and individual coupon identifiers not reported".into(),
            ),
        ]);
        conditions.extend(
            observations
                .iter()
                .map(|(key, value)| ((*key).into(), (*value).into())),
        );
        Quantity {
            value,
            unit: unit.into(),
            evidence: Evidence::Measured,
            source: "paper".into(),
            method: method.into(),
            conditions,
            bounds: None,
        }
    };
    let thickness = quantity(
        *thickness_mm,
        "mm",
        "Table 1 thickness at 1 kPa; ISO 5084; DM-2000 thickness gauge",
        &[
            ("pressure_pa", "1000"),
            ("sample_area_cm2", "20"),
            ("measurement_count", "10"),
            (
                "averaging",
                "Mean of measurements at ten locations on the specimen",
            ),
            (
                "ambient_conditions",
                "Thickness-test temperature and humidity not reported",
            ),
            (
                "state_scope",
                "Protocol thickness under 1 kPa; not unloaded loft or the nominal thickness label",
            ),
        ],
    );
    let mass = quantity(
        *areal_g_m2,
        "g/m2",
        "Table 1 mass per unit area; ISO 3801; Kern ALJ 220-4 analytical scale",
        &[
            ("measurement_count", "Not reported"),
            (
                "ambient_conditions",
                "Weighing temperature and humidity not reported",
            ),
            (
                "state_scope",
                "Reference specimen-group areal mass; no pressure-invariance characterization",
            ),
        ],
    );
    let thermal = quantity(
        *resistance,
        "m2*K/W",
        "Table 1 Rct; Thermetrics sweating-guarded hotplate; Equation (1) subtracts the bare-plate dry resistance",
        &[
            ("ambient_temperature_c", "20"),
            ("relative_humidity_percent", "65 +/- 1"),
            ("air_speed_m_s", "1"),
            ("plate_temperature_c", "Not reported"),
            ("measurement_pressure", "Not reported for the hotplate test"),
            ("measurement_count", "Not reported for thermal resistance"),
            (
                "state_scope",
                "Dry resistance of sample only; separately measured reference protocol, not resistance at any Table 2 compression pressure",
            ),
            (
                "temperature_model_scope",
                "Single scalar observation; no measured temperature interval or temperature-dependent law",
            ),
        ],
    );
    let mut properties = BTreeMap::from([
        ("thickness".into(), thickness),
        ("areal_density".into(), mass),
        ("thermal_resistance".into(), thermal),
    ]);
    for ((pressure_kpa, pressure_pa), displacement) in [
        (50, "50000"),
        (100, "100000"),
        (150, "150000"),
        (200, "200000"),
    ]
    .into_iter()
    .zip(displacement_mm)
    {
        properties.insert(
            format!("compression_displacement_{pressure_kpa}kpa"),
            quantity(
                *displacement,
                "mm",
                "Table 2 compressive displacement; Instron 5567; mean of five measurements",
                &[
                    ("pressure_pa", pressure_pa),
                    ("measurement_count", "5"),
                    ("pressure_scope", "Applied compressive stress; not a direct submerged thermal-resistance measurement"),
                    ("displacement_reference", DISPLACEMENT_REFERENCE),
                    ("loading_history", "Loading rate, dwell, unloading and recovery not reported"),
                    ("ambient_conditions", "Compression-test temperature and humidity not reported"),
                    ("state_scope", "Measured displacement, not final thickness; no pressure-conditioned material law inferred"),
                ],
            ),
        );
    }
    let (form, lining) = if mark.contains("NO") {
        (Form::Sheet, "No lining")
    } else if mark.contains("OS") {
        (Form::Laminate, "One-sided laminated lining")
    } else {
        (Form::Laminate, "Both-sided laminated linings")
    };
    Material {
        id: format!("zagreb/neoprene-2025/{}", mark.to_ascii_lowercase()),
        name: format!("PotocicMatkovic2025 {mark}: {lining} neoprene"),
        form,
        composition: Vec::new(),
        composition_complete: false,
        structure: BTreeMap::from([
            ("source_sample_mark".into(), (*mark).into()),
            ("construction".into(), "Closed-cell neoprene foam sheet".into()),
            ("lining".into(), lining.into()),
            ("composition_scope".into(), "Neoprene identity is reported; whole-assembly polymer, lining, adhesive and additive mass fractions are not reported".into()),
            ("identity_scope".into(), "Study specimen group from one of four anonymized manufacturers M1-M4; supplier, product, lot and individual coupon identifiers not reported".into()),
            ("sample_code_scope".into(), "NO: no lining; OS: one-sided lining; BS: both-sided linings; final 3, 5 or 7 denotes nominal millimetres. Letter variants remain distinct source groups.".into()),
            ("loading_data_scope".into(), "Four measured pressure-specific displacements retained as observations; no loaded thickness, interpolation, unloading, recovery or pressure-invariant Rct inferred".into()),
            ("thermal_model_scope".into(), "Measured reference dry Rct only. Figures 2 and 4 estimate loaded resistance from cross-sample regression; those estimates are not imported.".into()),
        ]),
        properties,
        membrane_response: None,
        membrane_relaxation: None,
        thermal_response: None,
        optical_faces: None,
        sources: BTreeMap::from([("paper".into(), Source {
            reference: ARTICLE.into(),
            accessed_on: "2026-10-19".into(),
            note: "Vesna Marija Potočić Matković, Ivana Salopek Čubrić and Alenka Pavko Čuden (2025), Dependence of Thermal Comfort of Diving Suit on Neoprene Properties and Diving Depth, Polymers 17(21):2820, DOI 10.3390/polym17212820. Tables 1 and 2 with section 2 protocols; 33 study sample groups. CC BY 4.0, copyright 2025 the authors. Attributed numerical facts only; no article text or images redistributed. Table 1 is the numerical authority: M3NOa3 Rct is 0.0535, whereas the narrative gives 0.052. Thermal and thickness/compression test conditions are separately retained; no measured loaded resistance is asserted.".into(),
        })]),
    }
}

pub(super) fn records() -> Vec<Material> {
    ROWS.iter().map(record).collect()
}

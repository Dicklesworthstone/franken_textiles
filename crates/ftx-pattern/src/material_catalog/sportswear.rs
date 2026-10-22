//! Source-table observations from Salopek Čubrić et al. (2022), DOI 10.3390/ma15093306.
//! Yarn identity and construction stay separate: neither tenacity nor elongation at break
//! supplies a membrane law. Table 6 reports volume flow, not area-normalized permeability.

use std::collections::BTreeMap;

use crate::materials::{Component, Evidence, Form, Material, Quantity, Source};

const ARTICLE: &str = "https://doi.org/10.3390/ma15093306";
const PREFIX: &str = "zagreb/sportswear-2022";

struct Yarn {
    name: &'static str,
    composition: &'static [(&'static str, f64)],
    tex: f64,
    twist_m: Option<f64>,
    threads: usize,
    tenacity_cn_tex: f64,
    elongation_percent: f64,
    mass_cv_percent: Option<f64>,
    hair_counts: Option<[f64; 4]>,
}

const YARNS: [Yarn; 5] = [
    Yarn {
        name: "Cotton",
        composition: &[("cotton", 1.)],
        tex: 29.1,
        twist_m: Some(661.),
        threads: 2,
        tenacity_cn_tex: 13.90,
        elongation_percent: 5.20,
        mass_cv_percent: Some(11.75),
        hair_counts: Some([952., 147., 23., 5.]),
    },
    Yarn {
        name: "Wool",
        composition: &[("wool", 1.)],
        tex: 68.6,
        twist_m: Some(88.),
        threads: 1,
        tenacity_cn_tex: 5.25,
        elongation_percent: 2.90,
        mass_cv_percent: Some(19.04),
        hair_counts: Some([1604., 494., 156., 88.]),
    },
    Yarn {
        name: "PAN bouclé",
        composition: &[("polyacrylonitrile", 1.)],
        tex: 72.3,
        twist_m: Some(10.),
        threads: 1,
        tenacity_cn_tex: 4.66,
        elongation_percent: 12.55,
        mass_cv_percent: Some(12.04),
        hair_counts: Some([1350., 718., 346., 283.]),
    },
    Yarn {
        name: "Wool 50% / PAN 50%",
        composition: &[("wool", 0.5), ("polyacrylonitrile", 0.5)],
        tex: 71.4,
        twist_m: Some(114.),
        threads: 1,
        tenacity_cn_tex: 12.23,
        elongation_percent: 14.30,
        mass_cv_percent: Some(9.47),
        hair_counts: Some([1804., 535., 174., 95.]),
    },
    Yarn {
        name: "PA filament",
        composition: &[("polyamide", 1.)],
        tex: 15.4,
        twist_m: None,
        threads: 2,
        tenacity_cn_tex: 40.76,
        elongation_percent: 301.27,
        mass_cv_percent: None,
        hair_counts: None,
    },
];

const STRUCTURES: [(&str, &str); 5] = [
    ("sp", "single plain"),
    ("sm", "single miss"),
    ("st", "single tuck / single piqué"),
    ("dp", "double plain"),
    ("dm", "double Milano rib"),
];

// Tables 4–7, ordered SP, SM, ST, DP, DM within each yarn Y1–Y5. All are original
// published units: horizontal/vertical loops per cm, g/m2, mm, m2*K/W, L/min, m2*Pa/W.
const FABRICS: [[[f64; 7]; 5]; 5] = [
    [
        [7.98, 8.32, 260.38, 1.23, 0.0444, 140.12, 3.7448],
        [7.88, 8.32, 287.17, 1.37, 0.0504, 119.72, 3.9135],
        [5.32, 6.68, 267.35, 1.54, 0.0424, 206.14, 5.6966],
        [6.40, 9.08, 433.40, 1.92, 0.0464, 82.44, 4.1322],
        [7.26, 8.82, 430.49, 1.80, 0.0419, 77.14, 3.9222],
    ],
    [
        [6.00, 8.00, 238.19, 0.98, 0.0584, 100.40, 4.4758],
        [6.70, 7.76, 281.49, 1.22, 0.0694, 88.20, 2.0459],
        [4.66, 6.64, 286.40, 1.85, 0.0834, 193.30, 3.5071],
        [5.76, 8.76, 438.49, 2.00, 0.0824, 70.92, 2.4678],
        [6.54, 8.42, 453.85, 1.77, 0.0704, 66.20, 2.7070],
    ],
    [
        [5.80, 7.90, 233.73, 1.22, 0.0564, 78.76, 3.5522],
        [6.36, 7.92, 267.36, 1.52, 0.0614, 64.68, 3.8274],
        [4.62, 5.72, 250.79, 1.78, 0.0844, 110.66, 4.8722],
        [5.16, 8.72, 459.36, 2.28, 0.0714, 56.78, 5.6936],
        [6.08, 8.64, 453.15, 2.01, 0.0614, 51.84, 4.9208],
    ],
    [
        [5.88, 7.64, 234.20, 1.00, 0.0754, 126.18, 3.7760],
        [6.44, 7.62, 263.41, 1.24, 0.0634, 108.74, 5.7246],
        [4.68, 6.80, 299.42, 1.89, 0.0854, 194.86, 3.1658],
        [5.82, 8.88, 471.54, 2.06, 0.0812, 64.90, 6.0944],
        [6.44, 8.08, 455.06, 1.76, 0.0724, 74.60, 5.7542],
    ],
    [
        [5.52, 8.20, 92.68, 0.58, 0.0122, 145.88, 0.6112],
        [5.88, 6.36, 132.51, 0.77, 0.0160, 130.18, 0.7075],
        [3.88, 3.86, 132.51, 1.40, 0.0172, 146.28, 0.5570],
        [5.36, 5.88, 231.54, 1.70, 0.0123, 93.92, 3.0855],
        [6.02, 5.16, 216.42, 1.44, 0.0162, 91.02, 1.3329],
    ],
];

fn quantity(value: f64, unit: &str, method: &str, specimen: &str) -> Quantity {
    Quantity {
        value,
        unit: unit.into(),
        evidence: Evidence::Measured,
        source: "paper".into(),
        method: method.into(),
        conditions: BTreeMap::from([
            ("specimen".into(), specimen.into()),
            ("uncertainty".into(), "Replicate count, per-observation SD and confidence intervals are not tabulated; bounds remain absent".into()),
        ]),
        bounds: None,
    }
}

fn material(yarn: usize, suffix: &str, name: &str, form: Form) -> Material {
    Material {
        id: format!("{PREFIX}/y{}{suffix}", yarn + 1),
        name: format!("Salopek Čubrić 2022 Y{}: {name}", yarn + 1),
        form,
        composition: YARNS[yarn].composition.iter().map(|&(fiber, fraction)| Component {
            fiber: fiber.into(), fraction, source: "paper".into(),
        }).collect(),
        composition_complete: true,
        structure: BTreeMap::from([
            ("identity_scope".into(), "Published study group; supplier SKU, lot and individual coupon identifiers not reported".into()),
            ("composition_scope".into(), "Source Table 2 composition; no independent composition assay or mixture law".into()),
            ("mechanical_scope".into(), "No sheet membrane, bending, contact, damage or seam law is inferred from yarn tensile data or construction".into()),
        ]),
        properties: BTreeMap::new(),
        membrane_response: None, membrane_relaxation: None, thermal_response: None,
        optical_faces: None,
        sources: BTreeMap::from([("paper".into(), Source {
            reference: ARTICLE.into(), accessed_on: "2026-10-22".into(),
            note: "Salopek Čubrić, Potočić Matković, Pavlović and Pavko Čuden (2022), Material and Structural Functionalization of Knitted Fabrics for Sportswear, Materials 15(9):3306. Tables 2–7 and sections 3.1–3.2. CC BY 4.0, copyright the authors. Attributed numerical transcription only; no source article or images redistributed. Reviewed Europe PMC fullTextXML https://www.ebi.ac.uk/europepmc/webservices/rest/PMC9104909/fullTextXML SHA-256 d26ec767f236288486cc9145b6af5c62f7e23ee59b2032278c608193a11928df. Source table/prose discrepancies remain explicitly recorded; no current commercial-product claim.".into(),
        })]),
    }
}

fn yarn_record(index: usize) -> Material {
    let row = &YARNS[index];
    let mut m = material(index, "-yarn", &format!("{} yarn", row.name), Form::Yarn);
    let specimen = format!("SalopekCubric2022:Y{}:yarn", index + 1);
    let q = |value, unit, method| {
        let mut q = quantity(value, unit, method, &specimen);
        q.conditions.insert(
            "conditioning".into(),
            "Conditioned yarn packages; numerical temperature/humidity and duration not stated"
                .into(),
        );
        q
    };
    m.properties = BTreeMap::from([
        (
            "yarn_linear_density".into(),
            q(row.tex, "tex", "Table 2; ISO 2060 skein method"),
        ),
        (
            "yarn_tenacity".into(),
            q(
                row.tenacity_cn_tex,
                "cN/tex",
                "Table 2; ISO 2062 method B, Statimat M dynamometer; not an elastic modulus",
            ),
        ),
        (
            "yarn_elongation_break".into(),
            q(
                row.elongation_percent,
                "%",
                "Table 2; ISO 2062 method B, Statimat M dynamometer; not reversible working strain",
            ),
        ),
    ]);
    if let Some(twist) = row.twist_m {
        m.properties.insert(
            "twist".into(),
            q(
                twist,
                "1/m",
                "Table 2; Mesdan untwist/retwist measurement; twist handedness not stated",
            ),
        );
    }
    if let Some(cv) = row.mass_cv_percent {
        m.properties.insert("yarn_mass_cv".into(), q(cv, "%", "Table 3 mass-variation coefficient; Keisokki evenness tester 80 type B, +50/+50/+200 sensitivity; not measurement uncertainty"));
    }
    if let Some(counts) = row.hair_counts {
        for (length, count) in [2, 4, 6, 8].into_iter().zip(counts) {
            let mut observation = q(
                count,
                "count",
                "Table 3; Zweigle G565 hairiness tester, 50 m/min; total evaluated yarn length not reported",
            );
            observation
                .conditions
                .insert("protruding_length_mm".into(), length.to_string());
            m.properties
                .insert(format!("yarn_hair_count_{length}mm"), observation);
        }
    }
    m.structure
        .insert("threads_used_for_knitting".into(), row.threads.to_string());
    m.structure.insert("form_scope".into(), "Measured original yarn; the number of parallel knitting threads does not multiply its tex or create a ply/yarn material".into());
    if index == 4 {
        m.structure.insert("missing_observations".into(), "Source dashes for twist, mass CV and hairiness mean unreported, not zero. PA grade and filament count unreported".into());
    }
    m
}

fn fabric_record(yarn: usize, construction: usize) -> Material {
    let (slug, name) = STRUCTURES[construction];
    let [horizontal, vertical, mass, thickness, rct, flow, ret] = FABRICS[yarn][construction];
    let mut m = material(
        yarn,
        &format!("-{slug}"),
        &format!("{} {name}", YARNS[yarn].name),
        Form::Knit,
    );
    let specimen = format!(
        "SalopekCubric2022:Y{}:{}",
        yarn + 1,
        slug.to_ascii_uppercase()
    );
    let q = |value, unit, method| {
        let mut q = quantity(value, unit, method, &specimen);
        q.conditions.insert("conditioning".into(), "Relaxed 48 h under source-described standard conditions; numerical conditioning atmosphere unreported".into());
        q
    };
    let mut gauge = q(
        thickness,
        "mm",
        "Table 4; ISO 5084 gauge thickness, 0.01 mm instrument accuracy; not an uncertainty bound",
    );
    gauge
        .conditions
        .insert("gauge_pressure_reported".into(), "20 g/cm2".into());
    gauge.conditions.insert("scope".into(), "Pressure-loaded gauge measurement; source section 4.2 explicitly distinguishes unloaded thickness".into());
    let mut thermal = q(
        rct,
        "m2*K/W",
        "Table 5; ISO 11092, MTNW sweating guarded hotplate",
    );
    thermal.conditions.extend([
        ("air_temperature_c".into(), "20".into()),
        ("relative_humidity_percent".into(), "65".into()),
        ("air_speed_m_s".into(), "1".into()),
        (
            "test_pressure_and_plate_temperature".into(),
            "Not explicitly stated; gauge pressure is a different test".into(),
        ),
        (
            "thermal_scope".into(),
            "Reported effective resistance, no inferred conductivity or temperature-dependent law"
                .into(),
        ),
    ]);
    let mut vapor = q(
        ret,
        "m2*Pa/W",
        "Table 7; ISO 11092, MTNW sweating guarded hotplate with semipermeable membrane",
    );
    vapor.conditions.extend([
        ("air_temperature_c".into(), "35".into()),
        ("relative_humidity_percent".into(), "40".into()),
        ("air_speed_m_s".into(), "1".into()),
        (
            "test_pressure_and_plate_temperature".into(),
            "Not explicitly stated; no unstated temperature/humidity validity interval".into(),
        ),
    ]);
    let area_cm2 = if yarn == 4 { 5. } else { 20. };
    let mut air = q(
        flow,
        "L/min",
        "Table 6; ISO 9237, Air-Tronic B, measured volume flow through the separately stated test area",
    );
    air.conditions.extend([
        ("pressure_drop_pa".into(), "50".into()),
        ("test_area_cm2".into(), area_cm2.to_string()),
        ("quantity_scope".into(), "Volume flow, not air_permeability in m/s; different test areas are not silently comparable".into()),
    ]);
    let mut mass_observation = q(
        mass,
        "g/m2",
        "Table 4; mass of a 100 x 100 mm specimen, Kern ALJ 220-4 balance",
    );
    if yarn == 0 && construction == 2 {
        mass_observation.conditions.insert(
            "source_literal".into(),
            "267,35; decimal comma interpreted as 267.35 g/m2".into(),
        );
    }
    m.properties = BTreeMap::from([
        ("areal_density".into(), mass_observation),
        ("reported_thickness".into(), gauge),
        (
            "knit_wale_density".into(),
            q(
                horizontal,
                "1/cm",
                "Table 4 horizontal density; loops counted over 50 mm and expressed per cm",
            ),
        ),
        (
            "knit_course_density".into(),
            q(
                vertical,
                "1/cm",
                "Table 4 vertical density; loops counted over 50 mm and expressed per cm",
            ),
        ),
        ("thermal_resistance".into(), thermal),
        ("vapor_resistance".into(), vapor),
        ("air_volume_flow".into(), air),
    ]);
    m.structure.extend([
        ("specimen".into(), specimen),
        ("construction".into(), name.into()),
        ("yarn_record".into(), format!("{PREFIX}/y{}-yarn", yarn + 1)),
        ("parallel_knitting_threads".into(), YARNS[yarn].threads.to_string()),
        ("knitting_machine".into(), "Shima Seiki SES 122 RT, gauge 12E; common cam settings".into()),
        ("transport_scope".into(), "Source-qualified dry and vapor resistance for existing steady consumers; no heat capacity, moisture storage, liquid transport, wearer comfort or garment validation inferred".into()),
        ("thickness_scope".into(), "reported_thickness is retained for inspection; no unloaded contact/sewing clearance or hotplate pressure inferred".into()),
    ]);
    if yarn == 3 && construction == 3 {
        m.structure.insert("source_discrepancy".into(), "Table 7 Y4 DP is 6.0944 m2*Pa/W, exceeding the 5.7542 maximum stated in section 4.3; table retained without correction".into());
    }
    if yarn == 4 && construction == 2 {
        m.structure.insert("source_discrepancy".into(), "Table 4 repeats Y5 SM and Y5 ST mass as 132.51 g/m2; retained as printed, without inferred correction".into());
    }
    m
}

pub(super) fn records() -> Vec<Material> {
    (0..YARNS.len())
        .flat_map(|yarn| {
            std::iter::once(yarn_record(yarn)).chain(
                (0..STRUCTURES.len()).map(move |construction| fabric_record(yarn, construction)),
            )
        })
        .collect()
}

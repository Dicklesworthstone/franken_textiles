//! Factual transcription of NISTIR 7282, June 2005, Tables 1–4, Appendix A and §6.4.
//! Source coefficients below are in their ORIGINAL units; the adapter emits the existing Material.
//! Material-group fits are not measured knots or present-day branded-product specifications.

use std::collections::BTreeMap;

use crate::material_thermal::{PolynomialFit, TemperatureSeries, ThermalResponse};
use crate::materials::{Evidence, Form, Material, Quantity, Source};

const REPORT: &str = "https://nvlpubs.nist.gov/nistpubs/Legacy/IR/nistir7282.pdf";
const CONDITIONING: &str = "NISTIR7282:mass-equilibrium-at-23+/-3C-and-50+/-10percent-RH";

struct Row {
    slug: &'static str,
    name: &'static str,
    maker: &'static str,
    role: &'static str,
    thickness_mm: f64,
    density_kg_m3: f64,
    // Table 3: k = k0 + k1*T, W/(m K), T in Celsius, valid 20..70 C.
    k: [f64; 2],
    k_r_squared: f64,
    k20_printed: f64,
    // Table 4: Cp in J/(g K); increasing Celsius powers. Do not interpret as Kelvin powers.
    cp: [f64; 4],
    cp_r_squared: f64,
    cp20_printed: f64,
    cp_conflict: &'static str,
    mass_conflict: &'static str,
}

const ROWS: [Row; 10] = [
    Row {
        slug: "aralite",
        name: "Aralite",
        maker: "Southern Mills",
        role: "thermal liner",
        thickness_mm: 3.59,
        density_kg_m3: 74.2,
        k: [0.0304, 0.0003],
        k_r_squared: 0.980,
        k20_printed: 0.036,
        cp: [1.798, -0.0446, 0.0012, -0.000007],
        cp_r_squared: 0.985,
        cp20_printed: 1.33,
        cp_conflict: "",
        mass_conflict: "",
    },
    Row {
        slug: "breathe-tex",
        name: "Breathe-Tex",
        maker: "Alden Industries",
        role: "moisture barrier",
        thickness_mm: 1.22,
        density_kg_m3: 120.7,
        k: [0.0282, 0.0003],
        k_r_squared: 0.998,
        k20_printed: 0.034,
        cp: [2.4036, -0.0312, 0.0008, -0.000005],
        cp_r_squared: 0.954,
        cp20_printed: 2.06,
        cp_conflict: "",
        mass_conflict: "",
    },
    Row {
        slug: "breathe-tex-plus",
        name: "Breathe-Tex Plus",
        maker: "Alden Industries",
        role: "moisture barrier",
        thickness_mm: 1.12,
        density_kg_m3: 179.4,
        k: [0.0332, 0.0002],
        k_r_squared: 0.962,
        k20_printed: 0.037,
        cp: [2.1723, -0.0218, 0.0006, -0.000004],
        cp_r_squared: 0.946,
        cp20_printed: 1.94,
        cp_conflict: "",
        mass_conflict: "",
    },
    Row {
        slug: "cotton-duck",
        name: "Cotton Duck",
        maker: "not reported",
        role: "outer shell",
        thickness_mm: 1.32,
        density_kg_m3: 518.9,
        k: [0.0740, 0.0005],
        k_r_squared: 0.941,
        k20_printed: 0.084,
        cp: [1.4464, -0.0028, 0.000002, 0.00000009],
        cp_r_squared: 0.994,
        cp20_printed: 1.39,
        cp_conflict: "Withheld: main Table 4 has +9e-8*T^3; Appendix A Table 4A text has -9e-8*T^3; Figure 8A text has +0.0028*T rather than the tables' -0.0028*T. Appendix Table 5A also reports experimental Cp(50C)=1.62 versus calculated 1.32 J/(g K). No preferred equation or correction inferred.",
        mass_conflict: "",
    },
    Row {
        slug: "nomex-e89-crosstech",
        name: "Nomex E89 Crosstech",
        maker: "W. L. Gore & Associates",
        role: "moisture barrier",
        thickness_mm: 0.96,
        density_kg_m3: 143.1,
        k: [0.0299, 0.0003],
        k_r_squared: 0.930,
        k20_printed: 0.036,
        cp: [2.9263, -0.0274, 0.00007, 0.000002],
        cp_r_squared: 0.972,
        cp20_printed: 2.42,
        cp_conflict: "Withheld: main Table 4 prints 7e-5*T^2, whereas Appendix A Table 4A text prints 7e-6*T^2. No preferred equation, erratum or corrected value inferred.",
        mass_conflict: "",
    },
    Row {
        slug: "neo-guard",
        name: "Neo-Guard",
        maker: "Alden Industries",
        role: "moisture barrier",
        thickness_mm: 0.55,
        density_kg_m3: 597.4,
        k: [0.0491, 0.0011],
        k_r_squared: 0.827,
        k20_printed: 0.071,
        cp: [1.6263, -0.0021, -0.00007, 0.000001],
        cp_r_squared: 0.963,
        cp20_printed: 1.56,
        cp_conflict: "",
        mass_conflict: "",
    },
    Row {
        slug: "nomex-iii-defender",
        name: "Nomex III-Defender",
        maker: "Southern Mills",
        role: "outer shell",
        thickness_mm: 0.82,
        density_kg_m3: 316.9,
        k: [0.0401, 0.0005],
        k_r_squared: 0.966,
        k20_printed: 0.050,
        cp: [1.8322, -0.049, 0.0012, -0.000007],
        cp_r_squared: 0.976,
        cp20_printed: 1.28,
        cp_conflict: "",
        mass_conflict: "",
    },
    Row {
        slug: "nomex-iiia-pajama-crosstech",
        name: "Nomex IIIA Pajama Check Crosstech",
        maker: "W. L. Gore & Associates",
        role: "moisture barrier",
        thickness_mm: 0.52,
        density_kg_m3: 316.8,
        k: [0.0337, 0.0005],
        k_r_squared: 0.967,
        k20_printed: 0.044,
        cp: [2.3202, -0.0419, 0.001, -0.000006],
        cp_r_squared: 0.918,
        cp20_printed: 1.84,
        cp_conflict: "",
        mass_conflict: "",
    },
    Row {
        slug: "pbi-kevlar-kombat",
        name: "PBI Kevlar Kombat",
        maker: "Southern Mills",
        role: "outer shell",
        thickness_mm: 0.80,
        density_kg_m3: 321.8,
        k: [0.0354, 0.0007],
        k_r_squared: 0.995,
        k20_printed: 0.049,
        cp: [1.8212, -0.037, 0.0004, -0.0000002],
        cp_r_squared: 0.994,
        cp20_printed: 1.25,
        cp_conflict: "",
        mass_conflict: "",
    },
    Row {
        slug: "scotchlite",
        name: "Scotchlite",
        maker: "3M",
        role: "trim",
        thickness_mm: 0.75,
        density_kg_m3: 81.6,
        k: [0.0584, 0.0012],
        k_r_squared: 0.924,
        k20_printed: 0.082,
        cp: [1.2319, -0.0163, 0.000258, -0.0000008],
        cp_r_squared: 0.990,
        cp20_printed: 1.09,
        cp_conflict: "",
        mass_conflict: "Areal mass withheld: NISTIR 7282 Table 2 prints density 81.6 kg/m3; the related NISTIR 6512 Table 2 prints 55.0 g over 305x305 mm at 0.7493 mm thickness, inconsistent with its printed 0.0816 g/cm3 density. Preserve the reported density but do not choose a correction or derive transient mass from it.",
    },
];

fn quantity(value: f64, unit: &str, evidence: Evidence, method: &str, specimen: &str) -> Quantity {
    Quantity {
        value, unit: unit.into(), evidence, source: "nist7282".into(), method: method.into(),
        conditions: BTreeMap::from([
            ("specimen".into(), specimen.into()), ("conditioning".into(), CONDITIONING.into()),
            ("scope".into(), "Conditioned material group, not one identical coupon for every test; batch identifier unreported".into()),
        ]), bounds: None,
    }
}

fn series(
    coefficients: [f64; 4],
    domain_c: [f64; 2],
    unit: &str,
    method: &str,
    r2: f64,
) -> TemperatureSeries {
    TemperatureSeries {
        unit: unit.into(), evidence: Evidence::Published, source: "nist7282".into(), method: method.into(),
        conditions: BTreeMap::from([
            ("r_squared".into(), r2.to_string()),
            ("uncertainty_scope".into(), "R-squared is not uncertainty; coefficient covariance not supplied and measurement uncertainty is not propagated by this model".into()),
        ]), points: Vec::new(), polynomial: Some(PolynomialFit { domain_c, coefficients }),
    }
}

pub(super) fn records() -> Vec<Material> {
    ROWS.iter().flat_map(|row| {
        let specimen = format!("NISTIR7282:{}:conditioned-material-group", row.slug);
        let mut properties = BTreeMap::from([
            ("thickness".into(), quantity(row.thickness_mm, "mm", Evidence::Published,
                "Table 2 mean micrometer thickness, avoiding visible compression; pressure dependence not characterized", &specimen)),
            ("bulk_density".into(), quantity(row.density_kg_m3, "kg/m3", Evidence::Published,
                "Table 2 reported material-group bulk density; not constituent solid-fiber density", &specimen)),
            ("reported_k_at_20_c".into(), quantity(row.k20_printed, "W/(m*K)", Evidence::Published,
                "Table 3 rounded k at 20 C; retained separately from the equation, not a measured knot", &specimen)),
            ("reported_cp_at_20_c".into(), quantity(row.cp20_printed, "J/(g*K)", Evidence::Published,
                "Table 4 rounded Cp at 20 C; retained even when the competing polynomial is withheld", &specimen)),
        ]);
        if row.mass_conflict.is_empty() {
            properties.insert("areal_density".into(), quantity(row.thickness_mm * 0.001 * row.density_kg_m3,
                "kg/m2", Evidence::Estimate,
                "Product of Table 2 mean thickness and reported bulk density. Derived effective areal mass, not a separately measured mass/area or mean of paired products; covariance unavailable", &specimen));
        }
        let mut structure = BTreeMap::from([
            ("manufacturer_at_source_date".into(), row.maker.into()),
            ("reported_role".into(), row.role.into()),
            ("identity_scope".into(), "Historical conditioned material group; not a current branded SKU or raw fiber law".into()),
            ("construction_scope".into(), "Detailed yarn, weave/knit, finish and ply construction not established by the retained tables".into()),
            ("source_tables".into(), "NISTIR 7282 June 2005: Tables 1 and 2 p.4; Tables 3 and 4 p.14; methods sections 3 and 4; uncertainty section 6.4".into()),
            ("conductivity_scope".into(), "Effective single-layer regression from stacked-specimen heat-flow-meter tests; not a directly measured single-sheet k or validated pressure law".into()),
            ("specific_heat_scope".into(), "PerkinElmer DSC7, three specimens, 0..100 C fitted range; conditioned at least six months; pan-contact/swelling limitations retained".into()),
            ("geometry_scope".into(), "Nominal uncompressed material-group geometry fixed with temperature; thermal expansion, pressure and moisture changes are not modeled".into()),
            ("reported_uncertainty".into(), "Section 6.4 method-level relative combined standard / expanded (coverage factor 2): conductivity 5.8% / 11.7%; specific heat 27% / 54%; density 5% / 10%. Not per-coefficient covariance, sample SD, hard bounds, or a computed prediction interval".into()),
            ("specific_heat_main_table_coefficients_j_g_k".into(), format!("{:?}", row.cp)),
            ("specific_heat_main_table_r_squared".into(), row.cp_r_squared.to_string()),
            ("source_conflict_specific_heat".into(), if row.cp_conflict.is_empty() { "none identified in retained equation comparison" } else { row.cp_conflict }.into()),
            ("source_conflict_areal_mass".into(), if row.mass_conflict.is_empty() { "none identified at the adopted Table 2 effective-property scope" } else { row.mass_conflict }.into()),
            ("validation_scope".into(), "Source transcription and numerical-model checks only; no held-out garment, cold-weather, PPE, flammability or machine-process qualification".into()),
            ("rights".into(), "Attributed factual numerical observations and equations; no source images or pages redistributed".into()),
        ]);
        if !row.mass_conflict.is_empty() {
            structure.insert("density_crosscheck_source".into(), "https://nvlpubs.nist.gov/nistpubs/Legacy/IR/nistir6512.pdf#page=10".into());
        }
        let material = Material {
            id: format!("nist/7282/{}", row.slug), name: format!("NISTIR 7282 {} (conditioned historical specimens)", row.name),
            form: if row.slug == "cotton-duck" { Form::Woven } else { Form::Sheet },
            composition: Vec::new(), composition_complete: false, structure, properties,
            membrane_response: None, membrane_relaxation: None, optical_faces: None,
            thermal_response: Some(ThermalResponse {
                specimen, conditioning: CONDITIONING.into(),
                conductivity: Some(series([row.k[0], row.k[1], 0.0, 0.0], [20.0, 70.0], "W/(m*K)",
                    "Main Table 3 Celsius linear fit; reported k20 rounding is retained separately; fitted single-layer values are estimates", row.k_r_squared)),
                specific_heat: row.cp_conflict.is_empty().then(|| series(row.cp.map(|c| c * 1000.0), [0.0, 100.0], "J/(kg*K)",
                    "Main Table 4 Celsius cubic fit, original J/(g K) coefficients retained in structure; each coefficient multiplied by 1000 g/kg", row.cp_r_squared)),
            }),
            sources: BTreeMap::from([("nist7282".into(), Source { reference: REPORT.into(), accessed_on: "2026-10-14".into(),
                note: "Lawson, Walton, Bryner and Amon. Estimates of Thermal Properties for Fire Fighters' Protective Clothing Materials. NISTIR 7282, June 2005. Tables 1–4 and sections 3, 4, 6.4. Main tables visually checked; Appendix A conflict leads from extracted text, so disputed curves are withheld pending facsimile/erratum verification.".into() })]),
        };
        let alternative = (row.slug == "cotton-duck")
            .then(|| cotton_duck_linear_cp(material.clone()));
        std::iter::once(material).chain(alternative)
    }).collect()
}

/// A separately published model, not a repair of the conflicting cubic or another specimen group.
fn cotton_duck_linear_cp(mut material: Material) -> Material {
    material.id = "nist/7282/cotton-duck-linear-cp".into();
    material.name =
        "NISTIR 7282 Cotton Duck (conditioned historical specimens; published linear Cp fit)"
            .into();
    material.structure.extend(BTreeMap::<String, String>::from([
        ("thermal_model_parent_id".into(), "nist/7282/cotton-duck".into()),
        ("thermal_model_basis".into(), "Explicit alternative: source-authored Appendix A Table 1 and Figure 7A linear Cp fit. The conflicting cubic is still withheld; no coefficient correction or averaging inferred".into()),
        ("source_tables".into(), "NISTIR 7282 June 2005: main Tables 1 and 2 p.4; Table 3 p.14; sections 3, 4 and 6.4; Appendix A methods pp.A-1/A-2, linear-fit Tables 1/2/3 pp.A-3/A-4/A-5, Figure 7A p.A-12".into()),
        ("specific_heat_scope".into(), "Appendix A fabric D: three DSC specimens from the same conditioned Cotton Duck material group, not the identical conductivity coupons; group conditioned more than six months at 23+/-3 C and 50+/-10% RH. PerkinElmer DSC7, ASTM E1269, non-rolled pans because encapsulation was difficult, scan rate 5 C/min; source-authored linear fit valid 0..100 C. Exact batch and ply recipe unreported".into()),
        ("specific_heat_linear_coefficients_j_g_k".into(), "[1.4229, 0.0039]".into()),
        ("specific_heat_linear_source_diagnostics".into(), "Appendix A Table 2 reports experimental/calculated Cp(50 C) 1.62/1.62 J/(g K), 0.3% difference; Table 3 reports 0..100 C experimental/calculated averages 1.62/1.62 and percent-difference range -0.9..+2.6 using (experiment-calculated)/experiment*100. Rounded source summaries, not raw observations, held-out validation or propagated uncertainty".into()),
        ("source_pdf_sha256".into(), "97f68ce919dee22e00cbb7e11e0d277153208e0d14d1522baa3281816d5d140b".into()),
        ("source_facsimile_verification".into(), "2026-10-27: Appendix A Table 1, Table 2, Table 3 and Figure 7A visually checked against primary PDF pages 29, 30, 31 and 38 (one-based). Celsius axis, J/(g K) units, coefficients and R-squared agree; published valid domain is 0..100 C in Appendix A p.A-2".into()),
    ]));
    // Keep the original specimen/conditioning, geometry, conductivity and unresolved cubic facts.
    // Only the explicit Cp model choice and its source receipt differ from the parent material.
    if let Some(response) = material.thermal_response.as_mut() {
        response.specific_heat = Some(series(
            [1.4229, 0.0039, 0.0, 0.0].map(|c| c * 1000.0),
            [0.0, 100.0],
            "J/(kg*K)",
            "Appendix A Table 1 and Figure 7A Celsius linear least-squares fit; original Cp=(0.0039*T+1.4229) J/(g K), each coefficient multiplied by 1000 g/kg. Separate published alternative, not a corrected cubic; reported fit diagnostics do not establish independent validation",
            0.9824,
        ));
    }
    material.sources.insert("nist7282".into(), Source {
        reference: REPORT.into(),
        accessed_on: "2026-10-27".into(),
        note: "Lawson, Walton, Bryner and Amon. Estimates of Thermal Properties for Fire Fighters' Protective Clothing Materials. NISTIR 7282, June 2005. Same historical conditioned Cotton Duck group as main Tables 1-3, with the separate Appendix A Table 1/Figure 7A linear Cp fit and its Table 2/3 summaries. Appendix A tables and Figure 7A facsimile-verified 2026-10-27; conflicting cubic remains withheld. Source PDF SHA256 in structure.".into(),
    });
    material
}

//! Paired before/after-five-wash observations from Kanat and Ozdil (2026), Tables 2-5.
//! These are fabric-code group means, not a cycle-dependent law or an unchanged-mass assumption.

use std::collections::BTreeMap;

use ftx_core::{Refusal, RefusalCode};

use super::Selection;
use crate::material_treatment::{self, Input};
use crate::materials::{Catalog, Evidence, Form, Material, Quantity, Source};

const ARTICLE: &str = "https://doi.org/10.3390/ma19153264";
const PROCESS: &str = "ISO 6330 household laundering; Type A washer; 100 g standard 3-ECE reference detergent; 2 kg fabric load; 30 degrees C; 40 minutes per cycle; flat drying before testing";
const BEFORE: &str = "Before the study's five washing cycles; tested at 20 +/- 2 degrees C and 65 +/- 4 percent RH; conditioning duration not reported";
const AFTER: &str = "After five specified washing cycles and flat drying; tested at 20 +/- 2 degrees C and 65 +/- 4 percent RH; conditioning duration not reported";

#[derive(Clone, Copy)]
struct State {
    mass_g_m2: f64,
    thickness_mm: f64,
    cover: f64,
    resistance: f64,
    resistance_sd: f64,
    air_l_m2_s: f64,
    air_sd: f64,
    rwvp_percent: f64,
    rwvp_sd: f64,
}

struct Row {
    code: u8,
    elastane_percent: &'static str,
    weft: &'static str,
    shrinkage_warp_percent: f64,
    shrinkage_weft_percent: f64,
    // Table 3 and Table 5, in BW then AW order; original units retained below.
    states: [State; 2],
}

const ROWS: [Row; 5] = [
    Row {
        code: 1,
        elastane_percent: "1.50",
        weft: "All core-spun yarns",
        shrinkage_warp_percent: 4.83,
        shrinkage_weft_percent: 5.50,
        states: [
            State {
                mass_g_m2: 339.93,
                thickness_mm: 0.84,
                cover: 0.8813,
                resistance: 0.0152,
                resistance_sd: 0.00066,
                air_l_m2_s: 25.77,
                air_sd: 0.88,
                rwvp_percent: 50.67,
                rwvp_sd: 3.18,
            },
            State {
                mass_g_m2: 370.10,
                thickness_mm: 1.23,
                cover: 0.8976,
                resistance: 0.0245,
                resistance_sd: 0.00094,
                air_l_m2_s: 19.41,
                air_sd: 1.21,
                rwvp_percent: 52.79,
                rwvp_sd: 3.31,
            },
        ],
    },
    Row {
        code: 2,
        elastane_percent: "1.05",
        weft: "3 core-spun yarns + 1 plain yarn",
        shrinkage_warp_percent: 3.60,
        shrinkage_weft_percent: 5.30,
        states: [
            State {
                mass_g_m2: 339.83,
                thickness_mm: 0.83,
                cover: 0.8738,
                resistance: 0.0148,
                resistance_sd: 0.00082,
                air_l_m2_s: 33.01,
                air_sd: 3.66,
                rwvp_percent: 51.30,
                rwvp_sd: 1.10,
            },
            State {
                mass_g_m2: 364.80,
                thickness_mm: 1.20,
                cover: 0.8923,
                resistance: 0.0220,
                resistance_sd: 0.00130,
                air_l_m2_s: 21.83,
                air_sd: 1.06,
                rwvp_percent: 53.45,
                rwvp_sd: 1.15,
            },
        ],
    },
    Row {
        code: 3,
        elastane_percent: "0.95",
        weft: "2 core-spun yarns + 1 plain yarn",
        shrinkage_warp_percent: 3.50,
        shrinkage_weft_percent: 5.30,
        states: [
            State {
                mass_g_m2: 339.70,
                thickness_mm: 0.81,
                cover: 0.8725,
                resistance: 0.0139,
                resistance_sd: 0.00073,
                air_l_m2_s: 34.30,
                air_sd: 3.48,
                rwvp_percent: 51.60,
                rwvp_sd: 1.10,
            },
            State {
                mass_g_m2: 361.30,
                thickness_mm: 1.12,
                cover: 0.8920,
                resistance: 0.0214,
                resistance_sd: 0.00066,
                air_l_m2_s: 22.89,
                air_sd: 0.73,
                rwvp_percent: 53.76,
                rwvp_sd: 1.15,
            },
        ],
    },
    Row {
        code: 4,
        elastane_percent: "0.70",
        weft: "1 core-spun yarn + 1 plain yarn",
        shrinkage_warp_percent: 1.83,
        shrinkage_weft_percent: 4.60,
        states: [
            State {
                mass_g_m2: 339.35,
                thickness_mm: 0.77,
                cover: 0.8716,
                resistance: 0.0128,
                resistance_sd: 0.00025,
                air_l_m2_s: 35.89,
                air_sd: 2.91,
                rwvp_percent: 52.40,
                rwvp_sd: 1.10,
            },
            State {
                mass_g_m2: 355.60,
                thickness_mm: 1.08,
                cover: 0.8870,
                resistance: 0.0195,
                resistance_sd: 0.00129,
                air_l_m2_s: 27.03,
                air_sd: 1.23,
                rwvp_percent: 54.60,
                rwvp_sd: 1.15,
            },
        ],
    },
    Row {
        code: 5,
        elastane_percent: "0.50",
        weft: "1 core-spun yarn + 2 plain yarns",
        shrinkage_warp_percent: 1.60,
        shrinkage_weft_percent: 3.00,
        states: [
            State {
                mass_g_m2: 339.08,
                thickness_mm: 0.75,
                cover: 0.8651,
                resistance: 0.0124,
                resistance_sd: 0.00032,
                air_l_m2_s: 44.21,
                air_sd: 2.11,
                rwvp_percent: 53.35,
                rwvp_sd: 0.45,
            },
            State {
                mass_g_m2: 344.70,
                thickness_mm: 1.01,
                cover: 0.8792,
                resistance: 0.0196,
                resistance_sd: 0.00069,
                air_l_m2_s: 32.67,
                air_sd: 2.60,
                rwvp_percent: 55.59,
                rwvp_sd: 0.47,
            },
        ],
    },
];

fn specimen(code: u8) -> String {
    format!("KanatOzdil2026:fabric-{code}")
}

fn id(code: u8, washed: bool) -> String {
    let state = if washed {
        "after-5-washes"
    } else {
        "before-washing"
    };
    format!("kanat-ozdil-2026/denim-{code}/{state}")
}

fn source() -> Source {
    Source {
        reference: ARTICLE.into(),
        accessed_on: "2026-10-26".into(),
        note: "Zehra Evrim Kanat and Nilgun Ozdil (2026), Influence of Washing on the Thermal Comfort Parameters of Denim Fabrics with Varying Elastane Content, Materials 19(15), 3264; published 2026-08-02, CC BY. Sections 2.1-2.2 and Tables 1-5, PDF pages 3-6, visually checked. Publisher PDF: https://mdpi-res.com/d_attachment/materials/materials-19-03264/article_deploy/materials-19-03264.pdf; SHA-256 0d9fc029c60b79745b2c123a4d4d9bc34fa69734b511cf20d1bccc94fef01e2c. Selected numerical facts and original attribution only; no source article or images redistributed. BW/AW are fabric-code group observations, not individually identified coupons or retail SKUs.".into(),
    }
}

fn record(row: &Row, washed: bool) -> Material {
    let state = row.states[usize::from(washed)];
    let conditioning = if washed { AFTER } else { BEFORE };
    let conditions = BTreeMap::from([
        ("specimen".into(), specimen(row.code)),
        ("conditioning".into(), conditioning.into()),
        (
            "source_row".into(),
            format!("{}-{}", row.code, if washed { "AW" } else { "BW" }),
        ),
        (
            "specimen_scope".into(),
            "Fabric-code group; lot, individual coupon identity and raw replicates not supplied"
                .into(),
        ),
    ]);
    let q = |value, unit: &str, method: &str| Quantity {
        value,
        unit: unit.into(),
        evidence: Evidence::Measured,
        source: "paper".into(),
        method: method.into(),
        conditions: conditions.clone(),
        bounds: None,
    };
    let mass = q(
        state.mass_g_m2,
        "g/m2",
        "Table 3, page 5; TS EN 12127; specimen conditioning retained; uncertainty and replicate count not reported",
    );
    let mut thickness = q(
        state.thickness_mm,
        "mm",
        "Table 3, page 5; Alambeta; three measurements per fabric sample; numerical uncertainty not reported",
    );
    thickness.conditions.insert("measurement_pressure".into(), "Not reported; this observation does not establish unloaded contact or sewing clearance thickness".into());
    let mut resistance = q(
        state.resistance,
        "m2*K/W",
        "Table 5, pages 5-6; Alambeta; arithmetic mean of three measurements",
    );
    resistance.conditions.extend([
        ("sample_count".into(), "3".into()),
        ("measurement_pressure_and_plate_temperatures".into(), "Not reported; ambient conditioning is not a thermal-response domain".into()),
        ("uncertainty".into(), "Table 5 sample standard deviation retained separately; not a bound or confidence interval".into()),
    ]);
    let mut air = q(
        state.air_l_m2_s,
        "L/(m2*s)",
        "Table 5, pages 5-6; TS 391 EN ISO 9237; FX3300; arithmetic mean of seven measurements",
    );
    air.conditions.insert(
        "pressure_difference_and_test_area".into(),
        "Not reported; no pressure-dependent flow law is inferred".into(),
    );
    let mut vapor = q(
        state.rwvp_percent,
        "%",
        "Table 5, pages 5-6; Permetest; ISO 11092:2014; arithmetic mean of three measurements",
    );
    vapor.conditions.insert(
        "quantity_scope".into(),
        "Relative water vapor permeability; not evaporative resistance in m2*Pa/W".into(),
    );
    let mut cover = q(
        state.cover,
        "1",
        "Table 3, page 5; fractional fabric cover computed by the source using Equation 1; not a measured porosity",
    );
    cover.evidence = Evidence::Published;
    let mut properties = BTreeMap::from([
        ("areal_density".into(), mass),
        ("reported_thickness".into(), thickness),
        ("thermal_resistance".into(), resistance),
        (
            "reported_thermal_resistance_sd".into(),
            q(
                state.resistance_sd,
                "m2*K/W",
                "Table 5, pages 5-6; reported sample standard deviation, not a confidence interval",
            ),
        ),
        ("reported_air_permeability".into(), air),
        (
            "reported_air_permeability_sd".into(),
            q(
                state.air_sd,
                "L/(m2*s)",
                "Table 5, pages 5-6; reported sample standard deviation, not a confidence interval",
            ),
        ),
        ("reported_relative_water_vapor_permeability".into(), vapor),
        (
            "reported_relative_water_vapor_permeability_sd".into(),
            q(
                state.rwvp_sd,
                "%",
                "Table 5, pages 5-6; reported sample standard deviation, not a confidence interval",
            ),
        ),
        ("reported_fractional_cover".into(), cover),
    ]);
    if washed {
        for (key, value, direction) in [
            ("shrinkage_warp", row.shrinkage_warp_percent, "warp"),
            ("shrinkage_weft", row.shrinkage_weft_percent, "weft"),
        ] {
            let mut shrinkage = q(
                value,
                "%",
                "Table 4, page 5; TS EN ISO 5077; total dimensional shrinkage after five washing cycles; positive contraction; no reported uncertainty or replicate count",
            );
            shrinkage.conditions.extend([
                ("direction".into(), direction.into()),
                ("process".into(), PROCESS.into()),
                ("cycles".into(), "5".into()),
                ("conditioning_before".into(), BEFORE.into()),
                ("conditioning_after".into(), AFTER.into()),
            ]);
            properties.insert(key.into(), shrinkage);
        }
    } else {
        // Initial weaving densities are not claimed as post-treatment observations.
        for (key, value) in [("thread_density_warp", 27.0), ("thread_density_weft", 20.0)] {
            let mut count = q(
                value,
                "1/cm",
                "Section 2.1, page 3; source-declared initial weaving density; no independent count method reported",
            );
            count.evidence = Evidence::Published;
            properties.insert(key.into(), count);
        }
    }
    let mut structure = BTreeMap::from([
        ("source_sample_mark".into(), specimen(row.code)),
        ("weave".into(), "2/1 Z twill denim".into()),
        ("warp_yarn".into(), "100% cotton; Ne 8; twist coefficient alpha_e 4.5".into()),
        ("weft_yarn".into(), "Ne 14; twist coefficient alpha_e 4.2; core-spun yarn has cotton sheath and 78 dtex elastane filament core at draft ratio 5.2".into()),
        ("weft_arrangement".into(), row.weft.into()),
        ("source_initial_elastane_ratio_percent".into(), row.elastane_percent.into()),
        ("composition_scope".into(), "Table 2 reports fabric elastane ratio (%) without an explicit mass denominator or composition test; no complete dry-mass composition or post-wash compositional invariance is assigned".into()),
        ("thermal_scope".into(), "Measured effective resistance for the existing constant-property steady consumer; no inferred conductivity, heat capacity, temperature response or wearer comfort".into()),
        ("treatment_scope".into(), "One observed before/after-five-cycles transition; no intermediate cycle interpolation, repeat application, mass conservation, mechanical law, shrinkage under sewn constraints or qualified sewing clearance".into()),
    ]);
    if washed {
        structure.extend([
            ("treatment_process".into(), PROCESS.into()),
            ("treatment_cycles".into(), "5".into()),
        ]);
    }
    Material {
        id: id(row.code, washed),
        name: format!(
            "Kanat-Ozdil 2026 cotton/elastane denim {}, reported {}% elastane, {}",
            row.code,
            row.elastane_percent,
            if washed {
                "after five washes"
            } else {
                "before washing"
            }
        ),
        form: Form::Woven,
        composition: Vec::new(),
        composition_complete: false,
        structure,
        properties,
        membrane_response: None,
        membrane_relaxation: None,
        thermal_response: None,
        optical_faces: None,
        sources: BTreeMap::from([("paper".into(), source())]),
    }
}

pub(super) fn records() -> Vec<Material> {
    ROWS.iter()
        .flat_map(|row| [record(row, false), record(row, true)])
        .collect()
}

/// Build the source's exact five-cycle transition using ordinary frozen catalog selections.
/// No hashes, intermediate cycles, missing properties or invariant mass are fabricated.
///
/// # Errors
/// Unknown fabric code, invalid bundled data, or a refusal from canonical treatment admission.
pub fn treatment_request(fabric_code: u8) -> Result<Input, Refusal> {
    if !ROWS.iter().any(|row| row.code == fabric_code) {
        return Err(Refusal::new(
            RefusalCode::InvalidDocument,
            "Kanat-Ozdil 2026 fabric code must be 1 through 5",
        ));
    }
    let catalog = Catalog::builtin()?;
    let before = catalog.resolve(&Selection {
        id: id(fabric_code, false),
        ..Selection::default()
    })?;
    let after = catalog.resolve(&Selection {
        id: id(fabric_code, true),
        ..Selection::default()
    })?;
    let input = Input {
        schema: material_treatment::SCHEMA.into(),
        before: before.frozen_selection(),
        after: after.frozen_selection(),
        specimen: specimen(fabric_code),
        process: PROCESS.into(),
        cycles: 5,
        conditioning_before: BEFORE.into(),
        conditioning_after: AFTER.into(),
        shrinkage_warp: after.authored().properties["shrinkage_warp"].clone(),
        shrinkage_weft: after.authored().properties["shrinkage_weft"].clone(),
        sources: BTreeMap::from([("paper".into(), source())]),
        allow_estimates: false,
    };
    material_treatment::evaluate(&input)?;
    Ok(input)
}

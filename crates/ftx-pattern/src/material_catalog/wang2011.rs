//! Factual origin-node observations from Wang, O'Brien and Ramamoorthi (2011).
//! The source's inverse-fitted C matrix is not a direct measurement or a generic membrane law.
//! Its tensor-shear basis, negative couplings and indefinite origin remain unchanged.

use std::collections::BTreeMap;

use crate::materials::{Component, Evidence, Form, Material, Quantity, Source};

const PARAMETERS: &str = "https://jamesobrien.com/papers/Wang-DDE-2011-08/material_parameters.pdf";
const PARAMETERS_SHA256: &str = "f39b2b5270e3588321228d8579bacd844d0b5466be9b43b0c03ce61d1cd47749";
const CONDITIONING: &str = "Temperature, humidity, preconditioning, moisture content and loading rate are not reported for these parameter tables";

struct Row {
    slug: &'static str,
    name: &'static str,
    form: Form,
    composition: &'static [(&'static str, f64)],
    page: u8,
    areal_kg_m2: f64,
    // Original Origin column in source row order c11, c12, c22, c33; SI N/m.
    origin: [f64; 4],
}

const ROWS: [Row; 10] = [
    Row {
        slug: "ivory-rib-knit",
        name: "Ivory Rib Knit",
        form: Form::Knit,
        composition: &[("cotton", 0.95), ("elastane", 0.05)],
        page: 1,
        areal_kg_m2: 0.276,
        origin: [42.871021, -0.234556, 65.166023, 83.175644],
    },
    Row {
        slug: "pink-ribbon-brown",
        name: "Pink Ribbon Brown",
        form: Form::Sheet,
        composition: &[("polyester", 1.0)],
        page: 1,
        areal_kg_m2: 0.228,
        origin: [151.503906, 106.028160, 244.830673, 30.250183],
    },
    Row {
        slug: "white-dots-on-blk",
        name: "White Dots on Blk",
        form: Form::Sheet,
        composition: &[("polyester", 1.0)],
        page: 1,
        areal_kg_m2: 0.128,
        origin: [2002.08618, 0.345588, 2027.48559, 19.189022],
    },
    Row {
        slug: "navy-sparkle-sweat",
        name: "Navy Sparkle Sweat",
        form: Form::Sheet,
        composition: &[("polyester", 0.96), ("elastane", 0.04)],
        page: 2,
        areal_kg_m2: 0.224,
        origin: [12.486332, -19.875324, 3.272434, 17.616289],
    },
    Row {
        slug: "camel-ponte-roma",
        name: "Camel Ponte Roma",
        form: Form::Sheet,
        composition: &[("polyester", 0.60), ("rayon", 0.40)],
        page: 2,
        areal_kg_m2: 0.284,
        origin: [31.146198, -12.802702, 44.028667, 31.896357],
    },
    Row {
        slug: "gray-interlock",
        name: "Gray Interlock",
        form: Form::Knit,
        composition: &[("cotton", 0.60), ("polyester", 0.40)],
        page: 2,
        areal_kg_m2: 0.187,
        origin: [16.593832, -14.695990, 34.477123, 36.860302],
    },
    Row {
        slug: "11oz-black-denim",
        name: "11oz Black Denim",
        form: Form::Woven,
        composition: &[("cotton", 0.99), ("elastane", 0.01)],
        page: 2,
        areal_kg_m2: 0.324,
        origin: [205.352005, 10.234077, 1013.88629, 53.387184],
    },
    Row {
        slug: "white-swim-solid",
        name: "White Swim Solid",
        form: Form::Sheet,
        composition: &[("nylon", 0.87), ("elastane", 0.13)],
        page: 2,
        areal_kg_m2: 0.204,
        origin: [47.971176, 6.834521, 104.615456, 60.773663],
    },
    Row {
        slug: "tango-red-jet-set",
        name: "Tango Red Jet Set",
        form: Form::Sheet,
        composition: &[("polyester", 1.0)],
        page: 3,
        areal_kg_m2: 0.113,
        origin: [43.067627, 54.373299, 171.424835, 14.314700],
    },
    Row {
        slug: "royal-target",
        name: "Royal target",
        form: Form::Sheet,
        composition: &[("cotton", 0.65), ("polyester", 0.35)],
        page: 3,
        areal_kg_m2: 0.220,
        origin: [2016.55456, 9.170511, 2034.97644, 104.759674],
    },
];

pub(super) fn records() -> Vec<Material> {
    ROWS.iter().map(|row| {
        let id = format!("wang/2011/{}", row.slug);
        let conditions = BTreeMap::from([
            ("specimen".into(), id.clone()),
            ("conditioning".into(), CONDITIONING.into()),
            ("source_page".into(), row.page.to_string()),
            ("uncertainty".into(), "Rounded published value; coefficient uncertainty, replicate spread and specimen-to-specimen variability not reported".into()),
        ]);
        let mut properties = BTreeMap::from([("areal_density".into(), Quantity {
            value: row.areal_kg_m2,
            unit: "kg/m2".into(),
            evidence: Evidence::Published,
            source: "paper".into(),
            method: "Supplementary specimen density in kg/m2; also main-paper Table 1. Source-reported mass per area, not independently remeasured".into(),
            conditions: conditions.clone(),
            bounds: None,
        })]);
        for (coefficient, value) in ["c11", "c12", "c22", "c33"].into_iter().zip(row.origin) {
            let mut coefficient_conditions = conditions.clone();
            coefficient_conditions.extend([
                ("strain_basis".into(), "green_lagrange_tensor_shear".into()),
                ("source_node".into(), "lambda_max=0".into()),
                ("model_scope".into(), "Source inverse-fitted origin of C(lambda_max, phi) in sigma=C*epsilon; epsilon=[E11,E22,E12]. This is one node, not a finite-strain interval, generic Young modulus or engineering shear modulus".into()),
            ]);
            properties.insert(format!("wang2011_origin_{coefficient}"), Quantity {
                value,
                unit: "N/m".into(),
                evidence: Evidence::Published,
                source: "paper".into(),
                method: format!("Supplementary Origin column, {coefficient}; main paper section 3.1 equations (1)-(3), fitted from the section 4.1 cloth experiments. Original source-model parameter, not an independently measured modulus"),
                conditions: coefficient_conditions,
                bounds: None,
            });
        }
        let mut structure = BTreeMap::from([
            ("specimen".into(), id.clone()),
            ("source_specimen_name".into(), row.name.into()),
            ("source_model".into(), "Wang-OBrien-Ramamoorthi-2011-C-origin".into()),
            ("source_pdf_sha256".into(), PARAMETERS_SHA256.into()),
            ("source_pdf_bytes".into(), "2260286".into()),
            ("identity_scope".into(), "Named study specimen; supplier SKU, lot, finish and current purchasability not established".into()),
            ("composition_scope".into(), "Source-reported percentages, not an independent assay. Spandex is recorded as elastane; rayon subtype remains unspecified".into()),
            ("construction_scope".into(), "Named rib/interlock knits and the paper's explicitly discussed denim twill are classified; other specimens retain sheet form without inferring a weave or knit from use or appearance".into()),
            ("mechanics_scope".into(), "Only the fitted origin matrix is retained. A bounded linear approximation needs explicit estimate admission; source nonlinear interpolation, sheet bending, thickness and contact are not supplied by this record".into()),
            ("rights".into(), "Attributed factual numerical transcription only. Source PDF, photographs, textures, raw archive and code are not redistributed. Source terms require scholarly citation, author notification for non-scholarly use, and permission for endorsement or other uses; no broader license grant or completed author notification is asserted".into()),
        ]);
        if row.slug == "navy-sparkle-sweat" {
            structure.insert("origin_admissibility".into(), "The published normal block has c11*c22-c12*c12 < 0. Preserve the source values; a positive-definite local membrane must refuse this origin, never project or clamp it".into());
        }
        Material {
            id,
            name: format!("Wang 2011 specimen: {}", row.name),
            form: row.form,
            composition: row.composition.iter().map(|&(fiber, fraction)| Component {
                fiber: fiber.into(), fraction, source: "paper".into(),
            }).collect(),
            composition_complete: true,
            structure,
            properties,
            membrane_response: None,
            membrane_relaxation: None,
            thermal_response: None,
            optical_faces: None,
            sources: BTreeMap::from([("paper".into(), Source {
                reference: PARAMETERS.into(),
                accessed_on: "2026-11-03".into(),
                note: format!("Huamin Wang, James F. O'Brien and Ravi Ramamoorthi (2011), Data-Driven Elastic Models for Cloth: Modeling and Measurement, ACM TOG 30(4), Article 71, DOI 10.1145/2010324.1964966. Author-hosted supplementary parameter PDF, p{}, specimen {}. 2260286 bytes, SHA-256 {}. Main paper https://wanghmin.github.io/publication/wang-2011-dde/Wang-2011-DDE.pdf, sections 3.1 and 4.1 and Table 1. Source table visually checked; only factual origin coefficients, mass and composition transcribed. Source copyright and use conditions remain applicable; no open-license or author-notification claim. Section 6.2 manually chooses damping/friction, which are not imported.", row.page, row.name, PARAMETERS_SHA256),
            })]),
        }
    }).collect()
}
